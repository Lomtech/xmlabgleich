//! Streaming-Scanner für XML.
//!
//! Liest beliebig große Dokumente in Blöcken und meldet Elementanfänge,
//! Elementenden und Text. Der gesamte Zustand steckt in dieser Struktur —
//! wo ein Block endet, ist damit gleichgültig. Genau darauf kommt es an: Ein
//! Fingerabdruck, der davon abhängt, wie die Datei zufällig in Blöcke
//! zerfällt, wäre wertlos.
//!
//! Bewusst kein vollständiger XML-Verarbeiter. Es wird nicht geprüft, ob das
//! Dokument gültig ist, und Entitäten werden nicht aufgelöst. Was zählt, ist
//! dass zwei Dokumente gleich gelesen werden — nicht, dass sie einer Norm
//! genügen.

/// Was der Scanner meldet.
pub trait Beobachter {
    /// Öffnendes Element. Der Name ist ohne Namensraum-Präfix.
    fn element_beginn(&mut self, name: &[u8]);
    /// Schließendes Element. Auch bei `<TAG/>`, unmittelbar nach dem Beginn.
    fn element_ende(&mut self);
    /// Ein Stück Text. Kann je Element mehrfach kommen, wenn der Text über
    /// eine Blockgrenze läuft.
    fn text(&mut self, teil: &[u8]);
    /// Ein Attribut des zuletzt geöffneten Elements.
    ///
    /// Bei ISO 20022 steckt die Währung im Attribut — `<Amt Ccy="EUR">12.34`.
    /// Wer nur Elementinhalte vergleicht, hält Beträge ohne Währung
    /// gegeneinander und merkt eine Umstellung von EUR auf USD nicht.
    fn attribut(&mut self, _name: &[u8], _wert: &[u8]) {}
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Zustand {
    /// Zwischen Tags.
    Text,
    /// `<` gelesen, noch unklar was folgt — wird im Merker gesammelt.
    NachSpitz,
    /// Im Namen eines öffnenden Elements.
    Name,
    /// Im Namen eines schließenden Elements.
    SchlussName,
    /// Hinter dem Namen: Attribute, bis `>` oder `/>`.
    ImTag,
    /// Im Namen eines Attributs.
    AttributName,
    /// Zwischen Attributname und Anführungszeichen (Gleichheitszeichen,
    /// Leerraum).
    VorAttributWert,
    /// In einem Attributwert; das Zeichen ist das öffnende Anführungszeichen.
    /// Nötig, weil `>` innerhalb eines Attributwerts erlaubt ist.
    ImAttribut(u8),
    /// `<!-- … -->`
    Kommentar,
    /// `<![CDATA[ … ]]>` — der Inhalt ist Text, auch wenn `<` darin steht.
    CData,
    /// `<? … ?>` — endet auf `?>`.
    Anweisung,
    /// `<!DOCTYPE …>`, `<!ENTITY …>` und alles andere mit `!`, das weder
    /// Kommentar noch CDATA ist. Endet auf einem `>`, **nicht** auf `?>`.
    ///
    /// Das ist kein Detail: Wurde eine Deklaration wie `<? … ?>` behandelt,
    /// suchte der Scanner ein `?>`, das nie kam — und verwarf den Rest der
    /// Datei. Eine Datei mit DOCTYPE lieferte dadurch null Elemente, und zwei
    /// verschiedene solche Dateien hatten beide einen leeren Abdruck, galten
    /// also als gleich.
    ///
    /// Der Zähler ist die Schachtelungstiefe des internen Subsets
    /// (`<!DOCTYPE a [ <!ENTITY …> ]>`): Innerhalb der eckigen Klammern
    /// beendet ein `>` die Deklaration nicht.
    Erklaerung(u32),
}

/// Längste Zeichenfolge, die nach `<` geprüft werden muss: `![CDATA[`.
const MERKER_GROESSE: usize = 8;
/// Längster Elementname, den wir am Stück halten. Ein unbegrenzter Puffer
/// wäre ein Einfallstor; solche Namen kommen in Datenaustauschformaten
/// ohnehin nicht vor.
///
/// Der überzählige Teil wird nicht verworfen, sondern in einen Hash
/// gerechnet, der dem gemeldeten Namen angehängt wird. Sonst wären zwei
/// verschiedene Namen mit gleichen ersten 256 Zeichen für das Werkzeug
/// dasselbe Element — ein stilles „kein Unterschied".
const NAME_GROESSE: usize = 256;

/// Ebenso für Attributwerte. Auch hier gilt: was nicht mehr in den Puffer
/// passt, geht in den Überlauf-Hash statt verloren.
const ATTRIBUTWERT_GROESSE: usize = 4096;

const FNV_ANFANG: u32 = 2166136261;
const FNV_PRIM: u32 = 16777619;

/// Rechnet ein Byte in den Überlauf-Hash ein. Der Hash beginnt bei 0 und
/// bleibt 0, solange nichts überlief — so ist „kein Überlauf" von „Überlauf,
/// dessen Hash zufällig 0 wäre" unterscheidbar.
#[inline]
fn ueberlauf_weiter(h: u32, b: u8) -> u32 {
    let h = if h == 0 { FNV_ANFANG } else { h };
    let h = (h ^ b as u32).wrapping_mul(FNV_PRIM);
    if h == 0 { 1 } else { h }
}

/// Hängt die Überlauf-Kennung an. Sichtbar, damit im Bericht erkennbar
/// bleibt, dass hier gekürzt wurde.
fn mit_ueberlauf(kopf: &[u8], hash: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(kopf.len() + 12);
    v.extend_from_slice(kopf);
    v.extend_from_slice(format!("…+{hash:08x}").as_bytes());
    v
}

pub struct Scanner {
    zustand: Zustand,
    /// Ist noch kein Byte gelesen? Nur dann wird auf Kodierung geprüft.
    erster_block: bool,
    /// Die Datei ist nicht UTF-8 (UTF-16 mit oder ohne Kennung). Der Scanner
    /// liefert dann Unsinn statt eines Fehlers: In UTF-16LE steht hinter dem
    /// `<` ein Nullbyte, kein `/`, also wird *jedes* schließende Tag als
    /// öffnendes gelesen — der Pfadstapel wächst unbegrenzt, kein Blatt wird
    /// je gezählt, und zwei verschiedene Dateien haben beide einen leeren
    /// Abdruck. Darum wird hier abgebrochen und nach oben gemeldet, statt
    /// eine Zahl zu erfinden.
    pub nicht_utf8: bool,
    /// Sammelt die Zeichen direkt nach `<`, um zu entscheiden, was folgt.
    merker: [u8; MERKER_GROESSE],
    merker_len: usize,
    /// Elementname im Aufbau; kann über eine Blockgrenze laufen.
    name: [u8; NAME_GROESSE],
    name_len: usize,
    /// Rollendes Fenster der zuletzt gelesenen Bytes, um Endekennungen wie
    /// `-->`, `]]>` und `?>` zu finden.
    ///
    /// Ein Trefferzähler wäre naheliegend, aber falsch: Bei `]]]>` müsste er
    /// nach dem dritten `]` auf zwei Treffer zurückspringen statt auf einen,
    /// sonst wird das folgende `>` nicht mehr erkannt. Das Fenster hat dieses
    /// Problem nicht — es vergleicht immer die tatsächlich letzten Bytes.
    fenster: [u8; 3],
    fenster_len: usize,
    /// Hat das zuletzt geöffnete Element schon gemeldet, dass es beginnt?
    /// Nötig, weil der Name erst mit dem Trennzeichen vollständig ist.
    name_gemeldet: bool,
    /// Selbstschließendes Element erkannt (`/` unmittelbar vor `>`).
    selbstschluss: bool,
    /// Name und Wert des laufenden Attributs; beide können über eine
    /// Blockgrenze reichen.
    attr_name: Vec<u8>,
    attr_wert: Vec<u8>,
    /// Hash dessen, was nicht mehr in `attr_wert` passte (0 = nichts).
    attr_wert_ueberlauf: u32,
    /// Hash dessen, was nicht mehr in `name` passte (0 = nichts).
    name_ueberlauf: u32,
    /// War das Attribut eine Namensraum-Erklärung? Muss beim Verwerfen des
    /// Präfixes festgehalten werden: Aus `xmlns:ns` wird sonst `ns`, und die
    /// Erklärung sähe aus wie ein gewöhnliches Attribut.
    attr_ist_xmlns: bool,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::neu()
    }
}

