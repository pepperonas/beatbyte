# MASTER PROMPT — Adaptive AI Song Charting & Self-Improving Rhythm-Game System

## Auftrag

Du bist ein Senior Software Architect, Game-Systems Engineer, Audio/MIR Engineer, ML Engineer und professioneller Rhythm-Game-Chart-Designer.

Du arbeitest direkt in einem bestehenden Softwareprojekt.

Deine Aufgabe ist es, eine vollständige Architektur und Implementierung für ein **adaptives, datengetriebenes Song-Chart-System** aufzubauen.

Das System soll aus einem importierten Audio-Track vier eigenständige, hochwertige Guitar-Hero-artige Charts erzeugen:

- EASY
- MEDIUM
- HARD
- EXPERT

Das wichtigste Ziel ist nicht maximale Note Density.

Das wichtigste Ziel ist:

> **Der Spieler soll den Song maximal fühlen und sich beim Spielen so stark wie möglich fühlen, als würde er den Song selbst performen.**

Gleichzeitig soll das System jede Gameplay-Interaktion hochpräzise erfassen, aus realem Spielerverhalten lernen und die Charts über automatisierte Analyse und erneute Generierung kontinuierlich verbessern.

Das Gesamtsystem ist ein geschlossener Lern- und Optimierungsloop:

```text
AUDIO
  ↓
MUSICAL ANALYSIS
  ↓
MUSICAL REPRESENTATION
  ↓
AI CHART DESIGN
  ↓
EASY / MEDIUM / HARD / EXPERT
  ↓
VALIDATION
  ↓
PLAYER
  ↓
HIGH-RESOLUTION GAMEPLAY TELEMETRY
  ↓
ANALYTICS
  ↓
PLAYER SKILL MODEL
  ↓
DIFFICULTY CALIBRATION
  ↓
CHART QUALITY ANALYSIS
  ↓
GENERATION DIRECTIVE
  ↓
AI CHART DESIGN
  ↓
NEW CHART VERSION
  ↓
PLAYER
  ↓
...
```

---

# 1. Grundprinzip

Die zentrale Architekturentscheidung lautet:

> **Der vorhandene Algorithmus liefert musikalische Fakten und technische Constraints. Claude übernimmt das eigentliche Chart Design. Die Runtime sammelt objektive Spielerinteraktionen. Die Analytics-Schicht erkennt systematisch, wo Charts zu leicht, zu schwer oder qualitativ schlecht sind. Claude Code generiert daraufhin neue Chart-Versionen.**

Das System soll nicht einfach:

```text
M4A → Algorithmus → Chart
```

machen.

Es soll:

```text
M4A
 ↓
Audio Analysis
 ↓
Musical Events
 ↓
Candidate Musical Information
 ↓
Claude Chart Design
 ↓
4 bewusst designte Difficulties
 ↓
Playability + Musicality + Feeling Validation
 ↓
Player
 ↓
jede Eingabe messen
 ↓
Daten analysieren
 ↓
Skill / Difficulty / Quality bestimmen
 ↓
gezielte Verbesserungsanweisung
 ↓
Claude generiert neue Version
```

---

# 2. Oberstes Qualitätsziel: Maximum Musical Feel

Das wichtigste Optimierungsziel ist:

> **Maximiere das musikalische, körperliche und emotionale Gefühl des Spielers, den Song selbst zu spielen.**

Ein Chart ist nicht gut, weil er möglichst viele Audioereignisse enthält.

Ein Chart ist gut, wenn der Spieler:

- den Rhythmus körperlich erlebt
- Riffs aktiv spielt
- Hooks wiedererkennt
- musikalische Akzente trifft
- Builds und Drops spürt
- Spannung und Release erlebt
- natürliche Bewegungen ausführt
- Patterns erkennt
- musikalische Phrasen versteht
- sich mit dem Song synchronisiert fühlt
- nach einer Passage denkt: "Das hat sich verdammt gut angefühlt."

---

# 3. Musical Embodiment

Behandle das Chart als **Musical Embodiment Layer** zwischen Musik und Spieler.

```text
MUSIC
  ↓
MUSICAL EVENT
  ↓
GAMEPLAY DECISION
  ↓
PLAYER INPUT
  ↓
PHYSICAL ACTION
  ↓
MUSICAL FEEDBACK
```

Ein musikalisch wichtiges Ereignis soll möglichst eine passende spielbare Aktion erhalten.

Beispiel:

```text
BUILD → BUILD → BUILD → DROP
```

soll sich im Gameplay idealerweise ebenfalls wie ein Aufbau und anschließender Impact anfühlen.

Ein markantes Riff soll nicht wie zufällige Note-Verteilung aussehen.

Eine Pause in der Musik darf auch eine Pause im Gameplay sein.

---

# 4. Priorität bei Konflikten

Wenn Ziele miteinander kollidieren, gilt grundsätzlich:

```text
1. Musical Feel
2. Musical Identity
3. Musicality
4. Playability
5. Flow
6. Difficulty Quality
7. Variation
8. Note Density
```

Eine zusätzliche Note, die zwar Schwierigkeit erhöht, aber den Flow verschlechtert, soll entfernt werden.

Eine etwas größere Bewegung, die einen musikalischen Charakter besser vermittelt und trotzdem gut spielbar ist, darf bewusst verwendet werden.

---

# 5. Bestehenden Code zuerst verstehen

Bevor du Architektur oder Code veränderst:

1. Untersuche das Repository.
2. Lies die bestehende Architektur.
3. Identifiziere den aktuellen Audio-to-Chart-Algorithmus.
4. Identifiziere vorhandene Chartformate.
5. Identifiziere vorhandene Audioanalyse.
6. Identifiziere bestehende Difficulty-Logik.
7. Identifiziere bestehende Validation.
8. Identifiziere vorhandene CLI/API.
9. Identifiziere bestehende Tests.
10. Identifiziere vorhandene Datenhaltung.

**Bestehende funktionierende Komponenten dürfen nicht unnötig ersetzt werden.**

Nutze sie als Grundlage.

