//! Der Fingerabdruck: zwei Ränder statt einer Matrix.
//!
//! Für jedes Blatt wird ein Wert gebildet, in den drei Dinge eingehen — der
//! Pfad, der Schlüssel des Datensatzes und der Inhalt. Dieser Wert wird in
//! zwei Richtungen verrechnet:
//!
//! * **Spaltenrand** je Pfad — findet, *welches Feld* abweicht.
//! * **Zeilenrand** je Eimer des Schlüssels — findet, *welcher Datensatz*.
//!
//! Weichen beide ab, kreuzen sie sich in der Zelle. Der Rand kostet N + M
//! Werte statt N × M einer vollen Matrix; gemessen sind das bei 4096 Eimern
//! und 58 Spalten 83 kB gegen 3,8 MB.
//!
//! Rechnerisch ist der Rand ab zwei Fehlern mehrdeutig — k Fehler ergeben k²
//! Kandidaten, von denen k echt sind. In der Anwendung fällt das nicht ins
//! Gewicht, weil die Kandidaten anschließend nachgelesen werden
//! (`einzelheiten`): Der Rand grenzt ein, der zweite Durchlauf entscheidet.

use crate::scanner::Beobachter;
use std::collections::HashMap;

const FNV_ANFANG: u32 = 2166136261;
const FNV_PRIM: u32 = 16777619;

/// So viele verschiedene Feldfolgen werden aufbewahrt. Ein einheitlich
/// aufgebauter Bestand hat eine, ein Bestand mit optionalen Feldern eine
/// Handvoll. Darüber hinaus lautet die Aussage ohnehin „uneinheitlich", und
/// der Abdruck soll unabhängig von der Dateigröße klein bleiben.
const ANORDNUNGEN_HOECHSTENS: usize = 1000;

/// So viele Schlüssel werden im Klartext gemerkt, um die erste
/// Abweichung in der Satzfolge benennen zu können. Mehr braucht es nicht:
/// Wer die erste Stelle kennt, findet den Rest selbst.
const ERSTE_SCHLUESSEL_HOECHSTENS: usize = 5000;

#[inline]
fn fnv_weiter(h: u32, daten: &[u8]) -> u32 {
    let mut h = h;
    for &b in daten {
        h ^= b as u32;
        h = h.wrapping_mul(FNV_PRIM);
    }
    h
}

#[inline]
fn fnv(daten: &[u8]) -> u32 {
    fnv_weiter(FNV_ANFANG, daten)
}

/// Aggregat eines Randfelds. Summe und XOR sind beide kommutativ, reagieren
/// aber auf verschiedene Fehlerbilder — zusammen sind sie deutlich schwerer
/// zufällig zu treffen als einer allein.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Aggregat {
    pub summe: u32,
    pub xor: u32,
    pub anzahl: u32,
}

impl Aggregat {
    #[inline]
    fn nimm(&mut self, h: u32) {
        self.summe = self.summe.wrapping_add(h);
        self.xor ^= h;
        self.anzahl += 1;
    }
}

pub struct Abdruck {
    // ---- Aufbau
    /// Pfad des Elements, das einen Datensatz bildet, in Segmenten.
    /// Leer heißt: das ganze Dokument ist ein Datensatz.
    datensatz: Vec<Vec<u8>>,
    /// Pfad zum Schlüsselfeld, relativ zum Datensatz. Mehrere ergeben einen
    /// zusammengesetzten Schlüssel.
    schluessel: Vec<Vec<Vec<u8>>>,
    eimer: usize,

    // ---- laufender Zustand
    pfad: Vec<Vec<u8>>,
    /// Je Ebene: Hat dieses Element Kindelemente? Nur Blätter tragen Werte.
    hat_kinder: Vec<bool>,
    /// Laufender Texthash des aktuellen Elements, über Blockgrenzen fortgeführt.
    text_hash: u32,
    text_hat_inhalt: bool,
    /// Zurückgehaltene Leerzeichen: Einrückung darf den Abdruck nicht
    /// verändern, deshalb werden Leerzeichen erst mitgehasht, wenn ihnen
    /// wieder Inhalt folgt.
    text_wartend: usize,
    /// Derselbe Text, den der Hash sieht — nur als Bytes, für die
    /// Satzfolge. Nur Schlüsselfelder brauchen ihn, aber welches Element
    /// ein Schlüssel ist, steht erst beim Schließen fest.
    text_klartext: Vec<u8>,

