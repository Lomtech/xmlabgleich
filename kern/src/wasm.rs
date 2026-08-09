//! Schnittstelle zum Browser.
//!
//! Bewusst ohne `wasm-bindgen`: Es wandern nur Bytes und Zahlen über die
//! Grenze, dafür braucht es keine Bindungsschicht. Das Modul bleibt dadurch
//! klein genug, um in einer einzelnen HTML-Datei mitzureisen — und genau das
//! ist der Zweck: Die Datei soll sich verschicken lassen und per Doppelklick
//! laufen, ohne Installation.
//!
//! Ablauf: JavaScript schreibt einen Block der Datei in den Arbeitspuffer,
//! ruft `…_block` auf, und liest am Ende ein JSON-Ergebnis aus dem
//! Ausgabepuffer.

use crate::abdruck::Abdruck;
use crate::einzelheiten::Einzelheiten;
use crate::erkundung::Erkundung;
use crate::scanner::Scanner;

/// Größter Block, den JavaScript am Stück hereinreicht.
const PUFFER_GROESSE: usize = 8 * 1024 * 1024;
/// Platz für das Ergebnis. Bei 20.000 Pfaden zu je rund 100 Byte reicht das
/// mit weitem Abstand.
const AUSGABE_GROESSE: usize = 16 * 1024 * 1024;

static mut PUFFER: [u8; PUFFER_GROESSE] = [0; PUFFER_GROESSE];
static mut AUSGABE: [u8; AUSGABE_GROESSE] = [0; AUSGABE_GROESSE];
static mut AUSGABE_LEN: usize = 0;

static mut SCANNER: Option<Scanner> = None;
static mut ERKUNDUNG: Option<Erkundung> = None;
static mut ABDRUCK: Option<Abdruck> = None;
static mut EINZELHEITEN: Option<Einzelheiten> = None;
/// Zwischenlager für die Suchvorgabe des zweiten Durchlaufs.
static mut SUCH_DATENSATZ: String = String::new();
static mut SUCH_SCHLUESSEL: Vec<String> = Vec::new();
static mut SUCH_EIMER_GESAMT: u32 = 4096;
static mut SUCH_EIMER: Vec<usize> = Vec::new();
static mut SUCH_PFADE: Vec<Vec<u8>> = Vec::new();
/// Aus welcher Tabelle die Einzelheiten stammen sollen.
static mut SUCH_TABELLE: Option<Vec<u8>> = None;

#[no_mangle]
pub extern "C" fn puffer_ptr() -> *mut u8 {
    &raw mut PUFFER as *mut u8
}

#[no_mangle]
pub extern "C" fn puffer_groesse() -> u32 {
    PUFFER_GROESSE as u32
}

#[no_mangle]
pub extern "C" fn ausgabe_ptr() -> *mut u8 {
    &raw mut AUSGABE as *mut u8
}

#[no_mangle]
pub extern "C" fn ausgabe_len() -> u32 {
    unsafe { AUSGABE_LEN as u32 }
}

fn puffer(len: u32) -> &'static [u8] {
    unsafe { core::slice::from_raw_parts(&raw const PUFFER as *const u8, len as usize) }
}

fn schreibe(text: &str) {
    let b = text.as_bytes();
    let n = b.len().min(AUSGABE_GROESSE);
    unsafe {
        let ziel = core::slice::from_raw_parts_mut(&raw mut AUSGABE as *mut u8, AUSGABE_GROESSE);
        ziel[..n].copy_from_slice(&b[..n]);
        AUSGABE_LEN = n;
    }
}

/// JSON-Zeichenkette absichern. Elementnamen kommen aus fremden Dateien und
/// dürfen die Ausgabe nicht zerlegen.
fn json_text(roh: &[u8], ziel: &mut String) {
    ziel.push('"');
    for &c in roh {
        match c {
            b'"' => ziel.push_str("\\\""),
            b'\\' => ziel.push_str("\\\\"),
            b'\n' => ziel.push_str("\\n"),
            b'\r' => ziel.push_str("\\r"),
            b'\t' => ziel.push_str("\\t"),
            0x00..=0x1f => ziel.push_str(&format!("\\u{:04x}", c)),
            _ => ziel.push(c as char),
        }
    }
    ziel.push('"');
}

