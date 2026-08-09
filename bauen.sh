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
BASE64=$(base64 < "$WASM" | tr -d '\n')
# awk statt sed: Die Base64-Zeichenkette ist zu lang für sed unter macOS.
awk -v b64="$BASE64" '{ gsub(/%%WASM%%/, b64); print }' web/vorlage.html > xmlabgleich.html

echo "  Fertig:"
printf "    WASM  %8s Byte\n" "$(wc -c < "$WASM" | tr -d ' ')"
cp xmlabgleich.html index.html
printf "    HTML  %8s Byte  → xmlabgleich.html + index.html\n" "$(wc -c < xmlabgleich.html | tr -d ' ')"
