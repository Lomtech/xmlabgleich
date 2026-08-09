//! Prüfungen für den Scanner.
//!
//! Die wichtigste steht ganz oben: Dieselben Daten müssen dieselben
//! Ereignisse liefern, gleichgültig wie sie in Blöcke zerfallen. Alles andere
//! im Werkzeug baut darauf auf.

use crate::scanner::{Beobachter, Scanner};

/// Sammelt die Ereignisse als lesbare Zeilen, damit Abweichungen im
/// Fehlerfall sofort erkennbar sind.
#[derive(Default)]
pub struct Mitschrift {
    pub zeilen: Vec<String>,
    laufender_text: String,
}

impl Mitschrift {
    fn text_abschliessen(&mut self) {
        if !self.laufender_text.is_empty() {
            // Nur bedeutsamen Text mitschreiben — Einrückung darf keine Rolle
            // spielen, sonst hinge alles an der Formatierung.
            let t = self.laufender_text.trim();
            if !t.is_empty() {
                self.zeilen.push(format!("text {t}"));
            }
            self.laufender_text.clear();
        }
    }
}

impl Beobachter for Mitschrift {
    fn element_beginn(&mut self, name: &[u8]) {
        self.text_abschliessen();
        self.zeilen
            .push(format!("auf  {}", String::from_utf8_lossy(name)));
    }
    fn element_ende(&mut self) {
        self.text_abschliessen();
        self.zeilen.push("zu".to_string());
    }
    fn text(&mut self, teil: &[u8]) {
        self.laufender_text
            .push_str(&String::from_utf8_lossy(teil));
    }
    fn attribut(&mut self, name: &[u8], wert: &[u8]) {
        self.zeilen.push(format!(
            "attr {}={}",
            String::from_utf8_lossy(name),
            String::from_utf8_lossy(wert)
        ));
    }
}

