//! Prüfungen für den Fingerabdruck.
//!
//! Zwei Eigenschaften müssen gelten, und sie ziehen in verschiedene
//! Richtungen:
//!
//! * **Unempfindlich** gegen alles, was nichts bedeutet — Einrückung,
//!   Reihenfolge der Datensätze, Namensraum-Präfixe, Blockgrenzen.
//! * **Empfindlich** gegen alles, was etwas bedeutet — jeder geänderte Wert,
//!   jedes fehlende Feld, jeder vertauschte Inhalt.
//!
//! Ein Abdruck, der nur das erste kann, ist bequem und wertlos.

#![cfg(test)]

use crate::abdruck::Abdruck;
use crate::scanner::Scanner;

const DATENSATZ: &str = "INVESTMENTS/INVESTMENT";
const SCHLUESSEL: &[&str] = &["ID"];

fn abdruck_von(xml: &str, blockgroesse: usize) -> Abdruck {
    let mut s = Scanner::neu();
    let mut a = Abdruck::neu(DATENSATZ, SCHLUESSEL, 64);
    let daten = xml.as_bytes();
    let mut i = 0;
    while i < daten.len() {
        let bis = (i + blockgroesse).min(daten.len());
        s.block(&daten[i..bis], &mut a);
        i = bis;
    }
    s.abschliessen(&mut a);
    a
}

/// Vergleichbare Kurzfassung: Zeilen- und Spaltenrand.
fn kennwerte(a: &Abdruck) -> (Vec<(String, u32, u32, u32)>, Vec<(u32, u32)>, u64) {
    let mut spalten: Vec<(String, u32, u32, u32)> = a
        .spalten
        .iter()
        .map(|(p, w)| {
            (
                String::from_utf8_lossy(p).to_string(),
                w.summe,
                w.xor,
                w.anzahl,
            )
        })
        .collect();
    spalten.sort();
    let zeilen: Vec<(u32, u32)> = a.zeilen.iter().map(|z| (z.summe, z.xor)).collect();
    (spalten, zeilen, a.datensaetze)
}

fn dok(saetze: &[&str]) -> String {
    format!("<INVESTMENTS>{}</INVESTMENTS>", saetze.concat())
}

fn satz(id: &str, typ: &str, betrag: &str) -> String {
    format!("<INVESTMENT><ID>{id}</ID><TYP>{typ}</TYP><BETRAG>{betrag}</BETRAG></INVESTMENT>")
}

// ------------------------------------------------------------- unempfindlich

#[test]
fn blockgroesse_aendert_nichts() {
    let x = dok(&[&satz("A1", "BOND", "100.00"), &satz("A2", "EQUITY", "250.50")]);
    let erwartet = kennwerte(&abdruck_von(&x, x.len()));
    for g in [1, 2, 3, 5, 7, 13, 64, 1000] {
        assert_eq!(
            kennwerte(&abdruck_von(&x, g)),
            erwartet,
            "Blockgröße {g} liefert einen anderen Abdruck"
        );
    }
}

#[test]
fn einrueckung_aendert_nichts() {
    let kompakt = dok(&[&satz("A1", "BOND", "100.00")]);
    let huebsch = "<INVESTMENTS>\n  <INVESTMENT>\n    <ID>A1</ID>\n    <TYP>BOND</TYP>\n    <BETRAG>100.00</BETRAG>\n  </INVESTMENT>\n</INVESTMENTS>";
    assert_eq!(
        kennwerte(&abdruck_von(&kompakt, 5)),
        kennwerte(&abdruck_von(huebsch, 5))
    );
}

#[test]
fn namensraum_praefix_aendert_nichts() {
    let ohne = dok(&[&satz("A1", "BOND", "100.00")]);
    let mit = "<n:INVESTMENTS xmlns:n=\"u\"><n:INVESTMENT><n:ID>A1</n:ID><n:TYP>BOND</n:TYP><n:BETRAG>100.00</n:BETRAG></n:INVESTMENT></n:INVESTMENTS>";
    assert_eq!(
        kennwerte(&abdruck_von(&ohne, 6)),
        kennwerte(&abdruck_von(mit, 6))
    );
}

#[test]
fn kommentare_aendern_nichts() {
    let ohne = dok(&[&satz("A1", "BOND", "100.00")]);
    let mit = format!(
        "<INVESTMENTS><!-- erzeugt am 2026-08-09 -->{}</INVESTMENTS>",
        satz("A1", "BOND", "100.00")
    );
    assert_eq!(
        kennwerte(&abdruck_von(&ohne, 9)),
        kennwerte(&abdruck_von(&mit, 9))
    );
}