impl Scanner {
    pub fn neu() -> Self {
        Scanner {
            zustand: Zustand::Text,
            erster_block: true,
            nicht_utf8: false,
            merker: [0; MERKER_GROESSE],
            merker_len: 0,
            name: [0; NAME_GROESSE],
            name_len: 0,
            fenster: [0; 3],
            fenster_len: 0,
            name_gemeldet: false,
            selbstschluss: false,
            attr_name: Vec::new(),
            attr_wert: Vec::new(),
            attr_wert_ueberlauf: 0,
            name_ueberlauf: 0,
            attr_ist_xmlns: false,
        }
    }

    /// Verarbeitet einen Block. Beliebig oft aufrufbar; der Zustand bleibt
    /// erhalten. Nach dem letzten Block `abschliessen` rufen.
    pub fn block<B: Beobachter>(&mut self, daten: &[u8], b: &mut B) {
        let mut i = 0;

        if self.erster_block && !daten.is_empty() {
            self.erster_block = false;
            match daten {
                // UTF-16 mit Kennung, oder ohne Kennung erkennbar am
                // Nullbyte neben dem ersten `<`.
                [0xFF, 0xFE, ..] | [0xFE, 0xFF, ..] | [b'<', 0, ..] | [0, b'<', ..] => {
                    self.nicht_utf8 = true;
                }
                // UTF-8-Kennung: gehört nicht zum Inhalt.
                [0xEF, 0xBB, 0xBF, ..] => i = 3,
                _ => {}
            }
        }
        if self.nicht_utf8 {
            return;
        }

        while i < daten.len() {
            match self.zustand {
                Zustand::Text => {
                    // Der häufigste Fall: am Stück bis zum nächsten `<`.
                    let von = i;
                    while i < daten.len() && daten[i] != b'<' {
                        i += 1;
                    }
                    if i > von {
                        b.text(&daten[von..i]);
                    }
                    if i < daten.len() {
                        self.zustand = Zustand::NachSpitz;
                        self.merker_len = 0;
                        i += 1;
                    }
                }

                Zustand::NachSpitz => {
                    let c = daten[i];
                    if self.merker_len < MERKER_GROESSE {
                        self.merker[self.merker_len] = c;
                        self.merker_len += 1;
                    }
                    match self.entscheide() {
                        Entscheidung::Warten => {
                            i += 1;
                        }
                        Entscheidung::Kommentar => {
                            self.zustand = Zustand::Kommentar;
                            self.fenster_len = 0;
                            i += 1;
                        }
                        Entscheidung::CData => {
                            self.zustand = Zustand::CData;
                            self.fenster_len = 0;
                            i += 1;
                        }
                        Entscheidung::Anweisung => {
                            self.zustand = Zustand::Anweisung;
                            self.fenster_len = 0;
                            i += 1;
                        }
                        Entscheidung::Erklaerung => {
                            // Das `!` selbst ist schon gelesen; ein `[` darin
                            // eröffnet das interne Subset und muss zählen.
                            self.zustand = Zustand::Erklaerung(
                                self.merker[..self.merker_len]
                                    .iter()
                                    .filter(|c| **c == b'[')
                                    .count() as u32,
                            );
                            self.fenster_len = 0;
                            i += 1;
                        }
                        Entscheidung::Schluss => {
                            self.zustand = Zustand::SchlussName;
                            self.name_len = 0;
                            self.name_ueberlauf = 0;
                            i += 1;
                        }
                        Entscheidung::Beginn => {
                            // Das erste Zeichen gehört schon zum Namen.
                            self.zustand = Zustand::Name;
                            self.name_len = 0;
                            self.name_ueberlauf = 0;
                            self.name_gemeldet = false;
                            self.selbstschluss = false;
                            // nicht i erhöhen: das Zeichen im Namen-Zustand lesen
                        }
                    }
                }

                Zustand::Name => {
                    let c = daten[i];
                    if ist_namensende(c) {
                        self.melde_beginn(b);
                        // `<TAG/>` — das `/` folgt hier unmittelbar auf den
                        // Namen und wird mit diesem Schritt verbraucht. Ohne
                        // den Vermerk hier fiele der Selbstschluss unter den
                        // Tisch und das Element bliebe offen.
                        if c == b'/' {
                            self.selbstschluss = true;
                        }
                        self.zustand = if c == b'>' {
                            self.nach_tagende(b);
                            Zustand::Text
                        } else {
                            Zustand::ImTag
                        };
                        i += 1;
                    } else {
                        // Namensraum-Präfix verwerfen: `ns:TAG` wird `TAG`.
                        // Präfixe sind frei wählbar und dürfen den Abdruck
                        // nicht beeinflussen.
                        if c == b':' {
                            self.name_len = 0;
                            self.name_ueberlauf = 0;
                        } else if self.name_len < NAME_GROESSE {
                            self.name[self.name_len] = c;
                            self.name_len += 1;
                        } else {
                            self.name_ueberlauf = ueberlauf_weiter(self.name_ueberlauf, c);
                        }
                        i += 1;
                    }
                }

                Zustand::SchlussName => {
                    let c = daten[i];
                    if c == b'>' {
                        b.element_ende();
                        self.zustand = Zustand::Text;
                    } else if c == b':' {
                        self.name_len = 0;
                        self.name_ueberlauf = 0;
                    } else if self.name_len < NAME_GROESSE {
                        self.name[self.name_len] = c;
                        self.name_len += 1;
                    } else {
                        self.name_ueberlauf = ueberlauf_weiter(self.name_ueberlauf, c);
                    }
                    i += 1;
                }

                Zustand::ImTag => {
                    let c = daten[i];
                    match c {
                        b'/' => self.selbstschluss = true,
                        b'>' => {
                            self.nach_tagende(b);
                            self.zustand = Zustand::Text;
                        }
                        c if c.is_ascii_whitespace() => {}
                        _ => {
                            // Beginn eines Attributnamens. Der Elementbeginn
                            // muss vorher gemeldet sein, damit der Beobachter
                            // das Attribut zuordnen kann.
                            self.melde_beginn(b);
                            self.selbstschluss = false;
                            self.attr_name.clear();
                            self.attr_wert.clear();
                            self.attr_ist_xmlns = false;
                            self.attr_name.push(c);
                            self.zustand = Zustand::AttributName;
                        }
                    }
                    i += 1;
                }

                Zustand::AttributName => {
                    let c = daten[i];
                    if c == b'=' || c.is_ascii_whitespace() {
                        self.zustand = Zustand::VorAttributWert;
                    } else if c == b'>' {
                        // Attribut ohne Wert — kommt in XML nicht vor, in
                        // freier Wildbahn trotzdem.
                        self.attr_name.clear();
                        self.nach_tagende(b);
                        self.zustand = Zustand::Text;
                    } else if c == b':' {
                        // Namensraum-Präfix verwerfen, wie bei Elementen —
                        // vorher aber festhalten, ob es eine
                        // Namensraum-Erklärung war. Diese Prüfung muss vor
                        // der Längengrenze stehen: Sonst bliebe bei einem
                        // überlangen Präfix das Präfix stehen und der
                        // eigentliche Name fiele weg.
                        if self.attr_name == b"xmlns" {
                            self.attr_ist_xmlns = true;
                        }
                        self.attr_name.clear();
                    } else if self.attr_name.len() < NAME_GROESSE {
                        self.attr_name.push(c);
                    }
                    i += 1;
                }

                Zustand::VorAttributWert => {
                    let c = daten[i];
                    if c == b'"' || c == b'\'' {
                        self.zustand = Zustand::ImAttribut(c);
                    } else if c == b'>' {
                        self.attr_name.clear();
                        self.nach_tagende(b);
                        self.zustand = Zustand::Text;
                    } else if !c.is_ascii_whitespace() && c != b'=' {
                        // Doch kein Wert — das war schon der nächste Name.
                        // Merker mit zurücksetzen, sonst unterdrückt eine
                        // vorangegangene Namensraum-Erklärung das folgende
                        // Attribut (bei ISO 20022 wäre das die Währung).
                        self.attr_name.clear();
                        self.attr_wert.clear();
                        self.attr_wert_ueberlauf = 0;
                        self.attr_ist_xmlns = false;
                        self.attr_name.push(c);
                        self.zustand = Zustand::AttributName;
                    }
                    i += 1;
                }

                Zustand::ImAttribut(anfuehrung) => {
                    while i < daten.len() {
                        if daten[i] == anfuehrung {
                            // Namensraum-Erklärungen sind Metadaten des
                            // Dokuments, keine Daten — sie dürfen den Abdruck
                            // nicht verändern, sonst hinge er am Präfix.
                            if !self.attr_name.is_empty()
                                && self.attr_name != b"xmlns"
                                && !self.attr_ist_xmlns
                            {
                                let name = std::mem::take(&mut self.attr_name);
                                let wert = std::mem::take(&mut self.attr_wert);
                                if self.attr_wert_ueberlauf == 0 {
                                    b.attribut(&name, &wert);
                                } else {
                                    b.attribut(&name, &mit_ueberlauf(&wert, self.attr_wert_ueberlauf));
                                }
                            }
                            self.attr_name.clear();
                            self.attr_wert.clear();
                            self.attr_wert_ueberlauf = 0;
                            self.attr_ist_xmlns = false;
                            self.zustand = Zustand::ImTag;
                            i += 1;
                            break;
                        }
                        if self.attr_wert.len() < ATTRIBUTWERT_GROESSE {
                            self.attr_wert.push(daten[i]);
                        } else {
                            self.attr_wert_ueberlauf =
                                ueberlauf_weiter(self.attr_wert_ueberlauf, daten[i]);
                        }
                        i += 1;
                    }
                }

                Zustand::Kommentar => {
                    i = self.bis_ende(daten, i, b"-->");
                }
                Zustand::CData => {
                    // Der Inhalt ist Text und wird gemeldet — sonst ginge ein
                    // eingefasster Wert verloren. Gemeldet wird nur, was aus
                    // dem Fenster herausfällt: Diese Bytes können nicht mehr
                    // zur Endekennung gehören und sind damit sicher Inhalt.
                    let mut inhalt: Vec<u8> = Vec::new();
                    while i < daten.len() {
                        if let Some(raus) = self.fenster_schieben(daten[i], 3) {
                            inhalt.push(raus);
                        }
                        i += 1;
                        if self.fenster_passt(b"]]>") {
                            self.fenster_len = 0;
                            self.zustand = Zustand::Text;
                            break;
                        }
                    }
                    if !inhalt.is_empty() {
                        b.text(&inhalt);
                    }
                }
                Zustand::Anweisung => {
                    i = self.bis_ende(daten, i, b"?>");
                }
                Zustand::Erklaerung(tiefe) => {
                    let mut t = tiefe;
                    while i < daten.len() {
                        let c = daten[i];
                        i += 1;
                        match c {
                            b'[' => t += 1,
                            b']' => t = t.saturating_sub(1),
                            b'>' if t == 0 => {
                                self.zustand = Zustand::Text;
                                break;
                            }
                            _ => {}
                        }
                        self.zustand = Zustand::Erklaerung(t);
                    }
                    if self.zustand != Zustand::Text {
                        self.zustand = Zustand::Erklaerung(t);
                    }
                }
            }
        }
    }