    /// Ab welcher Tiefe wir uns in einem Datensatz befinden (None = außerhalb).
    im_datensatz_ab: Option<usize>,
    /// Felder des laufenden Datensatzes, bis sein Schlüssel feststeht.
    /// Ein Datensatz ist klein; das Zwischenlagern kostet nichts.
    laufende_felder: Vec<(Vec<u8>, u32)>,
    laufender_schluessel: Vec<Option<u32>>,
    /// Schlüssel des laufenden Datensatzes im Klartext, für die Satzfolge.
    laufender_schluessel_text: Vec<Option<Vec<u8>>>,

    /// Ebene 2 — die Indextabelle: Feldbezeichnungen in der Reihenfolge ihres
    /// ersten Auftretens. Sie ist die gemeinsame Sprache, in der sich
    /// Anordnungen ausdrücken lassen.
    pub feld_index: Vec<Vec<u8>>,
    feld_nummer: HashMap<Vec<u8>, u32>,

    /// Feldfolge des laufenden Datensatzes, als Nummern aus der Indextabelle.
    laufende_folge: Vec<u32>,
    /// Alle vorkommenden Feldfolgen mit ihrer Häufigkeit.
    ///
    /// Als Folgen und nicht als Hash: Ein Hash sagt nur, *dass* die Anordnung
    /// abweicht. Die Folgen sagen, *an welcher Stelle* — und das ist der
    /// Unterschied zwischen einem Befund und einer Diagnose.
    pub anordnungen: HashMap<Vec<u32>, u64>,

    /// Hash über die Schlüssel in **Dokumentreihenfolge** — bewusst nicht
    /// kommutativ, im Gegensatz zu allem anderen hier.
    ///
    /// Die Ränder sollen eine Umsortierung überstehen, weil dieselben Sätze
    /// dieselben Sätze bleiben. Ob eine geänderte Satzfolge hinnehmbar ist,
    /// hängt aber vom Verarbeitungsweg ab und nicht von der Annahme dieses
    /// Werkzeugs. Es stellt sie deshalb fest und überlässt die Bewertung dem,
    /// der die Strecke kennt.
    satzfolge: u32,
    /// Die ersten Schlüssel im Klartext, um die erste Abweichung benennen zu
    /// können statt nur „irgendwo anders sortiert".
    pub erste_schluessel: Vec<Vec<u8>>,

    // ---- Ergebnis
    pub spalten: HashMap<Vec<u8>, Aggregat>,
    pub zeilen: Vec<Aggregat>,
    pub datensaetze: u64,
    pub blaetter: u64,
    /// Datensätze, in denen kein Schlüsselfeld gefunden wurde. Das ist ein
    /// Befund für sich: Ohne Schlüssel ist die Zeilenachse blind.
    pub ohne_schluessel: u64,
}

