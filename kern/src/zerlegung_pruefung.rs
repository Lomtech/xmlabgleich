//! Prüfungen für die Zerlegung in Tabellen.
//!
//! Der Grund für dieses Verfahren steht im ersten Test: Ohne eigene Tabellen
//! sind zwei vertauschte Einträge einer Wiederholgruppe unsichtbar, weil ihre
//! Werte in derselben Spalte landen und die Summe kommutativ ist.

#![cfg(test)]

use crate::scanner::Scanner;
use crate::zerlegung::Zerlegung;

const DATENSATZ: &str = "INVESTMENTS/INVESTMENT";
const SCHLUESSEL: &[&str] = &["ID"];
const UNTER: &[&str] = &["KENNUNGEN/KENNUNG", "BEDINGUNGEN/BEDINGUNG"];

fn zerlege(xml: &str, blockgroesse: usize) -> Zerlegung {
    let mut s = Scanner::neu();
    let mut z = Zerlegung::neu(DATENSATZ, SCHLUESSEL, 64, UNTER);
    let daten = xml.as_bytes();
    let mut i = 0;
    while i < daten.len() {
        let bis = (i + blockgroesse).min(daten.len());
        s.block(&daten[i..bis], &mut z);
        i = bis;
    }
    s.abschliessen(&mut z);
    z
}

/// Ohne Untertabellen wäre das der blinde Fleck: Zwei Kennungen tauschen
/// ihre Werte. Beide stehen im selben Feldpfad, die Summe bleibt gleich.
#[test]
fn vertauschte_eintraege_einer_wiederholgruppe_fallen_auf() {
    let a = "<INVESTMENTS><INVESTMENT><ID>A1</ID><KENNUNGEN>\
        <KENNUNG><ART>ISIN</ART><WERT>DE0001</WERT></KENNUNG>\
        <KENNUNG><ART>WKN</ART><WERT>123456</WERT></KENNUNG>\
        </KENNUNGEN></INVESTMENT></INVESTMENTS>";
    let b = "<INVESTMENTS><INVESTMENT><ID>A1</ID><KENNUNGEN>\
        <KENNUNG><ART>ISIN</ART><WERT>123456</WERT></KENNUNG>\
        <KENNUNG><ART>WKN</ART><WERT>DE0001</WERT></KENNUNG>\
        </KENNUNGEN></INVESTMENT></INVESTMENTS>";

    assert_ne!(
        zerlege(a, 9).gesamt(),
        zerlege(b, 9).gesamt(),
        "ISIN und WKN vertauscht — das muss auffallen"
    );
}

#[test]
fn wiederholgruppe_wird_eine_eigene_tabelle() {
    let x = "<INVESTMENTS><INVESTMENT><ID>A1</ID><KENNUNGEN>\
        <KENNUNG><ART>ISIN</ART><WERT>DE0001</WERT></KENNUNG>\
        <KENNUNG><ART>WKN</ART><WERT>123456</WERT></KENNUNG>\
        </KENNUNGEN></INVESTMENT></INVESTMENTS>";
    let z = zerlege(x, 11);

    let namen: Vec<String> = {
        let mut n: Vec<String> = z
            .tabellen
            .keys()
            .map(|k| String::from_utf8_lossy(k).to_string())
            .collect();
        n.sort();
        n
    };
    assert_eq!(namen, vec!["", "KENNUNGEN/KENNUNG"]);

    let unter = &z.tabellen[b"KENNUNGEN/KENNUNG".as_slice()];
    assert_eq!(unter.zeilenzahl, 2, "zwei Kennungen, zwei Zeilen");

    let mut spalten: Vec<String> = unter
        .spalten
        .keys()
        .map(|k| String::from_utf8_lossy(k).to_string())
        .collect();
    spalten.sort();
    assert_eq!(spalten, vec!["ART", "WERT"]);

    // Die Haupttabelle hat die Kennungen nicht mehr als eigene Felder …
    let haupt: Vec<String> = z
        .haupt()
        .spalten
        .keys()
        .map(|k| String::from_utf8_lossy(k).to_string())
        .collect();
    assert!(haupt.contains(&"ID".to_string()));
    assert!(
        !haupt.iter().any(|s| s.contains("ART")),
        "die Kennungsfelder gehören in ihre eigene Tabelle: {haupt:?}"
    );
    // … wohl aber einen Vermerk je Zeile, sonst wäre eine fehlende Gruppe
    // im Abdruck der Haupttabelle unsichtbar.
    assert!(
        haupt.iter().any(|s| s.ends_with("[]")),
        "die Zugehörigkeit muss in der Haupttabelle stehen: {haupt:?}"
    );
}

