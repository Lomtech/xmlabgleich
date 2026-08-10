//! Erkundung: Was steckt überhaupt in dieser Datei?
//!
//! Vor dem Abgleich muss zweierlei feststehen — welches Element einen
//! Datensatz bildet, und welches Feld ihn identifiziert. Beides lässt sich
//! nicht raten, aber gut vorschlagen: Der Datensatz ist das Element, das am
//! häufigsten auf möglichst geringer Tiefe vorkommt; als Schlüssel taugen
//! Felder, die in jedem Datensatz genau einmal auftreten und dabei
//! unterschiedliche Werte tragen.

use crate::scanner::Beobachter;
use std::collections::HashMap;

#[derive(Default, Clone)]
pub struct Vorkommen {
    pub anzahl: u64,
    pub tiefe: usize,
    /// Größte Zahl gleichnamiger Geschwister unter einem Elternelement.
    ///
    /// Das und nicht die Gesamtzahl entscheidet, was ein Datensatz ist: In
    /// einer echten Datei kam `INVESTMENTIDENTIFIER` 2.406-mal vor und
    /// `INVESTMENT` nur 401-mal — trotzdem ist `INVESTMENT` der Datensatz,
    /// denn die Bezeichner stehen zu sechst *innerhalb* je eines Investments.
    /// Nach Gesamtzahl gewählt, hätte das Werkzeug die falsche Ebene
    /// abgeglichen und alles Übergeordnete übersehen.
    pub geschwister: u64,
    /// Nur für Blätter: verschiedene Werte, bis zur Grenze gezählt.
    pub verschiedene: usize,
    /// Ist der Pfad ein Blatt (trägt Werte) oder nur Struktur?
    pub blatt: bool,
    /// Der Elternteil enthält ausschließlich gleichnamige Kinder — er ist
    /// ein reiner Sammelcontainer (`INVESTMENTS` → `INVESTMENT`).
    pub nur_kind: bool,
    /// Wie viele verschiedene Kindnamen dieses Element trägt. Ein Datensatz
    /// hat Felder (mehrere Namen); eine Durchgangsstation hat einen.
    pub kindnamen: usize,
}

/// Über so viele verschiedene Werte hinaus wird nicht mehr gezählt. Für die
/// Frage „taugt das als Schlüssel" reicht das aus und der Speicher bleibt
/// beschränkt — sonst würde eine Datei mit Millionen Datensätzen die
/// Erkundung so teuer wie den Abgleich.
const WERTE_GRENZE: usize = 100_000;

pub struct Erkundung {
    pfad: Vec<Vec<u8>>,
    hat_kinder: Vec<bool>,
    /// Je offener Ebene: wie oft jeder Kindname dort schon vorkam.
    kinder: Vec<HashMap<Vec<u8>, u64>>,
    text_hash: u32,
    text_hat_inhalt: bool,
    pub pfade: HashMap<Vec<u8>, Vorkommen>,
    /// Werthashes je Blattpfad, um Eindeutigkeit zu beurteilen.
    werte: HashMap<Vec<u8>, std::collections::HashSet<u32>>,
    pub elemente: u64,
}

const FNV_ANFANG: u32 = 2166136261;
const FNV_PRIM: u32 = 16777619;

impl Default for Erkundung {
    fn default() -> Self {
        Self::neu()
    }
}

impl Erkundung {
    pub fn neu() -> Self {
        Erkundung {
            pfad: Vec::new(),
            hat_kinder: Vec::new(),
            kinder: vec![HashMap::new()],
            text_hash: FNV_ANFANG,
            text_hat_inhalt: false,
            pfade: HashMap::new(),
            werte: HashMap::new(),
            elemente: 0,
        }
    }

    fn voller_pfad(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (i, t) in self.pfad.iter().enumerate() {
            if i > 0 {
                out.push(b'/');
            }
            out.extend_from_slice(t);
        }
        out
    }