/// Ein einzelner Wert über den gesamten Bestand — vorlesbar, mailbar, in ein
/// Ticket schreibbar.
///
/// Wichtig ist die Asymmetrie seiner Aussage:
///
/// * **verschieden** heißt sicher verschieden,
/// * **gleich** heißt sehr wahrscheinlich gleich, aber nicht beweisbar.
///
/// Denn Summe und XOR sind kommutativ: Zwei Änderungen könnten sich
/// rechnerisch aufheben. Wer das ausnutzen will, muss die Hashfunktion
/// brechen; bei Versehen ist es praktisch ausgeschlossen. Trotzdem darf aus
/// „Gesamtwert gleich" nie „geprüft und in Ordnung" werden — dafür sind die
/// Ränder da.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Gesamt {
    pub wert: u64,
    /// Summe über die Spaltenachse und über die Zeilenachse. Beide müssen
    /// übereinstimmen — jeder Zellwert geht in genau eine Spalte und genau
    /// eine Zeile ein. Weichen sie ab, ist der Abdruck selbst fehlerhaft.
    pub ueber_spalten: u32,
    pub ueber_zeilen: u32,
    pub stimmig: bool,

    /// Nur die Feldbezeichnungen: welche Pfade kommen überhaupt vor.
    /// Weicht dieser Wert ab, hat sich das Schema geändert — und alles
    /// Weitere ist Folge davon.
    pub struktur: u32,
    /// Nur die Anordnung: in welcher Folge die Felder je Datensatz stehen,
    /// ohne ihre Werte. Trennt eine Umstellung von einer Wertänderung.
    pub anordnung: u32,
    /// Nur die Inhalte. Der Wert, der übrig bleibt, wenn Struktur und
    /// Anordnung übereinstimmen.
    pub ausprägungen: u32,
    /// Nur die Reihenfolge der Datensätze. Nicht kommutativ — eine
    /// Umsortierung ist hier sichtbar und sonst nirgends.
    pub satzfolge: u32,
}

impl Abdruck {
    /// Fasst beide Ränder zu einem Wert zusammen.
    pub fn gesamt(&self) -> Gesamt {
        let mut s_summe: u32 = 0;
        let mut s_xor: u32 = 0;
        for w in self.spalten.values() {
            s_summe = s_summe.wrapping_add(w.summe);
            s_xor ^= w.xor;
        }
        let mut z_summe: u32 = 0;
        let mut z_xor: u32 = 0;
        for z in &self.zeilen {
            z_summe = z_summe.wrapping_add(z.summe);
            z_xor ^= z.xor;
        }

        // In den Gesamtwert gehen alle drei Ebenen ein — Struktur, Anordnung
        // und Ausprägungen — sowie die Zählwerte.
        //
        // Die Anordnung ausdrücklich mit: Die Ränder kennen nur Pfad,
        // Schlüssel und Wert, nicht die Position. Zwei Dateien mit
        // umgestellten Feldern hätten ohne sie denselben Gesamtwert, obwohl
        // sich das Dokument geändert hat.
        let mut h = fnv_weiter(FNV_ANFANG, &s_summe.to_le_bytes());
        h = fnv_weiter(h, &s_xor.to_le_bytes());
        h = fnv_weiter(h, &self.struktur_wert().to_le_bytes());
        h = fnv_weiter(h, &self.anordnungs_wert().to_le_bytes());
        h = fnv_weiter(h, &self.satzfolge.to_le_bytes());
        h = fnv_weiter(h, &self.datensaetze.to_le_bytes());
        h = fnv_weiter(h, &self.blaetter.to_le_bytes());
        h = fnv_weiter(h, &(self.spalten.len() as u64).to_le_bytes());

        let mut h2 = fnv_weiter(FNV_ANFANG, &z_summe.to_le_bytes());
        h2 = fnv_weiter(h2, &z_xor.to_le_bytes());
        h2 = fnv_weiter(h2, &self.blaetter.to_le_bytes());

        Gesamt {
            wert: ((h as u64) << 32) | h2 as u64,
            ueber_spalten: s_summe,
            ueber_zeilen: z_summe,
            stimmig: s_summe == z_summe && s_xor == z_xor,
            struktur: self.struktur_wert(),
            anordnung: self.anordnungs_wert(),
            ausprägungen: s_summe,
            satzfolge: self.satzfolge,
        }
    }

    /// Nur die Feldbezeichnungen, in fester Ordnung — damit die Reihenfolge,
    /// in der die Pfade zufällig in der Streutabelle liegen, nichts ändert.
    pub fn struktur_wert(&self) -> u32 {
        let mut pfade: Vec<&Vec<u8>> = self.spalten.keys().collect();
        pfade.sort();
        let mut h = FNV_ANFANG;
        for p in pfade {
            h = fnv_weiter(h, p);
            h = fnv_weiter(h, b"\x1f");
        }
        h
    }

