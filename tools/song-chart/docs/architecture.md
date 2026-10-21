# Architektur und Ableitungen

## Bestehendes Beatbyte-Projekt

Untersuchter Stand: siehe `reports/beatbyte-revision.txt`. Quellprojekt: `/Users/martin/claude/beat-byte`. Der ursprünglich externe Prototyp ist jetzt unter `tools/song-chart` versioniert; die App-Pipeline bleibt unverändert.

Die bestehende Pipeline ist bereits umfangreich. `beatbyte-audio/src/analysis/mod.rs` decodiert / halbiert hohe Sampleraten, erkennt Spectral-Flux-Onsets, schätzt Tempo, verfolgt ein variables Beat-Raster, extrahiert Melodien und findet Wiederholungen. Die eingebaute Analyse allein liefert keine Downbeats. `beatbyte-meter` ergänzt Beat This!. `beatbyte-poly` führt Basic Pitch über den bestehenden verifizierten ONNX-Runtime aus. `beatbyte-chart/src/generate.rs` erzeugt den Standardchart, `generate/lead_study.rs` den vorhandenen instrumentbezogenen Versuch.

Der normale Generator hat feste Schwierigkeitsprofile und setzt unter anderem Akkorde über saliente Anschläge. Der Lead-Study-Ansatz folgt belegten Tonhöhen einer Instrumentenspur, unterdrückt dabei künstliche Akkorde und faltet Tonhöhen innerhalb einer Phrase auf fünf Spuren. Unser Prototyp vergleicht sich mit beiden; er ersetzt keinen davon.

Bridge-Import: `.chart` und `.mid` werden in `beatbyte-chart/src/bridge` zunächst zu TickSong mit Tempo- und Taktwechseln gelesen. `assemble` übersetzt auf Sekunden / Spuren und übernimmt HOPO- und Sustain-Konventionen. `fill_missing_levels` ergänzt fehlende untere Levels. Daher sind vier Levels in einem JSON **kein Nachweis für vier menschlich erstellte Referenzen**.

## Neue Verarbeitung

`bbchart.sh` → Python CLI → kanonischer Rust-Decode → Mixanalyse / Beat This! → optionale lokale Demucs-Trennung → Basic Pitch oder DSP-Melodie → belegte Ereigniskandidaten → Phrase / Griffoptimierung → verschachtelte Reduktion → Format-v1-Export → echter Beatbyte-Validator → Hör- und Bildvorschau.

Die Spielchart-Logik ist deterministisch. Im KI-Pfad sind Demucs-Seed und Shift-Verhalten fixiert; Modelle werden mit Prüfsummen dokumentiert. Bitidentität von GPU-Inferenz über unterschiedliche Hardware / Frameworkversionen wird nicht versprochen. Audio wird nicht hochgeladen. Modellgewichte können beim ersten Setup aus den registrierten Bezugsquellen geladen werden.

### Zeitachse

Der Adapter verwendet `beatbyte_audio::decode_file`: Der vom Container deklarierte Encoder-Vorlauf wird genauso wie in Beatbyte übersprungen. Der Original-Trim wird beim WAV-Zwischenschritt separat bewahrt und in die Chartdatei übernommen. Demucs sieht ausschließlich diesen kanonischen PCM-Decode. Fremde Instrumentenspuren müssen dieselbe Länge (±25 ms) und denselben Sample-Nullpunkt haben; die Längenprüfung allein kann eine verschobene Spur nicht erkennen.

Alle Noten speichern absolute Songsekunden. Das Beat-Raster enthält einzelne Beats und übernimmt variable Tempi. Das Raster dient der begrenzten Quantisierung, nicht der Erfindung von Noten. `--bpm` überschreibt das Raster ausdrücklich mit konstantem Tempo; `--first-beat` setzt dessen Ursprung. Dieser manuelle Pfad nimmt 4/4 an; ohne belegte Downbeats erfindet der automatische Pfad keine Taktanfänge.

### Anschläge und Harmonie