    /// Vorschlag für das Datensatz-Element: das mit den meisten
    /// gleichnamigen Geschwistern, bei Gleichstand das flachere. Genau die
    /// Wahl, die ein Mensch beim Blick auf die Datei träfe.
    ///
    /// Diese Wahl allein versagt aber, sobald eine Datei nur **einen**
    /// Datensatz enthält: Dann schlägt jede Wiederholgruppe darin den
    /// Datensatz. Gemessen an einer echten Einzelsatz-Datei kam
    /// `INVESTMENTCLASSIFICATION` siebenmal vor, `INVESTMENT` einmal — das
    /// Werkzeug hätte die Klassifizierungen abgeglichen und den Datensatz,
    /// um den es geht, nie angesehen.
    ///
    /// Darum zuerst die Bauform: Liegt ein Element unter einem reinen
    /// Sammelcontainer und trägt selbst mehrere verschiedene Kindnamen —
    /// hat also Felder statt nur einer Fortsetzung —, und liegt es *flacher*
    /// als der Vielgeschwister-Kandidat, dann ist es der Datensatz. Bei
    /// gleicher Tiefe behält die Geschwisterzahl das letzte Wort.
    pub fn datensatz_vorschlag(&self) -> Option<(Vec<u8>, u64)> {
        let nach_geschwistern = self
            .pfade
            .iter()
            .filter(|(_, v)| !v.blatt && v.geschwister > 1)
            .max_by(|a, b| {
                a.1.geschwister
                    .cmp(&b.1.geschwister)
                    .then_with(|| b.1.tiefe.cmp(&a.1.tiefe))
            });

        let nach_bauform = self
            .pfade
            .iter()
            .filter(|(_, v)| !v.blatt && v.nur_kind && v.kindnamen > 1)
            .min_by(|a, b| a.1.tiefe.cmp(&b.1.tiefe).then_with(|| a.0.cmp(b.0)));

        let wahl = match (nach_bauform, nach_geschwistern) {
            (Some(b), Some(g)) if b.1.tiefe < g.1.tiefe => b,
            (_, Some(g)) => g,
            (b, None) => b?,
        };
        Some((wahl.0.clone(), wahl.1.anzahl))
    }

    /// Elemente innerhalb des Datensatzes, die mehrfach nebeneinander
    /// stehen — Wiederholgruppen. Jede davon wird eine eigene Tabelle.
    ///
    /// Ohne diese Trennung landen alle Einträge einer Gruppe im selben
    /// Feldpfad, und ein Tausch zwischen ihnen bleibt unsichtbar.
    /// Zurückgegeben wird der Pfad relativ zum Datensatz, samt Vorkommen und
    /// größter Gruppengröße.
    ///
    /// Nur Gruppen mit eigenen Feldern werden vorgeschlagen. Ein wiederholtes
    /// **Blatt** — etwa mehrere `<TELEFON>` nebeneinander — bleibt eine
    /// Spalte; seine Werte gehen weiter in dieselbe Summe ein, und ein Tausch
    /// untereinander fällt dort nicht auf.
    pub fn untertabellen_vorschlaege(&self, datensatz: &[u8]) -> Vec<(Vec<u8>, u64, u64)> {
        let mut praefix = datensatz.to_vec();
        praefix.push(b'/');

        let mut treffer: Vec<(Vec<u8>, u64, u64)> = self
            .pfade
            .iter()
            .filter(|(p, v)| !v.blatt && v.geschwister > 1 && p.starts_with(&praefix))
            .map(|(p, v)| (p[praefix.len()..].to_vec(), v.anzahl, v.geschwister))
            .collect();
        // Flache zuerst — die äußere Gruppe muss vor der inneren angelegt
        // werden, sonst hängt die innere am falschen Elternteil.
        treffer.sort_by(|a, b| {
            a.0.iter()
                .filter(|c| **c == b'/')
                .count()
                .cmp(&b.0.iter().filter(|c| **c == b'/').count())
                .then_with(|| a.0.cmp(&b.0))
        });
        treffer
    }

    /// Felder, die als Schlüssel taugen: genau einmal je Datensatz und
    /// möglichst unterschiedliche Werte. Zurückgegeben wird der Pfad relativ
    /// zum Datensatz, die Zahl der Vorkommen und die der verschiedenen Werte.
    pub fn schluessel_vorschlaege(&self, datensatz: &[u8]) -> Vec<(Vec<u8>, u64, usize)> {
        let d_anzahl = match self.pfade.get(datensatz) {
            Some(v) => v.anzahl,
            None => return Vec::new(),
        };
        let mut praefix = datensatz.to_vec();
        praefix.push(b'/');

        let mut kandidaten: Vec<(Vec<u8>, u64, usize)> = self
            .pfade
            .iter()
            .filter(|(p, v)| v.blatt && p.starts_with(&praefix))
            .filter(|(_, v)| v.anzahl == d_anzahl)
            .map(|(p, v)| (p[praefix.len()..].to_vec(), v.anzahl, v.verschiedene))
            .collect();

        // Eindeutigste zuerst — ein Feld mit so vielen verschiedenen Werten
        // wie Datensätzen ist ein natürlicher Schlüssel.
        kandidaten.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        kandidaten
    }
}

