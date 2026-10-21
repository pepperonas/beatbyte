# Experimenteller Audio-zu-Chart-Generator

Der eigenständige Forschungsprototyp liegt unter [`tools/song-chart`](../../tools/song-chart/README.md).
Er erzeugt aus einer Audiodatei vier Beatbyte-Schwierigkeitsstufen und nutzt
Beatbytes Decoder, Modelle und Chartvalidator. Seit 0.19.2 verwendet der Audioimport im Spiel diese Pipeline, wenn sie auf dem Gerät ausdrücklich eingerichtet wurde. Seit 0.19.3 ist sie der einzige erlaubte Importweg: Ohne registrierte Installation wird der Import mit einer Einrichtungsanweisung abgebrochen.

- [Installation und Skriptaufrufe](../../tools/song-chart/README.md)
- [Audio einlesen und in Beatbyte probespielen](../../tools/song-chart/docs/play-in-beatbyte.md)
- [Architektur](../../tools/song-chart/docs/architecture.md)
- [Messmethodik](../../tools/song-chart/docs/methodology.md)
- [Ergebnisse](../../tools/song-chart/reports/results.md)
- [Grenzen und nächste Schritte](../../tools/song-chart/docs/limitations-and-next-steps.md)

48 vollständige Songs wurden mit DSP und KI ausgewertet. Der neue KI-Ansatz
liegt beim zeitlichen Expert-F1 bei 0,443, der vorhandene Mix-Generator bei
0,549. Eine Verbesserung der musikalischen Spielqualität ist bisher nicht
nachgewiesen; menschliche Hör- und Spieltests stehen aus.

Audio, Modellgewichte, Umgebungen und generierte Ausgaben bleiben lokal.
Die historischen Berichte nennen den geprüften Stand und die Messbedingungen.

## Im Spiel benutzen

Auf diesem Mac ist der Runtime-Pfad registriert. Nach dem Neubauen von Beatbyte
eine MP3-, M4A-, WAV-, OGG- oder FLAC-Datei ins Hauptmenü oder den Songbrowser
ziehen. Der Import läuft im Hintergrund und zeigt Audio-, Beat-, Instrumenten-,
Noten- und Chartphasen an. Erst nach nativer Validierung aller vier Stufen
erscheint der Song im Browser. Bestehende Songs werden übersprungen.

Für einen neuen Checkout einmal `cd tools/song-chart && ./setup.sh --demucs`
ausführen. Eine bereits eingerichtete Umgebung wird mit `./bbchart.sh register`
registriert. Das schreibt nur die gerätespezifische Datei
`<data>/beatbyte/song-chart-runtime.json` mit Protokollversion und absolutem
Runtime-Pfad. Dadurch funktioniert der Import auch aus einer installierten App
und unabhängig vom Arbeitsverzeichnis. Python, Demucs und Modelle werden separat
eingerichtet; diese Änderung bündelt sie nicht in DMG oder ZIP.

Während des Imports werden keine Modelle heruntergeladen. Ist die registrierte
Installation defekt oder liefert die KI keine ausreichenden Belege, schlägt der
Import sichtbar fehl und bleibt wiederholbar. Es wird dann kein anderer Chart
als Ersatz erzeugt. Ohne Registrierung wird ebenfalls kein Chart erzeugt. Classic, Guitar Study,
Standard-Fallback, alte Neugenerierung und fertige Bridge-/Community-Chartimporte
sind im Spiel und in den alten CLI-Einstiegspunkten deaktiviert. Die Algorithmen
bleiben im Quellcode erhalten; vorhandene Charts bleiben spielbar.

`chart-generation.json` neben dem fertigen Chart hält Konfiguration,
Modell-/Code-Prüfsummen und Messdaten fest. Die ältere automatische
Gitarren-Nachoptimierung ist vollständig deaktiviert; optionale
Gesangsanalyse bleibt möglich. Fehlerprotokolle liegen unter
`<data>/beatbyte/song-chart-jobs/*.log`. Die Analyse hat ein Zeitlimit von
30 Minuten; abgebrochene Jobs werden beendet und temporäre Ausgaben entfernt.

Die Integration verwendet bewusst dieselben Python-Regeln wie der geprüfte
Skriptlauf. Eine zweite native Implementierung würde die Regeln duplizieren.
Die externe Runtime wird ausdrücklich registriert; Code aus dem aktuellen
Arbeitsverzeichnis wird nicht automatisch ausgeführt.

## Entwicklerprüfung

Der normale Workspace-Testlauf prüft Registrierung, Subprozessargumente,
Fortschritt, Fehler, Validierung und Wiederholbarkeit ohne Modelle. Der
ignorierte Test `installed_pipeline_imports_real_audio_and_preserves_all_four_levels`
benutzt die tatsächlich registrierte Runtime. `BEATBYTE_AUTOPILOT_DROP` benennt
dabei die Eingabedatei; optional vergleicht `BBCHART_REFERENCE_CHART` die Noten
und das Raster byteinhaltlich mit dem vorherigen CLI-Ergebnis. Der eigentliche
Spiel-Harness nimmt dieselbe Datei über das Drop-Ereignis an.

## Speicherort

Die Audiodatei wird aus Downloads oder einem anderen Quellordner in den
eingestellten Bibliotheksordner kopiert. Dort liegen auch `chart.json`,
`chart-generation.json`, `song.json`, Lautheitsdaten und gegebenenfalls Lyrics
zusammen im Songordner. Die Quelldatei bleibt erhalten. Das Anzeigen einer
USB-Bibliothek bedeutet noch nicht, dass dieser Ordner das Importziel ist:
Das Ziel wird unter SETTINGS > LIBRARY beziehungsweise gerätespezifisch in
`<data>/beatbyte/library-location.json` festgelegt. Ist das gewählte Laufwerk
nicht erreichbar, bricht der Import ab und schreibt nicht auf die interne SSD.
Modelle, Analyse-Cache und Fehlerprotokolle bleiben lokal auf dem Mac; sie sind
zum Abspielen des fertigen Songordners nicht erforderlich.