Basic Pitch liefert Tonhöhen mit Anfang, Ende und Modellamplitude. Zusätzliche starke Anschläge innerhalb einer belegten gehaltenen Tonhöhe können wiederholtes Anschlagen derselben Note rekonstruieren. Diese Regel ist eine Hypothese und kann bei Instrumentenübersprechen falsch anschlagen. Ein Akkord braucht zeitgleich beginnende verschiedene Tonhöhen. Lautstärke oder Schlagzeugenergie allein erzeugen keinen Akkord.

Das DSP-Profil verwendet Beatbytes eingebauten Melodietracker. Ohne übergebene Instrumentenspur ist es ausdrücklich eine Gesamtmix-Baseline und kann keine Gitarre identifizieren.

### Griffmuster und Schwierigkeiten

Kandidaten werden bei Pausen und nach höchstens 16 Beats in Phrasen geteilt. Ein dynamisches Programm optimiert innerhalb einer Phrase fünf Spuren / mögliche Akkordformen. Kosten berücksichtigen Tonhöhenrang, Richtung der Melodie, konstante Wiederholungsgriffe und schnelle große Handbewegungen. Exakt wiedererkannte rhythmisch-melodische Phrasen verwenden dieselben Griffmuster; ihre aktuellen Zeitpunkte werden nicht kopiert.

Das ist keine Gitarrentabulatur: fünf Spielspuren repräsentieren musikalische Konturen und Griffmuster. Extreme Sprünge unter 120 ms werden ausgedünnt. Sustain endet vor dem nächsten Masteranschlag; leichtere Levels verlängern ihn nicht über ausgelassene Noten.

Hard ⊆ Expert, Medium ⊆ Hard und Easy ⊆ Medium bezüglich der Ereigniszeiten. Vier-Beat-Fenster erhalten lokale Budgets, damit ein lauter Refrain nicht alle Noten aus der leiseren Strophe verdrängt. Griffvereinfachung und Spurfaltung sind innerhalb dieser Reduktion explizit. Easy: drei Spuren, einzelne Noten; Medium: vier Spuren, höchstens Zweierakkorde; Hard: fünf Spuren, Zweierakkorde; Expert: fünf Spuren, höchstens Dreierakkorde. HOPOs entstehen nur für nahe, wechselnde Einzeltöne und werden nach jeder Reduktion neu bestimmt.

### Fehler und Wiederaufnahme

Unzureichende Tonhöhenbelege führen zu einer Ablehnung statt einem aufgefüllten Chart. Fehlende Modelle / defekte Audiodateien / fehlgeschlagene Trennung führen zu einem Fehler; DSP muss explizit gewählt werden. Ein vorhandenes `chart.json` wird nicht überschrieben. Die spielbare Datei wird nach der Validierung als letztes veröffentlicht. Ein gescheiterter Lauf kann Zwischenergebnisse im Zielordner behalten und ist erneut startbar, solange dort kein fertiger Chart liegt.

## Schnittstellen und Dateien

Der Rust-Adapter bietet `decode`, `analyze`, `poly`, `baseline`, `study`, `validate`, `source-info`, `install-models`. Er kommuniziert über JSON/stdout; Fehler gehen an stderr und führen zu Exitcode 1. Die Python-CLI bietet `scan`, `analyze`, `generate`, `evaluate`, `models`.

Pro Song enthalten `analysis.json` die musikalischen Belege und Cacheverweise, `config.json` die Regeln und `report.json` Herkunft, Modell-Hashes, Metriken und Validatorergebnisse. `chart.json` bleibt Beatbyte Format v1; keine neuen Felder im App-Schema. Laufkonfigurationen und Vergleiche befinden sich in den jeweiligen Ausgabeordnern.

## Quellen

- Beat This!, offizieller Code / Paper-Verweis: https://github.com/CPJKU/beat_this
- Demucs, Modelle und experimenteller 6-Spur-Pfad: https://github.com/facebookresearch/demucs
- Basic Pitch, offizieller Code: https://github.com/spotify/basic-pitch
- Maßgeblich für Parser / Zeitsystem / Modellverwendung sind die tatsächlich eingebundenen lokalen Beatbyte-Crates; Versionsstand im Bericht.
