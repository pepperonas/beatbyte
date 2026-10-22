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

## Audio-only-Import und Anschlagen – 22.10.2026, Beatbyte 0.19.3

- Die alten Standard-/Study-/Classic-Generatoren und fertige Bridge-/Community-
  Chartimporte bleiben im Quellcode, sind aber an den Spiel-, CLI- und
  Skript-Einstiegen deaktiviert. Fünf echte CLI-Aufrufe wurden mit nicht
  existierenden Eingaben geprüft: alle brechen vor Dateizugriffen mit der
  Abschaltmeldung ab. Auch die alten Batch-Skripte verweigern die Ausführung.
- Zwei neue Verhaltenstests prüfen einen echten WAV-Import ohne registrierte
  Pipeline samt Aufräumen und die Ablehnung eines unerreichbaren Bibliotheksziels.
  Beide erkannten die gezielt wieder eingeschalteten Altwege. Danach wurden
  die Änderungen zurückgenommen und die vollständigen Projektprüfungen bestanden:
  fmt, Clippy aller Targets/Features mit verbotenen Warnungen, Workspace-Tests,
  Check sowie Rustdoc mit Standard- und allen Features. Quellinventar: 2.322 Tests.
- Die vollständige M4A-Aufnahme wurde durch den nativen Import geprüft:
  Noten aller vier Stufen und Beat-Raster entsprechen exakt dem vorherigen
  Skriptergebnis. Die bestehende USB-Kopie ist bytegleich mit der Quelldatei.
- Der finale Release-Build mit `--features ml` wurde mit einer separaten
  Testbibliothek direkt auf dem USB-Laufwerk geprüft: M4A per Drop importiert,
  Originalaudio bytegleich kopiert, vier Charts, `chart-generation.json`,
  `song.json` und Lautheitsdaten gespeichert, anschließend Expert gespielt.
  Bei **Tap-Hilfe OFF**: 103 perfekte Spielereignisse, 0 Misses, 0 Overstrums,
  100 % Genauigkeit, explizites `run PASSED`, Exit 0.
- Der echte Einstellungsbildschirm wurde mit fehlendem `tap_mode`-Feld und
  gespeichertem `false` fotografiert und per OCR sowie Sichtprüfung als OFF
  geprüft. Ein ausdrücklich gespeichertes `true` bleibt ON. Das globale
  Default ist OFF; bestehende bewusste Opt-ins werden nicht beim Laden erzwungen.
- Menüs und Tastenhilfe zeigen die deaktivierten Bridge-/Redesign-Aktionen
  nicht mehr. Das README-Mockup wird aus aktuellen Spielaufnahmen mit dem
  bestehenden HTML/CSS-Renderer erzeugt. Für statische Menübilder sind die
  Startanimationen reduziert; die eigentliche Spielrunde nutzt die gespeicherten
  Spieleinstellungen.
- Ein bestehender Bibliotheks-Test änderte noch die echte Gerätewahl. Er nutzt
  jetzt ausschließlich ein Testverzeichnis. Die vollständige Testsuite erhält
  die gewählte USB-Bibliothek; nach allen GUI-Prüfungen wurden die Einstellungen
  wiederhergestellt. Test-Songs lagen außerhalb der echten Songsammlung und
  wurden nach erfolgreicher Prüfung entfernt.

Lokale Protokolle, Aufnahmen und Prüfbelege liegen unter
`temp/audio-only-import` und werden nicht gepusht. Ältere, erneut erzeugbare
Compiler-Ausgaben wurden platzsparend auf dem USB-Laufwerk gesichert; die
Dateiliste samt SHA-256 liegt in `build-cache-relocation.json` dieses Ordners.
Songdateien und Modelle wurden dabei nicht verschoben.


## Projektlokale Laufzeit – 22.10.2026

Der frühere Ordner `/Users/martin/Downloads/bb-testing-song-chart` wurde
vollständig nach `tools/song-chart` überführt. `.venv`, `models` und
`adapter/target` sind echte Verzeichnisse im Projekt; ihre alten Verknüpfungen
sind entfernt. Demucs liegt als eigene `.demucs-venv` ebenfalls im Projekt.
Absolute Pfade in den Startskripten beider Python-Umgebungen wurden angepasst.
Der gemeinsame ältere Demucs-Installationsordner bleibt unberührt, wird von
dieser Einrichtung jedoch nicht mehr benötigt.

Die ursprünglichen Ausgaben liegen in `outputs/prototype-history/`, die übrigen
Originaldateien und zusätzlichen Berichte in `local-archive/downloads-prototype/`.
Der historische Forschungs-Cache bleibt auf USB; die archivierte Verknüpfung
zeigt weiterhin dorthin. Historische Messberichte behalten ihre damaligen Pfade.
Modelle, Umgebungen, Testaudio und Archiv werden nicht eingecheckt.

Prüfung bei bereits entferntem Downloads-Ordner:

- Explizite Offline-Registrierung erfolgreich, Modelle und Separator geprüft.
- Alle 19 Python-Tests erfolgreich, einschließlich der beiden echten Adaptertests.
- Die umgezogenen `pip`- und `demucs`-Startskripte laufen am neuen Ort.
- Nativer M4A-Import auf eine isolierte USB-Testbibliothek erfolgreich:
  vier Schwierigkeitsstufen, bytegleich kopiertes Audio und Song-Begleitdaten.
  Anschließender Expert-Spieltest: 103 perfekte Treffer, 100 %, keine Fehler
  oder zusätzlichen Anschläge; `tap_mode=false`, Ergebnis `run PASSED`.
- Persönliche Einstellungen und die echte USB-Bibliotheksauswahl wurden
  nach dem Spieltest wiederhergestellt.

Formatierung, Clippy (alle Targets/Features), sämtliche Workspace-Tests,
`cargo check` und Rustdoc mit sowie ohne optionale Features bestehen ebenfalls.

Lokale Prüfprotokolle: `temp/runtime-relocation/` (nicht versioniert).