// ------------------------------------------------------------- Erkundung

#[no_mangle]
pub extern "C" fn erkundung_start() {
    unsafe {
        SCANNER = Some(Scanner::neu());
        ERKUNDUNG = Some(Erkundung::neu());
    }
}

#[no_mangle]
pub extern "C" fn erkundung_block(len: u32) {
    let daten = puffer(len);
    unsafe {
        if let (Some(s), Some(e)) = (
            (&mut *(&raw mut SCANNER)).as_mut(),
            (&mut *(&raw mut ERKUNDUNG)).as_mut(),
        ) {
            s.block(daten, e);
        }
    }
}

/// Schließt die Erkundung ab und legt das Ergebnis als JSON bereit.
#[no_mangle]
pub extern "C" fn erkundung_fertig() {
    let (s, e) = unsafe {
        (
            (&mut *(&raw mut SCANNER)).as_mut(),
            (&mut *(&raw mut ERKUNDUNG)).as_mut(),
        )
    };
    let (s, e) = match (s, e) {
        (Some(s), Some(e)) => (s, e),
        _ => {
            schreibe("{\"fehler\":\"nicht begonnen\"}");
            return;
        }
    };
    s.abschliessen(e);

    let mut j = String::with_capacity(64 * 1024);
    j.push_str("{\"elemente\":");
    j.push_str(&e.elemente.to_string());

    match e.datensatz_vorschlag() {
        Some((pfad, n)) => {
            j.push_str(",\"datensatz\":");
            json_text(&pfad, &mut j);
            j.push_str(",\"datensaetze\":");
            j.push_str(&n.to_string());
            j.push_str(",\"schluesselKandidaten\":[");
            for (i, (feld, anzahl, verschieden)) in
                e.schluessel_vorschlaege(&pfad).iter().take(40).enumerate()
            {
                if i > 0 {
                    j.push(',');
                }
                j.push_str("{\"feld\":");
                json_text(feld, &mut j);
                j.push_str(",\"anzahl\":");
                j.push_str(&anzahl.to_string());
                j.push_str(",\"verschieden\":");
                j.push_str(&verschieden.to_string());
                j.push('}');
            }
            j.push(']');

            // Wiederholgruppen — jede wird eine eigene Tabelle.
            j.push_str(",\"untertabellen\":[");
            for (i, (pfad, anzahl, gruppe)) in
                e.untertabellen_vorschlaege(&pfad).iter().take(50).enumerate()
            {
                if i > 0 {
                    j.push(',');
                }
                j.push_str("{\"pfad\":");
                json_text(pfad, &mut j);
                j.push_str(",\"anzahl\":");
                j.push_str(&anzahl.to_string());
                j.push_str(",\"groesste\":");
                j.push_str(&gruppe.to_string());
                j.push('}');
            }
            j.push(']');
        }
        None => j.push_str(
            ",\"datensatz\":null,\"datensaetze\":0,\"schluesselKandidaten\":[],\"untertabellen\":[]",
        ),
    }

    // Die Pfade, absteigend nach Häufigkeit — als Überblick über den Aufbau.
    let mut pfade: Vec<_> = e.pfade.iter().collect();
    pfade.sort_by(|a, b| b.1.anzahl.cmp(&a.1.anzahl).then_with(|| a.0.cmp(b.0)));
    j.push_str(",\"pfade\":[");
    for (i, (p, v)) in pfade.iter().take(500).enumerate() {
        if i > 0 {
            j.push(',');
        }
        j.push_str("{\"pfad\":");
        json_text(p, &mut j);
        j.push_str(",\"anzahl\":");
        j.push_str(&v.anzahl.to_string());
        j.push_str(",\"blatt\":");
        j.push_str(if v.blatt { "true" } else { "false" });
        j.push('}');
    }
    j.push_str("],\"pfadeGesamt\":");
    j.push_str(&e.pfade.len().to_string());
    j.push('}');

    schreibe(&j);
}

// -------------------------------------------------------------- Abdruck

