//! Verschachtelung als eigene Tabellen.
//!
//! Bis hierher war ein Datensatz eine flache Menge von Feldern: Alle sechs
//! `INVESTMENTIDENTIFIER` eines Investments landeten im selben Feldpfad, ihre
//! Werte in derselben Summe. Werden zwei davon untereinander vertauscht, ändert
//! sich nichts — der Abgleich ist an dieser Stelle blind.
//!
//! Deshalb wird jede Wiederholgruppe zu einer **eigenen Tabelle**, rekursiv,
//! mit einem Fremdschlüssel auf den übergeordneten Datensatz. Auf jede Tabelle
//! wird dasselbe Verfahren angewandt: Spaltenrand, Zeilenrand, Ränder zu einem
//! Wert. Genau so, wie ein Ladeprogramm die Nachricht in Kopf- und
//! Positionstabellen zerlegt.
//!
//! ```text
//!   INVESTMENT                     ← Haupttabelle, Schlüssel: ISIN
//!     ├── GENERALDATA/…            ← Felder direkt am Datensatz
//!     ├── INVESTMENTIDENTIFIER     ← Untertabelle, Fremdschlüssel + lfd. Nr.
//!     └── CONDITION                ← Untertabelle
//!           └── STAFFEL            ← und deren Untertabelle
//! ```
//!
//! Die laufende Nummer als Zeilenschlüssel macht die Reihenfolge innerhalb
//! einer Wiederholgruppe bedeutsam. Das ist dieselbe Entscheidung wie bei der
//! Satzfolge: Das Werkzeug stellt fest und bewertet nicht.

use crate::abdruck::Aggregat;
use std::collections::HashMap;

const FNV_ANFANG: u32 = 2166136261;
const FNV_PRIM: u32 = 16777619;

#[inline]
pub fn fnv_weiter(h: u32, daten: &[u8]) -> u32 {
    let mut h = h;
    for &b in daten {
        h ^= b as u32;
        h = h.wrapping_mul(FNV_PRIM);
    }
    h
}

#[inline]
pub fn fnv(daten: &[u8]) -> u32 {
    fnv_weiter(FNV_ANFANG, daten)
}

/// Eine Tabelle — die Haupttabelle oder eine Wiederholgruppe.
pub struct Tabelle {
    /// Pfad des Elements, das eine Zeile bildet. Leer für die Haupttabelle.
    pub name: Vec<u8>,
    /// Pfad der übergeordneten Tabelle, für die Zugehörigkeit.
    pub gehoert_zu: Vec<u8>,
    /// Wie tief die Verschachtelung reicht (0 = Haupttabelle).
    pub tiefe: usize,

    pub spalten: HashMap<Vec<u8>, Aggregat>,
    pub zeilen: Vec<Aggregat>,
    pub zeilenzahl: u64,
    pub felder: u64,

    /// Indextabelle und Feldfolgen, wie bei der Haupttabelle.
    pub feld_index: Vec<Vec<u8>>,
    pub feld_nummer: HashMap<Vec<u8>, u32>,
    pub anordnungen: HashMap<Vec<u32>, u64>,
}

impl Tabelle {
    pub fn neu(name: Vec<u8>, gehoert_zu: Vec<u8>, tiefe: usize, eimer: usize) -> Self {
        Tabelle {
            name,
            gehoert_zu,
            tiefe,
            spalten: HashMap::new(),
            zeilen: vec![Aggregat::default(); eimer],
            zeilenzahl: 0,
            felder: 0,
            feld_index: Vec::new(),
            feld_nummer: HashMap::new(),
            anordnungen: HashMap::new(),
        }
    }

    pub fn feldnummer(&mut self, pfad: &[u8]) -> u32 {
        match self.feld_nummer.get(pfad) {
            Some(n) => *n,
            None => {
                let n = self.feld_index.len() as u32;
                self.feld_index.push(pfad.to_vec());
                self.feld_nummer.insert(pfad.to_vec(), n);
                n
            }
        }
    }

    /// Fasst die Tabelle zu einem Wert zusammen — dasselbe Verfahren wie für
    /// den Gesamtabdruck, nur auf diese Tabelle beschränkt.
    pub fn wert(&self) -> u32 {
        let mut summe: u32 = 0;
        let mut xor: u32 = 0;
        for w in self.spalten.values() {
            summe = summe.wrapping_add(w.summe);
            xor ^= w.xor;
        }
        let mut h = fnv_weiter(FNV_ANFANG, &summe.to_le_bytes());
        h = fnv_weiter(h, &xor.to_le_bytes());
        h = fnv_weiter(h, &self.zeilenzahl.to_le_bytes());
        h = fnv_weiter(h, &(self.spalten.len() as u64).to_le_bytes());
        h
    }

    /// Nur die Feldbezeichnungen dieser Tabelle.
    pub fn struktur(&self) -> u32 {
        let mut pfade: Vec<&Vec<u8>> = self.spalten.keys().collect();
        pfade.sort();
        let mut h = FNV_ANFANG;
        for p in pfade {
            h = fnv_weiter(h, p);
            h = fnv_weiter(h, b"\x1f");
        }
        h
    }
}

