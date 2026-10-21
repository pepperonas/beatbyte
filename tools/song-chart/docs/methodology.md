# Messmethodik und Grenzen

## Was ein guter Chart leisten muss

1. Ein Ereignis muss zu einem hörbaren Instrumentenanschlag passen.
2. Die gewählte Stimme muss musikalisch zusammenhängend bleiben.
3. Wiederholte Riffs sollen wiedererkennbare Handbewegungen ergeben.
4. Schwierigkeit muss die Belastung verändern, ohne Rhythmus und Motive beliebig umzuschreiben.
5. Eine spielbare Datei allein beweist keine musikalische Qualität.

Eine menschliche Chartdatei ist eine künstlerische Auswahl, kein vollständiges Audiotranskript. Ein automatischer Generator kann musikalisch plausible Alternativen liefern, die von diesem einen Referenzchart abweichen. Umgekehrt kann eine hohe zeitliche Trefferquote mit den falschen Instrumenten erreicht werden.

## Korpus

48 Songgruppen: Rock, Metal, Pop, Elektronik mit je 8 Entwicklung / 4 Holdout. Genres werden aus vorhandenen Tags klassifiziert, Metal vor Elektronik vor Rock vor Pop. Gemischte / fehlerhafte Tags können die Gruppierung beeinflussen. Die Auswahl ist nach Songgruppen-Hash deterministisch, vorhandene Originalquellen erhalten Vorrang. Keine Auswahl nach späterem Testergebnis.

Gruppierung kombiniert normalisierte Künstler-/Songnamen und identische im Metadatensatz gespeicherte Audio-Hashes. Der Hash ist ein bereits vorhandenes FNV-Kennzeichen, kein neu berechneter Audiofingerprint. Für die tatsächliche Verarbeitung werden zusätzlich SHA-256-Hashes berechnet. Live-/Remaster-Klammern werden konservativ zusammengefasst. Umbenannte und neu codierte Duplikate ohne gleiche Titel/Hashes können unentdeckt bleiben.

Original-Level-Herkunft wird nur aus verfügbaren `.chart`-/MIDI-Tracklisten bestätigt. Die meisten ursprünglichen Bridge-Pfade zeigen auf ein nicht verfügbares Laufwerk. Automatisch ergänzte untere Levels werden nicht über ihre Zahlen erraten. Bei unbekannter Herkunft bleiben Expert-Vergleiche ein Vergleich gegen den importierten Chart, untere Stufen sind unverifizierte Importvergleiche.

`tools/report_inventory.py` kann Reduktionsverhältnisse ausschließlich aus verifizierten ursprünglichen Entwicklungs-Levelpaaren ableiten. Weniger als fünf Paare je Übergang reichen nicht: Dann bleiben die dokumentierten Design-Vorgaben erhalten. So wird eine Classic-Ladder nicht als menschliches Lernsignal ausgegeben.

## Auswahl des Trennmodells

Zwei Entwicklungsaufnahmen je Genre, jeweils 60 Sekunden ab Sekunde 30 (bei kurzen Dateien früher), werden mit `htdemucs/other` und `htdemucs_6s/guitar` verarbeitet. Gleiche Audios, gleicher Chartgenerator, gleiche Konfiguration. Keine Holdout-Audiodaten in dieser Auswahl.

Verglichen wird Expert-Ereignis-F1 bei ±50 ms. Das experimentelle 6s-Modell ersetzt den Standard nur bei wenigstens vier beidseitig erfolgreichen Paaren und mehr als 0,02 absolutem Makro-F1-Vorsprung. Diese konservative Entscheidungsregel wird vor der Messung festgelegt. Sie ist ein Timing-Proxy, keine Bewertung der klanglichen Isolation. `config/frozen.json` hält das Ergebnis für den vollständigen Durchlauf fest.

## Zeitliche Bewertung

Einzelne JSON-Noten werden wie im Spiel innerhalb von 5 ms ab dem ersten Ereignis zu einem Akkord gruppiert. Gezählt werden Ereignisse, nicht gedrückte Tasten. Vorhersagen und Referenzereignisse werden zeitlich sortiert und maximal monoton eins-zu-eins innerhalb ±25, ±50 und ±100 ms zugeordnet.

Precision = korrekte Vorhersagen / alle Vorhersagen. Recall = getroffene Referenzen / alle Referenzereignisse. F1 = harmonisches Mittel. Mehrere generierte Ereignisse können nicht denselben Referenzanschlag mehrfach treffen. Die Ergebnisübersicht mittelt pro Song; lange Songs dominieren das Ergebnis nicht.