/// Eine fehlende Wiederholgruppe muss auch am Datensatz selbst auffallen.
#[test]
fn fehlende_wiederholgruppe_faellt_auf() {
    let mit = "<INVESTMENTS><INVESTMENT><ID>A1</ID><KENNUNGEN>\
        <KENNUNG><ART>ISIN</ART></KENNUNG><KENNUNG><ART>WKN</ART></KENNUNG>\
        </KENNUNGEN></INVESTMENT></INVESTMENTS>";
    let weniger = "<INVESTMENTS><INVESTMENT><ID>A1</ID><KENNUNGEN>\
        <KENNUNG><ART>ISIN</ART></KENNUNG>\
        </KENNUNGEN></INVESTMENT></INVESTMENTS>";

    let (x, y) = (zerlege(mit, 8), zerlege(weniger, 8));
    assert_ne!(x.gesamt(), y.gesamt());
    assert_eq!(x.tabellen[b"KENNUNGEN/KENNUNG".as_slice()].zeilenzahl, 2);
    assert_eq!(y.tabellen[b"KENNUNGEN/KENNUNG".as_slice()].zeilenzahl, 1);
    assert_ne!(
        x.haupt().struktur() == y.haupt().struktur(),
        false,
        "die Feldnamen der Haupttabelle bleiben gleich"
    );
}

/// Verschachtelung in mehreren Stufen: Eine Untertabelle in einer
/// Untertabelle.
#[test]
fn untertabellen_verschachteln_sich() {
    let x = "<INVESTMENTS><INVESTMENT><ID>A1</ID><BEDINGUNGEN>\
        <BEDINGUNG><ART>ZINS</ART><STAFFELN>\
          <STAFFEL><AB>0</AB><SATZ>1.5</SATZ></STAFFEL>\
          <STAFFEL><AB>100</AB><SATZ>2.0</SATZ></STAFFEL>\
        </STAFFELN></BEDINGUNG></BEDINGUNGEN></INVESTMENT></INVESTMENTS>";

    let mut s = Scanner::neu();
    let mut z = Zerlegung::neu(
        DATENSATZ,
        SCHLUESSEL,
        64,
        &["BEDINGUNGEN/BEDINGUNG", "STAFFELN/STAFFEL"],
    );
    s.block(x.as_bytes(), &mut z);
    s.abschliessen(&mut z);

    let mut namen: Vec<String> = z
        .tabellen
        .keys()
        .map(|k| String::from_utf8_lossy(k).to_string())
        .collect();
    namen.sort();
    assert_eq!(
        namen,
        vec!["", "BEDINGUNGEN/BEDINGUNG", "BEDINGUNGEN/BEDINGUNG/STAFFELN/STAFFEL"]
    );
    assert_eq!(
        z.tabellen[b"BEDINGUNGEN/BEDINGUNG/STAFFELN/STAFFEL".as_slice()].zeilenzahl,
        2
    );
}

/// Blockgrößen dürfen auch hier nichts ändern — der Stapel offener Zeilen
/// muss jede Grenze überstehen.
#[test]
fn blockgroesse_aendert_nichts() {
    let x = "<INVESTMENTS><INVESTMENT><ID>A1</ID><KENNUNGEN>\
        <KENNUNG><ART>ISIN</ART><WERT>DE0001</WERT></KENNUNG>\
        <KENNUNG><ART>WKN</ART><WERT>123456</WERT></KENNUNG>\
        </KENNUNGEN></INVESTMENT>\
        <INVESTMENT><ID>A2</ID><KENNUNGEN>\
        <KENNUNG><ART>ISIN</ART><WERT>DE0002</WERT></KENNUNG>\
        </KENNUNGEN></INVESTMENT></INVESTMENTS>";
    let erwartet = zerlege(x, x.len()).gesamt();
    for g in [1, 2, 3, 5, 7, 13, 31, 64, 500] {
        assert_eq!(zerlege(x, g).gesamt(), erwartet, "Blockgröße {g}");
    }
}

