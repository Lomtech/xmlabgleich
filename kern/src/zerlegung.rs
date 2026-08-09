//! Zerlegt ein Dokument in Tabellen und bildet je Tabelle einen Abdruck.
//!
//! Siehe [`crate::tabellen`] für den Gedanken. Hier steht die Umsetzung: ein
//! Stapel offener Zeilen, der mit der Verschachtelung wächst und schrumpft.
//! Jede Wiederholgruppe öffnet eine Zeile in ihrer eigenen Tabelle, jedes
//! Blatt trägt zu der Zeile bei, in der es steht — nicht zur Haupttabelle.

use crate::abdruck::Aggregat;
use crate::scanner::Beobachter;
use crate::tabellen::{
    fnv, fnv_weiter, mit_index, unterschluessel, OffeneZeile, Tabelle, WartendeZeile,
};
use std::collections::{HashMap, HashSet};

const FNV_ANFANG: u32 = 2166136261;

pub struct Zerlegung {
    /// Pfad des Datensatz-Elements, in Segmenten.
    datensatz: Vec<Vec<u8>>,
    /// Schlüsselfelder der Haupttabelle, relativ zum Datensatz.
    schluessel: Vec<Vec<Vec<u8>>>,
    eimer: usize,
    /// Welche Pfade (relativ zum Datensatz) eine eigene Tabelle bilden.
    untertabellen: HashSet<Vec<u8>>,

    // ---- laufender Zustand
    pfad: Vec<Vec<u8>>,
    hat_kinder: Vec<bool>,
    text_hash: u32,
    text_hat_inhalt: bool,
    text_wartend: usize,
    text_klartext: Vec<u8>,

    im_datensatz_ab: Option<usize>,
    /// Stapel offener Zeilen. Ganz unten die Haupttabelle, darüber die
    /// Wiederholgruppen in ihrer Verschachtelung.
    stapel: Vec<OffeneZeile>,

    // ---- Ergebnis
    pub tabellen: HashMap<Vec<u8>, Tabelle>,
    pub datensaetze: u64,
    pub ohne_schluessel: u64,
    satzfolge: u32,
    pub erste_schluessel: Vec<Vec<u8>>,
}

/// So viele Schlüssel werden im Klartext gemerkt.
///
/// Nicht nur für die Satzfolge, sondern auch um zu sagen, *welche* Datensätze
/// fehlen — die nützlichste Auskunft, wenn zwei Bestände verschieden groß
/// sind. Bei 50.000 Schlüsseln zu 20 Zeichen sind das rund 1 MB; darüber
/// hinaus sagt der Bericht, dass die Liste unvollständig ist.
const ERSTE_SCHLUESSEL_HOECHSTENS: usize = 50_000;
const ANORDNUNGEN_HOECHSTENS: usize = 1000;

impl Zerlegung {
    pub fn neu(
        datensatz: &str,
        schluessel: &[&str],
        eimer: usize,
        untertabellen: &[&str],
    ) -> Self {
        let mut tabellen = HashMap::new();
        tabellen.insert(
            Vec::new(),
            Tabelle::neu(Vec::new(), Vec::new(), 0, eimer.max(1)),
        );
        Zerlegung {
            datensatz: zerlege_pfad(datensatz),
            schluessel: schluessel.iter().map(|s| zerlege_pfad(s)).collect(),
            eimer: eimer.max(1),
            untertabellen: untertabellen
                .iter()
                .map(|u| u.trim_matches('/').as_bytes().to_vec())
                .filter(|u| !u.is_empty())
                .collect(),
            pfad: Vec::new(),
            hat_kinder: Vec::new(),
            text_hash: FNV_ANFANG,
            text_hat_inhalt: false,
            text_wartend: 0,
            text_klartext: Vec::new(),
            im_datensatz_ab: None,
            stapel: Vec::new(),
            tabellen,
            datensaetze: 0,
            ohne_schluessel: 0,
            satzfolge: FNV_ANFANG,
            erste_schluessel: Vec::new(),
        }
    }

    /// Pfad ab dem Datensatz, mit `/` verbunden.
    fn pfad_ab_datensatz(&self) -> Vec<u8> {
        let ab = self.im_datensatz_ab.unwrap_or(0);
        verbinde(&self.pfad[ab.min(self.pfad.len())..])
    }

