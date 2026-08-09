//! Zweiter Durchlauf: die konkreten Werte hinter einer Abweichung.
//!
//! Der Fingerabdruck ist bewusst datenfrei — er soll einen Betrieb verlassen
//! dürfen. Beim Vergleich zweier Dateien auf demselben Rechner schützt diese
//! Einschränkung aber nichts; sie verhindert nur die Antwort auf die einzige
//! Frage, die wirklich zählt: *Was* steht da anders?
//!
//! Der Durchlauf verwendet dieselbe Zerlegung wie [`crate::zerlegung`] —
//! denselben Stapel offener Zeilen, dieselben Schlüssel, dieselbe
//! Normalisierung. Nur so zeigt er auf dieselben Zeilen, die der erste
//! Durchlauf gemeldet hat. Gesammelt wird ausschließlich, was gesucht ist:
//! eine Tabelle, ein paar Eimer, ein paar Felder.

use crate::scanner::Beobachter;
use crate::tabellen::{fnv, unterschluessel, OffeneZeile};
use std::collections::HashSet;


/// Ein gefundener Wert, in Klartext.
pub struct Fund {
    /// Zeilenschlüssel — bei einer Untertabelle einschließlich Fremdschlüssel
    /// und laufender Nummer.
    pub schluessel: Vec<u8>,
    /// Tabelle, aus der der Wert stammt. Leer für den Datensatz selbst.
    pub tabelle: Vec<u8>,
    pub pfad: Vec<u8>,
    pub wert: Vec<u8>,
}

/// Eine abgeschlossene Untertabellenzeile im Klartext, die auf den
/// Schlüssel ihres Elternteils wartet.
struct WartendKlartext {
    tabelle: Vec<u8>,
    nummer: u32,
    felder: Vec<(Vec<u8>, Vec<u8>)>,
    kinder: Vec<WartendKlartext>,
}

/// So viele Funde werden höchstens gesammelt. Eine Abweichung, die
/// Zehntausende Stellen betrifft, ist ohnehin keine Einzelfallfrage mehr.
const FUNDE_HOECHSTENS: usize = 2000;

pub struct Einzelheiten {
    datensatz: Vec<Vec<u8>>,
    schluessel_pfade: Vec<Vec<Vec<u8>>>,
    eimer: usize,
    untertabellen: HashSet<Vec<u8>>,

    /// Nur diese Eimer. Leer heißt: alle.
    gesuchte_eimer: HashSet<usize>,
    /// Nur diese Feldpfade. Leer heißt: alle.
    gesuchte_pfade: HashSet<Vec<u8>>,
    /// Nur diese Tabelle. `None` heißt: alle.
    gesuchte_tabelle: Option<Vec<u8>>,

    pfad: Vec<Vec<u8>>,
    hat_kinder: Vec<bool>,
    text: Vec<u8>,
    im_datensatz_ab: Option<usize>,
    stapel: Vec<OffeneZeile>,
    /// Klartextfelder je offener Zeile, gleichlaufend zum Stapel.
    inhalte: Vec<Vec<(Vec<u8>, Vec<u8>)>>,
    /// Abgeschlossene Untertabellenzeilen, die auf den Elternschlüssel warten
    /// — gleichlaufend zum Stapel. Dieselbe Verzögerung wie in der Zerlegung;
    /// ohne sie bekämen die Zeilen einen anderen Schlüssel als dort.
    wartende: Vec<Vec<WartendKlartext>>,

    pub funde: Vec<Fund>,
    pub abgeschnitten: bool,
}