// --------------------------------------------------------------- empfindlich

#[test]
fn geaenderter_wert_faellt_auf() {
    let a = dok(&[&satz("A1", "BOND", "100.00"), &satz("A2", "EQUITY", "250.50")]);
    let b = dok(&[&satz("A1", "BOND", "100.01"), &satz("A2", "EQUITY", "250.50")]);

    let (sa, za, _) = kennwerte(&abdruck_von(&a, 8));
    let (sb, zb, _) = kennwerte(&abdruck_von(&b, 8));

    // Genau die Spalte BETRAG weicht ab — und keine andere.
    let abweichende: Vec<&str> = sa
        .iter()
        .zip(sb.iter())
        .filter(|(x, y)| x != y)
        .map(|(x, _)| x.0.as_str())
        .collect();
    assert_eq!(abweichende, vec!["BETRAG"], "nur BETRAG darf abweichen");

    // Und genau ein Eimer.
    let eimer: Vec<usize> = za
        .iter()
        .zip(zb.iter())
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(eimer.len(), 1, "genau ein Eimer darf abweichen: {eimer:?}");
}

#[test]
fn fehlender_datensatz_faellt_auf() {
    let a = dok(&[&satz("A1", "BOND", "100.00"), &satz("A2", "EQUITY", "250.50")]);
    let b = dok(&[&satz("A1", "BOND", "100.00")]);
    assert_ne!(kennwerte(&abdruck_von(&a, 11)), kennwerte(&abdruck_von(&b, 11)));
    assert_eq!(abdruck_von(&a, 11).datensaetze, 2);
    assert_eq!(abdruck_von(&b, 11).datensaetze, 1);
}

#[test]
fn fehlendes_feld_faellt_auf() {
    let voll = dok(&[&satz("A1", "BOND", "100.00")]);
    let ohne = "<INVESTMENTS><INVESTMENT><ID>A1</ID><TYP>BOND</TYP></INVESTMENT></INVESTMENTS>";
    let (sa, _, _) = kennwerte(&abdruck_von(&voll, 6));
    let (sb, _, _) = kennwerte(&abdruck_von(ohne, 6));
    assert!(
        sa.iter().any(|s| s.0 == "BETRAG") && !sb.iter().any(|s| s.0 == "BETRAG"),
        "die fehlende Spalte muss erkennbar sein"
    );
}

/// Der Fall, der eine reine Spaltensumme austricksen würde: Zwei Datensätze
/// tauschen ihre Werte. Spaltenweise ist die Summe gleich — durch die
/// Verbindung mit dem Schlüssel fällt es trotzdem auf.
#[test]
fn vertauschte_werte_zwischen_datensaetzen_fallen_auf() {
    let a = dok(&[&satz("A1", "BOND", "100.00"), &satz("A2", "EQUITY", "250.50")]);
    let b = dok(&[&satz("A1", "EQUITY", "250.50"), &satz("A2", "BOND", "100.00")]);

    let (sa, _, _) = kennwerte(&abdruck_von(&a, 10));
    let (sb, _, _) = kennwerte(&abdruck_von(&b, 10));
    assert_ne!(
        sa, sb,
        "vertauschte Werte müssen auffallen, obwohl die Wertemenge je Spalte gleich ist"
    );
}

/// Ein leeres Element ist etwas anderes als ein fehlendes.
#[test]
fn leeres_element_unterscheidet_sich_von_fehlendem() {
    let leer = "<INVESTMENTS><INVESTMENT><ID>A1</ID><LISTED/></INVESTMENT></INVESTMENTS>";
    let fehlt = "<INVESTMENTS><INVESTMENT><ID>A1</ID></INVESTMENT></INVESTMENTS>";
    let (sl, _, _) = kennwerte(&abdruck_von(leer, 5));
    let (sf, _, _) = kennwerte(&abdruck_von(fehlt, 5));
    assert!(sl.iter().any(|s| s.0 == "LISTED"));
    assert!(!sf.iter().any(|s| s.0 == "LISTED"));
}

/// Gleicher Wert an anderer Stelle darf nicht denselben Beitrag liefern —
/// sonst hebt sich ein Verschieben zwischen Feldern auf.
#[test]
fn gleicher_wert_in_anderer_spalte_ist_verschieden() {
    let a = "<INVESTMENTS><INVESTMENT><ID>A1</ID><X>gleich</X><Y>anders</Y></INVESTMENT></INVESTMENTS>";
    let b = "<INVESTMENTS><INVESTMENT><ID>A1</ID><X>anders</X><Y>gleich</Y></INVESTMENT></INVESTMENTS>";
    assert_ne!(kennwerte(&abdruck_von(a, 7)), kennwerte(&abdruck_von(b, 7)));
}