pub fn lies(daten: &[u8], blockgroesse: usize) -> Vec<String> {
    let mut s = Scanner::neu();
    let mut m = Mitschrift::default();
    let mut i = 0;
    while i < daten.len() {
        let bis = (i + blockgroesse).min(daten.len());
        s.block(&daten[i..bis], &mut m);
        i = bis;
    }
    s.abschliessen(&mut m);
    m.text_abschliessen();
    m.zeilen
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Test, auf dem alles steht.
    fn blockgroessen_unabhaengig(daten: &str) {
        let bytes = daten.as_bytes();
        let erwartet = lies(bytes, bytes.len().max(1));

        // Alle kleinen Blockgrößen und einige krumme dazu. Blockgröße 1 ist
        // der härteste Fall: Jede Kennung zerfällt in Einzelbytes.
        for g in [1, 2, 3, 4, 5, 7, 11, 13, 16, 31, 64, 127, 1000] {
            let gemessen = lies(bytes, g);
            assert_eq!(
                gemessen, erwartet,
                "\nBlockgröße {g} liefert etwas anderes.\nErwartet: {erwartet:#?}\nBekommen: {gemessen:#?}\nEingabe: {daten}"
            );
        }
    }

    #[test]
    fn schlichtes_dokument() {
        blockgroessen_unabhaengig("<A><B>eins</B><C>zwei</C></A>");
    }

    #[test]
    fn mit_deklaration_und_einrueckung() {
        blockgroessen_unabhaengig(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<WURZEL>\n  <KIND>Wert</KIND>\n</WURZEL>",
        );
    }

    #[test]
    fn selbstschliessende_elemente() {
        blockgroessen_unabhaengig("<A><LEER/><B>x</B><AUCH_LEER /></A>");
    }

    /// Ein `>` im Attributwert darf das Tag nicht beenden.
    #[test]
    fn spitzklammer_im_attribut() {
        blockgroessen_unabhaengig(r#"<A><B attr="x > y" zweit='a > b'>Wert</B></A>"#);
    }

    #[test]
    fn kommentare_werden_uebergangen() {
        blockgroessen_unabhaengig("<A><!-- ein <B> Kommentar --><C>echt</C></A>");
    }

    /// Im Kommentar dürfen Bindestriche stehen, ohne ihn zu beenden.
    #[test]
    fn kommentar_mit_bindestrichen() {
        blockgroessen_unabhaengig("<A><!-- a-b--c ---><B>x</B></A>");
    }

    #[test]
    fn cdata_wird_zu_text() {
        blockgroessen_unabhaengig("<A><B><![CDATA[roh <nicht> ein Tag]]></B></A>");
    }

    /// Eckige Klammern in CDATA dürfen den Abschluss nicht vortäuschen.
    #[test]
    fn cdata_mit_klammern() {
        blockgroessen_unabhaengig("<A><B><![CDATA[a]b]]c]]]></B></A>");
    }

    #[test]
    fn namensraeume_werden_verworfen() {
        let mit = lies(br#"<ns:A xmlns:ns="u"><ns:B>x</ns:B></ns:A>"#, 4);
        let ohne = lies(br#"<A><B>x</B></A>"#, 4);
        assert_eq!(mit, ohne, "Namensraum-Präfixe dürfen nichts ändern");
    }

    #[test]
    fn doctype_wird_uebergangen() {
        blockgroessen_unabhaengig("<!DOCTYPE A SYSTEM \"a.dtd\"><A><B>x</B></A>");
    }

    #[test]
    fn echte_struktur_aus_dem_bestand() {
        blockgroessen_unabhaengig(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<INVESTMENTMASTERDATA xmlns="http://beispiel.invalid/pruefdaten/v1.0">
<HEADER>
  <MESSAGEID>00000000000000000000000000000000</MESSAGEID>
  <DATECREATED>2026-02-25T15:28:53</DATECREATED>
</HEADER>
<INVESTMENTS>
  <INVESTMENT>
    <INVESTMENTIDENTIFIERS>
      <INVESTMENTIDENTIFIER>
        <INVESTMENTIDSYSTEM>ISIN</INVESTMENTIDSYSTEM>
        <INVESTMENTID>DE0001234567</INVESTMENTID>
      </INVESTMENTIDENTIFIER>
    </INVESTMENTIDENTIFIERS>
    <GENERALDATA>
      <INVESTMENTTYPE>BOND</INVESTMENTTYPE>
      <LISTED/>
    </GENERALDATA>
  </INVESTMENT>
</INVESTMENTS>
</INVESTMENTMASTERDATA>"#,
        );
    }

    /// Die Formatierung darf den Abdruck nicht verändern — sonst wäre jeder
    /// Vergleich zwischen einem eingerückten und einem kompakten Dokument
    /// wertlos.
    #[test]
    fn einrueckung_aendert_nichts() {
        let eingerueckt = lies(b"<A>\n  <B>x</B>\n  <C>y</C>\n</A>", 5);
        let kompakt = lies(b"<A><B>x</B><C>y</C></A>", 5);
        assert_eq!(eingerueckt, kompakt);
    }

    /// Windows-Zeilenenden dürfen ebenfalls nichts ändern.
    #[test]
    fn zeilenenden_aendern_nichts() {
        let crlf = lies(b"<A>\r\n  <B>x</B>\r\n</A>", 3);
        let lf = lies(b"<A>\n  <B>x</B>\n</A>", 3);
        assert_eq!(crlf, lf);
    }

    #[test]
    fn inhalt_wird_richtig_gelesen() {
        let z = lies(b"<A><B>eins</B><C/><D>zwei</D></A>", 3);
        assert_eq!(
            z,
            vec![
                "auf  A", "auf  B", "text eins", "zu", "auf  C", "zu", "auf  D", "text zwei", "zu",
                "zu"
            ]
        );
    }

    // ----------------------------------------------------------- Attribute

    #[test]
    fn attribute_werden_gemeldet() {
        let z = lies(br#"<A><B art="x" nr='7'>Wert</B></A>"#, 5);
        assert_eq!(
            z,
            vec!["auf  A", "auf  B", "attr art=x", "attr nr=7", "text Wert", "zu", "zu"]
        );
    }

    #[test]
    fn attribute_ueberstehen_blockgrenzen() {
        blockgroessen_unabhaengig(r#"<A><B art="ein langer Wert" nr="42"/><C d="x">t</C></A>"#);
    }

    /// Der Fall aus ISO 20022: Die Währung steht im Attribut. Zwei Beträge
    /// mit verschiedener Währung dürfen nicht als gleich gelten.
    #[test]
    fn waehrung_im_attribut_faellt_auf() {
        let eur = lies(br#"<Ntry><Amt Ccy="EUR">1234.56</Amt></Ntry>"#, 6);
        let usd = lies(br#"<Ntry><Amt Ccy="USD">1234.56</Amt></Ntry>"#, 6);
        assert_ne!(eur, usd, "die Währung darf nicht unter den Tisch fallen");
        assert!(eur.contains(&"attr Ccy=EUR".to_string()));
    }

    /// Namensraum-Erklärungen sind Metadaten des Dokuments. Sie dürfen den
    /// Abdruck nicht verändern, sonst hinge er am gewählten Präfix.
    #[test]
    fn xmlns_wird_nicht_als_attribut_gemeldet() {
        let mit = lies(br#"<A xmlns="http://beispiel.invalid/v1"><B>x</B></A>"#, 7);
        let ohne = lies(br#"<A><B>x</B></A>"#, 7);
        assert_eq!(mit, ohne);
    }

    /// Ein `>` im Attributwert darf weder das Tag beenden noch den Wert
    /// zerschneiden.
    #[test]
    fn spitzklammer_im_attributwert_bleibt_erhalten() {
        let z = lies(br#"<A><B bed="x > y">t</B></A>"#, 4);
        assert!(z.contains(&"attr bed=x > y".to_string()), "{z:?}");
    }

    /// Ein Element, dessen Name genau am Dateiende aufhört.
    #[test]
    fn abgeschnittenes_dokument() {
        let z = lies(b"<A><B", 2);
        assert_eq!(z, vec!["auf  A", "auf  B"]);
    }
}
