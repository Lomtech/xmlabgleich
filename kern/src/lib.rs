//! xmlabgleich — Fingerabdrücke für XML-Dokumente beliebiger Größe.
//!
//! Zwei XML-Dateien werden verglichen, ohne dass eine davon den Rechner
//! verlässt und ohne dass sie in den Speicher passen müssen. Gelesen wird in
//! Blöcken; verglichen werden am Ende zwei Ränder von einigen zehn Kilobyte.

pub mod abdruck;
pub mod erkundung;
pub mod scanner;
pub mod wasm;

mod abdruck_pruefung;
pub mod scanner_pruefung;
