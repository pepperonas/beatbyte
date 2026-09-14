# Gitarren-Pilot: Gegenprobe mit Metallica

Stand: 2026-09-14, unveränderter Generator 0.15.7. Nach dem positiven
Spielerurteil zu Van Halen auf Mittel und Schwer wird **Nothing Else Matters**
als zweiter separater Pilot geprüft. Keine Umstellung des Imports oder der
Bibliothek. Vorgeschichte: [Van-Halen-Protokoll](guitar-feel-pilot.md).

## Kontrollierter Vergleich

Die Aufnahme liegt in `songs/imported/metallica---metallica--nothing-else-matters-m4a/`.
`chart-active.json` verweist auf **chart.v5.json**. Diese aktive Version enthält
314/606/897/1062 Noten (Easy/Medium/Hard/Expert); ihr Hash im Vergleichslauf ist
`7ab1477b12a06372`. Die ältere `chart.json` ist nicht die Referenz.

Wieder verwendet: vorhandener Decoder und installiertes Demucs-Modell
`htdemucs`, Quelle `other`, keine neuen Abhängigkeiten oder Modell-Downloads.
Der Decoder entfernt 1024 Priming-Samples; die Ausgabe dauert 385,856 Sekunden.
Die Trennung dauerte ungefähr drei Minuten auf CPU. `other` kann mehrere
Instrumente enthalten.

```sh
target/debug/beatbyte-cli decode \
  'songs/imported/metallica---metallica--nothing-else-matters-m4a/Metallica - Metallica- Nothing Else Matters.m4a' \
  --out local/guitar-study-metallica/mix.wav
demucs -n htdemucs --two-stems other --shifts 0 -d cpu -j 1 \
  -o local/guitar-study-metallica/stems local/guitar-study-metallica/mix.wav
target/debug/examples/guitar_study \
  songs/imported/metallica---metallica--nothing-else-matters-m4a/chart.v5.json \
  local/guitar-study-metallica/stems/htdemucs/mix/other.wav \
  local/guitar-study-metallica/comparison
```

Das Beispiel wird wie im ersten Protokoll über Cargo gebaut. Die Ausgabe
benötigt einen neuen Zielordner. Alle Varianten verwenden das akzeptierte
Raster mit 899 Beats. Wiederholungsübertragung ist für den Vergleich aus;
die Standardkontrolle ist deshalb nicht gleich der archivierten aktiven Version.

| Quelle / Generator | Easy | Medium | Hard | Expert | Expert-HOPOs | Expert-Sustains |
|---|---:|---:|---:|---:|---:|---:|
| Gesamtmix / Standard | 306 | 606 | 881 | 1050 | 357 | 294 |
| Gesamtmix / Pilot | 316 | 561 | 766 | 819 | 120 | 424 |
| other / Standard | 318 | 632 | 851 | 1001 | 307 | 340 |
| other / Pilot | 297 | 498 | 679 | 724 | 129 | 426 |

Gezählt werden Noten inklusive etwaiger Akkordbestandteile, keine musikalische
Qualität. Im Pilot bleiben die Schwierigkeiten nach Notenzeit verschachtelt.
Ihre kleinsten Abstände sind 0,50/0,30/0,20/0,10 Sekunden; alle bisherigen
Mindestabstände sind erfüllt. Expert enthält elf schnelle Anschlagspaare
derselben Taste.

## Konkrete Grenze: 0:53 bis 1:00

Medium/Hard/Expert haben einen Notenabstand von 53,16 bis 59,90 Sekunden
(6,74 Sekunden); nach Abzug des vorherigen Sustains bleiben 6,378 Sekunden
ohne Note. Auf Easy beträgt der längste Abstand 7,79 Sekunden, davon
7,167 Sekunden ohne Sustain.

In `lead-analysis.json` endet eine Melodienote bei 53,522 Sekunden, die nächste
beginnt erst bei 59,907. Im Fenster 54–59 Sekunden werden dennoch zwölf Onsets
erkannt. Die Quelle hat dort etwa **−26,1 dBFS RMS** (PCM16, alle Kanäle,
ungefiltertes quadratisches Mittel): keine digitale Stille. Der Gesamtmix
enthält im selben Fenster drei Melodieereignisse und zwölf Onsets; sein RMS
liegt gerundet ebenfalls bei −26,1 dBFS. Ähnliches RMS beweist weder gleiches
Audio noch dieselben Instrumente.

Die Lücke existiert also bereits in der gewählten Tonhöhenanalyse, vor der
Easy-/Medium-Reduktion. Offen ist, welche Töne tatsächlich gespielt werden und
welche interne Detektorstufe sie gegebenenfalls verwirft. Gesamtmix-Anschläge
werden deshalb nicht als angebliche Gitarrennoten ergänzt. Hier muss gezielt
angehört werden; eine echte Gitarrenpause ist nicht nachgewiesen.

## Metrische Gruppierung