    /// Pfad ab der aktuell offenen Zeile — das ist der Spaltenname innerhalb
    /// ihrer Tabelle.
    fn pfad_ab_zeile(&self) -> Vec<u8> {
        match self.stapel.last() {
            Some(z) => verbinde(&self.pfad[z.ab_tiefe.min(self.pfad.len())..]),
            None => self.pfad_ab_datensatz(),
        }
    }

    fn passt_datensatz(&self) -> bool {
        !self.datensatz.is_empty()
            && self.pfad.len() >= self.datensatz.len()
            && self.pfad[self.pfad.len() - self.datensatz.len()..] == self.datensatz[..]
    }

    /// Ist dieser Pfad als Untertabelle vermerkt?
    ///
    /// Beide Schreibweisen gelten: der volle Pfad ab dem Datensatz
    /// (`BEDINGUNGEN/BEDINGUNG/STAFFELN/STAFFEL`) und der Pfad ab der
    /// umgebenden Zeile (`STAFFELN/STAFFEL`). Der volle ist eindeutig, der
    /// kurze ist der, den man hinschreibt.
    fn ist_untertabelle(&self) -> bool {
        if self.im_datensatz_ab.is_none() {
            return false;
        }
        self.untertabellen.contains(&self.pfad_ab_datensatz())
            || self.untertabellen.contains(&self.pfad_ab_zeile())
    }

    fn schluessel_index(&self) -> Option<usize> {
        let ab = self.im_datensatz_ab?;
        // Schlüssel gelten nur für die Haupttabelle; innerhalb einer
        // Wiederholgruppe hat das gleichnamige Feld eine andere Bedeutung.
        if self.stapel.len() != 1 {
            return None;
        }
        let rel: Vec<&Vec<u8>> = self.pfad.iter().skip(ab).collect();
        self.schluessel
            .iter()
            .position(|s| s.len() == rel.len() && s.iter().zip(rel.iter()).all(|(a, b)| a == *b))
    }

    fn nimm_feld(&mut self, pfad: Vec<u8>, werthash: u32) {
        let tabellenname = match self.stapel.last() {
            Some(z) => z.tabelle.clone(),
            None => return,
        };

        // Wiederholte Blätter bekommen ihre laufende Nummer. Ohne sie gingen
        // mehrere gleichnamige Felder derselben Zeile in einer Summe auf, und
        // ihr Tausch bliebe unsichtbar.
        let pfad = {
            let z = self.stapel.last_mut().expect("Zeile offen");
            let n = z.feldzaehler.entry(pfad.clone()).or_insert(0);
            let nummer = *n;
            *n += 1;
            mit_index(&pfad, nummer)
        };
        let nummer = self
            .tabellen
            .get_mut(&tabellenname)
            .map(|t| {
                t.felder += 1;
                t.feldnummer(&pfad)
            })
            .unwrap_or(0);

        if let Some(z) = self.stapel.last_mut() {
            z.folge.push(nummer);
            z.felder.push((pfad, werthash));
        }
    }

    /// Schließt die oberste Zeile ab.
    ///
    /// Eine Untertabellenzeile wird **nicht** sofort verrechnet, sondern an
    /// ihre Elternzeile gehängt: Deren Schlüssel kann noch fehlen, wenn das
    /// Schlüsselfeld hinter der Wiederholgruppe steht. Erst wenn die
    /// Haupttabellenzeile schließt, ist der Schlüssel vollständig und alle
    /// wartenden Zeilen werden rekursiv aufgelöst.
    fn schliesse_zeile(&mut self) {
        let zeile = match self.stapel.pop() {
            Some(z) => z,
            None => return,
        };

        if !zeile.tabelle.is_empty() {
            let wartend = WartendeZeile {
                tabelle: zeile.tabelle,
                nummer: zeile.nummer,
                felder: zeile.felder,
                folge: zeile.folge,
                kinder: zeile.wartende,
            };
            if let Some(eltern) = self.stapel.last_mut() {
                eltern.wartende.push(wartend);
            }
            return;
        }

        // Haupttabelle: Jetzt steht der Schlüssel fest.
        let schluessel = zeile.schluessel();
        self.datensaetze += 1;
        if !zeile.hat_vollstaendigen_schluessel() {
            self.ohne_schluessel += 1;
        }
        self.satzfolge = fnv_weiter(self.satzfolge, &schluessel);
        self.satzfolge = fnv_weiter(self.satzfolge, b"\x1e");
        if self.erste_schluessel.len() < ERSTE_SCHLUESSEL_HOECHSTENS {
            self.erste_schluessel.push(schluessel.clone());
        }

        let mut nachtrag: Vec<(Vec<u8>, u32)> = Vec::new();
        for w in zeile.wartende {
            self.loese_auf(w, &schluessel, &mut nachtrag);
        }

        let mut felder = zeile.felder;
        felder.extend(nachtrag);
        self.verrechne(&Vec::new(), &schluessel, &felder, &zeile.folge);
    }

