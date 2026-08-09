# xmlabgleich

Vergleicht zwei XML-Dateien beliebiger Größe und zeigt, **welches Feld in
welchem Datensatz** abweicht. Läuft vollständig im Browser — die Dateien
verlassen den Rechner nicht.

Eine einzige HTML-Datei, 139 kB. Verschicken, doppelklicken, fertig. Keine
Installation, kein Server, kein Netz.

## Wozu

Beim Zerlegen von XML in Tabellen (ISO 20022, SAP-Importe, Meldewesen) geht die
Struktur verloren und Fehler fallen erst Monate später auf. Dieses Werkzeug
beantwortet zwei Fragen über die volle Menge, nicht stichprobenartig:

* **Welches Feld** hat sich geändert?
* **Welcher Datensatz** ist betroffen?

## Vier Ebenen

Ein Vergleich, der nur „Feld X weicht ab" sagt, verschweigt das Entscheidende:
ob ein Feld **umbenannt**, **umgestellt** oder nur ein **Wert geändert**
wurde. Das sind drei verschiedene Probleme mit drei verschiedenen Ursachen und
Reichweiten. Deshalb vier getrennte Ebenen:

| | Ebene | Frage | Reichweite einer Abweichung |
|---|---|---|---|
| 1 | **Feldbezeichnungen** | Dieselben Felder? | Schema geändert — alle Datensätze |
| 2 | **Indextabelle** | Dieselbe Anordnung, an welcher Stelle? | Erzeugung geändert — alle Datensätze |
| 3 | **Ränder** | Dieselben Werte, in welchem Feld und Datensatz? | Datenfehler — einzelne Datensätze |
| 4 | **Fingerabdruck** | Überhaupt eine Abweichung? | erster Schritt, ein Wert |

**Ebene 2** ist mehr als ein Hash: Die Feldbezeichnungen bekommen Nummern in
der Reihenfolge ihres ersten Auftretens, und jeder Datensatz wird zu einer
Zahlenfolge. Damit lässt sich die Abweichung benennen statt nur melden —

```
Nr.  Quelle                        Ziel
13   GENERALDATA/INVESTMENTTYPE    GENERALDATA/SECURITY_GROUP   umgestellt
14   GENERALDATA/SECURITY_GROUP    GENERALDATA/INVESTMENTTYPE   umgestellt
```

Verglichen werden dabei alle Feldfolgen, nicht nur die häufigste: Ist ein
einzelner Datensatz von 401 umgestellt, bleibt die Regelfolge auf beiden
Seiten gleich und die Abweichung steckt in einer seltenen.

**Ebene 4** fasst alle drei zu einem Wert zusammen, kurz genug zum Vorlesen:

```
52BC-3948-BDC6-5227
```

Damit ist ohne Dateiaustausch klärbar, ob überhaupt etwas abweicht. Die
Aussage ist bewusst unsymmetrisch:

* **verschieden** heißt sicher verschieden,
* **gleich** heißt sehr wahrscheinlich gleich — aber nicht beweisbar, denn
  Summe und XOR sind kommutativ und zwei Änderungen könnten sich rechnerisch
  aufheben.

Er trägt außerdem eine **Selbstkontrolle**: Jeder Zellwert geht in genau eine
Spalte und genau eine Zeile ein, also müssen beide Achsen dieselbe Summe
ergeben. Weichen sie ab, ist nicht die Datei falsch, sondern der Abdruck.

### Was die Ebenen an echten Daten trennen

| Änderung | Fingerabdruck | Feldnamen | Anordnung | Werte |
|---|---|---|---|---|
| nur umformatiert | gleich | gleich | gleich | gleich |
| zwei Felder vertauscht | ≠ | gleich | **≠** | gleich |
| ein Wert geändert | ≠ | gleich | gleich | **≠** |
| ein Datensatz entfernt | ≠ | gleich | gleich | **≠** |

## Wie