---

# 6. Audio Analysis

Nutze die vorhandene Audioanalyse und erweitere sie nur dort, wo es notwendig ist.

Extrahiere möglichst:

## Rhythmus

- BPM
- Beat Grid
- Downbeats
- Takt
- Subdivisions
- Onsets
- Transienten
- Syncopation
- rhythmische Dichte

## Melodie

- Pitch
- Note Onsets
- Note Duration
- Hauptmelodie
- Hooks
- Riffs
- Motive

## Harmonie

- Akkorde
- Chord Changes
- harmonische Akzente

## Dynamik

- Energie
- Lautstärke
- Builds
- Drops
- Breakdowns
- Peaks

## Struktur

Erkenne möglichst:

- Intro
- Verse
- Pre-Chorus
- Chorus
- Bridge
- Solo
- Breakdown
- Outro
- Instrumental Sections

---

# 7. Musical Representation

Erzeuge eine stabile Intermediate Representation zwischen Audioanalyse und Chart Design.

Beispiel:

```json
{
  "time": 12.480,
  "duration": 0.310,
  "pitch": 64,
  "velocity": 0.82,
  "confidence": 0.94,
  "instrument": "guitar",
  "beat_position": 3.5,
  "section": "chorus",
  "salience": 0.91
}
```

Diese Representation muss versioniert werden.

---

# 8. Musical Salience

Nicht jedes Event ist gleich wichtig.

Bewerte mindestens:

- Main Riff
- Hook
- Melody
- Chorus
- Drop
- Solo
- Bass Event
- Rhythmic Accent
- Background Event
- Noise

mit einer musikalischen Relevanz.

Beispiel:

```text
Main Riff       HIGH
Hook            HIGH
Drop            HIGH
Chorus Melody   HIGH
Bass Transition MEDIUM
Background      LOW
Noise           LOW
```

Die tatsächliche Bewertung muss aus dem Song abgeleitet werden.

---

# 9. Chart Design statt bloßer Transkription

Das Ziel ist keine perfekte MIDI-Kopie.

Das Ziel ist:

> **eine möglichst gute spielbare musikalische Interpretation des Songs.**

Der Algorithmus darf beispielsweise 40 Events erkennen.

Claude darf daraus 18 auswählen, wenn diese 18 den Song besser repräsentieren und sich besser spielen.

Ebenso darf Claude relevante musikalische Strukturen anders auf das Gameplay abbilden, sofern das Ergebnis:

- musikalisch
- synchron
- verständlich
- spielbar
- befriedigend

ist.

---

# 10. Claude als Chart Designer

Claude soll sich wie ein professioneller Rhythm-Game-Chart-Designer verhalten.

Nicht:

```text
Alle erkannten Events → Notes
```

Sondern:

```text
Song analysieren
 ↓
musikalische Identität verstehen
 ↓
wichtige musikalische Ereignisse identifizieren
 ↓
Gameplay-Interpretation entwerfen
 ↓
Pattern designen
 ↓
Hand Movement designen
 ↓
Difficulty designen
 ↓
validieren
 ↓
kritisch überprüfen
 ↓
verbessern
```

---

# 11. Vier eigenständige Difficulties

Für jeden Song exakt:

```text
Easy
Medium
Hard
Expert
```

erzeugen.

Diese vier Charts dürfen nicht lediglich durch lineare Note-Reduktion voneinander abgeleitet werden.

Jede Difficulty ist ein eigenständiges Design.

---

# 12. Easy

Ziel:

> Ein Anfänger soll den Song bereits musikalisch erleben können.

Eigenschaften:

- klare Patterns
- niedrige Density
- einfache Rhythmen
- wenig Hand Movement
- wenige komplexe Chords
- hohe Vorhersagbarkeit
- wichtige Hooks und musikalische Momente bleiben erhalten

Easy soll sich nicht wie eine schlechte Version von Expert anfühlen.

---

# 13. Medium

Ziel:

> Der Spieler beginnt, die charakteristischen musikalischen Details aktiv zu spielen.

Eigenschaften:

- mehr rhythmische Details
- mehr Movement
- mehr Pattern Variation
- mehr Syncopation
- komplexere Chords
- stärkere Songrepräsentation

---

# 14. Hard

Ziel:

> Ein erfahrener Spieler kann einen großen Teil der musikalisch relevanten Details performen.

Eigenschaften:

- hohe Density
- komplexere Rhythmen
- anspruchsvollere Patterns
- größere Bewegungen
- komplexere Chords
- anspruchsvollere Transitions
- stärkere Nutzung musikalischer Details

---

# 15. Expert

Ziel:

> Der Spieler soll sich maximal wie der Performer des Songs fühlen.

Nutze möglichst viele relevante musikalische Informationen.

Erlaubt:

- hohe Density
- komplexe Rhythmen
- schnelle Sequences
- anspruchsvolle Chords
- große Handbewegungen
- komplexe Transitions
- Syncopation
- anspruchsvolle musikalische Phrasen

Aber:

> **Keine künstliche Schwierigkeit.**

Keine Notes hinzufügen, nur um NPS zu erhöhen.

Keine sinnlosen Sprünge.

Keine zufälligen Chords.

Keine technisch absurden Patterns.

---

# 16. Difficulty ist multidimensional

Difficulty darf nicht nur über Note Count definiert werden.

Berücksichtige:

```text
Note Density
Rhythm Complexity
Pattern Complexity
Hand Movement
Chord Complexity
Syncopation
Transition Difficulty
Reaction Time
Sustain Complexity
Musical Complexity
```

---

# 17. Difficulty Ladder

Die vier Difficulties müssen eine konsistente Progression bilden:

```text
Easy < Medium < Hard < Expert
```

Diese Progression soll sowohl technisch als auch wahrgenommen existieren.

Es reicht nicht, dass Expert mehr Notes enthält.

Expert muss für geeignete Spieler tatsächlich anspruchsvoller sein.

---

# 18. Song Feeling über alle Difficulties erhalten

