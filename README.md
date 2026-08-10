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

## Verschachtelung als Tabellen

XML hat mehrstufige Tiefen. Eine flache Feldliste verliert dabei etwas
Wesentliches: Alle sechs `INVESTMENTIDENTIFIER` eines Investments landeten in
demselben Feldpfad, ihre Werte in derselben Summe. Werden ISIN und interne
Nummer vertauscht, ändert sich nichts — der Abgleich ist dort blind.

Deshalb wird jede Wiederholgruppe eine **eigene Tabelle**, rekursiv, mit
Fremdschlüssel auf die übergeordnete Zeile:

```
Stufe 0  (Datensatz VERTRAG)                     1 Zeile,  2 Spalten
Stufe 1  POSITIONEN/POSITION                     2 Zeilen, 2 Spalten  ← gehört zum Datensatz
Stufe 2  …/STAFFELN/STAFFEL                      3 Zeilen, 2 Spalten  ← gehört zu POSITION
Stufe 3  …/TERMINE/TERMIN                        4 Zeilen, 1 Spalte   ← gehört zu STAFFEL
```

Auf jede Tabelle wird dasselbe Verfahren angewandt — Spaltenrand, Zeilenrand,
Abdruck. Genau so, wie ein Ladeprogramm die Nachricht in Kopf- und
Positionstabellen zerlegt. Gemessen an echten Daten: Ohne Untertabellen ist
ein Tausch innerhalb einer Gruppe **unsichtbar**, mit ihnen wird er **erkannt**
und im Klartext benannt.

Der Zeilenschlüssel einer Gruppe ist Fremdschlüssel plus laufende Nummer
(`2015-01-01#0`). Damit zählt auch die Reihenfolge innerhalb der Gruppe.

Dasselbe gilt für **wiederholte einzelne Felder**: Das erste bleibt `TELEFON`,
jedes weitere wird `TELEFON[1]`, `TELEFON[2]`. Ohne diese Nummer gingen zwei
Telefonnummern in einer Summe auf und ihr Tausch bliebe unsichtbar; für
Felder, die nur einmal dastehen, ändert sich nichts.

**Ein Fallstrick dabei:** Das Schlüsselfeld eines Datensatzes kann *nach* der
Wiederholgruppe stehen. Würde die Gruppenzeile beim Öffnen zugeordnet, bekäme
sie einen leeren Fremdschlüssel — alle Zeilen aller Datensätze lägen unter
demselben Platzhalter, und die Zugehörigkeit wäre verloren, ohne dass es
auffiele. Die Zeilen warten deshalb, bis die Elternzeile schließt.

## Vier Ebenen

Ein Vergleich, der nur „Feld X weicht ab" sagt, verschweigt das Entscheidende:
ob ein Feld **umbenannt**, **umgestellt** oder nur ein **Wert geändert**
wurde. Das sind drei verschiedene Probleme mit drei verschiedenen Ursachen und
Reichweiten. Deshalb vier getrennte Ebenen:

| | Ebene | Frage | Reichweite einer Abweichung |
|---|---|---|---|
| 1 | **Feldbezeichnungen** | Dieselben Felder? | Schema geändert — alle Datensätze |
| 2 | **Indextabelle** | Dieselbe Anordnung, an welcher Stelle? | Erzeugung geändert — alle Datensätze |
| 3 | **Satzfolge** | Dieselbe Reihenfolge der Datensätze? | andere Sortierung — je nach Ladeweg erheblich |
| 4 | **Ränder** | Dieselben Werte, in welchem Feld und Datensatz? | Datenfehler — einzelne Datensätze |
| 5 | **Fingerabdruck** | Überhaupt eine Abweichung? | erster Schritt, ein Wert |

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

**Ebene 3** ist als einzige **nicht kommutativ**. Alle anderen Prüfungen
überstehen eine Umsortierung mit Absicht — dieselben Sätze bleiben dieselben
Sätze. Ob eine geänderte Reihenfolge stört, hängt aber vom Ladeweg ab: Manche
Verarbeitungen arbeiten in Dokumentfolge und bauen Abhängigkeiten zwischen
Sätzen auf. Das Werkzeug stellt deshalb fest und bewertet nicht — es meldet
„Gleiche Werte, andere Reihenfolge" als eigenes Urteil und benennt den ersten
Satz, ab dem die Folgen auseinanderlaufen.

**Ebene 5** fasst alle vier zu einem Wert zusammen, kurz genug zum Vorlesen:

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

