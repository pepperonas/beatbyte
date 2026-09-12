# Gitarren-Gefühl: Recherche und Einzelversuch

Stand: 2026-09-12, Experiment 0.15.6. Auftrag: vorhandene Spielbarkeit erhalten,
aber die Verbindung zwischen Gitarrenpart und Eingabe verbessern. Erst einen
Song vergleichen; keine Neuberechnung der Bibliothek.

## Was der aktuelle Pfad tatsächlich macht

- `crates/beatbyte-game/src/import.rs:700`, `import_song`: dekodiert die Aufnahme,
  analysiert den Gesamtmix mit `SpectralAnalyzer` und ruft `generate_chart` auf.
  Ein Build mit `ml` kann außerdem das Beat-Raster mit Beat This! verfeinern.
- `crates/beatbyte-game/src/song_select.rs:888`, `redesign_song`: die Taste **G**
  analysiert ebenfalls den Gesamtmix. Hier fehlt der optionale Meter-Aufruf,
  den CLI und Import besitzen. Das kann verschiedene Raster erzeugen; es ist
  ein separat zu prüfender Integrationspunkt, nicht im Pilot verändert.
- `crates/beatbyte-chart/src/redesign.rs:39`, `merged_redesign`: ersetzt nur Hard
  und Expert; Easy und Medium stammen aus der aktiven Version. Ein normales
  Redesign würde einen Versuch auf Medium deshalb gar nicht sichtbar machen.
- `crates/beatbyte-audio/src/analysis/melody.rs`: HPSS trennt tonale und
  perkussive Spektrogrammanteile. Das ist keine Instrumententrennung. Ein
  einzelner Tonhöhenpfad kann auch Gesang, Bass-Obertöne oder Keyboard verfolgen.
- `crates/beatbyte-chart/src/generate.rs:843`, `merge_candidates`: ergänzt diese
  Melodie um Anschläge des Gesamtmixes. Nicht zugeordnete Anschläge bekommen
  Tasten über spektrale Helligkeit und einen zeitabhängigen Hash.
- `generate.rs:404`, `break_jacks`, verändert schnelle Tonwiederholungen zu wechselnden Tasten.
  `generate.rs:731`, `derive_notes`, erklärt nahe Tastenwechsel zu HOPOs und starke Einzelereignisse
  teilweise zu Akkorden. Das fördert Abwechslung und Spielbarkeit, beweist aber
  weder einen Tonwechsel noch Legato-Technik oder einen Gitarrenakkord.
- `generate.rs:1057`, `ContourMapper`, summiert Tonhöhenintervalle und begrenzt sie am Halsrand.
  Dadurch kann derselbe Ton nach einem großen Sprung auf einer anderen Taste
  landen. `unify_repeats` kopiert aus Ähnlichkeit des Gesamtmixes ganze Abschnitte;
  eine Gitarrenvariation kann trotz ähnlicher Begleitung real sein.

Die Konsequenz ist plausibel: Der Spieler folgt einem Arrangement aus
Ereignissen mehrerer Instrumente. Eine weitere Anpassung von Notendichte oder
Beat-Synchronität allein löst die fehlende Instrumentenzuordnung nicht.

## Was die Recherche trägt

