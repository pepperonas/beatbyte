# Beatbyte – automatische Song-Charts

Eigenständiger Forschungsprototyp: neue Audioaufnahme → lokale Audio-KI → musikalische Ereignisse → Guitar-Hero-Griffmuster → vier Beatbyte-Schwierigkeiten. Der Prototyp liegt unter `tools/song-chart`; er verändert weder die App-Pipeline noch die USB-Bibliothek.

Abgeschlossen: 48 vollständige Songs jeweils mit DSP und KI verarbeitet, 17 Tests bestanden und 164 Chartdateien einschließlich Vergleichsfassungen geprüft. Im zeitlichen Expert-Referenzvergleich liegt der KI-Prototyp bei F1 0,443 gegenüber 0,549 für den vorhandenen Mix-Generator. Ein musikalischer Qualitätsgewinn ist noch nicht nachgewiesen; dafür sind die Hör-/Spielvergleiche vorbereitet.

## Einen neuen Song verarbeiten

Im Terminal:

```sh
cd /Users/martin/claude/beat-byte/tools/song-chart
./bbchart.sh generate "/Pfad/zum/neuen-song.mp3" \
  --out "outputs/meine-tests/mein-neuer-song" \
  --title "[AUTO] Mein Song" --artist "Interpret"
```

Standard: lokale KI, eingefrorene Modell-/Regelauswahl aus `config/frozen.json` (wenn vorhanden). Ein Song braucht je nach Länge und Rechner mehrere Minuten. Es erfolgt kein Audio-Upload. Modell-Downloads können beim ersten Setup nötig sein.

Der Ausgabeordner enthält `song.*`, `chart.json`, `analysis.json`, `config.json`, `report.json`, `preview.wav` und `chart-preview.png`. In der Stereo-Vorschau ist links der Song, rechts der Song mit Notenklicks zu hören. Die Darstellung zeigt alle vier Schwierigkeiten. Für das Spiel sind die Audiodatei und `chart.json` maßgeblich; der Ordner kann später als separater Song in eine Testbibliothek übernommen werden. Der Generator registriert ihn nicht automatisch in deiner aktiven Bibliothek.

Wähle für jeden Versuch einen neuen Ausgabeordner. Vorhandene fertige Charts werden nicht überschrieben.

### Varianten

```sh
# Eigene, samplegenau zur Aufnahme ausgerichtete Gitarrenspur nutzen:
./bbchart.sh generate "/Pfad/song.flac" --lead "/Pfad/guitar.wav" \
  --out outputs/mit-gitarrenspur

# Anderes Trennmodell ausdrücklich ausprobieren:
./bbchart.sh generate "/Pfad/song.m4a" --separator htdemucs_6s \
  --out outputs/gitarre-6s

# Konstantes Raster manuell vorgeben, erster Beat bei 0,37 s:
./bbchart.sh generate "/Pfad/song.mp3" --bpm 125 --first-beat 0.37 \
  --out outputs/manuelles-raster

# Schneller DSP-Vergleich ohne KI (Gesamtmix kann keine Gitarre isolieren):
./bbchart.sh generate "/Pfad/song.mp3" --mode dsp --out outputs/dsp-vergleich

# Nur analysieren:
./bbchart.sh analyze "/Pfad/song.mp3" --out outputs/nur-analyse
```

Ohne belegte Tonhöhen wird kein Chart mit künstlich aufgefüllten Beats geschrieben. Instrumentale Pausen werden beibehalten. Ein übergebener Lead muss dieselbe kanonische Zeitachse haben; die automatische Längenprüfung beweist keine Sample-Ausrichtung.

## Was die Logik macht

1. Beatbytes Decoder setzt den Audio-Nullpunkt einschließlich Encoder-Vorlauf korrekt.
2. Beat This! liefert ein variables Beat-/Taktraster; Demucs trennt eine harmonische Instrumentenspur.
3. Basic Pitch liefert Tonhöhen, Akkordkandidaten und Haltedauern. Starke Anschläge innerhalb gehaltener Töne rekonstruieren gegebenenfalls wiederholte Strums.
4. Eine Phrasenoptimierung bildet musikalische Konturen auf fünf Spielspuren ab. Wiedererkannte Phrasen bekommen dieselben Griffmuster.
5. Hard, Medium und Easy entstehen durch lokale, musikalisch gewichtete Reduktion des Expert-Masters. Weniger Ereignisse, kleinere Akkorde und einfachere Griffe; erhaltene Ereignisse behalten ihre Zeitposition.
6. Jeder fertige Chart muss Beatbytes echten Validator passieren.

Die fünf Spielspuren sind keine Gitarrentabulatur. Das Modell erkennt Instrumententöne; das Skript entscheidet nachvollziehbar, wie sie gespielt werden sollen. Ein eigenes neuronales Chartmodell wird in v1 nicht trainiert.

## Gespeicherte Daten und Berichte

Versioniert sind Quellcode, Konfiguration, Tests und die bisherigen Ergebnisberichte. Audio, Modelle, Python-Umgebungen, Rust-Builds, Cache und `outputs/` werden nicht eingecheckt. Die vollständige Bibliotheksinventur `reports/inventory.json` entsteht durch `scan` und bleibt lokal. Die Berichte sind historische Messungen vom 21.10.2026; darin enthaltene Dateipfade und Prüfsummen beschreiben den damaligen Lauf. Die Links auf Audio-/Chart-Vergleiche funktionieren erst nach Erzeugung der zugehörigen Ausgaben. Die ursprünglichen vollständigen Testdaten liegen auf diesem Mac weiterhin unter `/Users/martin/Downloads/bb-testing-song-chart`.