impl Einzelheiten {
    #[allow(clippy::too_many_arguments)]
    pub fn neu(
        datensatz: &str,
        schluessel: &[&str],
        eimer: usize,
        untertabellen: &[&str],
        gesuchte_eimer: &[usize],
        gesuchte_pfade: &[&[u8]],
        gesuchte_tabelle: Option<&[u8]>,
    ) -> Self {
        Einzelheiten {
            datensatz: zerlege_pfad(datensatz),
            schluessel_pfade: schluessel.iter().map(|s| zerlege_pfad(s)).collect(),
            eimer: eimer.max(1),
            untertabellen: untertabellen
                .iter()
                .map(|u| u.trim_matches('/').as_bytes().to_vec())
                .filter(|u| !u.is_empty())
                .collect(),
            gesuchte_eimer: gesuchte_eimer.iter().copied().collect(),
            gesuchte_pfade: gesuchte_pfade.iter().map(|p| p.to_vec()).collect(),
            gesuchte_tabelle: gesuchte_tabelle.map(|t| t.to_vec()),
            pfad: Vec::new(),
            hat_kinder: Vec::new(),
            text: Vec::new(),
            im_datensatz_ab: None,
            stapel: Vec::new(),
            inhalte: Vec::new(),
            wartende: Vec::new(),
            funde: Vec::new(),
            abgeschnitten: false,
        }
    }

    fn verbinde(teile: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for (i, t) in teile.iter().enumerate() {
            if i > 0 {
                out.push(b'/');
            }
            out.extend_from_slice(t);
        }
        out
    }

    fn pfad_ab_datensatz(&self) -> Vec<u8> {
        let ab = self.im_datensatz_ab.unwrap_or(0);
        Self::verbinde(&self.pfad[ab.min(self.pfad.len())..])
    }

    fn pfad_ab_zeile(&self) -> Vec<u8> {
        match self.stapel.last() {
            Some(z) => Self::verbinde(&self.pfad[z.ab_tiefe.min(self.pfad.len())..]),
            None => self.pfad_ab_datensatz(),
        }
    }

    fn passt_datensatz(&self) -> bool {
        !self.datensatz.is_empty()
            && self.pfad.len() >= self.datensatz.len()
            && self.pfad[self.pfad.len() - self.datensatz.len()..] == self.datensatz[..]
    }

    fn ist_untertabelle(&self) -> bool {
        if self.im_datensatz_ab.is_none() {
            return false;
        }
        self.untertabellen.contains(&self.pfad_ab_datensatz())
            || self.untertabellen.contains(&self.pfad_ab_zeile())
    }

    fn schluessel_index(&self) -> Option<usize> {
        let ab = self.im_datensatz_ab?;
        if self.stapel.len() != 1 {
            return None;
        }
        let rel: Vec<&Vec<u8>> = self.pfad.iter().skip(ab).collect();
        self.schluessel_pfade
            .iter()
            .position(|s| s.len() == rel.len() && s.iter().zip(rel.iter()).all(|(a, b)| a == *b))
    }

    fn schliesse_zeile(&mut self) {
        let zeile = match self.stapel.pop() {
            Some(z) => z,
            None => return,
        };
        let felder = self.inhalte.pop().unwrap_or_default();
        let kinder = self.wartende.pop().unwrap_or_default();

        if !zeile.tabelle.is_empty() {
            let w = WartendKlartext {
                tabelle: zeile.tabelle,
                nummer: zeile.nummer,
                felder,
                kinder,
            };
            if let Some(eltern) = self.wartende.last_mut() {
                eltern.push(w);
            }
            return;
        }

        let schluessel = zeile.schluessel();
        self.sammle(&Vec::new(), &schluessel, felder);
        for k in kinder {
            self.loese_auf(k, &schluessel);
        }
    }

    fn loese_auf(&mut self, w: WartendKlartext, eltern_schluessel: &[u8]) {
        let schluessel = unterschluessel(eltern_schluessel, w.nummer);
        self.sammle(&w.tabelle, &schluessel, w.felder);
        for k in w.kinder {
            self.loese_auf(k, &schluessel);
        }
    }