impl Beobachter for Erkundung {
    fn element_beginn(&mut self, name: &[u8]) {
        if let Some(l) = self.hat_kinder.last_mut() {
            *l = true;
        }

        // Wie oft steht dieser Name unter dem aktuellen Elternelement schon?
        let geschwister = {
            let ebene = self.kinder.last_mut().expect("Wurzelebene fehlt");
            let z = ebene.entry(name.to_vec()).or_insert(0);
            *z += 1;
            *z
        };

        self.pfad.push(name.to_vec());
        self.hat_kinder.push(false);
        self.kinder.push(HashMap::new());
        self.text_hash = FNV_ANFANG;
        self.text_hat_inhalt = false;
        self.elemente += 1;

        let p = self.voller_pfad();
        let tiefe = self.pfad.len();
        let e = self.pfade.entry(p).or_insert(Vorkommen {
            tiefe,
            ..Default::default()
        });
        e.anzahl += 1;
        if geschwister > e.geschwister {
            e.geschwister = geschwister;
        }
    }

    fn element_ende(&mut self) {
        // Enthält dieses Element nur gleichnamige Kinder, ist es ein reiner
        // Sammelcontainer und sein Kind ein Datensatz-Kandidat. Die Wurzel-
        // ebene wird nie geschlossen, das Dokumentelement fällt darum von
        // selbst heraus.
        let (kindnamen, einziger) = {
            let k = self.kinder.last().expect("Ebene fehlt");
            (
                k.len(),
                if k.len() == 1 {
                    k.keys().next().cloned()
                } else {
                    None
                },
            )
        };
        let hier = self.voller_pfad();
        if let Some(e) = self.pfade.get_mut(&hier) {
            if kindnamen > e.kindnamen {
                e.kindnamen = kindnamen;
            }
        }
        if let Some(name) = einziger {
            let mut kindpfad = hier.clone();
            kindpfad.push(b'/');
            kindpfad.extend_from_slice(&name);
            if let Some(e) = self.pfade.get_mut(&kindpfad) {
                e.nur_kind = true;
            }
        }

        let ist_blatt = !*self.hat_kinder.last().unwrap_or(&false);
        if ist_blatt {
            let p = self.voller_pfad();
            let h = if self.text_hat_inhalt {
                self.text_hash
            } else {
                0
            };
            if let Some(e) = self.pfade.get_mut(&p) {
                e.blatt = true;
            }
            let menge = self.werte.entry(p.clone()).or_default();
            if menge.len() < WERTE_GRENZE {
                menge.insert(h);
            }
            let n = menge.len();
            if let Some(e) = self.pfade.get_mut(&p) {
                e.verschiedene = n;
            }
        }
        self.pfad.pop();
        self.hat_kinder.pop();
        self.kinder.pop();
        self.text_hash = FNV_ANFANG;
        self.text_hat_inhalt = false;
    }

    fn attribut(&mut self, name: &[u8], _wert: &[u8]) {
        let mut p = self.voller_pfad();
        p.push(b'@');
        p.extend_from_slice(name);
        let tiefe = self.pfad.len();
        let e = self.pfade.entry(p).or_insert(Vorkommen {
            tiefe,
            ..Default::default()
        });
        e.anzahl += 1;
        e.blatt = true;
    }