    /// Die Menge der vorkommenden Anordnungen, sortiert — die Reihenfolge der
    /// Datensätze soll auch hier keine Rolle spielen.
    ///
    /// Gehasht werden die Feldbezeichnungen, nicht ihre Nummern: Die
    /// Indextabelle zweier Dateien kann verschieden nummeriert sein, obwohl
    /// die Anordnung dieselbe ist. Über die Namen ist der Wert vergleichbar.
    pub fn anordnungs_wert(&self) -> u32 {
        let mut folgen: Vec<Vec<u8>> = self
            .anordnungen
            .keys()
            .map(|f| {
                let mut b = Vec::new();
                for n in f {
                    if let Some(name) = self.feld_index.get(*n as usize) {
                        b.extend_from_slice(name);
                    }
                    b.push(0x1f);
                }
                b
            })
            .collect();
        folgen.sort();
        let mut h = FNV_ANFANG;
        for f in folgen {
            h = fnv_weiter(h, &f);
            h = fnv_weiter(h, b"\x1e");
        }
        h
    }

    /// Die häufigste Feldfolge — der Regelaufbau eines Datensatzes.
    pub fn haupt_anordnung(&self) -> Option<(&Vec<u32>, u64)> {
        self.anordnungen.iter().max_by_key(|(_, n)| **n).map(|(f, n)| (f, *n))
    }

    pub fn feldname(&self, nummer: u32) -> &[u8] {
        self.feld_index
            .get(nummer as usize)
            .map(|v| v.as_slice())
            .unwrap_or(b"?")
    }
}

impl Abdruck {
    pub fn neu(datensatz: &str, schluessel: &[&str], eimer: usize) -> Self {
        Abdruck {
            datensatz: zerlege_pfad(datensatz),
            schluessel: schluessel.iter().map(|s| zerlege_pfad(s)).collect(),
            eimer: eimer.max(1),
            pfad: Vec::new(),
            hat_kinder: Vec::new(),
            text_hash: FNV_ANFANG,
            text_hat_inhalt: false,
            text_wartend: 0,
            text_klartext: Vec::new(),
            im_datensatz_ab: None,
            laufende_felder: Vec::new(),
            laufender_schluessel: vec![None; schluessel.len()],
            laufender_schluessel_text: vec![None; schluessel.len()],
            feld_index: Vec::new(),
            feld_nummer: HashMap::new(),
            laufende_folge: Vec::new(),
            anordnungen: HashMap::new(),
            satzfolge: FNV_ANFANG,
            erste_schluessel: Vec::new(),
            spalten: HashMap::new(),
            zeilen: vec![Aggregat::default(); eimer.max(1)],
            datensaetze: 0,
            blaetter: 0,
            ohne_schluessel: 0,
        }
    }

    /// Pfad ab dem Datensatz, mit `/` verbunden — das ist der Spaltenname.
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

    /// Wird nach dem Entfernen des geschlossenen Elements geprüft.
    ///
    /// `im_datensatz_ab` ist die Pfadtiefe *einschließlich* des
    /// Datensatz-Elements. Der Datensatz ist also erst zu Ende, wenn dieses
    /// Element selbst entfernt wurde — die Tiefe ist dann um eins kleiner.
    /// Mit `==` statt `+ 1 ==` schlösse schon das erste Feld den Datensatz,
    /// und alle weiteren Felder fielen unter den Tisch.
    fn ist_datensatz_ende(&self) -> bool {
        matches!(self.im_datensatz_ab, Some(ab) if self.pfad.len() + 1 == ab)
    }

    /// Prüft, ob der aktuelle Pfad auf den Datensatz passt.
    fn passt_datensatz(&self) -> bool {
        if self.datensatz.is_empty() {
            return false;
        }
        self.pfad.len() >= self.datensatz.len()
            && self.pfad[self.pfad.len() - self.datensatz.len()..] == self.datensatz[..]
    }

