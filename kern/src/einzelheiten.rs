//! Zweiter Durchlauf: die konkreten Werte hinter einer Abweichung.
//!
//! Der Fingerabdruck ist bewusst datenfrei — er soll einen Betrieb verlassen
//! dürfen. Beim Vergleich zweier Dateien auf demselben Rechner schützt diese
//! Einschränkung aber nichts; sie verhindert nur die Antwort auf die einzige
//! Frage, die wirklich zählt: *Was* steht da anders?
//!
//! Deshalb ein zweiter Durchlauf, der ausschließlich die Stellen einsammelt,
//! die der erste als abweichend gemeldet hat. Bei 10.000 Datensätzen und 4096
//! Eimern trifft das zwei bis drei Sätze — der Durchlauf kostet einmal Lesen
//! und liefert Klartext statt Prüfwerte.

use crate::scanner::Beobachter;
use std::collections::HashSet;

const FNV_ANFANG: u32 = 2166136261;
const FNV_PRIM: u32 = 16777619;

#[inline]
fn fnv_weiter(h: u32, daten: &[u8]) -> u32 {
    let mut h = h;
    for &b in daten {
        h ^= b as u32;
        h = h.wrapping_mul(FNV_PRIM);
    }
    h
}

/// Ein gefundener Wert, in Klartext.
pub struct Fund {
    pub schluessel: Vec<u8>,
    pub pfad: Vec<u8>,
    pub wert: Vec<u8>,
    /// Laufende Nummer innerhalb des Datensatzes — nötig, wenn derselbe Pfad
    /// mehrfach vorkommt (Wiederholgruppen).
    pub folge: u32,
}

/// So viele Funde werden höchstens gesammelt. Eine Abweichung, die
/// Zehntausende Stellen betrifft, ist ohnehin keine Einzelfallfrage mehr.
const FUNDE_HOECHSTENS: usize = 2000;

pub struct Einzelheiten {
    datensatz: Vec<Vec<u8>>,
    schluessel_pfade: Vec<Vec<Vec<u8>>>,
    eimer: usize,

    /// Nur diese Eimer werden betrachtet. Leer heißt: alle.
    gesuchte_eimer: HashSet<usize>,
    /// Nur diese Pfade werden gesammelt. Leer heißt: alle.
    gesuchte_pfade: HashSet<Vec<u8>>,

    pfad: Vec<Vec<u8>>,
    hat_kinder: Vec<bool>,
    text: Vec<u8>,
    text_hat_inhalt: bool,
    im_datensatz_ab: Option<usize>,

    /// Felder des laufenden Datensatzes im Klartext, bis der Schlüssel
    /// feststeht.
    laufend: Vec<(Vec<u8>, Vec<u8>, u32)>,
    laufender_schluessel: Vec<Option<Vec<u8>>>,
    zaehler: std::collections::HashMap<Vec<u8>, u32>,

    pub funde: Vec<Fund>,
    pub abgeschnitten: bool,
}

impl Einzelheiten {
    pub fn neu(
        datensatz: &str,
        schluessel: &[&str],
        eimer: usize,
        gesuchte_eimer: &[usize],
        gesuchte_pfade: &[&[u8]],
    ) -> Self {
        Einzelheiten {
            datensatz: zerlege_pfad(datensatz),
            schluessel_pfade: schluessel.iter().map(|s| zerlege_pfad(s)).collect(),
            eimer: eimer.max(1),
            gesuchte_eimer: gesuchte_eimer.iter().copied().collect(),
            gesuchte_pfade: gesuchte_pfade.iter().map(|p| p.to_vec()).collect(),
            pfad: Vec::new(),
            hat_kinder: Vec::new(),
            text: Vec::new(),
            text_hat_inhalt: false,
            im_datensatz_ab: None,
            laufend: Vec::new(),
            laufender_schluessel: vec![None; schluessel.len()],
            zaehler: std::collections::HashMap::new(),
            funde: Vec::new(),
            abgeschnitten: false,
        }
    }

    fn pfad_ab_datensatz(&self) -> Vec<u8> {
        let ab = self.im_datensatz_ab.unwrap_or(0);
        let mut out = Vec::new();
        for (i, teil) in self.pfad.iter().enumerate().skip(ab) {
            if i > ab {
                out.push(b'/');
            }
            out.extend_from_slice(teil);
        }
        out
    }

    fn passt_datensatz(&self) -> bool {
        if self.datensatz.is_empty() {
            return false;
        }
        self.pfad.len() >= self.datensatz.len()
            && self.pfad[self.pfad.len() - self.datensatz.len()..] == self.datensatz[..]
    }

    fn schluessel_index(&self) -> Option<usize> {
        let ab = self.im_datensatz_ab?;
        let rel: Vec<&Vec<u8>> = self.pfad.iter().skip(ab).collect();
        self.schluessel_pfade
            .iter()
            .position(|s| s.len() == rel.len() && s.iter().zip(rel.iter()).all(|(a, b)| a == *b))
    }