    /// Nach dem letzten Block aufrufen. Meldet ein offenes Element, dessen
    /// Name genau am Dateiende endete.
    pub fn abschliessen<B: Beobachter>(&mut self, b: &mut B) {
        if self.zustand == Zustand::Name && !self.name_gemeldet {
            self.melde_beginn(b);
        }
    }

    fn melde_beginn<B: Beobachter>(&mut self, b: &mut B) {
        if !self.name_gemeldet {
            if self.name_ueberlauf == 0 {
                b.element_beginn(&self.name[..self.name_len]);
            } else {
                let voll = mit_ueberlauf(&self.name[..self.name_len], self.name_ueberlauf);
                b.element_beginn(&voll);
            }
            self.name_gemeldet = true;
        }
    }

    /// Nach `>` eines öffnenden Tags: bei `<TAG/>` sofort auch das Ende.
    fn nach_tagende<B: Beobachter>(&mut self, b: &mut B) {
        self.melde_beginn(b);
        if self.selbstschluss {
            b.element_ende();
            self.selbstschluss = false;
        }
    }

    /// Schiebt ein Byte ins Fenster und liefert das Byte, das dabei
    /// herausfällt — sofern das Fenster schon voll war.
    #[inline]
    fn fenster_schieben(&mut self, byte: u8, breite: usize) -> Option<u8> {
        let raus = if self.fenster_len == breite {
            let r = self.fenster[0];
            for j in 0..breite - 1 {
                self.fenster[j] = self.fenster[j + 1];
            }
            Some(r)
        } else {
            self.fenster_len += 1;
            None
        };
        self.fenster[self.fenster_len - 1] = byte;
        raus
    }