Ältere Charts ohne `audio_trim` werden für den Vergleich einmal im Speicher um den kanonischen Vorlauf verschoben. Es wird kein globaler Offset auf die Referenz optimiert: Das würde Timingfehler schönrechnen. Mix-Anschlag-Nähe wird nur als diagnostische Unterstützung betrachtet, nicht als Beweis des richtigen Instruments.

## Zusätzliche Metriken

Ereignisse/s über Gesamtdauer und aktive Spieldauer, Median/p95 der rollenden Einsekundendichte (Schritt 250 ms), Akkord-/Sustain-/HOPO-Anteile, mittlere Spurbewegung und schnelle große Sprünge. Die Phrase-Ausgabe dokumentiert exakt wiederverwendete Muster. Unterschiede in Dichte und Akkordanteil gehören neben Precision und Recall in jede Beurteilung.

Grundsätzlich ist die Expert-Referenz nicht das absolute Optimum. Exakte Taste-zu-Tonhöhe-Accuracy ist hier kein valides Ziel: Menschen nutzen dieselben fünf Spuren für unterschiedlich viele Tonhöhen, Handlagen und Akkordformen. Eine belastbare Griffbewertung braucht zusätzlich Hör- und Spieltests.

## Baselines

- `baseline_mix`: vorhandenes `generate_chart` auf der kanonischen Mixanalyse.
- `baseline_study`: vorhandenes `generate_lead_study` auf derselben Instrumentenspur wie der neue Generator, mit dem Mixraster.
- `full-dsp`: neuer regelbasierter Generator mit eingebauter Melodieanalyse auf dem Gesamtmix; bewusst keine behauptete Gitarrenisolierung.
- `full-ml`: neuer Generator mit lokaler Trennung, Beat This! und Basic Pitch.

Die Mix-Baseline verwendet absichtlich den bisherigen Standardrasterpfad. Die Lead-Study-Baseline erhält den im jeweiligen neuen Lauf verwendeten Mixraster; dies ist ein stärker kontrollierter Vergleich der Notenlogik, jedoch kein identischer Nachbau eines früheren Benutzerimports.

## Akzeptanz

Technisch erforderlich: gültige Chartdateien, reproduzierbare Regelentscheidungen bei gleichen Analysewerten, keine doppelten Spurnoten, keine erfundenen Noten bei Stille, konsistente Schwierigkeitsreduktion, kanonische Zeitachse und bestandene gezielte Tests. Fehler pro Song werden ausgewiesen und verschwinden nicht aus dem Nenner.

Musikalische Überlegenheit wird nur behauptet, wenn der gemessene Vergleich sie stützt. Ein schlechteres Ergebnis wird als solches dokumentiert. Subjektive Abnahme erfolgt über die erzeugten Hörbeispiele und einen tatsächlichen Spieltest. Dieser kann durch den automatisierten Durchlauf nicht ersetzt werden und wird als noch nicht durchgeführt markiert.

## Bekannte Grenzen von v1

- Eine ausgewählte harmonische Spur pro Lauf; kein zuverlässiger automatischer Wechsel zwischen getrennten Lead-, Rhythmusgitarren und Soloinstrumenten.
- `other` enthält auch Tasteninstrumente und Trennartefakte; 6s-Gitarre ist experimentell. Schwache Tonhöhenbelege können zu Ablehnungen oder zu lückenhaften Charts führen.
- Basic Pitch kann schnelle oder verzerrte Gitarrenläufe und Powerchords falsch lesen. Reartikulationsheuristiken können Leakage-Anschläge ergänzen.
- Phrasen sind vorläufige musikalische Fenster / Pausen, keine vollständig semantische Intro-/Strophen-/Refrain-Erkennung.
- Hype-Phrasen werden in v1 leer gelassen; dafür wird keine unbelegte musikalische Regel erfunden.
- Kein selbst trainiertes Chartmodell. Die verfügbaren Referenzen werden für Analyse und vorsichtige Parameterkalibrierung genutzt, nicht ungeprüft als Trainingslabels.

## Ressourcenprofil des abschließenden Laufs

Der endgültige Modellvergleich und KI-Benchmark verwenden 5-Sekunden-Demucs-Segmente, 0,1 Überlappung, keine zufälligen Shifts und einen fixierten Seed. Die Korpusshards laufen aus Speichergründen nacheinander. Die älteren Pilotläufe / erste Modellwahl mit dem Standardsegment sind getrennte Explorationsartefakte; maßgeblich ist die abschließende `model-selection-summary.json` mit dem eingefrorenen 5-Sekunden-Profil.
