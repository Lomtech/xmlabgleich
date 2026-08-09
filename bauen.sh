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

echo "  Einbetten …"
# Das Einsetzen läuft über Python und nicht über sed/awk: Die Base64-Fassung
# des Moduls ist rund 300 kB, und als Kommandozeilenargument übergeben
# sprengt sie unter Linux das Argumentlimit ("Argument list too long").
# Unter macOS fiel das nicht auf — der Prüflauf auf GitHub scheiterte
# stillschweigend bei jedem Push.
python3 - "$WASM" web/vorlage.html xmlabgleich.html <<'PY'
import base64, pathlib, sys

wasm, vorlage, ziel = sys.argv[1], sys.argv[2], sys.argv[3]
b64 = base64.b64encode(pathlib.Path(wasm).read_bytes()).decode("ascii")
text = pathlib.Path(vorlage).read_text(encoding="utf-8")
if "%%WASM%%" not in text:
    sys.exit("Platzhalter %%WASM%% fehlt in der Vorlage")
pathlib.Path(ziel).write_text(text.replace("%%WASM%%", b64), encoding="utf-8")
PY

# GitHub Pages sucht index.html; unter dem sprechenden Namen bleibt sie
# ebenfalls abrufbar.
cp xmlabgleich.html index.html

echo "  Fertig:"
printf "    WASM  %8s Byte\n" "$(wc -c < "$WASM" | tr -d ' ')"
printf "    HTML  %8s Byte  → xmlabgleich.html + index.html\n" "$(wc -c < xmlabgleich.html | tr -d ' ')"