    #[inline]
    fn fenster_passt(&self, kennung: &[u8]) -> bool {
        self.fenster_len == kennung.len() && self.fenster[..kennung.len()] == *kennung
    }

    /// Sucht die Endekennung. Das Fenster überlebt Blockgrenzen, deshalb ist
    /// gleichgültig, wo die Blöcke geschnitten sind.
    fn bis_ende(&mut self, daten: &[u8], mut i: usize, kennung: &[u8]) -> usize {
        while i < daten.len() {
            self.fenster_schieben(daten[i], kennung.len());
            i += 1;
            if self.fenster_passt(kennung) {
                self.fenster_len = 0;
                self.zustand = Zustand::Text;
                return i;
            }
        }
        i
    }

    fn entscheide(&self) -> Entscheidung {
        let m = &self.merker[..self.merker_len];
        match m {
            [b'/', ..] => Entscheidung::Schluss,
            [b'?', ..] => Entscheidung::Anweisung,
            [b'!'] => Entscheidung::Warten,
            [b'!', b'-'] => Entscheidung::Warten,
            [b'!', b'-', b'-', ..] => Entscheidung::Kommentar,
            [b'!', b'['] | [b'!', b'[', b'C'] | [b'!', b'[', b'C', b'D'] => Entscheidung::Warten,
            [b'!', b'[', b'C', b'D', b'A'] | [b'!', b'[', b'C', b'D', b'A', b'T'] => {
                Entscheidung::Warten
            }
            [b'!', b'[', b'C', b'D', b'A', b'T', b'A'] => Entscheidung::Warten,
            [b'!', b'[', b'C', b'D', b'A', b'T', b'A', b'['] => Entscheidung::CData,
            // `<!DOCTYPE`, `<!ENTITY` und alles andere mit `!`
            [b'!', ..] => Entscheidung::Erklaerung,
            _ => Entscheidung::Beginn,
        }
    }
}

enum Entscheidung {
    Warten,
    Beginn,
    Schluss,
    Kommentar,
    CData,
    Anweisung,
    Erklaerung,
}

#[inline]
fn ist_namensende(c: u8) -> bool {
    c == b'>' || c == b'/' || c.is_ascii_whitespace()
}