/// Beginnt einen Abdruck. Im Puffer stehen der Datensatzpfad und die
/// Schlüsselfelder, jeweils durch einen Zeilenumbruch getrennt; die erste
/// Zeile ist der Datensatz.
#[no_mangle]
pub extern "C" fn abdruck_start(konfig_len: u32, eimer: u32) {
    let roh = puffer(konfig_len);
    let text = String::from_utf8_lossy(roh);
    let mut zeilen = text.split('\n');
    let datensatz = zeilen.next().unwrap_or("").trim().to_string();
    let schluessel: Vec<String> = zeilen
        .map(|z| z.trim().to_string())
        .filter(|z| !z.is_empty())
        .collect();
    let als_str: Vec<&str> = schluessel.iter().map(|s| s.as_str()).collect();

    unsafe {
        SCANNER = Some(Scanner::neu());
        ABDRUCK = Some(Abdruck::neu(&datensatz, &als_str, eimer.max(1) as usize));
    }
}

#[no_mangle]
pub extern "C" fn abdruck_block(len: u32) {
    let daten = puffer(len);
    unsafe {
        if let (Some(s), Some(a)) = (
            (&mut *(&raw mut SCANNER)).as_mut(),
            (&mut *(&raw mut ABDRUCK)).as_mut(),
        ) {
            s.block(daten, a);
        }
    }
}

#[no_mangle]
pub extern "C" fn abdruck_fertig() {
    let (s, a) = unsafe {
        (
            (&mut *(&raw mut SCANNER)).as_mut(),
            (&mut *(&raw mut ABDRUCK)).as_mut(),
        )
    };
    let (s, a) = match (s, a) {
        (Some(s), Some(a)) => (s, a),
        _ => {
            schreibe("{\"fehler\":\"nicht begonnen\"}");
            return;
        }
    };
    s.abschliessen(a);

    let g = a.gesamt();
    let mut j = String::with_capacity(256 * 1024);
    j.push_str("{\"verfahren\":\"xmlabgleich/1\",\"gesamt\":\"");
    // In Vierergruppen, damit man ihn vorlesen und abtippen kann.
    let hex = format!("{:016x}", g.wert).to_uppercase();
    for (i, teil) in hex.as_bytes().chunks(4).enumerate() {
        if i > 0 {
            j.push('-');
        }
        j.push_str(std::str::from_utf8(teil).unwrap_or(""));
    }
    j.push_str("\",\"stimmig\":");
    j.push_str(if g.stimmig { "true" } else { "false" });
    // Die drei Ebenen einzeln, damit der Bericht sagen kann, *was* sich
    // geändert hat und nicht nur *dass*.
    j.push_str(",\"struktur\":");
    j.push_str(&g.struktur.to_string());
    j.push_str(",\"anordnung\":");
    j.push_str(&g.anordnung.to_string());
    j.push_str(",\"auspraegungen\":");
    j.push_str(&g.ausprägungen.to_string());
    j.push_str(",\"satzfolge\":");
    j.push_str(&g.satzfolge.to_string());
    j.push_str(",\"anordnungen\":");
    j.push_str(&a.anordnungen.len().to_string());

    // Ebene 2 — die Indextabelle. Erst sie macht aus „die Anordnung weicht
    // ab" die Angabe, an welcher Stelle welches Feld steht.
    j.push_str(",\"feldIndex\":[");
    for (i, name) in a.feld_index.iter().enumerate() {
        if i > 0 {
            j.push(',');
        }
        json_text(name, &mut j);
    }
    j.push(']');

    // Die Feldfolgen, häufigste zuerst.
    let mut folgen: Vec<_> = a.anordnungen.iter().collect();
    folgen.sort_by(|x, y| y.1.cmp(x.1));
    j.push_str(",\"folgen\":[");
    for (i, (folge, anzahl)) in folgen.iter().take(20).enumerate() {
        if i > 0 {
            j.push(',');
        }
        j.push_str("{\"anzahl\":");
        j.push_str(&anzahl.to_string());
        j.push_str(",\"felder\":[");
        for (k, n) in folge.iter().enumerate() {
            if k > 0 {
                j.push(',');
            }
            j.push_str(&n.to_string());
        }
        j.push_str("]}");
    }
    j.push(']');

    j.push_str(",\"ersteSchluessel\":[");
    for (i, s) in a.erste_schluessel.iter().take(5000).enumerate() {
        if i > 0 {
            j.push(',');
        }
        json_text(s, &mut j);
    }
    j.push(']');
    j.push_str(",\"datensaetze\":");
    j.push_str(&a.datensaetze.to_string());
    j.push_str(",\"blaetter\":");
    j.push_str(&a.blaetter.to_string());
    j.push_str(",\"ohneSchluessel\":");
    j.push_str(&a.ohne_schluessel.to_string());

    // Spaltenrand
    let mut spalten: Vec<_> = a.spalten.iter().collect();
    spalten.sort_by(|x, y| x.0.cmp(y.0));
    j.push_str(",\"spalten\":[");
    for (i, (pfad, w)) in spalten.iter().enumerate() {
        if i > 0 {
            j.push(',');
        }
        j.push('[');
        json_text(pfad, &mut j);
        j.push(',');
        j.push_str(&w.summe.to_string());
        j.push(',');
        j.push_str(&w.xor.to_string());
        j.push(',');
        j.push_str(&w.anzahl.to_string());
        j.push(']');
    }
    j.push(']');

    // Zeilenrand
    j.push_str(",\"zeilen\":[");
    for (i, z) in a.zeilen.iter().enumerate() {
        if i > 0 {
            j.push(',');
        }
        j.push('[');
        j.push_str(&z.summe.to_string());
        j.push(',');
        j.push_str(&z.xor.to_string());
        j.push(',');
        j.push_str(&z.anzahl.to_string());
        j.push(']');
    }
    j.push_str("]}");

    schreibe(&j);
}

