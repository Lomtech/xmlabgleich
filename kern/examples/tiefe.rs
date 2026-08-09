fn main() {
    use xmlabgleich::{scanner::Scanner, zerlegung::Zerlegung};
    // Drei Stufen: Vertrag → Position → Staffel → Termin
    let xml = r#"<VERTRAEGE>
      <VERTRAG><NR>V1</NR>
        <POSITIONEN>
          <POSITION><PNR>1</PNR>
            <STAFFELN>
              <STAFFEL><AB>0</AB>
                <TERMINE><TERMIN><DATUM>2026-01-01</DATUM></TERMIN>
                         <TERMIN><DATUM>2026-07-01</DATUM></TERMIN></TERMINE>
              </STAFFEL>
              <STAFFEL><AB>1000</AB>
                <TERMINE><TERMIN><DATUM>2027-01-01</DATUM></TERMIN></TERMINE>
              </STAFFEL>
            </STAFFELN>
          </POSITION>
          <POSITION><PNR>2</PNR>
            <STAFFELN><STAFFEL><AB>0</AB>
              <TERMINE><TERMIN><DATUM>2026-03-01</DATUM></TERMIN></TERMINE>
            </STAFFEL></STAFFELN>
          </POSITION>
        </POSITIONEN>
      </VERTRAG></VERTRAEGE>"#;

    let mut s = Scanner::neu();
    let mut z = Zerlegung::neu(
        "VERTRAEGE/VERTRAG", &["NR"], 64,
        &["POSITIONEN/POSITION", "STAFFELN/STAFFEL", "TERMINE/TERMIN"],
    );
    s.block(xml.as_bytes(), &mut z);
    s.abschliessen(&mut z);

    let mut namen: Vec<_> = z.tabellen.keys().cloned().collect();
    namen.sort();
    for n in namen {
        let t = &z.tabellen[&n];
        println!("  Stufe {}  {:<52} {} Zeilen, {} Spalten  ← gehört zu \"{}\"",
            t.tiefe,
            if n.is_empty() { "(Datensatz VERTRAG)".to_string() } else { String::from_utf8_lossy(&n).to_string() },
            t.zeilenzahl, t.spalten.len(),
            String::from_utf8_lossy(&t.gehoert_zu));
    }
}
