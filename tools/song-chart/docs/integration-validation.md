# Prüfung der Spielintegration – 21.10.2026, Beatbyte 0.19.2

Die Integration ruft dieselbe lokale Python-Pipeline auf wie der CLI-Test.
Die Installation dieses Macs ist pro Gerät registriert; die normale
`/Applications/BeatByte.app` startet den Projekt-Build mit `--features ml`.

## Geprüft

- 19 Python-Tests erfolgreich. Die Offline-Modellprüfung wurde zusätzlich
  mutiert; der Test erkannte die entfernte Download-Sperre.
- Workspace: Formatierung, Clippy für alle Targets/Features mit verbotenen
  Warnungen, Tests, Check und Rustdoc mit Standard- sowie allen Features
  erfolgreich. Die Quelldateien enthalten jetzt 2.320 Tests einschließlich
  ignorierter Integrationstests und Dokumentationsbeispiele.
- Native Integration mit einer vollständigen M4A-Aufnahme von ca. 2:32 Minuten:
  Noten aller vier Stufen und Beat-Raster stimmen exakt mit dem vorherigen
  CLI-Ergebnis überein. Diese Prüfung verwendet die tatsächlich registrierte
  Runtime und ist im normalen Testlauf wegen ihrer lokalen Voraussetzungen
  ignoriert; sie wurde hier ausdrücklich ausgeführt.
- Echter Spiel-Harness: M4A-Ausschnitt über das Drop-Ereignis eingereiht,
  KI-Chart erstellt, Songdokument geschrieben, Bibliothek neu eingelesen und
  Expert gespielt. Ergebnis: 103 perfekte Spielereignisse, 0 verfehlte Noten,
  0 Overstrums, 100 % Genauigkeit, explizites `run PASSED`, Exit 0.
- Die Importanzeige wurde auf dem gespeicherten Spiel-Screenshot geprüft.
  Die Testbibliothek war getrennt; Bibliothekswahl und Benutzereinstellungen
  wurden anschließend wiederhergestellt.
- Ein erster GUI-Lauf endete ohne Testurteil und wurde ausdrücklich nicht als
  Erfolg gewertet. Der vollständige erneute Lauf bestand bis zum Ergebnisbild.
- Subprozess-Tests prüfen wörtliche Argumentübergabe, ungültige Ausgabe,
  Zeitlimit samt Kindprozessgruppe und das normale Beenden der App. Ein
  fehlgeschlagener Import kann wiederholt werden und löscht keinen Altbestand.

Die Automatik beweist technische Spielbarkeit und die Identität zur CLI-Logik.
Sie ersetzt die menschliche Bewertung von Rhythmus und Griffgefühl nicht.

Lokale Rohprotokolle, Screenshots und der kurzlebige Testsong liegen unter
`temp/song-chart-integration` im Projekt; sie werden nicht gepusht. Die
historischen 48-Song-Benchmarks bleiben davon getrennt unter `reports`.
