# Die Charts in Beatbyte ausprobieren

Die Ausgabedateien wurden mit Beatbytes eigenem Format-v1-Parser und Validator geprüft. Ein echter menschlicher Spieltest wurde noch nicht durchgeführt. Der folgende Ladeweg stammt aus dem untersuchten Projektstand; eine ältere installierte App kann sich anders verhalten.

## Neue Audiodatei einlesen und spielen

Im Repository zum Prototyp wechseln, nach einem frischen Checkout zuerst `./setup.sh --demucs` ausführen:

```sh
cd tools/song-chart
./bbchart.sh generate "/vollstaendiger/Pfad/Mein Song.mp3" \
  --out "outputs/meine-tests/mein-song" \
  --title "[AUTO] Mein Song" --artist "Interpret"
```

M4A-Dateien mit AAC können genauso direkt übergeben werden; eine vorherige Konvertierung ist nicht nötig. Decoder und Encoder-Vorlauf wurden mit einer echten M4A-Datei geprüft.

Danach in Beatbyte `SETTINGS` → `LIBRARY` öffnen und den Ordner
`tools/song-chart/outputs/meine-tests` im Checkout auswählen. Im folgenden
Bestätigungsdialog **Leertaste** drücken, um den vorhandenen Ordner zu
verwenden. Den Song `[AUTO] Mein Song` wählen und beispielsweise mit Medium
beginnen. Für jeden weiteren Versuch einen neuen Ausgabeordner verwenden.

`preview.wav` und `chart-preview.png` im Songordner erlauben die Kontrolle
vor dem Spielen. Der Generator liefert alle vier Schwierigkeiten.

## Geeignete Testbibliothek

Für den direkten Vergleich ist dieser Ordner vorgesehen:

`/Users/martin/claude/beat-byte/tools/song-chart/outputs/comparisons`

Dieser Ordner wird nicht mit Git übertragen; er entsteht durch die Benchmark-/Vergleichsskripte. Die bereits vorbereiteten Vergleiche liegen auf diesem Mac weiterhin unter `/Users/martin/Downloads/bb-testing-song-chart/outputs/comparisons`.

Er enthält je ausgewähltem Fall getrennte Unterordner für Referenz, Mix-Baseline, Lead-Study und neuen Generator. Jeder dieser Ordner enthält Audio und `chart.json`. Welche Songs ausgewählt wurden, steht in `reports/listening-cases.md`.

Für alle neu generierten KI-Charts kann alternativ `outputs/full-ml-0` oder `outputs/full-ml-1` als Bibliothekswurzel dienen. `song.json` ist zum erstmaligen Einlesen nicht erforderlich; der Scanner kann direkt gültige Chartdateien laden. Der Scanner geht höchstens zwei Unterordnerebenen tief, weshalb die oben genannten Wurzeln passend gewählt sind.

## Vorhandenen Ordner auswählen

Im untersuchten Quellstand:

1. In Beatbyte `SETTINGS` → `LIBRARY` öffnen und mit Enter den Ordnerdialog aufrufen.
2. Den gewünschten Testbibliotheksordner auswählen.
3. Beim anschließenden Bestätigungszustand **Leertaste** drücken: `point_only()` verwendet den vorhandenen Ordner und scannt ihn neu. Enter würde an dieser Stelle den Kopier-/Umzugsablauf starten. Die Leertastenfunktion ist im Quellcode vorhanden, auch wenn der Dialogtext sie nicht aufführt.
4. Anschließend den Song im Browser auswählen und mehrere Schwierigkeiten vergleichen.
5. Zum bisherigen Bestand denselben Weg verwenden und wieder `/Volumes/USB-1 128GB/beatbyte/songs` mit der Leertaste auswählen.

Dieser manuelle Wechsel ändert die gewählte Bibliothekswurzel der App. Der Prototyp selbst hat weder diesen Wechsel ausgeführt noch deine bestehenden Bibliotheksdaten verändert.

Quellbelege: `beatbyte-game/src/settings_ui.rs` (`Act::Library`), `library_move.rs` (`point_only`), `library.rs` (`scan_roots`, `find_chart_files`) und `beatbyte-library/src/location.rs`.

## Hörvergleich ohne App-Wechsel

`preview.wav` lässt sich direkt abspielen: linker Kanal = Audio; rechter Kanal = Audio plus generierte Notenklicks. Die Vergleichsfamilien verwenden denselben 30-Sekunden-Ausschnitt. `chart-preview.png` zeigt Position, Akkorde und gehaltene Noten aller vier Schwierigkeiten.

Zuerst beurteilen, ob die beabsichtigte Instrumentenstimme getroffen wird. Danach Rhythmus, Wiederholung und Griffe prüfen. Erst beim tatsächlichen Spielen lässt sich beurteilen, ob die Schwierigkeitsabstände sinnvoll und die schnellen Wechsel angenehm sind.

Die Vergleichsvarianten besitzen absichtlich unterschiedliche Titelpräfixe. Beatbyte entfernt doppelte Browser-Einträge anhand von Titel und Interpret; identisch benannte Original- und Testfassungen aus mehreren Scan-Wurzeln können deshalb zusammenfallen. Für A/B-Vergleiche ist die vorbereitete Vergleichsbibliothek mit ihren eigenen Präfixen vorgesehen.