    /// Verrechnet eine wartende Zeile, nun mit bekanntem Elternschlüssel, und
    /// steigt rekursiv in ihre eigenen wartenden Zeilen ab.
    ///
    /// `nachtrag` sammelt die Vermerke, die in der Elternzeile landen — ohne
    /// sie wäre eine fehlende Wiederholgruppe im Abdruck des Datensatzes
    /// unsichtbar.
    fn loese_auf(
        &mut self,
        w: WartendeZeile,
        eltern_schluessel: &[u8],
        nachtrag: &mut Vec<(Vec<u8>, u32)>,
    ) {
        let schluessel = unterschluessel(eltern_schluessel, w.nummer);

        let mut eigener_nachtrag: Vec<(Vec<u8>, u32)> = Vec::new();
        for kind in w.kinder {
            self.loese_auf(kind, &schluessel, &mut eigener_nachtrag);
        }

        let mut felder = w.felder;
        felder.extend(eigener_nachtrag);
        self.verrechne(&w.tabelle, &schluessel, &felder, &w.folge);

        let beitrag = fnv_weiter(fnv(&w.tabelle), &fnv(&schluessel).to_le_bytes());
        let mut kennung = w.tabelle.clone();
        kennung.extend_from_slice(b"[]");
        nachtrag.push((kennung, beitrag));
    }

    /// Trägt eine fertige Zeile in ihre Tabelle ein.
    fn verrechne(
        &mut self,
        tabelle: &Vec<u8>,
        schluessel: &[u8],
        felder: &[(Vec<u8>, u32)],
        folge: &Vec<u32>,
    ) {
        let sh = fnv(schluessel);
        let eimer = (sh as usize) % self.eimer;
        let gehoert_zu = Vec::new();
        let tiefe = tabelle.iter().filter(|c| **c == b'/').count();
        let eimerzahl = self.eimer;

        let t = self
            .tabellen
            .entry(tabelle.clone())
            .or_insert_with(|| Tabelle::neu(tabelle.clone(), gehoert_zu, tiefe, eimerzahl));

        t.zeilenzahl += 1;
        for (pfad, werthash) in felder {
            let mut h = fnv_weiter(FNV_ANFANG, &sh.to_le_bytes());
            h = fnv_weiter(h, pfad);
            h = fnv_weiter(h, &werthash.to_le_bytes());
            t.spalten.entry(pfad.clone()).or_default().nimm(h);
            t.zeilen[eimer].nimm(h);
        }
        if let Some(z) = t.anordnungen.get_mut(folge) {
            *z += 1;
        } else if t.anordnungen.len() < ANORDNUNGEN_HOECHSTENS {
            t.anordnungen.insert(folge.clone(), 1);
        }
    }

    /// Der Gesamtwert über alle Tabellen.
    pub fn gesamt(&self) -> u64 {
        let mut namen: Vec<&Vec<u8>> = self.tabellen.keys().collect();
        namen.sort();
        let mut h = FNV_ANFANG;
        for n in namen {
            let t = &self.tabellen[n];
            h = fnv_weiter(h, n);
            h = fnv_weiter(h, &t.wert().to_le_bytes());
        }
        let mut h2 = fnv_weiter(FNV_ANFANG, &self.satzfolge.to_le_bytes());
        h2 = fnv_weiter(h2, &self.datensaetze.to_le_bytes());
        ((h as u64) << 32) | h2 as u64
    }

    pub fn haupt(&self) -> &Tabelle {
        &self.tabellen[&Vec::new()]
    }
}