    /// Nimmt die Felder einer fertigen Zeile auf, sofern sie gesucht ist.
    fn sammle(&mut self, tabelle: &Vec<u8>, schluessel: &[u8], felder: Vec<(Vec<u8>, Vec<u8>)>) {
        let eimer = (fnv(schluessel) as usize) % self.eimer;
        let tabelle_passt = match &self.gesuchte_tabelle {
            Some(t) => t == tabelle,
            None => true,
        };
        if !tabelle_passt {
            return;
        }
        if !self.gesuchte_eimer.is_empty() && !self.gesuchte_eimer.contains(&eimer) {
            return;
        }
        for (pfad, wert) in felder {
            if !self.gesuchte_pfade.is_empty() && !self.gesuchte_pfade.contains(&pfad) {
                continue;
            }
            if self.funde.len() >= FUNDE_HOECHSTENS {
                self.abgeschnitten = true;
                return;
            }
            self.funde.push(Fund {
                schluessel: schluessel.to_vec(),
                tabelle: tabelle.clone(),
                pfad,
                wert,
            });
        }
    }

    fn nimm(&mut self, pfad: Vec<u8>, wert: Vec<u8>) {
        if let Some(felder) = self.inhalte.last_mut() {
            felder.push((pfad, wert));
        }
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

        if self.im_datensatz_ab.is_none() && self.passt_datensatz() {
            self.im_datensatz_ab = Some(self.pfad.len());
            self.stapel.push(OffeneZeile::neu(
                Vec::new(),
                self.pfad.len(),
                Vec::new(),
                0,
                self.schluessel_pfade.len(),
            ));
            self.inhalte.push(Vec::new());
            self.wartende.push(Vec::new());
            return;
        }

        if self.ist_untertabelle() {
            let name_rel = self.pfad_ab_datensatz();
            let eltern = self.stapel.last().expect("Zeile offen").schluessel();
            let nummer = {
                let z = self.stapel.last_mut().expect("Zeile offen");
                let n = z.kinder.entry(name_rel.clone()).or_insert(0);
                *n += 1;
                *n - 1
            };
            self.stapel.push(OffeneZeile::neu(
                name_rel,
                self.pfad.len(),
                eltern,
                nummer,
                0,
            ));
            self.inhalte.push(Vec::new());
            self.wartende.push(Vec::new());
        }
    }

    fn element_ende(&mut self) {
        let ist_blatt = !*self.hat_kinder.last().unwrap_or(&false);
        let zeilenende = matches!(self.stapel.last(), Some(z) if z.ab_tiefe == self.pfad.len());

        if ist_blatt && !self.stapel.is_empty() && !zeilenende {
            let wert = normalisiere(&self.text);
            if let Some(k) = self.schluessel_index() {
                if let Some(z) = self.stapel.last_mut() {
                    z.schluessel_text[k] = Some(wert.clone());
                }
            }
            let pfad = self.pfad_ab_zeile();
            self.nimm(pfad, wert);
        }

        if zeilenende {
            self.schliesse_zeile();
        }

        self.pfad.pop();
        self.hat_kinder.pop();
        self.text.clear();

        if matches!(self.im_datensatz_ab, Some(ab) if self.pfad.len() + 1 == ab) {
            self.im_datensatz_ab = None;
        }
    }

    fn attribut(&mut self, name: &[u8], wert: &[u8]) {
        if self.stapel.is_empty() {
            return;
        }
        let mut pfad = self.pfad_ab_zeile();
        pfad.push(b'@');
        pfad.extend_from_slice(name);
        let normal = normalisiere(wert);
        self.nimm(pfad, normal);
    }

    fn text(&mut self, teil: &[u8]) {
        if self.text.len() < 4096 {
            self.text.extend_from_slice(teil);
        }
    }
}

/// Muss zeichengleich zur Normalisierung in der Zerlegung sein: Leerraum an
/// den Rändern weg, innen zu einem Leerzeichen zusammengezogen. Weicht sie ab,
/// findet der zweite Durchlauf andere Werte als der erste.
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

fn zerlege_pfad(s: &str) -> Vec<Vec<u8>> {
    s.split('/')
        .filter(|t| !t.is_empty())
        .map(|t| t.as_bytes().to_vec())
        .collect()
}

