#!/bin/sh
# Baut das WASM-Modul und setzt es als Base64 in die HTML-Datei ein.
#
# Ergebnis ist eine einzige Datei ohne Abhängigkeiten: Sie lässt sich
# verschicken und per Doppelklick öffnen. Genau darauf kommt es an — ein
# Werkzeug, das erst installiert werden muss, wird nicht benutzt.
set -e
cd "$(dirname "$0")"

echo "  Prüfungen …"
(cd kern && cargo test --quiet 2>&1 | grep -E 'test result|error' | head -3)

echo "  WASM bauen …"
(cd kern && cargo build --release --target wasm32-unknown-unknown --quiet)
WASM=kern/target/wasm32-unknown-unknown/release/xmlabgleich.wasm

# Fingerabdruck des Quelltextes, aus dem gebaut wird. Er landet in der
# HTML-Datei, damit sich später feststellen lässt, ob sie zum Stand des
# Quelltextes passt.
#
# Warum nicht das Ergebnis selbst vergleichen: Rust-Übersetzungen sind über
# verschiedene Werkzeugversionen hinweg nicht byteweise gleich — dasselbe
# Modul war unter macOS 224.495 und unter Linux 223.309 Byte groß. Ein
# Prüflauf, der Byte-Identität verlangt, kann deshalb nie grün werden. Der
# Quelltexthash dagegen ist überall derselbe.
QUELLHASH=$(cat kern/src/*.rs kern/Cargo.toml web/vorlage.html | shasum -a 256 | cut -c1-16)

echo "  Einbetten …"
# Das Einsetzen läuft über Python und nicht über sed/awk: Die Base64-Fassung
# des Moduls ist rund 300 kB, und als Kommandozeilenargument übergeben
# sprengt sie unter Linux das Argumentlimit ("Argument list too long").
# Unter macOS fiel das nicht auf — der Prüflauf auf GitHub scheiterte
# stillschweigend bei jedem Push.
python3 - "$WASM" web/vorlage.html xmlabgleich.html "$QUELLHASH" <<'PY'
import base64, pathlib, sys

wasm, vorlage, ziel, quellhash = sys.argv[1:5]
b64 = base64.b64encode(pathlib.Path(wasm).read_bytes()).decode("ascii")
text = pathlib.Path(vorlage).read_text(encoding="utf-8")
if "%%WASM%%" not in text:
    sys.exit("Platzhalter %%WASM%% fehlt in der Vorlage")
text = text.replace("%%WASM%%", b64)
text = text.replace("<head>", f'<head>\n<meta name="quelltext" content="{quellhash}">', 1)
pathlib.Path(ziel).write_text(text, encoding="utf-8")
PY

# GitHub Pages sucht index.html; unter dem sprechenden Namen bleibt sie
# ebenfalls abrufbar.
cp xmlabgleich.html index.html

echo "  Fertig:"
printf "    WASM  %8s Byte\n" "$(wc -c < "$WASM" | tr -d ' ')"
printf "    HTML  %8s Byte  → xmlabgleich.html + index.html\n" "$(wc -c < xmlabgleich.html | tr -d ' ')"
printf "    aus Quelltext %s\n" "$QUELLHASH"