impl Beobachter for Zerlegung {
    fn element_beginn(&mut self, name: &[u8]) {
        if let Some(l) = self.hat_kinder.last_mut() {
            *l = true;
        }
        self.pfad.push(name.to_vec());
        self.hat_kinder.push(false);
        self.text_hash = FNV_ANFANG;
        self.text_hat_inhalt = false;
        self.text_wartend = 0;
        self.text_klartext.clear();

        if self.im_datensatz_ab.is_none() && self.passt_datensatz() {
            self.im_datensatz_ab = Some(self.pfad.len());
            self.stapel.push(OffeneZeile::neu(
                Vec::new(),
                self.pfad.len(),
                Vec::new(),
                0,
                self.schluessel.len(),
            ));
            return;
        }

        if self.ist_untertabelle() {
            let name_rel = self.pfad_ab_datensatz();
            let nummer = {
                let z = self.stapel.last_mut().expect("Zeile offen");
                let n = z.kinder.entry(name_rel.clone()).or_insert(0);
                *n += 1;
                *n - 1
            };
            let gehoert_zu = self
                .stapel
                .last()
                .map(|z| z.tabelle.clone())
                .unwrap_or_default();
            let tiefe = self.stapel.len();
            let eimer = self.eimer;
            self.tabellen
                .entry(name_rel.clone())
                .or_insert_with(|| Tabelle::neu(name_rel.clone(), gehoert_zu, tiefe, eimer));
            // Kein Elternschlüssel: Er steht erst fest, wenn die
            // Elternzeile schließt. Die Zeile wartet so lange.
            self.stapel.push(OffeneZeile::neu(
                name_rel,
                self.pfad.len(),
                Vec::new(),
                nummer,
                0,
            ));
        }
    }

    fn element_ende(&mut self) {
        let ist_blatt = !*self.hat_kinder.last().unwrap_or(&false);

        // Endet hier eine Zeile? Das ist der Fall, wenn das schließende
        // Element genau das ist, das die Zeile geöffnet hat.
        let zeilenende = matches!(self.stapel.last(), Some(z) if z.ab_tiefe == self.pfad.len());

        if ist_blatt && !self.stapel.is_empty() && !zeilenende {
            let werthash = if self.text_hat_inhalt {
                self.text_hash
            } else {
                fnv(b"\x00leer")
            };
            if let Some(k) = self.schluessel_index() {
                let klartext = if self.text_hat_inhalt {
                    self.text_klartext.clone()
                } else {
                    Vec::new()
                };
                if let Some(z) = self.stapel.last_mut() {
                    z.schluessel_text[k] = Some(klartext);
                }
            }
            let pfad = self.pfad_ab_zeile();
            self.nimm_feld(pfad, werthash);
        }

        if zeilenende {
            self.schliesse_zeile();
        }

        self.pfad.pop();
        self.hat_kinder.pop();
        self.text_hash = FNV_ANFANG;
        self.text_hat_inhalt = false;
        self.text_wartend = 0;
        self.text_klartext.clear();

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
        let werthash = if normal.is_empty() {
            fnv(b"\x00leer")
        } else {
            fnv(&normal)
        };
        self.nimm_feld(pfad, werthash);
    }

    fn text(&mut self, teil: &[u8]) {
        for &c in teil {
            if c.is_ascii_whitespace() {
                if self.text_hat_inhalt {
                    self.text_wartend += 1;
                }
                continue;
            }
            if self.text_wartend > 0 {
                self.text_hash = fnv_weiter(self.text_hash, b" ");
                if self.text_klartext.len() < 256 {
                    self.text_klartext.push(b' ');
                }
                self.text_wartend = 0;
            }
            self.text_hash = fnv_weiter(self.text_hash, &[c]);
            if self.text_klartext.len() < 256 {
                self.text_klartext.push(c);
            }
            self.text_hat_inhalt = true;
        }
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

// Aggregat::nimm ist im Abdruck-Modul privat; hier die gleiche Rechnung.
trait Nimm {
    fn nimm(&mut self, h: u32);
}
impl Nimm for Aggregat {
    fn nimm(&mut self, h: u32) {
        self.summe = self.summe.wrapping_add(h);
        self.xor ^= h;
        self.anzahl += 1;
    }
}