Die veröffentlichte Leadsheet-Ausgabe nennt punktierte Viertel = 48;
Hal Leonard beschreibt seine Ensemble-Ausgabe ausdrücklich als 6/8. Diese
Angaben betreffen die Ausgaben, nicht eine Messung der importierten Aufnahme.
[Musicnotes / Universal-Ausgabe](https://www.musicnotes.com/sheetmusic/metallica/nothing-else-matters/MN0253739),
[Hal Leonard](https://www.halleonard.com/product/7014488/nothing-else-matters).

Das akzeptierte Raster hat 143,131 BPM und einen medianen Abstand von 0,420
Sekunden. Die Nähe zu drei Rasterimpulsen pro veröffentlichtem Puls deutet auf
eine andere Zählebene hin; sie beweist keinen Rasterfehler. Dieses Raster bleibt
unverändert. Die Vier-Beat-Fenster des Piloten sind keine nachgewiesenen
musikalischen Takte. Ob die Reduktion die gehörte Gruppierung erhält, bleibt
im Spielvergleich zu prüfen. Keine Taktarterkennung wurde ergänzt oder aus
dem Songnamen abgeleitet.

## Spielbarer Eintrag und Prüfung

Installiert unter `songs/imported/guitar-study-metallica/` als
**[Guitar Study] Nothing Else Matters**. Audio, Lautstärke-Sidecar, LRC und
Wortausrichtung sind bytegleiche Kopien des Originals. Audio-Priming und
gespeichertes Raster bleiben gleich. Originalsong und aktiver Versionszeiger
bleiben erhalten. Musik, Lyrics und Charts bleiben lokale Nutzerinhalte.

Der Vergleichslauf validierte alle vier Varianten auf allen vier Stufen.
Messwerte und Analysen: `local/guitar-study-metallica/metrics.json` und
`local/guitar-study-metallica/comparison/report.json`.

Die lokalen Qualitätsprüfungen sind bestanden: fmt, Clippy aller Targets und
Features mit `-D warnings`, Workspace-Tests (1.093 bestanden, vier bestehende
ignoriert), check und Rustdoc mit/ohne alle Features, jeweils `-D warnings`.
Der erste Test-Build scheiterte beim Linken an vollem Datenträger. Nach Bereinigung
des regenerierbaren Release-Caches bestand die Wiederholung; der fertige
App-Launcher wurde vorher gesichert und bytegleich wiederhergestellt.

### Autopilot-Läufe

Drei Läufe auf Medium waren nötig, bis ein Urteil vorlag. Die ersten beiden
scheiterten nachweislich nicht am Chart.

1. Der erste Lauf brach bei 266,213 → 267,235 Sekunden mit `clock teleported`
   und Exit 1 ab (`autopilot-medium.log`). Parallel war gerade der
   Qualitätslauf gestartet worden; die Prüfung vergleicht Song-Fortschritt mit
   Frame-Dauer und ist damit lastempfindlich. Ein ursächlicher Zusammenhang ist
   nicht bewiesen; in den Wiederholungen ohne parallelen Build trat der Abbruch
   nicht mehr auf. Weder die Prüfung noch die Spieluhr wurden verändert.
2. Der zweite Lauf spielte den Song zu Ende und meldete **fünf Overstrums** bei
   498 Noten — 498 perfekt, 0 verfehlt, Genauigkeit 100 %, Urteil trotzdem
   FAILED (`autopilot-medium-retry.log`). Das Telemetrie-Protokoll des Laufs
   zeigt jede der 498 Noten mit **0,0 ms Abweichung** getroffen und die fünf
   zusätzlichen Anschläge **zwischen** den Treffern (nach den Noten 0, 15, 25,
   40 und 42). Der Injektor schlägt höchstens einmal pro offener Note an; 503
   Anschläge kann er nicht erzeugen. Sie kamen von einem echten Eingabegerät am
   Rechner. Über alle 305 aufgezeichneten Autopilot-Sitzungen tragen sieben
   Overstrums, und in jeder einzelnen sind sämtliche Noten getroffen — dasselbe
   Muster. Seit 0.15.8 sind echte Geräte stumm, solange der Injektor spielt.
3. Der dritte Lauf, unberührter Rechner: **498 perfekt, Streak 498, 0 Miss,
   0 Overstrums, PASSED** (`autopilot-medium-clean.log`).

Hard im selben Zustand: **679 perfekt, Streak 679, 0 Miss, 0 Overstrums,
PASSED** (`autopilot-hard.log`).

Damit ist der Pilot auf Medium und Hard technisch fehlerfrei spielbar. Über das
Gitarrengefühl sagt das nichts: der Autopilot trifft jede Note, die dasteht.

Zuerst Medium und Hard vergleichen: Anfangsfiguren, Übergang bei 0:53–1:00,
Gesangsbegleitung und spätere dichtere Passagen. Ein Spielerurteil zu diesem
zweiten Pilot fehlt noch. Weniger HOPOs und mehr Sustains beweisen kein besseres
Gitarrengefühl. Quellenlücke und Gruppierung erfordern weitere Prüfung vor
einer allgemeinen Übernahme; Van Halens positives Urteil gilt nur für Van Halen.