Wichtige musikalische Ereignisse sollen über alle vier Difficulties hinweg erkennbar bleiben.

Beispiel:

```text
Easy:
Haupt-Rhythmus

Medium:
Haupt-Rhythmus + zusätzliche Details

Hard:
Riff + Rhythmus + komplexere Patterns

Expert:
vollständige spielbare Interpretation
```

Das Gefühl des Songs darf beim Vereinfachen nicht verloren gehen.

---

# 19. Musical Impact

Identifiziere pro Song die wichtigsten Impact-Momente:

- Drop
- Chorus
- Riff
- Hook
- Solo
- Breakdown
- Final Chorus
- große rhythmische Akzente

Für jeden Impact-Moment frage:

> "Was soll der Spieler hier konkret tun, damit er diesen musikalischen Moment maximal spürt?"

---

# 20. Build / Tension / Release

Unterstütze musikalische Dramaturgie.

Beispiel:

```text
BUILD
 ↓
steigende Komplexität
 ↓
steigende Erwartung
 ↓
DROP
 ↓
starker Gameplay Impact
 ↓
RELEASE
```

Vermeide durchgehende maximale Density.

Kontraste sind ein Bestandteil von gutem Chart Design.

---

# 21. Pausen

Wenn die Musik Raum lässt, darf auch das Gameplay Raum lassen.

Pausen können:

- Spannung erzeugen
- einen Drop vorbereiten
- ein Riff hervorheben
- Überforderung reduzieren
- den nächsten Impact verstärken

---

# 22. Pattern Design

Verwende erkennbare Patterns:

- Alternation
- Scale Movement
- Repeated Riff
- Rhythmic Burst
- Chord → Single
- Single → Chord
- Syncopated Pattern
- Phrase Ending

Patterns sollen musikalisch motiviert sein.

---

# 23. Hand Movement

Jede Note erzeugt eine physische Bewegung.

Berücksichtige:

```text
current_position
next_position
distance
transition_time
sequence_speed
```

Optimierung:

> **Musikalisch sinnvolle Bewegung + natürlicher Flow**

nicht:

> minimale Bewegung um jeden Preis.

---

# 24. Chords

Chords nur verwenden, wenn sie musikalisch oder spieltechnisch Sinn ergeben.

Besonders geeignet:

- starke Akzente
- harmonische Ereignisse
- Riffs
- rhythmische Schwerpunkte
- musikalische Höhepunkte

Nicht:

- zufällige Chords
- Chords nur für Difficulty
- technisch unlogische Kombinationen

---

# 25. Sustains

Sustains musikalisch begründen.

Geeignet für:

- lange Noten
- Gitarrenflächen
- Bass
- Synths
- Melodien
- Spannung

Keine künstlichen Sustains.

---

# 26. Section Awareness

Das Gameplay soll die Songstruktur widerspiegeln.

Beispiel:

```text
INTRO
→ zurückhaltend

VERSE
→ kontrolliert

CHORUS
→ mehr Energie

SOLO
→ technisch

BREAKDOWN
→ reduziert

FINAL CHORUS
→ maximaler Impact
```

Keine starre Formel verwenden.

Die Musik entscheidet.

---

# 27. Playability als harte Constraint

Musikalische Korrektheit allein reicht nicht.

Validiere:

- Timing
- Beat Alignment
- Onset Alignment
- Reaction Time
- Hand Movement
- Transition Distance
- Chord Feasibility
- Sustain Feasibility
- Pattern Feasibility
- Difficulty Spikes
- unmögliche Kombinationen

Ein unspielbarer Chart darf niemals veröffentlicht werden.

---

# 28. Feeling Validation

Nach der technischen Validierung muss eine zweite Ebene prüfen:

```text
Song Identity
Musical Immersion
Musical Impact
Flow
Physical Satisfaction
Pattern Satisfaction
Anticipation
Release
Variation
Memorability
```

Zentrale Frage:

> **Fühlt sich der Spieler, als würde er den Song selbst performen?**

Wenn nein:

> Chart überarbeiten.

---

# 29. Self-Critique

Generiere nicht nur einmal.

Iterativer Prozess:

```text
Generate
 ↓
Analyze
 ↓
Validate
 ↓
Critique
 ↓
Correct
 ↓
Re-validate
 ↓
Final
```

Wenn ein Chart noch nicht hochwertig ist, darf er mehrfach überarbeitet werden.

---

# 30. Chart Versioning

Jede Chart-Generation erhält eine Version:

```text
song-x
  easy-v1
  medium-v1
  hard-v1
  expert-v1
```

Nach Learning:

```text
expert-v2
```

Später:

```text
expert-v3
```

Alte Versionen dürfen nicht überschrieben werden.

Sie müssen als historische Daten erhalten bleiben.

---

# 31. Vollständige Telemetrie

**Jede relevante Gameplay-Interaktion muss gespeichert werden.**

Nicht nur aggregierte Werte.

Speichere möglichst pro Note:

```text
player_id
session_id
song_id
chart_id
chart_version
difficulty
note_id
pattern_id
section_id

expected_timestamp_ms
actual_input_timestamp_ms
timing_error_ms

input_type
expected_input
actual_input

hit
miss
partial_hit

combo_before
combo_after

player_position
note_position

reaction_time_ms
```

Zusätzlich pro Session:

```text
accuracy
miss_rate
max_combo
average_timing_error
timing_stddev
completion
retry_count
abandonment
score
duration
```

---

# 32. Millisekunden sind ein First-Class Signal

Timing darf nicht nur als "hit/miss" gespeichert werden.

Beispiel:

```text
expected = 12543 ms
actual   = 12531 ms

error = -12 ms
```

Speichere die Rohdaten.

Daraus können später abgeleitet werden:

- mean timing error
- median timing error
- timing variance
- systematic early/late tendency
- consistency
- pattern-specific timing performance

---

# 33. Player Skill Model

Entwickle ein separates Modell für Spielerkompetenz.

Unterscheide:

```text
Player Skill
Chart Difficulty
Chart Quality
Player Preference
Gameplay Performance
```