1. **Musikalische Übereinstimmung braucht menschliche Prüfung.** Harmonix
   beschreibt getrennte Aufgaben pro Instrument, Prüfungen aller Schwierigkeiten
   und anschließende Spieltests zur Verbindung von Chart und Audio. Automatische
   Prüfungen ergänzen diese Beurteilung. Daraus folgt für uns: Autopilot bestätigt
   Ausführbarkeit, nicht musikalische Richtigkeit.
   [Harmonix / Rhythm Authors](https://www.harmonixmusic.com/blog/a-fireside-chat-with-rhythm-authors).
2. **Die Instrumentalquelle kommt vor der Transkription.** Spotify Basic Pitch
   liefert polyphone Tonereignisse und Pitch-Bends, arbeitet laut eigener
   Dokumentation aber am besten mit einem Instrument gleichzeitig. Es ist selbst
   kein Gitarrenerkenner. Ein sinnvoller Ausbau wäre daher erst Trennung, dann
   Onsets, Offsets und Tonhöhen aus der ausgewählten Quelle.
   [Basic Pitch](https://github.com/spotify/basic-pitch).
3. **Ein Stem ist nicht automatisch eine reine Gitarre.** Demucs bietet im
   Vier-Spur-Modell vocals/drums/bass/other. Das Sechs-Spur-Modell ergänzt guitar
   und piano; die Autoren nennen Einschränkungen bei piano. Die Quelle muss am
   konkreten Song geprüft werden. Hier ist bereits `htdemucs` installiert und
   gecacht; der Pilot verwendet dessen `other`, ohne neue Modelle zu laden.
   [Demucs](https://github.com/facebookresearch/demucs).
4. **Auch größere Modelle sind keine sichere Komplettlösung.** YourMT3+
   untersucht gemeinsame Instrumentenerkennung und Transkription; die Autoren
   berichten weiterhin Grenzen bei Pop-Aufnahmen. Ein Modellname ersetzt keinen
   Vergleich mit hörbaren Ereignissen.
   [YourMT3+](https://arxiv.org/abs/2407.04822).
5. **Der hörbare Einfluss gehört zum Instrumentengefühl.** Das MIT-Lehrmaterial
   nutzt getrennte Gitarren- und Begleitspuren und koppelt die Gitarrenlautstärke
   an Treffer/Fehler. Für BeatByte wäre eine behutsame Rückmeldung über einen
   geprüften Gitarrenstem ein eigener Folgeschritt. Das verändert die Wiedergabe
   und wird nicht mit diesem Chart-Versuch vermischt.
   [MIT: Interactive Music Systems](https://ocw.mit.edu/courses/21m-385-interactive-music-systems-fall-2016/ad8c22178b0c4d54ee46bfcd85d98fb1_MIT21M_385F16_pset7.pdf).

## Der umgesetzte Versuch

`generate::generate_lead_study` ist ein ausdrücklich optionaler Generator.
Import und G-Redesign benutzen weiterhin den bisherigen Standard.

- Nur Tonereignisse aus einer fest gewählten Quelle dürfen Kandidaten werden;
  unzugeordnete Anschläge füllen keine tonalen Pausen.
- Ein hinreichend starker Anschlag innerhalb eines gehaltenen Tons teilt ihn in
  zwei Anschläge derselben Tonhöhe. Dies ist eine Heuristik auf dem Stem, keine
  sichere Erkennung der Anschlagstechnik.
- Tonhöhen werden innerhalb einer Phrase sortiert und auf fünf Tasten abgebildet.
  Derselbe gerundete MIDI-Ton erhält dieselbe Taste. Bei mehr als fünf Tonhöhen
  teilen sich Töne Tasten; nach langen Pausen darf die Zuordnung neu beginnen.
- Schnelle Wiederholungen bleiben auf derselben Taste und werden nicht künstlich
  zu Trillern/HOPOs. Die bestehende Schwierigkeitsreduktion begrenzt ihre Dichte.
  Sprünge über mehr als zwei Tasten innerhalb von 180 ms werden im Master
  ausgedünnt. So wird ein zu schneller großer Griffwechsel entfernt, ohne die
  Tonhöhe auf eine andere Taste umzudeuten. Der erste Van-Halen-Versuch hatte
  einen Grün-Orange-Sprung nach 109 ms; im endgültigen Master fehlt dieser Hit.
- Ohne polyphone Belege werden keine Akkorde aus Lautstärke erfunden. Deshalb
  ist dieser erste Versuch bewusst ein **einstimmiger Part**, keine vollständige
  Nachbildung von Rhythmusgitarre. Die bestehende HOPO-Abstandsregel bleibt für
  echte Tastenwechsel eine Spielabstraktion; Legato wird noch nicht erkannt.
- Der Pilot kopiert keine Wiederholungen aus Gesamtmix-Ähnlichkeit. Raster und
  Tempo bleiben für alle vier Varianten die des bisherigen Charts.

Es wurden keine neuen Rust- oder Python-Abhängigkeiten installiert. Das vorhandene
Demucs-Modell benötigte für 227,819 Sekunden Audio ungefähr 123 Sekunden auf CPU;
das ist ein gemessener Einzelwert, kein zugesagtes Import-Zeitbudget.

## Einzeltrack und reproduzierbare Durchführung

„Dreams“ wurde weder in `songs/` noch im benutzerspezifischen Song-Verzeichnis
gefunden. Verwendet wird die vorhandene Aufnahme von Van Halen,
**Ain't Talkin' 'Bout Love**. Dieser Name wählt den Testfall; es gibt keine
Songnamen-Sonderbehandlung im Generator.

Zuerst die Aufnahme mit `beatbyte-cli decode` exportieren. Der Decoder entfernt
Encoder-Priming; genau dieser Export ist die Eingabe des Separators:

```sh
target/debug/beatbyte-cli decode '<original.m4a>' --out local/guitar-study/mix.wav
demucs -n htdemucs --two-stems other --shifts 0 -d cpu -j 1 \
  -o local/guitar-study/stems local/guitar-study/mix.wav
cargo run -p beatbyte-cli --example guitar_study -- \
  songs/imported/van-halen---ain-t-talkin---bout-love-m4a/chart.json \
  local/guitar-study/stems/htdemucs/mix/other.wav \
  local/guitar-study/comparison-final
```

Das Beispiel verlangt ein neues Zielverzeichnis, validiert alle vier Charts und
lehnt mehr als 25 ms Längendifferenz ab. Gleiche Länge beweist keine korrekte
Ausrichtung; die Ableitung aus demselben Decoder-Export bleibt Voraussetzung.
Das Beispiel lädt keine Modelle, aktiviert keine Version und hat keinen `--all`-Pfad.

Lokal enthält `local/guitar-study/comparison-final/` beide Analysen, vier Charts, den
vorherigen Chart als Archiv, die dekodierte Aufnahme und `report.json`.
`original.json` ist ein Archiv mit ursprünglicher Audio-Referenz, kein eigenständig
spielbares Paket. Der Pilot ist separat unter
`songs/imported/guitar-study-van-halen/` als **[Guitar Study] Ain't Talkin' 'Bout Love**
installiert. Der bisherige Song bleibt als direkter Vergleich erhalten.
Der installierte Pilot spielt eine bytegleiche Kopie der ursprünglichen
Stereo-M4A mit deren Lautstärke-Sidecar und Encoder-Priming-Angabe ab. Der Stem
dient nur zur Analyse; die Wiedergabe wird für den A/B-Vergleich nicht verändert.
Audio, abgeleitete Charts und Vergleichsdateien bleiben lokale Nutzerinhalte.

## Gemessener Vergleich

Gleiche Aufnahme und gleiches akzeptiertes Raster; Zahlen sind die Zahl der
Chart-Noten inklusive etwaiger Akkordbestandteile, keine Qualitätsnote.

| Quelle / Zuordnung | Easy | Medium | Hard | Expert | Expert-HOPOs | Expert-Sustains |
|---|---:|---:|---:|---:|---:|---:|
| Gesamtmix / bisheriger Generator | 206 | 395 | 726 | 903 | 487 | 119 |
| Gesamtmix / Versuch | 204 | 387 | 635 | 687 | 171 | 87 |
| other / bisheriger Generator | 203 | 373 | 543 | 662 | 280 | 155 |
| other / Versuch | 206 | 330 | 474 | 512 | 117 | 176 |

Der unveränderte ursprüngliche Chart hat 206/396/726/908 Noten. Er ist nicht
bitgleich mit der neu erzeugten Kontrollvariante: Die kontrollierte Untersuchung
verwendet das gespeicherte Raster und schaltet die Wiederholungsübertragung ab.
Die Expert-Versuchsvariante erhält 19 schnelle Anschlagspaare derselben Taste.
Diese Zahlen zeigen, dass beide Stellgrößen wirken. Sie zeigen weder die
Treffsicherheit der Tonhöhenerkennung noch einen Sieg im Hörvergleich.

Offener Befund: Auf Easy entsteht im Pilot eine rund 13,7 Sekunden lange Pause,
auf Medium und Hard beträgt die längste Pause ungefähr vier Sekunden. Ob die
Gitarre dort wirklich pausiert oder die Erkennung/Reduktion zu viel verwirft,
muss im Hörvergleich geprüft werden. Auch Griffwechsel nach längeren Abständen
können weiterhin groß ausfallen. Easy ist damit noch nicht zur Übernahme empfohlen.

## Abnahme vor jeder Übernahme

Technisch geprüft am 2026-09-12:

- Fünf neue Generator-Tests bestehen; gezieltes Wiedereinsetzen von
  `break_jacks` beziehungsweise Abschalten der Griffwechselbegrenzung ließ
  die zugehörigen Regressionstests scheitern. Beide Mutationen wurden entfernt.
- `cargo fmt --all -- --check`, Workspace-Clippy mit allen Targets/Features
  und `-D warnings`, Workspace-Tests und `cargo check --workspace` bestanden.
  Rustdoc bestand mit und ohne alle Features, jeweils mit `-D warnings`.
  Vorhandene ignorierte Tests werden dadurch nicht als ausgeführt gewertet.
- Release-Build 0.15.6 erstellt. Der installierte Expert-Pilot durchlief das
  tatsächliche Spiel bis zum Ergebnis: **512 Perfect, 0 Misses, 0 Overstrums**,
  Autopilot `PASSED`, Prozess-Exit 0. Log lokal:
  `local/guitar-study/autopilot-expert.log`.
- Alle vier erzeugten Varianten wurden auf allen Schwierigkeiten validiert.
  Ein erneuter Aufruf mit bestehendem Zielordner wurde vor der Analyse mit
  Exit 1 abgelehnt. SHA-256-Vergleich: alle 596 zuvor vorhandenen JSON-Dateien
  der Bibliothek unverändert; Pilot- und Originalaufnahme bytegleich.

Diese Prüfungen belegen Ausführbarkeit und Schutz der bisherigen Bibliothek.
Es wurde damit weder die Tonerkennung gegen eine Referenztranskription gemessen
noch ein menschlicher Hör- und Spielvergleich durchgeführt.

Zuerst Original und Pilot jeweils auf Medium und Hard spielen: Anfangsriff,
Passage mit Gesang, längere gehaltene Töne und spätere Wiederkehr des Riffs.
Prüffragen: Folgen die Finger derselben hörbaren Stimme? Bleiben Tonwiederholungen
wiedererkennbar? Fehlen hörbare Anschläge? Sind Pausen tatsächlich Gitarrenpausen?
Wirken Haltenoten so lang wie die gespielte Stimme? Macht die geringere Zahl
künstlicher Tastenwechsel das Strumming glaubhafter?

Eine automatische fehlerfreie Partie prüft nur Spiellogik und Chart-Ausführbarkeit.
Die musikalische Bewertung steht bis zu diesem A/B-Test offen. Vor einem Ausbau
braucht es einen geprüften guitar-Stem, polyphone Ereignisse für echte Akkorde,
Phrasen-erhaltende Reduktion und mindestens einen zweiten stilistisch anderen
Testtrack. Erst danach kommt eine bewusste Übernahme in Import und Neuanalyse;
eine Bibliotheksmigration bleibt eine eigene Entscheidung.