Für jedes Blatt wird ein Wert aus **Pfad + Schlüssel + Inhalt** gebildet und in
zwei Richtungen verrechnet:

| Rand | verrechnet über | findet |
|---|---|---|
| Spalten | Pfad, z. B. `GENERALDATA/INVESTMENTTYPE` | welches Feld |
| Zeilen | 4096 Eimer des Datensatzschlüssels | welcher Datensatz |

Weichen beide ab, kreuzen sie sich in der Zelle. Der Rand kostet N + M Werte
statt N × M einer vollen Matrix. Dafür ist er ab zwei Fehlern nicht mehr
eindeutig — k Fehler ergeben k² Kandidaten, von denen k echt sind. Bei
Übernahmefehlern, die fast immer eine ganze Spalte betreffen, spielt das keine
Rolle.

## Gemessen

Investmentstammdaten in der Struktur eines Versicherer-Imports, 401 Datensätze, 37.716 Elemente:

| Fall | Ergebnis |
|---|---|
| Nur Formatierung geändert (kompakt statt eingerückt) | **bestanden**, 0 Abweichungen |
| Ein Wert geändert | **genau 1 Feld × 1 Gruppe** = eine einzige mögliche Stelle |
| Ein Datensatz entfernt | 401 → 400, als Mengendifferenz ausgewiesen |

Durchsatz an einer 801-MB-Datei derselben Struktur:

| | |
|---|---|
| Dauer | 3,67 s |
| Durchsatz | 218 MB/s |
| Speicher | 25 MB — unabhängig von der Dateigröße |
| Datensätze | 194.084 |
| Felder mit Wert | 13.786.256 |

Zum Vergleich: Der übliche Weg über `DOMParser` scheitert bei dieser Datei
**lautlos** — `await response.text()` liefert `undefined`, ohne eine Ausnahme
zu werfen, weil ein JavaScript-String die nötigen 1,6 GB nicht fassen kann.

## Bauen

```sh
./bauen.sh
```

Läuft die Prüfungen, übersetzt den Rust-Kern nach WebAssembly und bettet ihn
als Base64 in `xmlabgleich.html` ein.

```
kern/     Rust — Scanner, Fingerabdruck, Erkundung (34 Prüfungen)
web/      Vorlage der Oberfläche
```

## Was geprüft ist

Der Scanner arbeitet als Zustandsmaschine über Blöcke hinweg. Die tragende
Prüfung: **Dieselbe Datei in Blockgrößen von 1 bis 1000 Byte gelesen muss
denselben Abdruck ergeben.** Ein Fingerabdruck, der davon abhängt, wo die
Datei zufällig in Blöcke zerfällt, wäre wertlos.

Unempfindlich gegen: Einrückung, Zeilenenden, Reihenfolge der Datensätze,
Namensraum-Präfixe, Kommentare, Blockgrenzen.

Empfindlich gegen: jeden geänderten Wert, jedes fehlende Feld, vertauschte
Inhalte zwischen Datensätzen, leer gegen fehlend.

## Grenzen

* **Attribute werden nicht verglichen** — nur Elementinhalte.
* **Ab zwei betroffenen Feldern** ist die Kreuzung nicht mehr eindeutig.
* **Kein Schutz vor absichtlicher Veränderung** — FNV-1a ist kein
  kryptographisches Verfahren.
* **Der Schlüssel muss eindeutig sein.** Ist er es nicht, sagt das Werkzeug
  es (z. B. „399 verschiedene Werte bei 401 Datensätzen").

Jeder Bericht führt diese Grenzen selbst auf.

## Prüfdaten

Die Dateien unter `pruefdaten/` sind **erfunden**. Sie haben die Struktur
echter Investmentstammdaten — Verschachtelung, Wiederholgruppen, optionale
Felder —, aber keine echten Wertpapiere, Emittenten oder Kennungen. Erzeugt
mit festem Zufallskeim, also bei jedem Lauf gleich.

Das ist Absicht: Ein Werkzeug für Kundendaten darf keine Kundendaten
mitliefern.