    fn schliesse_datensatz(&mut self) {
        self.datensaetze += 1;

        // Satzfolge: Reihenfolge zählt, deshalb fortlaufend gehasht statt
        // aufsummiert. Ohne Schlüssel wird die laufende Nummer eingesetzt —
        // dann sagt die Prüfung nur noch, dass gleich viele Sätze da sind.
        let schluessel_text: Vec<u8> = self
            .laufender_schluessel_text
            .iter()
            .map(|t| t.clone().unwrap_or_else(|| b"?".to_vec()))
            .collect::<Vec<_>>()
            .join(&b'|');
        self.satzfolge = fnv_weiter(self.satzfolge, &schluessel_text);
        self.satzfolge = fnv_weiter(self.satzfolge, b"\x1e");
        if self.erste_schluessel.len() < ERSTE_SCHLUESSEL_HOECHSTENS {
            self.erste_schluessel.push(schluessel_text);
        }

        // Neue Folgen nur bis zur Grenze aufnehmen; bekannte immer zählen.
        // Ohne die Grenze könnte eine Datei mit lauter verschiedenen
        // Feldfolgen den Speicher füllen — der Abdruck soll konstant bleiben.
        let folge = std::mem::take(&mut self.laufende_folge);
        if let Some(z) = self.anordnungen.get_mut(&folge) {
            *z += 1;
        } else if self.anordnungen.len() < ANORDNUNGEN_HOECHSTENS {
            self.anordnungen.insert(folge, 1);
        }

        // Der Schlüssel: alle Teilschlüssel zu einem Wert verbunden.
        let mut sh = FNV_ANFANG;
        let mut vollstaendig = true;
        for teil in &self.laufender_schluessel {
            match teil {
                Some(h) => sh = fnv_weiter(sh, &h.to_le_bytes()),
                None => vollstaendig = false,
            }
        }
        if !vollstaendig || self.laufender_schluessel.is_empty() {
            self.ohne_schluessel += 1;
            // Ohne Schlüssel wird die laufende Nummer genommen, damit der
            // Datensatz wenigstens einem Eimer zufällt — die Zeilenachse ist
            // dann aber nicht mehr reihenfolgeunabhängig, und der Bericht
            // sagt das.
            sh = fnv_weiter(FNV_ANFANG, &self.datensaetze.to_le_bytes());
        }
        let eimer = (sh as usize) % self.eimer;

        for (pfad, werthash) in std::mem::take(&mut self.laufende_felder) {
            // Schlüssel, Pfad und Wert gemeinsam — damit derselbe Wert an
            // anderer Stelle nicht denselben Beitrag liefert.
            let mut h = fnv_weiter(FNV_ANFANG, &sh.to_le_bytes());
            h = fnv_weiter(h, &pfad);
            h = fnv_weiter(h, &werthash.to_le_bytes());

            self.spalten.entry(pfad).or_default().nimm(h);
            self.zeilen[eimer].nimm(h);
        }

        for t in &mut self.laufender_schluessel {
            *t = None;
        }
        for t in &mut self.laufender_schluessel_text {
            *t = None;
        }
    }

    /// Ist der zuletzt geschlossene Pfad ein Schlüsselfeld? Liefert dessen Index.
    fn schluessel_index(&self) -> Option<usize> {
        let ab = self.im_datensatz_ab?;
        let rel: Vec<&Vec<u8>> = self.pfad.iter().skip(ab).collect();
        self.schluessel.iter().position(|s| {
            s.len() == rel.len() && s.iter().zip(rel.iter()).all(|(a, b)| a == *b)
        })
    }
}

impl Beobachter for Abdruck {
    fn element_beginn(&mut self, name: &[u8]) {
        if let Some(letzte) = self.hat_kinder.last_mut() {
            *letzte = true;
        }
        self.pfad.push(name.to_vec());
        self.hat_kinder.push(false);
        self.text_hash = FNV_ANFANG;
        self.text_hat_inhalt = false;
        self.text_wartend = 0;
        self.text_klartext.clear();

        if self.im_datensatz_ab.is_none() && self.passt_datensatz() {
            self.im_datensatz_ab = Some(self.pfad.len());
        }
    }