// --------------------------------------------------------- Einzelheiten
//
// Zweiter Durchlauf: die Klartextwerte hinter einer gemeldeten Abweichung.
// Die Suche wird durch das Ergebnis des ersten Durchlaufs eingegrenzt — auf
// die abweichenden Eimer und Pfade —, sodass nur eine Handvoll Datensätze
// überhaupt eingesammelt wird.
//
// Ablauf von JavaScript aus:
//   einzelheiten_vorbereiten(konfig_len, eimer)
//   einzelheiten_eimer(nr)      je gesuchtem Eimer
//   einzelheiten_pfad(len)      je gesuchtem Pfad (Text im Puffer)
//   einzelheiten_start()
//   einzelheiten_block(len)     je Block
//   einzelheiten_fertig()

/// Übernimmt Datensatzpfad und Schlüsselfelder wie `abdruck_start` und leert
/// die Suchvorgabe.
#[no_mangle]
pub extern "C" fn einzelheiten_vorbereiten(konfig_len: u32, eimer: u32) {
    let roh = puffer(konfig_len);
    let text = String::from_utf8_lossy(roh);
    let mut zeilen = text.split('\n');
    let datensatz = zeilen.next().unwrap_or("").trim().to_string();
    let schluessel: Vec<String> = zeilen
        .map(|z| z.trim().to_string())
        .filter(|z| !z.is_empty())
        .collect();
    unsafe {
        SUCH_DATENSATZ = datensatz;
        SUCH_SCHLUESSEL = schluessel;
        SUCH_EIMER_GESAMT = eimer.max(1);
        SUCH_EIMER = Vec::new();
        SUCH_PFADE = Vec::new();
        SUCH_TABELLE = None;
    }
}

/// Legt fest, aus welcher Tabelle die Einzelheiten geholt werden. Ohne Aufruf
/// werden alle Tabellen durchsucht.
#[no_mangle]
pub extern "C" fn einzelheiten_tabelle(len: u32) {
    let t = puffer(len).to_vec();
    unsafe {
        SUCH_TABELLE = Some(t);
    }
}

#[no_mangle]
pub extern "C" fn einzelheiten_eimer(nr: u32) {
    unsafe {
        (&mut *(&raw mut SUCH_EIMER)).push(nr as usize);
    }
}

#[no_mangle]
pub extern "C" fn einzelheiten_pfad(len: u32) {
    let p = puffer(len).to_vec();
    unsafe {
        (&mut *(&raw mut SUCH_PFADE)).push(p);
    }
}