    fn text(&mut self, teil: &[u8]) {
        for &c in teil {
            if c.is_ascii_whitespace() {
                continue;
            }
            self.text_hash ^= c as u32;
            self.text_hash = self.text_hash.wrapping_mul(FNV_PRIM);
            self.text_hat_inhalt = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::Scanner;

    fn erkunde(xml: &str) -> Erkundung {
        let mut s = Scanner::neu();
        let mut e = Erkundung::neu();
        s.block(xml.as_bytes(), &mut e);
        s.abschliessen(&mut e);
        e
    }

    #[test]
    fn findet_das_datensatz_element() {
        let x = "<WURZEL><KOPF><A>1</A></KOPF><LISTE>\
                 <POSTEN><ID>1</ID></POSTEN>\
                 <POSTEN><ID>2</ID></POSTEN>\
                 <POSTEN><ID>3</ID></POSTEN></LISTE></WURZEL>";
        let (pfad, n) = erkunde(x).datensatz_vorschlag().unwrap();
        assert_eq!(String::from_utf8_lossy(&pfad), "WURZEL/LISTE/POSTEN");
        assert_eq!(n, 3);
    }

    /// Der Fall aus einer echten Datei: Ein tiefer liegendes Element kommt
    /// insgesamt häufiger vor als der Datensatz, steht aber nur zu wenigen
    /// nebeneinander. Nach Gesamtzahl gewählt, wäre die falsche Ebene
    /// abgeglichen worden.
    #[test]
    fn haeufigeres_kindelement_verdraengt_den_datensatz_nicht() {
        let mut x = String::from("<WURZEL><LISTE>");
        for i in 0..50 {
            x.push_str("<POSTEN><KENNUNGEN>");
            for j in 0..6 {
                x.push_str(&format!("<KENNUNG>{i}-{j}</KENNUNG>"));
            }
            x.push_str("</KENNUNGEN></POSTEN>");
        }
        x.push_str("</LISTE></WURZEL>");

        let e = erkunde(&x);
        // KENNUNG kommt 300-mal vor, POSTEN nur 50-mal …
        assert_eq!(e.pfade[b"WURZEL/LISTE/POSTEN/KENNUNGEN/KENNUNG".as_slice()].anzahl, 300);
        assert_eq!(e.pfade[b"WURZEL/LISTE/POSTEN".as_slice()].anzahl, 50);
        // … aber POSTEN steht zu 50 nebeneinander, KENNUNG nur zu 6.
        let (pfad, n) = e.datensatz_vorschlag().unwrap();
        assert_eq!(String::from_utf8_lossy(&pfad), "WURZEL/LISTE/POSTEN");
        assert_eq!(n, 50);
    }

    /// Der Fall aus den Einzelsatz-Dateien: Genau ein Datensatz, aber
    /// Wiederholgruppen darin. Nach Geschwisterzahl gewählt, hätte das
    /// Werkzeug die Klassifizierungen abgeglichen statt des Investments.
    #[test]
    fn einzelner_datensatz_wird_nicht_von_seinen_gruppen_verdraengt() {
        let mut x = String::from("<STAMM><KOPF><NR>1</NR></KOPF><POSTEN><P>");
        x.push_str("<KENNUNGEN>");
        for i in 0..5 {
            x.push_str(&format!("<KENNUNG><SYS>S{i}</SYS><WERT>{i}</WERT></KENNUNG>"));
        }
        x.push_str("</KENNUNGEN><KLASSEN>");
        for i in 0..7 {
            x.push_str(&format!("<KLASSE><ART>A{i}</ART><WERT>{i}</WERT></KLASSE>"));
        }
        x.push_str("</KLASSEN><ALLGEMEIN><NAME>x</NAME></ALLGEMEIN></P></POSTEN></STAMM>");

        let e = erkunde(&x);
        // KLASSE steht zu sieben nebeneinander, P nur ein einziges Mal …
        assert_eq!(e.pfade[b"STAMM/POSTEN/P/KLASSEN/KLASSE".as_slice()].geschwister, 7);
        assert_eq!(e.pfade[b"STAMM/POSTEN/P".as_slice()].geschwister, 1);
        // … trotzdem ist P der Datensatz: Es liegt unter einem reinen
        // Sammelcontainer und trägt eigene Felder.
        let (pfad, n) = e.datensatz_vorschlag().unwrap();
        assert_eq!(String::from_utf8_lossy(&pfad), "STAMM/POSTEN/P");
        assert_eq!(n, 1);
    }

    /// Gegenprobe: Sobald der Datensatz wiederholt vorkommt, bleibt die
    /// Geschwisterzahl maßgeblich — die Bauform darf sie nicht überstimmen,
    /// wenn beide auf dieselbe Ebene zeigen.
    #[test]
    fn viele_datensaetze_bleiben_bei_der_geschwisterzahl() {
        let mut x = String::from("<STAMM><POSTEN>");
        for i in 0..20 {
            x.push_str(&format!(
                "<P><KENNUNGEN><KENNUNG><SYS>S</SYS><WERT>{i}</WERT></KENNUNG>\
                 <KENNUNG><SYS>T</SYS><WERT>{i}</WERT></KENNUNG></KENNUNGEN>\
                 <NAME>n{i}</NAME></P>"
            ));
        }
        x.push_str("</POSTEN></STAMM>");
        let (pfad, n) = erkunde(&x).datensatz_vorschlag().unwrap();
        assert_eq!(String::from_utf8_lossy(&pfad), "STAMM/POSTEN/P");
        assert_eq!(n, 20);
    }

    #[test]
    fn schlaegt_eindeutige_felder_als_schluessel_vor() {
        let x = "<L><P><ID>1</ID><TYP>x</TYP></P>\
                 <P><ID>2</ID><TYP>x</TYP></P>\
                 <P><ID>3</ID><TYP>x</TYP></P></L>";
        let e = erkunde(x);
        let v = e.schluessel_vorschlaege(b"L/P");
        // ID hat drei verschiedene Werte, TYP nur einen — ID muss vorn stehen.
        assert_eq!(String::from_utf8_lossy(&v[0].0), "ID");
        assert_eq!(v[0].2, 3);
        assert_eq!(String::from_utf8_lossy(&v[1].0), "TYP");
        assert_eq!(v[1].2, 1);
    }

    #[test]
    fn felder_die_nicht_in_jedem_datensatz_stehen_taugen_nicht() {
        let x = "<L><P><ID>1</ID><OPT>a</OPT></P><P><ID>2</ID></P></L>";
        let v = erkunde(x).schluessel_vorschlaege(b"L/P");
        let namen: Vec<String> = v
            .iter()
            .map(|k| String::from_utf8_lossy(&k.0).to_string())
            .collect();
        assert!(namen.contains(&"ID".to_string()));
        assert!(
            !namen.contains(&"OPT".to_string()),
            "OPT kommt nur in einem von zwei Datensätzen vor"
        );
    }

    #[test]
    fn wiederholgruppen_werden_als_untertabellen_vorgeschlagen() {
        let x = "<L><P><ID>1</ID>\
            <KENNUNGEN><KENNUNG><A>x</A></KENNUNG><KENNUNG><A>y</A></KENNUNG></KENNUNGEN>\
            <EINZELN><B>z</B></EINZELN></P>\
            <P><ID>2</ID>\
            <KENNUNGEN><KENNUNG><A>q</A></KENNUNG></KENNUNGEN>\
            <EINZELN><B>w</B></EINZELN></P></L>";
        let v = erkunde(x).untertabellen_vorschlaege(b"L/P");
        let namen: Vec<String> = v
            .iter()
            .map(|k| String::from_utf8_lossy(&k.0).to_string())
            .collect();
        assert!(
            namen.contains(&"KENNUNGEN/KENNUNG".to_string()),
            "die Wiederholgruppe fehlt: {namen:?}"
        );
        assert!(
            !namen.contains(&"EINZELN".to_string()),
            "was nur einmal vorkommt, ist keine Gruppe: {namen:?}"
        );
    }

    /// Die äußere Gruppe muss vor der inneren stehen, sonst wird die innere
    /// am falschen Elternteil angelegt.
    #[test]
    fn aeussere_gruppen_kommen_zuerst() {
        let x = "<L><P><ID>1</ID><AS>\
            <A><BS><B><X>1</X></B><B><X>2</X></B></BS></A>\
            <A><BS><B><X>3</X></B></BS></A>\
            </AS></P></L>";
        let v = erkunde(x).untertabellen_vorschlaege(b"L/P");
        let namen: Vec<String> = v
            .iter()
            .map(|k| String::from_utf8_lossy(&k.0).to_string())
            .collect();
        let i_a = namen.iter().position(|n| n == "AS/A");
        let i_b = namen.iter().position(|n| n == "AS/A/BS/B");
        assert!(i_a.is_some() && i_b.is_some(), "{namen:?}");
        assert!(i_a < i_b, "die äußere Gruppe muss zuerst kommen: {namen:?}");
    }

    #[test]
    fn tiefe_pfade_werden_erfasst() {
        let x = "<L><P><A><B>tief</B></A></P><P><A><B>auch</B></A></P></L>";
        let e = erkunde(x);
        assert!(e.pfade.contains_key(b"L/P/A/B".as_slice()));
        assert_eq!(e.pfade[b"L/P/A/B".as_slice()].anzahl, 2);
        assert!(e.pfade[b"L/P/A/B".as_slice()].blatt);
        assert!(!e.pfade[b"L/P/A".as_slice()].blatt);
    }
}
