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
        }
        None => j.push_str(",\"datensatz\":null,\"datensaetze\":0,\"schluesselKandidaten\":[]"),
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