## Analyse und Ergebnisse lesen

- `reports/corpus-analysis.md`: Bibliotheksbefund, Herkunftsgrenzen und Referenzstatistik.
- `reports/corpus.csv`: die 48 Referenzaufnahmen samt Entwicklung/Holdout.
- `reports/model-selection-summary.json`: Entscheidung zwischen den Trennmodellen.
- `reports/results.md`: zusammengefasste gemessene Ergebnisse und Grenzen.
- `reports/listening-cases.md`: spielbare Vergleiche von Referenz, Mix-/Lead-Baselines und neuem Chart.
- `docs/architecture.md`: vorhandene Pipeline und neue Generatorlogik.
- `docs/play-in-beatbyte.md`: geprüfter Ladeweg im Projektstand und Hörvergleich.
- `docs/methodology.md`: genaue Messmethodik und bekannte Einschränkungen.
- `reports/audio-chart-relationships.md`: gemessene Beziehung zwischen Audio-Tonhöhen und Referenzgriffen.
- `docs/limitations-and-next-steps.md`: konkrete Grenzen und priorisierte Fortsetzung.
- `reports/tests.log`: technische Tests.

Zeitliche Treffer allein beweisen keine musikalische Qualität. Die Berichte nennen Precision und Recall gemeinsam und markieren den menschlichen Spieltest ausdrücklich als noch offen.

## Messungen wiederholen

```sh
./bbchart.sh scan "/Volumes/USB-1 128GB/beatbyte/songs"
.venv/bin/python tools/report_inventory.py
.venv/bin/python tools/model_selection.py
.venv/bin/python tools/benchmark.py
./bbchart.sh evaluate --mode dsp --tag weiterer-dsp-lauf
.venv/bin/python tools/prepare_comparisons.py
.venv/bin/python tools/summarize.py
.venv/bin/python tools/verify_outputs.py
.venv/bin/python -m unittest discover -s tests -v
```

`evaluate` kann mit `--limit`, `--split development|holdout`, `--config` und `--tag` eingeschränkt werden. Fertige Charts innerhalb eines bestehenden Benchmark-Tags werden wiederverwendet; für geänderte Generatorregeln einen neuen Tag wählen. Caches sind zwischen Läufen geteilt. Für unveränderte Bibliotheksaufnahmen wird kein Modell auf den Holdout-Referenznoten optimiert.

## Installation und Voraussetzungen

Auf diesem Mac ist die Umgebung bereits eingerichtet: ignorierte lokale Symlinks verwenden die geprüfte Python-Umgebung, Modelle und Adapter-Builds aus dem ursprünglichen Testordner weiter. Diese Verknüpfungen werden nicht gepusht. Nach einem frischen Checkout zuerst:

```sh
./setup.sh
# Nur falls kein bestehendes Demucs-Python verfügbar ist:
./setup.sh --demucs
```

Python, Rust/Cargo und gegebenenfalls FFmpeg müssen installiert sein. Der Rust-Adapter bindet die Beatbyte-Crates über relative Pfade in `adapter/Cargo.toml` ein. Der Ordner muss deshalb innerhalb dieses Repositorys bleiben. Python-Pakete sind in `requirements-lock.txt`, der Beatbyte-Stand in `reports/beatbyte-revision.txt` und Rust-Abhängigkeiten in `adapter/Cargo.lock` festgehalten.

Demucs wird bevorzugt aus `.demucs-venv`, sonst aus `/Users/martin/demucs-env` aufgerufen; `--demucs-python` überschreibt dies. Modelle liegen unter `models`, kanonisches Audio und wiederverwendbare Analysen unter `cache`. Nur die ausgewählte Instrumentenspur bleibt nach der Trennung im Cache. Bei ausdrücklich geänderten Modellgewichten / DSP-Algorithmen sollten betroffene Caches entfernt bzw. separat archiviert werden; der vorhandene Cache ist kein allgemeiner Versionsmanager.

## Grenzen

V1 verwendet eine ausgewählte harmonische Instrumentenspur pro Song. Die Instrumententrennung und Polyphonieerkennung können gerade bei verzerrten Gitarren, schnellen Soli und elektronischen Sounds irren. Vollautomatischer musikalischer Stimmenwechsel und semantische Songstruktur-Erkennung sind noch nicht zuverlässig gelöst. Hör- und Spieltests sind deshalb der nächste Qualitätsmaßstab, bevor der Generator in die App übernommen wird.

Die abschließenden Benchmarks verwenden 5-Sekunden-Segmente für die Instrumententrennung. GPU-Trennungen werden innerhalb dieses Testprojekts über eine Dateisperre nacheinander ausgeführt; auf dem 16-GB-Mac war gleichzeitige Trennung langsamer und speicherintensiv.

Bei den langen Benchmarks können bereits ausgewertete PCM-Zwischenkopien mit `tools/prune_completed_pcm.py` entfernt werden. Das Werkzeug rendert vorher fehlende Vorschauen und behält Charts, kopiertes Originalaudio, komprimierte Instrumentenspuren sowie Analyse-JSON. Ein späterer Analyseaufruf decodiert fehlendes PCM erneut aus dem Original. `reports/pruned-pcm.json` dokumentiert entfernte Zwischenkopien.