    fn element_ende(&mut self) {
        // Nur Blätter tragen Werte. Ein Element mit Kindern hat höchstens
        // Einrückung als Text.
        let ist_blatt = *self.hat_kinder.last().unwrap_or(&false) == false;

        if ist_blatt && self.im_datensatz_ab.is_some() {
            self.blaetter += 1;
            let werthash = if self.text_hat_inhalt {
                self.text_hash
            } else {
                // Leeres Element — ein eigener, von "fehlt" unterscheidbarer
                // Zustand. `<LISTED/>` ist etwas anderes als kein LISTED.
                fnv(b"\x00leer")
            };
            let pfad = self.pfad_ab_datensatz();

            if let Some(k) = self.schluessel_index() {
                self.laufender_schluessel[k] = Some(werthash);
                // Für die Satzfolge wird der Klartext gebraucht: Ein Hash
                // sagt nur, dass anders sortiert wurde, nicht ab welchem Satz.
                self.laufender_schluessel_text[k] = Some(if self.text_hat_inhalt {
                    self.text_klartext.clone()
                } else {
                    Vec::new()
                });
            }
            // Die Anordnung wird ohne die Werte fortgeschrieben — sonst
            // ließe sich eine Umstellung nicht von einer Wertänderung
            // unterscheiden.
            let nummer = match self.feld_nummer.get(&pfad) {
                Some(n) => *n,
                None => {
                    let n = self.feld_index.len() as u32;
                    self.feld_index.push(pfad.clone());
                    self.feld_nummer.insert(pfad.clone(), n);
                    n
                }
            };
            self.laufende_folge.push(nummer);
            self.laufende_felder.push((pfad, werthash));
        }

        self.pfad.pop();
        self.hat_kinder.pop();
        self.text_hash = FNV_ANFANG;
        self.text_hat_inhalt = false;
        self.text_wartend = 0;
        self.text_klartext.clear();

        if self.ist_datensatz_ende() {
            self.im_datensatz_ab = None;
            self.schliesse_datensatz();
        }
    }

    /// Attribute zählen wie Felder, mit `@` im Pfad — `<Amt Ccy="EUR">` wird
    /// zu `.../Amt@Ccy`. Ohne sie verglichen man bei ISO 20022 Beträge ohne
    /// ihre Währung.
    fn attribut(&mut self, name: &[u8], wert: &[u8]) {
        if self.im_datensatz_ab.is_none() {
            return;
        }
        let mut pfad = self.pfad_ab_datensatz();
        pfad.push(b'@');
        pfad.extend_from_slice(name);

        let normal = normalisiere(wert);
        let werthash = if normal.is_empty() {
            fnv(b"\x00leer")
        } else {
            fnv(&normal)
        };
        self.blaetter += 1;

        let nummer = match self.feld_nummer.get(&pfad) {
            Some(n) => *n,
            None => {
                let n = self.feld_index.len() as u32;
                self.feld_index.push(pfad.clone());
                self.feld_nummer.insert(pfad.clone(), n);
                n
            }
        };
        self.laufende_folge.push(nummer);
        self.laufende_felder.push((pfad, werthash));
    }

    fn text(&mut self, teil: &[u8]) {
        for &c in teil {
            if c.is_ascii_whitespace() {
                // Zurückhalten: Wenn danach nichts mehr kommt, war es
                // Einrückung und zählt nicht.
                if self.text_hat_inhalt {
                    self.text_wartend += 1;
                }
                continue;
            }
            // Zurückgehaltene Leerzeichen gehören doch dazu — als ein
            // einzelnes Leerzeichen, damit unterschiedliche Einrückung
            // innerhalb eines Werts nichts ändert.
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

/// Dieselbe Zusammenfassung, die auch der laufende Texthash vornimmt:
/// Leerraum an den Rändern weg, innen zu einem Leerzeichen. Sonst hinge der
/// Abdruck an der Formatierung.
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
