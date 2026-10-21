# Experimenteller Audio-zu-Chart-Generator

Der eigenständige Forschungsprototyp liegt unter [`tools/song-chart`](../../tools/song-chart/README.md).
Er erzeugt aus einer Audiodatei vier Beatbyte-Schwierigkeitsstufen und nutzt
Beatbytes Decoder, Modelle und Chartvalidator. Er ersetzt den App-Generator nicht.

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
