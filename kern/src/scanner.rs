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
    /// In einem Attributwert; das Zeichen ist das öffnende Anführungszeichen.
    /// Nötig, weil `>` innerhalb eines Attributwerts erlaubt ist.
    ImAttribut(u8),
    /// `<!-- … -->`
    Kommentar,
    /// `<![CDATA[ … ]]>` — der Inhalt ist Text, auch wenn `<` darin steht.
    CData,
    /// `<? … ?>` und `<!DOCTYPE …>`
    Anweisung,
}

/// Längste Zeichenfolge, die nach `<` geprüft werden muss: `![CDATA[`.
const MERKER_GROESSE: usize = 8;
/// Längster Elementname, den wir am Stück halten. Alles darüber wird
/// abgeschnitten — solche Namen kommen in Datenaustauschformaten nicht vor,
/// und ein unbegrenzter Puffer wäre ein Einfallstor.
const NAME_GROESSE: usize = 256;

pub struct Scanner {
    zustand: Zustand,
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
            merker: [0; MERKER_GROESSE],
            merker_len: 0,
            name: [0; NAME_GROESSE],
            name_len: 0,
            fenster: [0; 3],
            fenster_len: 0,
            name_gemeldet: false,
            selbstschluss: false,
        }
    }

    /// Verarbeitet einen Block. Beliebig oft aufrufbar; der Zustand bleibt
    /// erhalten. Nach dem letzten Block `abschliessen` rufen.
    pub fn block<B: Beobachter>(&mut self, daten: &[u8], b: &mut B) {
        let mut i = 0;
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
                        Entscheidung::Schluss => {
                            self.zustand = Zustand::SchlussName;
                            self.name_len = 0;
                            i += 1;
                        }
                        Entscheidung::Beginn => {
                            // Das erste Zeichen gehört schon zum Namen.
                            self.zustand = Zustand::Name;
                            self.name_len = 0;
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
                        } else if self.name_len < NAME_GROESSE {
                            self.name[self.name_len] = c;
                            self.name_len += 1;
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
                    } else if self.name_len < NAME_GROESSE {
                        self.name[self.name_len] = c;
                        self.name_len += 1;
                    }
                    i += 1;
                }

                Zustand::ImTag => {
                    let c = daten[i];
                    match c {
                        b'"' | b'\'' => {
                            self.zustand = Zustand::ImAttribut(c);
                            self.selbstschluss = false;
                        }
                        b'/' => self.selbstschluss = true,
                        b'>' => {
                            self.nach_tagende(b);
                            self.zustand = Zustand::Text;
                        }
                        c if c.is_ascii_whitespace() => {}
                        _ => self.selbstschluss = false,
                    }
                    i += 1;
                }

                Zustand::ImAttribut(anfuehrung) => {
                    while i < daten.len() {
                        if daten[i] == anfuehrung {
                            self.zustand = Zustand::ImTag;
                            i += 1;
                            break;
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
            b.element_beginn(&self.name[..self.name_len]);
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
            [b'!', ..] => Entscheidung::Anweisung,
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
}

#[inline]
fn ist_namensende(c: u8) -> bool {
    c == b'>' || c == b'/' || c.is_ascii_whitespace()
}