    /// Muss denselben Wert liefern wie der Abdruck, sonst zeigt der zweite
    /// Durchlauf auf andere Datensätze als der erste.
    fn eimer_von(&self, schluessel_teile: &[Option<Vec<u8>>]) -> usize {
        let mut sh = FNV_ANFANG;
        let mut vollstaendig = true;
        for teil in schluessel_teile {
            match teil {
                Some(t) => {
                    let h = werthash(t);
                    sh = fnv_weiter(sh, &h.to_le_bytes());
                }
                None => vollstaendig = false,
            }
        }
        if !vollstaendig || schluessel_teile.is_empty() {
            return usize::MAX; // ohne Schlüssel nicht zuzuordnen
        }
        (sh as usize) % self.eimer
    }

    fn schliesse_datensatz(&mut self) {
        let eimer = self.eimer_von(&self.laufender_schluessel);
        let gesucht = self.gesuchte_eimer.is_empty() || self.gesuchte_eimer.contains(&eimer);

        if gesucht && self.funde.len() < FUNDE_HOECHSTENS {
            let schluessel: Vec<u8> = self
                .laufender_schluessel
                .iter()
                .map(|t| t.clone().unwrap_or_else(|| b"?".to_vec()))
                .collect::<Vec<_>>()
                .join(&b'|');

            for (pfad, wert, folge) in std::mem::take(&mut self.laufend) {
                if !self.gesuchte_pfade.is_empty() && !self.gesuchte_pfade.contains(&pfad) {
                    continue;
                }
                if self.funde.len() >= FUNDE_HOECHSTENS {
                    self.abgeschnitten = true;
                    break;
                }
                self.funde.push(Fund {
                    schluessel: schluessel.clone(),
                    pfad,
                    wert,
                    folge,
                });
            }
        }

        self.laufend.clear();
        self.zaehler.clear();
        for t in &mut self.laufender_schluessel {
            *t = None;
        }
    }
}

/// Muss zeichengleich zur Normalisierung im Abdruck sein: Leerraum an den
/// Rändern weg, innen zu einem Leerzeichen zusammengezogen.
fn normalisiere(roh: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(roh.len());
    let mut wartend = false;
    for &c in roh {
        if c.is_ascii_whitespace() {
            if !out.is_empty() {
                wartend = true;
            }
            continue;
        }
        if wartend {
            out.push(b' ');
            wartend = false;
        }
        out.push(c);
    }
    out
}

fn werthash(wert: &[u8]) -> u32 {
    if wert.is_empty() {
        fnv_weiter(FNV_ANFANG, b"\x00leer")
    } else {
        fnv_weiter(FNV_ANFANG, wert)
    }
}

impl Beobachter for Einzelheiten {
    fn element_beginn(&mut self, name: &[u8]) {
        if let Some(l) = self.hat_kinder.last_mut() {
            *l = true;
        }
        self.pfad.push(name.to_vec());
        self.hat_kinder.push(false);
        self.text.clear();
        self.text_hat_inhalt = false;

        if self.im_datensatz_ab.is_none() && self.passt_datensatz() {
            self.im_datensatz_ab = Some(self.pfad.len());
        }
    }

    fn element_ende(&mut self) {
        let ist_blatt = !*self.hat_kinder.last().unwrap_or(&false);

        if ist_blatt && self.im_datensatz_ab.is_some() {
            let wert = normalisiere(&self.text);
            let pfad = self.pfad_ab_datensatz();

            if let Some(k) = self.schluessel_index() {
                self.laufender_schluessel[k] = Some(wert.clone());
            }
            let folge = {
                let z = self.zaehler.entry(pfad.clone()).or_insert(0);
                *z += 1;
                *z - 1
            };
            self.laufend.push((pfad, wert, folge));
        }

        self.pfad.pop();
        self.hat_kinder.pop();
        self.text.clear();
        self.text_hat_inhalt = false;

        if matches!(self.im_datensatz_ab, Some(ab) if self.pfad.len() + 1 == ab) {
            self.im_datensatz_ab = None;
            self.schliesse_datensatz();
        }
    }

    fn text(&mut self, teil: &[u8]) {
        // Rohtext sammeln; normalisiert wird erst am Elementende, damit die
        // Zusammenfassung über Blockgrenzen hinweg dieselbe ist.
        if self.text.len() < 4096 {
            self.text.extend_from_slice(teil);
            self.text_hat_inhalt = true;
        }
    }
}

fn zerlege_pfad(s: &str) -> Vec<Vec<u8>> {
    s.split('/')
        .filter(|t| !t.is_empty())
        .map(|t| t.as_bytes().to_vec())
        .collect()
}