// ------------------------------------------------------------------ Befunde

// ------------------------------------------------------------ Gesamtwert

/// Die eingebaute Selbstkontrolle: Jeder Zellwert geht in genau eine Spalte
/// und genau eine Zeile ein, also müssen beide Achsen dieselbe Summe ergeben.
/// Weichen sie ab, ist nicht die Datei falsch, sondern der Abdruck.
#[test]
fn beide_achsen_ergeben_dieselbe_summe() {
    let x = dok(&[
        &satz("A1", "BOND", "100.00"),
        &satz("A2", "EQUITY", "250.50"),
        &satz("A3", "FUND", "7.25"),
    ]);
    let g = abdruck_von(&x, 9).gesamt();
    assert!(
        g.stimmig,
        "Spaltenachse {} gegen Zeilenachse {}",
        g.ueber_spalten, g.ueber_zeilen
    );
}

#[test]
fn gesamtwert_ist_gleich_bei_gleichem_inhalt() {
    let a = dok(&[&satz("A1", "BOND", "100.00"), &satz("A2", "EQUITY", "250.50")]);
    // Nur anders eingerückt — gleiche Sätze in gleicher Folge.
    let b = format!(
        "<INVESTMENTS>\n  {}\n  {}\n</INVESTMENTS>",
        satz("A1", "BOND", "100.00"),
        satz("A2", "EQUITY", "250.50")
    );
    assert_eq!(
        abdruck_von(&a, 7).gesamt().wert,
        abdruck_von(&b, 7).gesamt().wert
    );
}

/// Umsortierte Datensätze sind ein Unterschied und werden als solcher
/// gemeldet — auch wenn dieselben Sätze mit denselben Werten dastehen.
///
/// Ob eine Umsortierung hinnehmbar ist, hängt vom Verarbeitungsweg ab: Manche
/// Ladeprogramme verarbeiten in Dokumentfolge und bauen Abhängigkeiten
/// zwischen Sätzen auf. Das Werkzeug stellt fest, es bewertet nicht.
#[test]
fn umsortierte_datensaetze_sind_ein_unterschied() {
    let a = dok(&[&satz("A1", "BOND", "100.00"), &satz("A2", "EQUITY", "250.50")]);
    let b = dok(&[&satz("A2", "EQUITY", "250.50"), &satz("A1", "BOND", "100.00")]);
    let (x, y) = (abdruck_von(&a, 7).gesamt(), abdruck_von(&b, 7).gesamt());

    assert_ne!(x.satzfolge, y.satzfolge, "die Satzfolge muss anschlagen");
    assert_ne!(x.wert, y.wert, "und damit auch der Gesamtwert");

    // Alles andere bleibt gleich — daran ist erkennbar, dass *nur*
    // umsortiert und nichts verändert wurde.
    assert_eq!(x.struktur, y.struktur);
    assert_eq!(x.anordnung, y.anordnung);
    assert_eq!(x.ausprägungen, y.ausprägungen);
}

/// Und die Umkehrung: Die Ränder überstehen eine Umsortierung. Sonst wäre
/// nicht unterscheidbar, ob umsortiert oder verändert wurde.
#[test]
fn raender_ueberstehen_eine_umsortierung() {
    let a = dok(&[&satz("A1", "BOND", "100.00"), &satz("A2", "EQUITY", "250.50")]);
    let b = dok(&[&satz("A2", "EQUITY", "250.50"), &satz("A1", "BOND", "100.00")]);
    let (sa, za, _) = kennwerte(&abdruck_von(&a, 7));
    let (sb, zb, _) = kennwerte(&abdruck_von(&b, 7));
    assert_eq!(sa, sb, "der Spaltenrand ist kommutativ");
    assert_eq!(za, zb, "der Zeilenrand auch");
}