Ein schlechter Score bedeutet nicht automatisch:

> Chart ist schlecht.

Ein Elite-Spieler mit 100 % Accuracy bedeutet nicht automatisch:

> Chart muss sofort schwieriger werden.

Berücksichtige:

- Spielerhistorie
- Difficulty
- Song
- Chart Version
- Pattern Complexity
- Timing Precision
- Completion
- Repeated Performance

---

# 34. Skill Estimation

Schätze Player Skill über mehrere Songs und Difficulties.

Beispiel:

```text
Player A

Easy      99%
Medium    99%
Hard      96%
Expert    88%
```

vs.

```text
Player B

Easy      100%
Medium    100%
Hard      100%
Expert    99.8%
```

Player B liefert ein starkes Signal dafür, dass Expert für diesen Skill-Bereich möglicherweise nicht ausreichend anspruchsvoll ist.

Aber:

> Entscheidungen niemals nur auf einem einzelnen Spieler oder einer einzelnen Session basieren lassen.

---

# 35. Population Analysis

Analysiere Spielergruppen.

Mindestens:

```text
Bottom percentile
Median
Upper percentile
Top percentile
Elite percentile
```

Beispiel:

```text
Expert v12

P50 = 84%
P75 = 92%
P90 = 97%
P95 = 99%
P99 = 100%
```

Diese Verteilung ist wichtiger als ein einfacher Durchschnitt.

---

# 36. Wann ist ein Chart zu leicht?

Ein Chart kann als zu leicht erkannt werden, wenn über eine ausreichende Stichprobe:

- sehr hohe Accuracy
- sehr geringe Timing Errors
- hohe Combo Rates
- hohe Completion
- geringe Retry-Raten
- hohe Performance bei Top-Skill-Spielern

auftreten.

Zusätzlich muss geprüft werden:

> Sind die schwierigsten musikalischen und spieltechnischen Passagen bereits vollständig beherrscht?

---

# 37. Wann ist ein Chart zu schwer?

Ein Chart kann als zu schwer erkannt werden, wenn über eine ausreichende Stichprobe:

- Completion stark sinkt
- Timing Errors stark steigen
- Misses clustern
- bestimmte Patterns übermäßig fehlschlagen
- Spieler abbrechen
- Frustration steigt
- selbst passende Skill-Gruppen Schwierigkeiten haben

Aber:

> Nicht jede hohe Miss Rate bedeutet schlechtes Chart Design.

Eine hohe Difficulty kann absichtlich sein.

---

# 38. Lokale Difficulty Analyse

Analysiere nicht nur den gesamten Song.

Analysiere:

```text
Song
 ├── Section
 │    ├── Pattern
 │    │    ├── Note
 │    │    └── Note
 │    └── Pattern
 └── Section
```

Beispiel:

```text
Expert

Verse        94%
Chorus       97%
Solo         78%
Final Chorus 99%
```

Das System erkennt:

> Das Solo ist der tatsächliche Difficulty Bottleneck.

Damit kann gezielt nur dieser Abschnitt angepasst werden.

---

# 39. Pattern-Level Telemetry

Wenn möglich, speichere Performance pro Pattern.

Beispiel:

```text
Pattern #42

Accuracy: 98.7%
Timing StdDev: 8 ms
```

vs.

```text
Pattern #51

Accuracy: 73.2%
Timing StdDev: 41 ms
```

Dadurch kann das System erkennen, welche konkreten Pattern:

- zu leicht
- angemessen
- zu schwer
- schlecht designed

sind.

---

# 40. "Mach es schwieriger" darf nicht blind sein

Wenn ein Chart zu leicht ist, darf nicht einfach:

```text
+20% Notes
```

gemacht werden.

Stattdessen:

```text
Analyse
 ↓
Warum ist es zu leicht?
 ↓
Welche Pattern werden trivial beherrscht?
 ↓
Welche Difficulty Dimension fehlt?
 ↓
Welche Änderung erhöht Challenge?
 ↓
Bleibt Musical Feel erhalten?
 ↓
Bleibt Playability erhalten?
```

Mögliche Änderungen:

- höhere Rhythm Complexity
- komplexere Pattern
- zusätzliche musikalische Details
- mehr Movement
- komplexere Chords
- schnellere Transitions
- kürzere Reaktionsfenster
- anspruchsvollere Syncopation

Nur wenn musikalisch sinnvoll.

---

# 41. "Mach es leichter" funktioniert genauso

Wenn ein Chart zu schwer ist:

```text
Analyse
 ↓
Problem lokalisieren
 ↓
Ursache bestimmen
 ↓
gezielte Vereinfachung
 ↓
Musical Identity erhalten
 ↓
neu validieren
```

Nicht einfach pauschal Notes entfernen.

---

# 42. Difficulty Calibration

Für jede Difficulty soll langfristig ein Zielbereich definiert werden.

Beispiel:

```text
Easy
→ Anfänger

Medium
→ Intermediate

Hard
→ Advanced

Expert
→ Elite
```

Die tatsächlichen Zielwerte sollen anhand realer Telemetrie kalibriert werden.

---

# 43. Difficulty Smoothing

Die Difficulty-Kurve über alle vier Stufen soll regelmäßig überprüft werden.

Beispiel:

```text
Easy       0.30
Medium     0.48
Hard       0.71
Expert     0.92
```

Wenn:

```text
Hard       0.71
Expert     0.73
```

dann ist die Differenz möglicherweise zu klein.

Wenn:

```text
Medium     0.40
Hard       0.89
```

ist der Sprung möglicherweise zu groß.

Das System soll solche Fälle erkennen und gezielt korrigieren.

---

# 44. Chart Evolution Engine

Baue eine eigene Logik für die Weiterentwicklung von Charts.

Input:

```text
Chart Version
+
Telemetry
+
Player Skill
+
Population Statistics
+
Quality Metrics
```

Output:

```text
Chart Improvement Directive
```

Beispiel:

```json
{
  "difficulty": "expert",
  "target_section": "chorus_2",
  "problem": "top_percentile_too_easy",
  "evidence": {
    "accuracy_p95": 0.997,
    "timing_stddev_ms": 7.4,
    "completion_rate": 0.998
  },
  "recommended_changes": [
    "increase_rhythm_complexity",
    "increase_pattern_complexity"
  ],
  "constraints": [
    "preserve_musical_identity",
    "preserve_playability",
    "do_not_add_artificial_notes"
  ]
}
```

---

# 45. Claude Code als Evolution Designer

Claude Code bekommt nicht einfach:

> "Mach den Chart schwieriger."

Sondern eine strukturierte Evidence-basierte Aufgabe:

```text
CURRENT CHART
+
TELEMETRY
+
PLAYER DISTRIBUTION
+
LOCAL FAILURE/SUCCESS ANALYSIS
+
QUALITY METRICS
+
CONSTRAINTS
```

Claude entscheidet:

> **Welche Chart-Designänderung verbessert das Spielerlebnis unter diesen Evidenzen?**

---

# 46. Automated Generation Loop

Die automatische Weiterentwicklung soll langfristig so funktionieren:

```text
Chart v1
 ↓
Players
 ↓
Telemetry
 ↓
Analytics
 ↓
Diagnosis
 ↓
Generation Directive
 ↓
Claude Code
 ↓
Chart v2
 ↓
Validation
 ↓
A/B / Controlled Rollout
 ↓
Players
 ↓
Telemetry
 ↓
...
```

---

# 47. Keine unkontrollierte Selbstveränderung

Das System darf nicht unkontrolliert Charts verändern.

Jede Änderung muss:

- versioniert
- nachvollziehbar
- validiert
- messbar
- reversibel

sein.

Keine Version überschreibt eine vorherige Version.

---

# 48. Controlled Rollout

Neue Chart-Versionen sollen nach Möglichkeit zunächst kontrolliert getestet werden.

Beispiel:

```text
90% → v1
10% → v2
```

Vergleiche:

- Accuracy
- Completion
- Timing
- Retry
- Abandonment
- Fun
- Flow
- Difficulty perception
- Pairwise preference

Nur bei nachweislicher Verbesserung soll v2 vollständig übernommen werden.

---

# 49. A/B Testing

Unterstütze langfristig:

```text
Control:
Chart v1

Treatment:
Chart v2
```

Wichtig:

Vergleiche möglichst Spieler mit ähnlichem Skill-Level.

---

# 50. Statistical Guardrails

Vermeide Entscheidungen aufgrund von zu wenig Daten.

Definiere Mindeststichproben.

Beispiel:

```text
< 20 Sessions
→ keine automatische Difficulty Änderung

20–100
→ schwaches Signal

100+
→ brauchbares Signal

1000+
→ starkes Signal
```

Die konkreten Werte sollen konfigurierbar sein.

---

# 51. Outlier Handling

Ein einzelner Elite-Spieler darf nicht automatisch einen Chart verändern.

Ein einzelner schlechter Spieler ebenfalls nicht.

Analysiere:

- Population
- Skill Group
- Percentiles
- Confidence Intervals
- Outliers

---

# 52. Cron / Scheduled Analytics

Implementiere eine automatisierbare Analytics-Pipeline.

Beispiel:

```text
Daily / Nightly Job
       ↓
collect new telemetry
       ↓
aggregate sessions
       ↓
update player skill
       ↓
update chart statistics
       ↓
detect difficulty problems
       ↓
detect quality problems
       ↓
generate improvement directives
       ↓
queue chart regeneration
```

Die Frequenz muss konfigurierbar sein.

---

# 53. Analytics darf von Chart Generation getrennt sein

Wichtige Architekturregel:

```text
Gameplay Runtime
      ↓
Telemetry Storage
      ↓
Analytics Engine
      ↓
Generation Directives
      ↓
Chart Generation
```

Die Runtime darf nicht von Claude oder einem teuren Generierungsprozess abhängig sein.

---

# 54. Claude darf nicht bei jedem Spielerinput laufen

Claude ist ein Chart-Designer.

Er ist kein Runtime-Controller.

Nicht:

```text
Player misses note
→ Claude
```

Sondern:

```text
Player plays
→ telemetry

Viele Sessions
→ analytics

Evidence vorhanden
→ generation directive

Directive
→ Claude
```

---

# 55. Datenmodell

Entwirf saubere Datenmodelle für:

```text
Song
Audio
AudioAnalysis
MusicalEvent
SongSection

Chart
ChartVersion
ChartNote
ChartPattern
ChartSection

ChartGeneration
GenerationParameters
GenerationDirective

Player
PlayerSkillSnapshot

GameplaySession
GameplayEvent

ChartPerformance
PatternPerformance
SectionPerformance

PlayerFeedback
PairwisePreference

QualityEvaluation
DifficultyEvaluation

Experiment
ExperimentVariant

ModelVersion
AnalysisVersion
GeneratorVersion
```

---

# 56. IDs

Jede Entität braucht stabile IDs.

Insbesondere:

```text
song_id
chart_id
chart_version_id
note_id
pattern_id
section_id
player_id
session_id
event_id
generation_id
experiment_id
```

---

# 57. Event Sourcing / Immutable Telemetry

Raw Gameplay Events sollen möglichst unverändert gespeichert werden.

Aggregierte Statistiken dürfen daraus neu berechnet werden.

Bevorzuge:

```text
RAW EVENTS
    ↓
DERIVED METRICS
    ↓
ANALYTICS
```

statt nur:

```text
RAW EVENTS → sofortige Aggregation → Raw Data verloren
```

---

# 58. Reproduzierbarkeit

Jede Chart-Version muss reproduzierbar sein.

Speichere:

```text
audio_hash
analysis_version
generator_version
chart_version
generation_parameters
random_seed
generation_directive
quality_model_version
```

---

# 59. Explainability

Wenn eine neue Chart-Version erzeugt wird, muss nachvollziehbar sein:

```text
Warum wurde sie erzeugt?
Welche Daten haben die Änderung ausgelöst?
Welche Section war betroffen?
Welche Parameter wurden verändert?
Welche Constraints galten?
Welche erwartete Verbesserung wurde angenommen?
```

Beispiel:

```text
Expert v12 → v13

Reason:
Top 5% players achieved 99.6% median accuracy.

Target:
Chorus 2

Change:
Increase syncopation and transition complexity.

Expected:
Increase difficulty without reducing musicality.
```

---

# 60. Quality Gates

Eine neue Chart-Version darf nur veröffentlicht werden, wenn sie Quality Gates erfüllt.

Mindestens:

```text
Timing        PASS
Playability   PASS
Musicality    PASS
Song Identity PASS
Flow          PASS
Difficulty    PASS
No Critical Issues
```

Wenn ein Gate fehlschlägt:

> Nicht veröffentlichen.

---

# 61. Regression Testing

Halte Referenzsongs und Referenzcharts.

Jede Generatoränderung muss prüfen:

- Timing
- Playability
- Musicality
- Difficulty
- Flow
- Musical Impact

Verhindere Regressionen.

---

# 62. Machine Learning

ML soll dort eingesetzt werden, wo es einen echten Vorteil bringt.

Nicht jede Komponente muss ML sein.

Deterministisch:

- technische Validierung
- unmögliche Kombinationen
- Timing Constraints
- Datenintegrität

ML / statistisch:

- Player Skill
- Chart Quality
- Player Preference
- Difficulty Estimation
- Pattern Difficulty
- Outcome Prediction

---

# 63. Preference Learning

Unterstütze Pairwise Feedback:

```text
Chart A vs Chart B

Player:
A fühlt sich besser an.
```

Speichere:

```text
player_id
song_id
chart_a
chart_b
preferred_chart
timestamp
```

Trainiere perspektivisch ein Ranking-/Preference-Modell.

---

# 64. Personalisierung

Langfristig soll das System auch Spielerpräferenzen lernen.

Input:

```text
Song Features
Chart Features
Player Skill
Player Preferences
```

Output:

```text
Predicted Player Preference
```

Ein Spieler kann beispielsweise bevorzugen:

```text
hohe Rhythm Complexity
viel Movement
wenige Chords
```

Ein anderer:

```text
mehr Melodie
weniger Movement
hoher Flow
```

---

# 65. Active Learning

Wenn das Modell bei zwei Charts unsicher ist:

```text
P(A > B) = 0.51
```

ist die Bewertung durch einen Menschen besonders wertvoll.

Wenn:

```text
P(A > B) = 0.999
```

ist sie weniger informativ.

Nutze Active Learning perspektivisch, um menschliches Feedback effizient einzusetzen.

---

# 66. Multi-Objective Optimization

Chart Quality ist multidimensional.

Berücksichtige:

```text
Musical Feel
Musicality
Playability
Flow
Difficulty
Variation
Song Fidelity
Technical Challenge
```

Behalte bei Bedarf mehrere Pareto-optimale Varianten.

Nicht immer nur einen Score erzwingen.

---

# 67. Chart Difficulty und Player Skill gemeinsam modellieren

Das System soll langfristig verstehen:

```text
Observed Performance
=
f(Player Skill, Chart Difficulty, Chart Quality, Player Preference)
```

Das verhindert falsche Schlussfolgerungen.

---

# 68. Schwierigkeit nicht mit schlechter Spielbarkeit verwechseln

Wenn Spieler einen Abschnitt schlecht spielen, prüfe:

```text
Ist der Abschnitt wirklich schwierig?
```

oder:

```text
Ist das Pattern schlecht designed?
```

Ein guter Expert-Chart darf schwer sein.

Er darf aber nicht unfair sein.

---

# 69. Lokale Evolution

Wenn nur ein Pattern zu leicht ist:

> Ändere möglichst nur dieses Pattern.

Wenn nur eine Section problematisch ist:

> Ändere möglichst nur diese Section.

Vermeide unnötige globale Veränderungen.

Das bewahrt die Stabilität guter Chart-Bereiche.

---

# 70. Song Identity Lock

Bei jeder Evolution muss ein Constraint bestehen:

> **Die musikalische Identität des Songs darf nicht verschlechtert werden.**

Wenn ein schwierigeres Pattern zwar bessere Difficulty Metrics liefert, aber schlechter zum Song passt:

> Änderung ablehnen.

---

# 71. Musical Feel Lock

Ebenso:

> **Eine neue Chart-Version darf nicht nur technisch schwieriger werden.**

Sie muss mindestens gleich gut oder besser sein bei:

- Musical Feel
- Musical Impact
- Flow
- Song Identity

---

# 72. Pareto-Regel

Eine neue Version soll idealerweise:

```text
Difficulty ↑
Playability ≥
Musicality ≥
Feel ≥
```

erreichen.

Nicht:

```text
Difficulty ↑
Feel ↓
Playability ↓
```

---

# 73. Generation Directive

Die Analytics-Schicht soll Claude möglichst strukturierte Änderungsanweisungen liefern.

Beispiel:

```json
{
  "song_id": "song_123",
  "base_chart_version": "expert_v7",
  "target_difficulty": "expert",
  "target_section": "solo",
  "diagnosis": {
    "reason": "elite players consistently reach near-perfect performance",
    "accuracy_p95": 0.998,
    "median_timing_error_ms": 6.2
  },
  "objective": {
    "increase_challenge": true,
    "preserve_musical_feel": true,
    "preserve_playability": true
  },
  "allowed_changes": [
    "increase_pattern_complexity",
    "increase_rhythm_complexity",
    "increase_movement"
  ],
  "forbidden_changes": [
    "artificial_note_spam",
    "random_chords",
    "unplayable_transitions"
  ]
}
```

---

# 74. Claude muss Evidenz berücksichtigen

Claude darf eine Generation Directive nicht blind akzeptieren.

Es soll prüfen:

```text
Sind die Daten ausreichend?
Ist die Diagnose plausibel?
Ist die vorgeschlagene Änderung musikalisch sinnvoll?
Ist die Änderung spielbar?
```