/// Die Zugehörigkeit muss stimmen: Dieselben Kennungen unter einem anderen
/// Investment sind etwas anderes.
#[test]
fn zugehoerigkeit_zaehlt() {
    let a = "<INVESTMENTS>\
        <INVESTMENT><ID>A1</ID><KENNUNGEN><KENNUNG><ART>ISIN</ART></KENNUNG></KENNUNGEN></INVESTMENT>\
        <INVESTMENT><ID>A2</ID><KENNUNGEN><KENNUNG><ART>WKN</ART></KENNUNG></KENNUNGEN></INVESTMENT>\
        </INVESTMENTS>";
    let b = "<INVESTMENTS>\
        <INVESTMENT><ID>A1</ID><KENNUNGEN><KENNUNG><ART>WKN</ART></KENNUNG></KENNUNGEN></INVESTMENT>\
        <INVESTMENT><ID>A2</ID><KENNUNGEN><KENNUNG><ART>ISIN</ART></KENNUNG></KENNUNGEN></INVESTMENT>\
        </INVESTMENTS>";
    assert_ne!(
        zerlege(a, 10).gesamt(),
        zerlege(b, 10).gesamt(),
        "getauschte Kennungen zwischen zwei Investments müssen auffallen"
    );
}

/// Ohne angegebene Untertabellen verhält sich die Zerlegung wie zuvor —
/// alles landet flach in der Haupttabelle.
#[test]
fn ohne_untertabellen_bleibt_alles_flach() {
    let x = "<INVESTMENTS><INVESTMENT><ID>A1</ID><KENNUNGEN>\
        <KENNUNG><ART>ISIN</ART></KENNUNG></KENNUNGEN></INVESTMENT></INVESTMENTS>";
    let mut s = Scanner::neu();
    let mut z = Zerlegung::neu(DATENSATZ, SCHLUESSEL, 64, &[]);
    s.block(x.as_bytes(), &mut z);
    s.abschliessen(&mut z);

    assert_eq!(z.tabellen.len(), 1);
    assert!(z
        .haupt()
        .spalten
        .contains_key(b"KENNUNGEN/KENNUNG/ART".as_slice()));
}

/// Attribute gehören in die Tabelle, in der sie stehen.
#[test]
fn attribute_landen_in_ihrer_tabelle() {
    let x = "<INVESTMENTS><INVESTMENT><ID>A1</ID><KENNUNGEN>\
        <KENNUNG art=\"amtlich\"><WERT>DE0001</WERT></KENNUNG>\
        </KENNUNGEN></INVESTMENT></INVESTMENTS>";
    let z = zerlege(x, 7);
    let unter = &z.tabellen[b"KENNUNGEN/KENNUNG".as_slice()];
    let spalten: Vec<String> = unter
        .spalten
        .keys()
        .map(|k| String::from_utf8_lossy(k).to_string())
        .collect();
    assert!(
        spalten.contains(&"@art".to_string()),
        "das Attribut der Zeile selbst: {spalten:?}"
    );
}

/// Der Fall, der die Zuordnung stillschweigend zerstört: Das Schlüsselfeld
/// des Datensatzes steht **nach** der Wiederholgruppe. Beim Öffnen der
/// Untertabelle ist der Elternschlüssel dann noch unbekannt — werden die
/// Zeilen zu diesem Zeitpunkt zugeordnet, landen alle unter demselben
/// Platzhalter und die Zugehörigkeit ist verloren.
#[test]
fn schluessel_nach_der_wiederholgruppe() {
    // Zwei Investments, die Kennungen jeweils VOR der ID.
    let a = "<INVESTMENTS>\
        <INVESTMENT><KENNUNGEN><KENNUNG><ART>ISIN</ART></KENNUNG></KENNUNGEN><ID>A1</ID></INVESTMENT>\
        <INVESTMENT><KENNUNGEN><KENNUNG><ART>WKN</ART></KENNUNG></KENNUNGEN><ID>A2</ID></INVESTMENT>\
        </INVESTMENTS>";
    // Dieselben Daten, aber die Kennungen über Kreuz getauscht.
    let b = "<INVESTMENTS>\
        <INVESTMENT><KENNUNGEN><KENNUNG><ART>WKN</ART></KENNUNG></KENNUNGEN><ID>A1</ID></INVESTMENT>\
        <INVESTMENT><KENNUNGEN><KENNUNG><ART>ISIN</ART></KENNUNG></KENNUNGEN><ID>A2</ID></INVESTMENT>\
        </INVESTMENTS>";

    assert_ne!(
        zerlege(a, 12).gesamt(),
        zerlege(b, 12).gesamt(),
        "die Zuordnung muss auch dann stimmen, wenn der Schlüssel erst nach \
         der Wiederholgruppe kommt"
    );
}
