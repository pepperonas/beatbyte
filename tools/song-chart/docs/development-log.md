# Entwicklungsprotokoll

## Ausgangslage

Der gewünschte Testordner war leer. Das vorhandene Rust-Projekt liegt unter `/Users/martin/claude/beat-byte`; die USB-Sammlung enthält 8.626 Songordner, 5 rohe Bridge-Quellordner und einen separaten Altbestand. Der entfernte Backup-Bestand wird nicht mit dem aktiven Korpus vermischt.

Vor dem Implementieren wurden 45-Sekunden-Ausschnitte von Duvet, Boilermaker und Saturday Night's Alright for Fighting mit einfachem Mix-Spectral-Flux untersucht. Es gab 564–701 Anschlagkandidaten gegenüber 95–217 Expert-Spielereignissen pro Ausschnitt. Die Spielereignisse lagen meist nahe an Anschlägen; die Auswahl war deutlich sparsamer als das Audio. Das begründete die Trennung von Audioereignissen und spielbaren Noten. Dies war ein explorativer Versuch, keine Modellqualitätsmessung.

## Umsetzung

Eigenständiger Python-Generator mit einem kleinen Rust-Adapter. Vorhandene, verifizierte Beatbyte-Modelle wurden in den Testordner kopiert; Demucs nutzt einen lokalen Projektcache. Keine App-Integration, keine Veröffentlichung, kein Schreiben in die USB-Songordner.

Die ursprüngliche Rust-Anbindung exportiert den Lead-Study-Generator im Modul `generate`, nicht im Crate-Root; dies wurde nach dem ersten Compilerlauf korrigiert. Die Bibliotheksstatistik wurde auf vektorisierte Einsekundenfenster umgestellt, um Ereignisse effizient zu zählen.

Der erste volle KI-Chart (Duvet) bestand den vorhandenen Beatbyte-Validator. Die Vorschau wurde visuell geprüft. Der zweite Gitarren-Trennpfad wurde mit demselben Song geprüft. Diese Piloten sind keine Holdout-Ergebnisse.

## Halbtempo-Fall

Boilermaker zeigte eine DSP-/KI-Abweichung von ungefähr 190/95 BPM. Die bestehende `beatbyte-meter::apply`-Politik schützt den App-Generator, indem sie unterschiedliche metrische Ebenen nicht austauscht. Im experimentellen KI-Pfad ist jedoch ausdrücklich das KI-Raster gewünscht. Der Adapter prüft deshalb ein nutzbares Modellraster und verwendet dieses einschließlich Downbeats; die Abweichung zum DSP-Tracker bleibt in den Metadaten erhalten (`explicit-model-grid-v2`).

Ältere Cache-Mixanalysen ohne diese Politik werden im KI-Modus erneut berechnet. Ursprüngliche fehlgeschlagene Modellwahl-Experimente werden nach dieser Korrektur wiederholt; nur der abschließende dokumentierte Durchlauf entscheidet das eingefrorene Trennmodell.

## Kalibrierungsgrenze

Nur zwei Songordner konnten mit tatsächlich vorhandenen Original-Leveldateien verknüpft werden. Der Entwicklungs-Korpus enthielt zu wenige verifizierte untere Levelpaare, um daraus belastbare Reduktionsquoten zu schätzen. Die Quoten bleiben deshalb explizite Designvorgaben, statt automatisch ergänzte Levels als menschliche Vorbilder auszugeben.

## Validierung

Gezielte Tests betreffen Codec-Vorlauf, veränderliches Raster / Triolen, Instrumentenpausen, mehrfaches Anschlagen gleicher Tonhöhen, echte Akkordbelege, verschachtelte Schwierigkeiten, Sustain-Kollisionen, deterministische Regeln, Eins-zu-eins-Matching, beschädigtes Audio, Stille und Schutz vorhandener Charts. Aktueller Bericht: `reports/tests.log`.

Der breit gefächerte Benchmark läuft auf 48 vollen Songs mit getrennten Entwicklungs-/Holdout-Gruppen. Erfolgreiche und fehlgeschlagene Versuche werden gemeinsam dokumentiert. Der menschliche Hör-/Spieltest bleibt ausdrücklich offen.

## Ressourcenbegrenzung