/// Die gemerkten Schlüssel müssen die erste abweichende Stelle benennen
/// können — „irgendwo anders sortiert" hilft niemandem.
#[test]
fn erste_schluessel_zeigen_wo_die_folge_auseinanderlaeuft() {
    let a = dok(&[&satz("A1", "x", "1"), &satz("A2", "y", "2"), &satz("A3", "z", "3")]);
    let b = dok(&[&satz("A1", "x", "1"), &satz("A3", "z", "3"), &satz("A2", "y", "2")]);
    let (x, y) = (abdruck_von(&a, 8), abdruck_von(&b, 8));

    let als_text = |ab: &Abdruck| {
        ab.erste_schluessel
            .iter()
            .map(|s| String::from_utf8_lossy(s).to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(als_text(&x), vec!["A1", "A2", "A3"]);
    assert_eq!(als_text(&y), vec!["A1", "A3", "A2"]);

    // Die erste Abweichung liegt an Stelle 2.
    let (fx, fy) = (als_text(&x), als_text(&y));
    let erste = fx.iter().zip(fy.iter()).position(|(p, q)| p != q);
    assert_eq!(erste, Some(1));
}

/// Der eigentliche Zweck: Im ersten Schritt genügt ein Wert, um eine
/// Abweichung auszuschließen. Jede Änderung muss ihn verändern.
#[test]
fn jede_aenderung_veraendert_den_gesamtwert() {
    let grund = dok(&[&satz("A1", "BOND", "100.00"), &satz("A2", "EQUITY", "250.50")]);
    let g0 = abdruck_von(&grund, 8).gesamt().wert;

    let faelle = [
        ("Wert geändert", dok(&[&satz("A1", "BOND", "100.01"), &satz("A2", "EQUITY", "250.50")])),
        ("Datensatz fehlt", dok(&[&satz("A1", "BOND", "100.00")])),
        ("Datensatz zusätzlich", dok(&[
            &satz("A1", "BOND", "100.00"),
            &satz("A2", "EQUITY", "250.50"),
            &satz("A3", "FUND", "1.00"),
        ])),
        ("Werte vertauscht", dok(&[
            &satz("A1", "EQUITY", "250.50"),
            &satz("A2", "BOND", "100.00"),
        ])),
        ("Feld fehlt", "<INVESTMENTS><INVESTMENT><ID>A1</ID><TYP>BOND</TYP></INVESTMENT><INVESTMENT><ID>A2</ID><TYP>EQUITY</TYP><BETRAG>250.50</BETRAG></INVESTMENT></INVESTMENTS>".to_string()),
        ("Schlüssel geändert", dok(&[&satz("XX", "BOND", "100.00"), &satz("A2", "EQUITY", "250.50")])),
    ];

    for (was, x) in faelle {
        assert_ne!(
            abdruck_von(&x, 8).gesamt().wert,
            g0,
            "{was} hat den Gesamtwert nicht verändert"
        );
    }
}

/// Und die Grenze, ausdrücklich festgehalten: Gleichheit des Gesamtwerts ist
/// kein Beweis. Wären zwei Zellhashes gegeneinander vertauscht, bliebe er
/// gleich — die Ränder nicht. Der Test hält fest, dass die Ränder die
/// eigentliche Prüfung sind.
#[test]
fn raender_sagen_mehr_als_der_gesamtwert() {
    let a = dok(&[&satz("A1", "BOND", "100.00")]);
    let x = abdruck_von(&a, 6);
    assert!(
        x.spalten.len() >= 3,
        "die Ränder tragen die Einzelheiten, der Gesamtwert nur das Urteil"
    );
}

// ------------------------------------ Struktur, Anordnung, Ausprägungen
//
// Drei Ebenen, die getrennt anschlagen müssen. Verschmelzen sie zu einer
// Aussage, sagt der Bericht nur „Feld X weicht ab" — und verschweigt, ob dort
// ein Feld umbenannt, umgestellt oder nur ein Wert geändert wurde. Das sind
// drei verschiedene Probleme mit drei verschiedenen Ursachen.

/// Nur ein Wert ändert sich: Struktur und Anordnung müssen gleich bleiben.
#[test]
fn wertaenderung_laesst_struktur_und_anordnung_unberuehrt() {
    let a = dok(&[&satz("A1", "BOND", "100.00"), &satz("A2", "EQUITY", "250.50")]);
    let b = dok(&[&satz("A1", "BOND", "999.99"), &satz("A2", "EQUITY", "250.50")]);
    let (x, y) = (abdruck_von(&a, 9).gesamt(), abdruck_von(&b, 9).gesamt());

    assert_eq!(x.struktur, y.struktur, "die Feldnamen sind unverändert");
    assert_eq!(x.anordnung, y.anordnung, "die Anordnung ist unverändert");
    assert_ne!(x.ausprägungen, y.ausprägungen, "der Wert muss auffallen");
}

/// Ein Feld heißt anders: Die Struktur muss anschlagen.
#[test]
fn umbenanntes_feld_faellt_der_struktur_auf() {
    let a = "<INVESTMENTS><INVESTMENT><ID>A1</ID><BETRAG>100.00</BETRAG></INVESTMENT></INVESTMENTS>";
    let b = "<INVESTMENTS><INVESTMENT><ID>A1</ID><AMOUNT>100.00</AMOUNT></INVESTMENT></INVESTMENTS>";
    let (x, y) = (abdruck_von(a, 7).gesamt(), abdruck_von(b, 7).gesamt());
    assert_ne!(x.struktur, y.struktur, "ein anderer Feldname ist Struktur");
}

/// Dieselben Felder, dieselben Werte — nur anders angeordnet. Genau der Fall,
/// den weder Struktur noch Ausprägungen sehen können.
#[test]
fn umgestellte_felder_fallen_nur_der_anordnung_auf() {
    let a = "<INVESTMENTS><INVESTMENT><ID>A1</ID><TYP>BOND</TYP><BETRAG>100.00</BETRAG></INVESTMENT></INVESTMENTS>";
    let b = "<INVESTMENTS><INVESTMENT><ID>A1</ID><BETRAG>100.00</BETRAG><TYP>BOND</TYP></INVESTMENT></INVESTMENTS>";
    let (x, y) = (abdruck_von(a, 8).gesamt(), abdruck_von(b, 8).gesamt());

    assert_eq!(x.struktur, y.struktur, "es sind dieselben Felder");
    assert_eq!(
        x.ausprägungen, y.ausprägungen,
        "es sind dieselben Werte in denselben Feldern"
    );
    assert_ne!(
        x.anordnung, y.anordnung,
        "die Umstellung darf nur hier auffallen — sonst wäre sie von einer \
         Wertänderung nicht zu unterscheiden"
    );
}

/// Die Indextabelle muss die Abweichung benennen können, nicht nur melden.
/// Ein Hash sagt „anders"; die Indextabelle sagt „an Stelle 2 steht BETRAG
/// statt TYP".
#[test]
fn indextabelle_benennt_die_stelle_der_umstellung() {
    let a = "<INVESTMENTS><INVESTMENT><ID>A1</ID><TYP>BOND</TYP><BETRAG>100.00</BETRAG></INVESTMENT></INVESTMENTS>";
    let b = "<INVESTMENTS><INVESTMENT><ID>A1</ID><BETRAG>100.00</BETRAG><TYP>BOND</TYP></INVESTMENT></INVESTMENTS>";

    let (x, y) = (abdruck_von(a, 8), abdruck_von(b, 8));
    let namen = |ab: &Abdruck| {
        let (folge, _) = ab.haupt_anordnung().unwrap();
        folge
            .iter()
            .map(|n| String::from_utf8_lossy(ab.feldname(*n)).to_string())
            .collect::<Vec<_>>()
    };

    assert_eq!(namen(&x), vec!["ID", "TYP", "BETRAG"]);
    assert_eq!(namen(&y), vec!["ID", "BETRAG", "TYP"]);

    // Beide kennen dieselben Felder — nur an anderer Stelle.
    let mut fx = namen(&x);
    let mut fy = namen(&y);
    fx.sort();
    fy.sort();
    assert_eq!(fx, fy, "es sind dieselben Felder");
}

/// Die Indextabelle darf nicht von der Nummerierung abhängen: Zwei Dateien
/// mit derselben Anordnung, aber anderer Reihenfolge des ersten Auftretens
/// im Dokument, müssen denselben Anordnungswert ergeben.
#[test]
fn anordnungswert_haengt_nicht_an_der_nummerierung() {
    // In beiden Dateien haben die Datensätze dieselbe Feldfolge — aber der
    // Bestand beginnt mit unterschiedlich vollständigen Datensätzen, was die
    // Nummernvergabe verschiebt.
    let a = "<INVESTMENTS>\
        <INVESTMENT><ID>1</ID><TYP>x</TYP></INVESTMENT>\
        <INVESTMENT><ID>2</ID><TYP>y</TYP></INVESTMENT></INVESTMENTS>";
    let b = "<INVESTMENTS>\
        <INVESTMENT><ID>3</ID><TYP>z</TYP></INVESTMENT>\
        <INVESTMENT><ID>4</ID><TYP>w</TYP></INVESTMENT></INVESTMENTS>";
    assert_eq!(
        abdruck_von(a, 9).gesamt().anordnung,
        abdruck_von(b, 9).gesamt().anordnung
    );
}

/// Bei einheitlichem Aufbau gibt es genau eine Anordnung; füllt die Quelle
/// optionale Felder unterschiedlich, mehrere. Auch das ist ein Befund.
#[test]
fn zahl_der_anordnungen_zeigt_wie_einheitlich_der_aufbau_ist() {
    let einheitlich = dok(&[
        &satz("A1", "BOND", "100.00"),
        &satz("A2", "EQUITY", "250.50"),
        &satz("A3", "FUND", "7.25"),
    ]);
    assert_eq!(abdruck_von(&einheitlich, 9).anordnungen.len(), 1);

    let gemischt = "<INVESTMENTS>\
        <INVESTMENT><ID>A1</ID><TYP>BOND</TYP><BETRAG>1</BETRAG></INVESTMENT>\
        <INVESTMENT><ID>A2</ID><BETRAG>2</BETRAG></INVESTMENT>\
        <INVESTMENT><ID>A3</ID><TYP>FUND</TYP></INVESTMENT></INVESTMENTS>";
    assert_eq!(
        abdruck_von(gemischt, 9).anordnungen.len(),
        3,
        "drei verschiedene Feldfolgen"
    );
}

/// Und die Zusammenfassung muss alle drei tragen.
#[test]
fn der_gesamtwert_deckt_alle_drei_ebenen_ab() {
    let grund = "<INVESTMENTS><INVESTMENT><ID>A1</ID><TYP>BOND</TYP><BETRAG>100.00</BETRAG></INVESTMENT></INVESTMENTS>";
    let g = abdruck_von(grund, 7).gesamt().wert;

    let umbenannt = "<INVESTMENTS><INVESTMENT><ID>A1</ID><ART>BOND</ART><BETRAG>100.00</BETRAG></INVESTMENT></INVESTMENTS>";
    let umgestellt = "<INVESTMENTS><INVESTMENT><ID>A1</ID><BETRAG>100.00</BETRAG><TYP>BOND</TYP></INVESTMENT></INVESTMENTS>";
    let wert = "<INVESTMENTS><INVESTMENT><ID>A1</ID><TYP>BOND</TYP><BETRAG>1.00</BETRAG></INVESTMENT></INVESTMENTS>";

    for (was, x) in [("umbenannt", umbenannt), ("umgestellt", umgestellt), ("Wert", wert)] {
        assert_ne!(
            abdruck_von(x, 7).gesamt().wert,
            g,
            "{was} muss schon im Gesamtwert auffallen"
        );
    }
}

#[test]
fn datensaetze_ohne_schluessel_werden_gezaehlt() {
    let x = "<INVESTMENTS><INVESTMENT><TYP>BOND</TYP></INVESTMENT><INVESTMENT><ID>A2</ID></INVESTMENT></INVESTMENTS>";
    let a = abdruck_von(x, 9);
    assert_eq!(a.datensaetze, 2);
    assert_eq!(a.ohne_schluessel, 1, "der Datensatz ohne ID muss auffallen");
}

#[test]
fn tief_verschachteltes_wird_zu_pfaden() {
    let x = "<INVESTMENTS><INVESTMENT><ID>A1</ID><A><B><C>tief</C></B></A></INVESTMENT></INVESTMENTS>";
    let a = abdruck_von(x, 4);
    let (s, _, _) = kennwerte(&a);
    let pfade: Vec<&str> = s.iter().map(|x| x.0.as_str()).collect();
    assert!(
        pfade.contains(&"A/B/C"),
        "Verschachtelung muss als Pfad erscheinen, gefunden: {pfade:?}"
    );
}

/// Derselbe Pfad mehrfach im Datensatz — beide Werte müssen eingehen.
#[test]
fn wiederholte_pfade_gehen_beide_ein() {
    let eins = "<INVESTMENTS><INVESTMENT><ID>A1</ID><KZ>a</KZ></INVESTMENT></INVESTMENTS>";
    let zwei = "<INVESTMENTS><INVESTMENT><ID>A1</ID><KZ>a</KZ><KZ>b</KZ></INVESTMENT></INVESTMENTS>";
    let a1 = abdruck_von(eins, 6);
    let a2 = abdruck_von(zwei, 6);
    let n1 = a1.spalten.get(b"KZ".as_slice()).unwrap().anzahl;
    let n2 = a2.spalten.get(b"KZ".as_slice()).unwrap().anzahl;
    assert_eq!((n1, n2), (1, 2));
}