#[no_mangle]
pub extern "C" fn einzelheiten_start() {
    unsafe {
        let datensatz = (&*(&raw const SUCH_DATENSATZ)).clone();
        let schluessel: Vec<&str> = (&*(&raw const SUCH_SCHLUESSEL))
            .iter()
            .map(|s| s.as_str())
            .collect();
        let eimer: Vec<usize> = (&*(&raw const SUCH_EIMER)).clone();
        let pfade: Vec<&[u8]> = (&*(&raw const SUCH_PFADE))
            .iter()
            .map(|p| p.as_slice())
            .collect();

        let unter: Vec<&str> = (&*(&raw const ZER_UNTER)).iter().map(|s| s.as_str()).collect();
        let tabelle = (&*(&raw const SUCH_TABELLE)).clone();
        SCANNER = Some(Scanner::neu());
        EINZELHEITEN = Some(Einzelheiten::neu(
            &datensatz,
            &schluessel,
            SUCH_EIMER_GESAMT as usize,
            &unter,
            &eimer,
            &pfade,
            tabelle.as_deref(),
        ));
    }
}

#[no_mangle]
pub extern "C" fn einzelheiten_block(len: u32) {
    let daten = puffer(len);
    unsafe {
        if let (Some(s), Some(e)) = (
            (&mut *(&raw mut SCANNER)).as_mut(),
            (&mut *(&raw mut EINZELHEITEN)).as_mut(),
        ) {
            s.block(daten, e);
        }
    }
}

#[no_mangle]
pub extern "C" fn einzelheiten_fertig() {
    let (s, e) = unsafe {
        (
            (&mut *(&raw mut SCANNER)).as_mut(),
            (&mut *(&raw mut EINZELHEITEN)).as_mut(),
        )
    };
    let (s, e) = match (s, e) {
        (Some(s), Some(e)) => (s, e),
        _ => {
            schreibe("{\"fehler\":\"nicht begonnen\"}");
            return;
        }
    };
    s.abschliessen(e);

    let mut j = String::with_capacity(64 * 1024);
    j.push_str("{\"abgeschnitten\":");
    j.push_str(if e.abgeschnitten { "true" } else { "false" });
    j.push_str(",\"funde\":[");
    for (i, f) in e.funde.iter().enumerate() {
        if i > 0 {
            j.push(',');
        }
        j.push('[');
        json_text(&f.schluessel, &mut j);
        j.push(',');
        json_text(&f.pfad, &mut j);
        j.push(',');
        json_text(&f.wert, &mut j);
        j.push(',');
        json_text(&f.tabelle, &mut j);
        j.push(']');
    }
    j.push_str("]}");
    schreibe(&j);
}

// -------------------------------------------------------------- Zerlegung
//
// Wie `abdruck_*`, aber mit Untertabellen: Jede Wiederholgruppe bekommt eine
// eigene Tabelle mit eigenem Rand. Die Gruppen werden vor dem Start einzeln
// angemeldet, weil sie aus der Erkundung kommen und ihre Zahl nicht feststeht.

static mut ZERLEGUNG: Option<crate::zerlegung::Zerlegung> = None;
static mut ZER_UNTER: Vec<String> = Vec::new();

#[no_mangle]
pub extern "C" fn zerlegung_vorbereiten(konfig_len: u32, eimer: u32) {
    einzelheiten_vorbereiten(konfig_len, eimer);
    unsafe {
        ZER_UNTER = Vec::new();
    }
}

#[no_mangle]
pub extern "C" fn zerlegung_untertabelle(len: u32) {
    let p = String::from_utf8_lossy(puffer(len)).to_string();
    unsafe {
        (&mut *(&raw mut ZER_UNTER)).push(p);
    }
}

#[no_mangle]
pub extern "C" fn zerlegung_start() {
    unsafe {
        let datensatz = (&*(&raw const SUCH_DATENSATZ)).clone();
        let schluessel: Vec<&str> = (&*(&raw const SUCH_SCHLUESSEL))
            .iter()
            .map(|s| s.as_str())
            .collect();
        let unter: Vec<&str> = (&*(&raw const ZER_UNTER)).iter().map(|s| s.as_str()).collect();
        SCANNER = Some(Scanner::neu());
        ZERLEGUNG = Some(crate::zerlegung::Zerlegung::neu(
            &datensatz,
            &schluessel,
            SUCH_EIMER_GESAMT as usize,
            &unter,
        ));
    }
}