/// Eine abgeschlossene Zeile, die noch auf den Schlüssel ihres Elternteils
/// wartet.
///
/// Nötig, weil das Schlüsselfeld eines Datensatzes **nach** einer
/// Wiederholgruppe stehen kann. Würde die Gruppenzeile beim Öffnen zugeordnet,
/// bekäme sie einen leeren Elternschlüssel — und alle Zeilen aller Datensätze
/// landeten unter demselben Platzhalter. Die Zugehörigkeit wäre verloren, ohne
/// dass es auffiele.
pub struct WartendeZeile {
    pub tabelle: Vec<u8>,
    pub nummer: u32,
    pub felder: Vec<(Vec<u8>, u32)>,
    pub folge: Vec<u32>,
    /// Zeilen tieferer Gruppen, die ihrerseits auf diese hier warten.
    pub kinder: Vec<WartendeZeile>,
}

/// Eine offene Zeile während des Scannens.
pub struct OffeneZeile {
    /// Zu welcher Tabelle die Zeile gehört.
    pub tabelle: Vec<u8>,
    /// Pfadtiefe, bei der die Zeile begonnen hat — für das Erkennen des Endes.
    pub ab_tiefe: usize,
    /// Fremdschlüssel: der Zeilenschlüssel der übergeordneten Zeile.
    pub eltern_schluessel: Vec<u8>,
    /// Laufende Nummer innerhalb der übergeordneten Zeile.
    pub nummer: u32,
    /// Gesammelte Felder, bis der Schlüssel feststeht.
    pub felder: Vec<(Vec<u8>, u32)>,
    /// Feldfolge als Nummern.
    pub folge: Vec<u32>,
    /// Klartext der Schlüsselfelder, soweit vorhanden.
    pub schluessel_text: Vec<Option<Vec<u8>>>,
    /// Wie oft welches Untertabellen-Element hier schon vorkam.
    pub kinder: HashMap<Vec<u8>, u32>,
    /// Abgeschlossene Zeilen tieferer Gruppen, die auf den Schlüssel dieser
    /// Zeile warten.
    pub wartende: Vec<WartendeZeile>,
    /// Wie oft welcher Blattpfad in dieser Zeile schon vorkam.
    pub feldzaehler: HashMap<Vec<u8>, u32>,
}

impl OffeneZeile {
    pub fn neu(
        tabelle: Vec<u8>,
        ab_tiefe: usize,
        eltern_schluessel: Vec<u8>,
        nummer: u32,
        schluesselzahl: usize,
    ) -> Self {
        OffeneZeile {
            tabelle,
            ab_tiefe,
            eltern_schluessel,
            nummer,
            felder: Vec::new(),
            folge: Vec::new(),
            schluessel_text: vec![None; schluesselzahl],
            kinder: HashMap::new(),
            wartende: Vec::new(),
            feldzaehler: HashMap::new(),
        }
    }

    /// Der Zeilenschlüssel der Haupttabelle: die Schlüsselfelder, verbunden.
    pub fn schluessel(&self) -> Vec<u8> {
        self.schluessel_text
            .iter()
            .map(|t| t.clone().unwrap_or_else(|| b"?".to_vec()))
            .collect::<Vec<_>>()
            .join(&b'|')
    }

    pub fn hat_vollstaendigen_schluessel(&self) -> bool {
        !self.schluessel_text.is_empty() && self.schluessel_text.iter().all(|t| t.is_some())
    }
}

/// Schlüssel einer Untertabellenzeile: Fremdschlüssel und laufende Nummer.
///
/// Die Nummer ist nötig, weil eine Wiederholgruppe von sich aus keinen
/// Schlüssel hat. Sie macht die Reihenfolge innerhalb der Gruppe bedeutsam —
/// dieselbe Entscheidung wie bei der Satzfolge.
pub fn unterschluessel(eltern: &[u8], nummer: u32) -> Vec<u8> {
    let mut s = eltern.to_vec();
    s.push(b'#');
    s.extend_from_slice(nummer.to_string().as_bytes());
    s
}

/// Höchster Index, der einem wiederholten Blatt angehängt wird. Darüber
/// hinaus landen alle weiteren in einem Sammeleintrag — sonst bekäme eine
/// Datei mit tausend Wiederholungen tausend Spalten.
pub const INDEX_HOECHSTENS: u32 = 64;

/// Hängt einem wiederholten Blattpfad seine laufende Nummer an.
///
/// Das erste Vorkommen bleibt unverändert: Für Felder, die nur einmal
/// dastehen — der Normalfall —, ändert sich dadurch nichts. Erst ab dem
/// zweiten wird die Position bedeutsam, und genau dort ist sie es auch:
/// `<TELEFON>111</TELEFON><TELEFON>222</TELEFON>` sind zwei verschiedene
/// Angaben, und ihr Tausch ist eine Änderung.
pub fn mit_index(pfad: &[u8], nummer: u32) -> Vec<u8> {
    if nummer == 0 {
        return pfad.to_vec();
    }
    let mut p = pfad.to_vec();
    p.push(b'[');
    if nummer <= INDEX_HOECHSTENS {
        p.extend_from_slice(nummer.to_string().as_bytes());
    } else {
        p.extend_from_slice(b"...");
    }
    p.push(b']');
    p
}