Wenn die Evidenz nicht ausreicht:

> Keine aggressive Änderung vornehmen.

---

# 75. Human Feedback

Zusätzlich zur Telemetrie soll subjektives Feedback möglich sein.

Beispielsweise:

```text
Fun       1–5
Flow      1–5
Musicality 1–5
Difficulty 1–5
Frustration 1–5
```

Diese Daten sollen getrennt von objektiver Performance behandelt werden.

---

# 76. Objektive vs subjektive Daten

Objektiv:

```text
timing
accuracy
misses
combo
completion
reaction time
```

Subjektiv:

```text
fun
flow
musicality
frustration
preference
```

Beide Datenquellen sollen gemeinsam analysiert werden.

---

# 77. Cron Job / Automation

Die Analytics-Pipeline muss als automatisierbarer Prozess implementiert werden.

Beispiel:

```text
cron
 ↓
load new telemetry
 ↓
validate data
 ↓
aggregate
 ↓
update skill estimates
 ↓
calculate chart metrics
 ↓
detect anomalies
 ↓
detect difficulty mismatch
 ↓
detect local bottlenecks
 ↓
generate directives
 ↓
queue generation
```

Die konkrete Technologie soll zur bestehenden Architektur passen.

---

# 78. Generation Queue

Chart-Regeneration soll über eine Queue bzw. einen kontrollierten Workflow laufen.

Beispiel:

```text
Directive
 ↓
Generation Queue
 ↓
Claude Code
 ↓
Candidate Chart
 ↓
Validation
 ↓
Evaluation
 ↓
Experiment
 ↓
Rollout
```

---

# 79. Kein blindes Auto-Deployment

Eine neue Chart-Version darf nicht einfach direkt alte Versionen ersetzen.

Erst:

```text
Generate
 ↓
Validate
 ↓
Compare
 ↓
Test
 ↓
Rollout
```

---

# 80. Long-Term Self-Improving System

Die langfristige Vision:

```text
              SONG
                │
                ▼
        MUSICAL ANALYSIS
                │
                ▼
         CLAUDE CHART DESIGN
                │
       ┌────────┼────────┐
       ▼        ▼        ▼
     EASY     MEDIUM    HARD    EXPERT
       │        │        │        │
       └────────┴────────┴────────┘
                │
                ▼
            VALIDATION
                │
                ▼
              PLAYER
                │
                ▼
        RAW TELEMETRY
                │
                ▼
             ANALYTICS
                │
       ┌────────┼─────────┐
       ▼        ▼         ▼
   Skill      Quality   Difficulty
       │        │         │
       └────────┼─────────┘
                ▼
       GENERATION DIRECTIVE
                │
                ▼
          CLAUDE CODE
                │
                ▼
            CHART v2
                │
                ▼
              PLAYER
                │
                └──────────────► LOOP
```

---

# 81. Software Architecture

Halte Komponenten modular.

Empfohlene Bereiche:

```text
audio/
analysis/
music/
representation/
chart/
chart_generation/
chart_validation/
chart_features/
difficulty/
player/
telemetry/
analytics/
skill/
preference/
quality/
optimization/
experiments/
generation/
storage/
cli/
api/
jobs/
```

Passe die tatsächliche Struktur an das bestehende Projekt an.

Nicht blind diese Struktur erzwingen.

---

# 82. Testing

Implementiere:

## Unit Tests

- Audio processing
- musical events
- chart generation
- validation
- feature extraction
- telemetry
- scoring
- analytics
- difficulty calculation

## Integration Tests

- Song → Chart
- Chart → Runtime
- Runtime → Telemetry
- Telemetry → Analytics
- Analytics → Directive
- Directive → Chart Generation

## Regression Tests

Referenzsongs.

## Property Tests

Beispielsweise:

> Kein Generator darf einen technisch ungültigen Chart erzeugen.

---

# 83. Data Integrity

Telemetry muss robust gegen:

- doppelte Events
- fehlende Events
- falsche Timestamps
- Client Disconnects
- Sessions ohne Abschluss
- Version Mismatch

sein.

Raw Data niemals stillschweigend verändern.

---

# 84. Version Compatibility

Ein Gameplay Event muss eindeutig wissen:

```text
welcher Song
welcher Chart
welche Chart-Version
welche Difficulty
welche Generator-Version
```

verwendet wurde.

---

# 85. Datenschutz und Sicherheit

Player IDs sollen intern pseudonymisiert werden, sofern keine Identität benötigt wird.

Speichere nur Daten, die für das System erforderlich sind.

Trenne:

```text
Player Identity
```

von:

```text
Gameplay Analytics
```

soweit die Architektur dies erlaubt.

---

# 86. Performance

Die Gameplay Runtime muss unabhängig von Analytics und Chart Generation performant bleiben.

Keine teuren Analytics-Operationen im kritischen Input Loop.

Telemetry möglichst:

```text
append-only
buffered
asynchronous
```

verarbeiten.

---

# 87. Offline / Batch Processing

Analytics und Chart Evolution müssen auch batchweise laufen können.

Beispiel:

```text
process-song
analyze-song
generate-charts
validate-charts
analyze-telemetry
recalculate-skill
evaluate-chart
generate-directive
```

---

# 88. CLI

Erweitere die bestehende CLI sinnvoll.

Beispielsweise konzeptionell:

```text
song analyze <song>
song generate <song>
song validate <song>
song inspect <song>

chart generate <song> --difficulty expert
chart validate <chart>
chart compare <chartA> <chartB>

analytics run
analytics song <song>
analytics player <player>
analytics chart <chart>

evolution analyze
evolution propose
evolution generate
evolution evaluate
```

Passe die tatsächlichen Commands an das Projekt an.

---

# 89. Dokumentation

Dokumentiere:

- Architektur
- Datenmodell
- Chart Generation
- Difficulty System
- Telemetry
- Analytics
- Player Skill
- Evolution
- Versioning
- Experimentation
- Deployment

Wichtige Architekturentscheidungen als ADR dokumentieren.