Zwei gleichzeitige MPS-Trennungen erzeugten auf dem 16-GB-Mac starken Swap-Druck und wurden langsamer. Nur die eigenen unfertigen Benchmarkprozesse wurden beendet. Der abschließende Lauf verarbeitet die beiden disjunkten Korpusshards nacheinander. Demucs-Segmente wurden auf 5 Sekunden begrenzt; die Modellwahl wird für diese Einstellung vollständig wiederholt. Neue Trennaufrufe sind zusätzlich durch eine lokale Dateisperre serialisiert, damit parallele CLI-Aufrufe nicht erneut mehrere GPU-Trennungen starten. Bereits vorhandene Piloten mit älterer Segmentlänge bleiben als separat gekennzeichnete Versuche erhalten.

Die Segmentumstellung traf außerdem eine CLI-Eigenheit: Demucs verlangt für `--segment` eine Ganzzahl; die anfängliche Übergabe `5.0` wurde abgelehnt. Die Argumente werden jetzt als Ganzzahl gebaut und durch einen Regressionstest gegen den tatsächlich installierten Demucs-Parser geprüft. Fehlgeschlagene Einrichtungsversuche werden wiederholt, bevor der abschließende Qualitätslauf ausgewertet wird. Die aktuelle Testsuite umfasst 15 bestandene Tests.

Der manuelle BPM-/Erstbeat-Pfad verwendet jetzt die DSP-Mixanalyse ohne obligatorischen Beat-Modell-Lauf. Dadurch funktioniert ein vorgegebenes Raster auch ohne automatische Periodizität. Ein zusätzlicher Test prüft dies auf stillen Audiodaten; die Chartgenerierung selbst verweigert Stille weiterhin. Aktuell: 16 bestandene Tests.

## Abschluss

Beide vollständigen Durchläufe sind abgeschlossen: 48 DSP- und 48 KI-Songausgaben, ohne Fehler im abschließenden Lauf. 164 Chartdateien einschließlich Piloten, Modellwahl, CLI-Kontrolllauf und Vergleichsfassungen wurden geprüft; alle bestehen die native Validierung und die jeweils anwendbaren Zusatzprüfungen. Vorhandene WAV-/PNG-Vorschauen sind lesbar. Acht Fälle mit je Referenz, Mix-Baseline, Lead-Study und neuem Generator liegen spielbar unter `outputs/comparisons`.

Die abschließende Testsuite enthält 17 bestandene Tests, einschließlich Schutz vor dem Überschreiben einer anderen Audiodatei in einem bereits bestehenden Ausgabeordner. Der normale CLI-Kontrolllauf erzeugt exakt dieselben Noten und dasselbe Raster wie der zugehörige Benchmarklauf.

Ergebnis: Expert-Ereignis-F1 bei ±50 ms im Makromittel 0,458 (neuer DSP-Pfad) und 0,443 (KI-Pfad), gegenüber 0,549 für den vorhandenen Mix-Generator. Kein nachgewiesener Produktionsersatz; die Fehlertypen und nächsten Schritte sind dokumentiert. Der menschliche Hör-/Spieltest bleibt offen. Das ursprüngliche Rust-Repository ist laut abschließendem Git-Status unverändert.

## Übernahme ins Beatbyte-Repository

Am 21.10.2026 nach `tools/song-chart` übernommen; Einstieg unter
`docs/audio/song-chart-prototype.md`. Adapter-Crate-Pfade, Modellregistrierung
und Testfixture sind relativ zum Checkout. Audio, Modelle, Umgebungen,
Builds, Cache, Ausgaben und die vollständige lokale Bibliotheksinventur
werden nicht versioniert. Die Berichte bleiben als historische Evidenz
erhalten; die Ausgangsdaten liegen weiterhin im Downloads-Testordner.
Die App-Pipeline wurde nicht durch den Prototyp ersetzt.

## Spielintegration in 0.19.2

Der gemeinsame Audioimport nutzt nun die ausdrücklich registrierte lokale
Runtime. Fortschritt, Offline-Betrieb, native Validierung, transaktionales
Anlegen des Songordners und Prozessbeendigung sind verbunden. Die ältere
automatische Gitarren-Nachoptimierung überschreibt diese neuen Charts nicht.
Prüfungen und tatsächlicher M4A-Spieltest stehen in
[integration-validation.md](integration-validation.md).