### Was die Ebenen trennen

| Änderung | Urteil | Feldnamen | Anordnung | Satzfolge | Werte |
|---|---|---|---|---|---|
| nur umformatiert | ✓ Übereinstimmung | = | = | = | = |
| zwei Felder vertauscht | ✗ Abweichungen | = | **≠** | = | = |
| Datensätze umsortiert | **! Gleiche Werte, andere Reihenfolge** | = | = | **≠** | = |
| eine Ziffer geändert | ✗ Abweichungen | = | = | = | **≠** |
| ein Datensatz entfernt | ✗ Abweichungen | = | = | ≠ | **≠** |

### Und welcher Wert?

Der Fingerabdruck ist datenfrei, damit er einen Betrieb verlassen darf. Beim
Vergleich zweier Dateien auf demselben Rechner schützt das aber nichts — es
verhindert nur die Antwort. Deshalb läuft nach dem Abgleich ein **zweiter,
gezielter Durchlauf**: Er liest nur die Gruppen und Felder nach, die der erste
als abweichend gemeldet hat, und stellt die Klartextwerte gegenüber. Bei
10.000 Datensätzen sind das zwei bis drei Sätze.

```
Schlüssel     Feld                      Quelle     Ziel
2015-01-01    GENERALDATA/ISSUEPRICE    83.9261    83.9267
```

Die abweichenden Zeichen sind hervorgehoben — bei einer vertauschten Ziffer in
einem langen Wert ist das der Unterschied zwischen „irgendwas ist anders" und
„hier".

### Über das Neuladen hinweg

Einstellungen und der letzte Bericht liegen in `localStorage`, die
Dateiverweise in IndexedDB. Nach einem Neuladen ist der Bericht sofort wieder
da; die Dateien selbst werden nie gespeichert, nur der Verweis darauf — beim
nächsten Öffnen fragt der Browser nach Erlaubnis.

Ein dauerhafter Verweis entsteht nur über den Systemdialog
(`showOpenFilePicker`), deshalb steht neben der gewöhnlichen Dateiauswahl der
Knopf **merkbar öffnen**.

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

## Der Bericht

Oben steht **eine Liste aller Unterschiede** — was, wo, warum:

```
1 × Datensatz fehlt im Ziel
1 × Wert abweichend
1 × Wert fehlt im Ziel

Was                       Datensatz              Feld                     Quelle    Ziel
Datensatz fehlt im Ziel   SUEDKASSE BORDURIA …                            vorhanden fehlt
Wert abweichend           NORDBANK RURITANIA …   GENERALDATA/ISSUEPRICE   83.9261   983.9261
Wert fehlt im Ziel        NORDBANK RURITANIA …   GENERALDATA/QUOTATION    1
```

Eine Zeile je Befund, filterbar, sortierbar, vollständig als
`unterschiede.csv`. Die Felder eines fehlenden Datensatzes stehen **nicht**
einzeln darin — sie fehlen mit dem Satz, und aus einem Befund würden sonst
vierundzwanzig.

Darunter, eingeklappt: **wie das geprüft wurde** — die vier Ebenen als
Herleitung, jede mit eigener CSV.

| | Ebene | Ausgabe |
|---|---|---|
| 1 | Fingerabdruck — gleich oder nicht | — |
| 2 | Feldbezeichnungen | `2_feldbezeichnungen.csv` |
| 3 | Anordnung und Reihenfolge | `3_reihenfolge.csv` |
| 4 | Ausprägungen | `4_felder_mit_abweichung.csv`, `4_werte.csv` |

Jede Liste blättert seitenweise wie im
[CSV-Betrachter](https://github.com/Lomtech/csvrs): immer nur 50 Zeilen im
Baum. Gemessen an einer Million Zeilen — erste Anzeige 5 ms, Blättern 1 ms,
Filtern 105 ms, Sortieren 86 ms. Die CSV enthält die gefilterte und sortierte
Liste vollständig, nicht nur die angezeigte Seite.

## Prüfdaten

Die Dateien unter `pruefdaten/` sind **erfunden**. Sie haben die Struktur
echter Investmentstammdaten — Verschachtelung, Wiederholgruppen, optionale
Felder —, aber keine echten Wertpapiere, Emittenten oder Kennungen. Erzeugt
mit festem Zufallskeim, also bei jedem Lauf gleich.

Das ist Absicht: Ein Werkzeug für Kundendaten darf keine Kundendaten
mitliefern.