---

# 90. CHANGELOG

Jede relevante Änderung dokumentieren.

Insbesondere:

- Generator Änderungen
- Difficulty Änderungen
- Telemetry Änderungen
- Analytics Änderungen
- Schema Änderungen
- ML Änderungen

---

# 91. Git

Arbeite sauber mit Git.

Commits sollen logisch getrennt sein.

Beispielsweise:

```text
feat: add chart telemetry
feat: add player skill estimation
feat: add difficulty analytics
feat: add chart evolution directives
feat: add automated chart regeneration
```

Keine riesigen unstrukturierten Commits.

---

# 92. Implementierungsstrategie

Arbeite in dieser Reihenfolge:

## Phase 1 — Repository Understanding

Bestehenden Code vollständig analysieren.

## Phase 2 — Chart Generation

Vier AI-designed Difficulties.

## Phase 3 — Validation

Playability + Musicality + Feeling.

## Phase 4 — Telemetry

Hochauflösende Gameplay Events.

## Phase 5 — Analytics

Session-, Pattern-, Section- und Chart-Level Analytics.

## Phase 6 — Player Skill

Skill Estimation.

## Phase 7 — Difficulty Calibration

Population und Percentile Analysis.

## Phase 8 — Evolution Engine

Generation Directives.

## Phase 9 — Automated Claude Generation

Directive → Chart Version.

## Phase 10 — Controlled Rollout

A/B / staged testing.

## Phase 11 — Preference Learning

Human Feedback + Pairwise Preferences.

## Phase 12 — Personalization

Player-specific Chart Optimization.

---

# 93. Wichtig: Nicht alles gleichzeitig kompliziert machen

Implementiere zuerst einen funktionierenden End-to-End-Loop:

```text
Song
→ 4 Charts
→ Player
→ Telemetry
→ Analytics
→ Difficulty Diagnosis
→ new Chart Version
```

Danach erweitern.

Keine unnötige ML-Komplexität im MVP.

---

# 94. Erste funktionierende Version

Die erste produktive Version muss bereits können:

1. Song importieren.
2. Audio analysieren.
3. musikalische Events erzeugen.
4. vier Charts generieren.
5. Charts validieren.
6. Charts speichern.
7. Spieler spielen lassen.
8. jede Noteingabe speichern.
9. Timing Error in Millisekunden speichern.
10. Sessions analysieren.
11. Spielerperformance gruppieren.
12. zu leichte/zu schwere Bereiche erkennen.
13. konkrete Generation Directive erzeugen.
14. neue Chart-Version erzeugen.
15. neue Version validieren.
16. Versionen vergleichen.

---

# 95. Erfolgsmetrik

Das Projekt soll langfristig nicht nur anhand technischer Metriken bewertet werden.

Die wichtigste Frage ist:

> **Wird das Spielerlebnis über Chart-Versionen messbar besser?**

Beobachte insbesondere:

```text
Player Preference
Fun
Flow
Musicality
Completion
Retry
Timing Precision
Engagement
Difficulty Satisfaction
```

---

# 96. Endgültiger Qualitätsstandard

Ein fertiger Chart muss diese Fragen bestehen:

### Song Identity

> Erkenne ich den Song durch das Gameplay?

### Musicality

> Sind musikalisch relevante Ereignisse sinnvoll gemappt?

### Musical Impact

> Fühlen sich wichtige musikalische Momente stark an?

### Flow

> Fühlt sich die Note Sequence natürlich an?

### Playability

> Ist der Chart physisch sinnvoll spielbar?

### Difficulty

> Ist die Schwierigkeit für diese Stufe angemessen?

### Immersion

> Fühlt sich der Spieler, als würde er den Song selbst performen?

---

# 97. ABSOLUTES OBERSTES ZIEL

Das gesamte System soll auf dieses Ergebnis hinarbeiten:

```text
                     AUDIO
                       ↓
                     SONG
                       ↓
              MUSICAL INTERPRETATION
                       ↓
                GAMEPLAY DESIGN
                       ↓
                 PLAYER INPUT
                       ↓
              PHYSICAL PERFORMANCE
                       ↓
                MUSICAL FEEDBACK
                       ↓
                 PLAYER FEELING

        "ICH SPIELE DIESEN SONG."
```

Und danach:

```text
PLAYER
  ↓
DATA
  ↓
LEARNING
  ↓
BETTER CHART
  ↓
PLAYER
  ↓
BETTER DATA
  ↓
BETTER CHART
  ↓
...
```

Das System soll sich dadurch langfristig selbst verbessern.

---

# 98. Abschließende Arbeitsanweisung an Claude Code

Beginne NICHT sofort mit dem Schreiben großer Mengen Code.

Zuerst:

1. Repository untersuchen.
2. Bestehende Architektur dokumentieren.
3. Audio-to-Chart-Pipeline verstehen.
4. Chartformat verstehen.
5. Runtime und Input-System verstehen.
6. Datenhaltung verstehen.
7. Tests verstehen.
8. Lücken identifizieren.

Danach:

9. Zielarchitektur vorschlagen.
10. Bestehende Komponenten wiederverwenden.
11. Datenmodell definieren.
12. Schnittstellen definieren.
13. Implementierungsplan erstellen.
14. Tests planen.
15. Danach schrittweise implementieren.

Bei jeder Implementierungsphase:

```text
IMPLEMENT
 ↓
TEST
 ↓
RUN
 ↓
INSPECT
 ↓
FIX
 ↓
DOCUMENT
```

Arbeite nicht nur darauf hin, dass der Code kompiliert.

Arbeite darauf hin, dass das **gesamte System nachweisbar bessere spielbare Charts erzeugt**.

---

# 99. Leitprinzip in einem Satz

> **Baue kein System, das lediglich Songs in Notes übersetzt. Baue ein System, das versteht, welche spielbaren Interaktionen einen Menschen den Song maximal fühlen lassen, diese Interaktionen misst, daraus lernt und die Charts über reale Spielerfahrung kontinuierlich verbessert.**