#[no_mangle]
pub extern "C" fn zerlegung_block(len: u32) {
    let daten = puffer(len);
    unsafe {
        if let (Some(s), Some(z)) = (
            (&mut *(&raw mut SCANNER)).as_mut(),
            (&mut *(&raw mut ZERLEGUNG)).as_mut(),
        ) {
            s.block(daten, z);
        }
    }
}

#[no_mangle]
pub extern "C" fn zerlegung_fertig() {
    let (s, z) = unsafe {
        (
            (&mut *(&raw mut SCANNER)).as_mut(),
            (&mut *(&raw mut ZERLEGUNG)).as_mut(),
        )
    };
    let (s, z) = match (s, z) {
        (Some(s), Some(z)) => (s, z),
        _ => {
            schreibe("{\"fehler\":\"nicht begonnen\"}");
            return;
        }
    };
    s.abschliessen(z);

    let mut j = String::with_capacity(256 * 1024);
    j.push_str("{\"verfahren\":\"xmlabgleich/2\",\"gesamt\":\"");
    let hex = format!("{:016x}", z.gesamt()).to_uppercase();
    for (i, teil) in hex.as_bytes().chunks(4).enumerate() {
        if i > 0 {
            j.push('-');
        }
        j.push_str(std::str::from_utf8(teil).unwrap_or(""));
    }
    j.push_str("\",\"datensaetze\":");
    j.push_str(&z.datensaetze.to_string());
    j.push_str(",\"ohneSchluessel\":");
    j.push_str(&z.ohne_schluessel.to_string());

    j.push_str(",\"ersteSchluessel\":[");
    for (i, s) in z.erste_schluessel.iter().take(5000).enumerate() {
        if i > 0 {
            j.push(',');
        }
        json_text(s, &mut j);
    }
    j.push(']');

    // Je Tabelle: Ränder, Struktur, Zählwerte.
    let mut namen: Vec<&Vec<u8>> = z.tabellen.keys().collect();
    namen.sort();
    j.push_str(",\"tabellen\":[");
    for (i, name) in namen.iter().enumerate() {
        if i > 0 {
            j.push(',');
        }
        let t = &z.tabellen[*name];
        j.push_str("{\"name\":");
        json_text(name, &mut j);
        j.push_str(",\"gehoertZu\":");
        json_text(&t.gehoert_zu, &mut j);
        j.push_str(",\"tiefe\":");
        j.push_str(&t.tiefe.to_string());
        j.push_str(",\"zeilen\":");
        j.push_str(&t.zeilenzahl.to_string());
        j.push_str(",\"felder\":");
        j.push_str(&t.felder.to_string());
        j.push_str(",\"struktur\":");
        j.push_str(&t.struktur().to_string());
        j.push_str(",\"wert\":");
        j.push_str(&t.wert().to_string());

        let mut spalten: Vec<_> = t.spalten.iter().collect();
        spalten.sort_by(|a, b| a.0.cmp(b.0));
        j.push_str(",\"spalten\":[");
        for (k, (pfad, w)) in spalten.iter().enumerate() {
            if k > 0 {
                j.push(',');
            }
            j.push('[');
            json_text(pfad, &mut j);
            j.push(',');
            j.push_str(&w.summe.to_string());
            j.push(',');
            j.push_str(&w.xor.to_string());
            j.push(',');
            j.push_str(&w.anzahl.to_string());
            j.push(']');
        }
        j.push_str("],\"eimer\":[");
        for (k, e) in t.zeilen.iter().enumerate() {
            if k > 0 {
                j.push(',');
            }
            j.push('[');
            j.push_str(&e.summe.to_string());
            j.push(',');
            j.push_str(&e.xor.to_string());
            j.push(',');
            j.push_str(&e.anzahl.to_string());
            j.push(']');
        }
        j.push_str("]}");
    }
    j.push_str("]}");
    schreibe(&j);
}
