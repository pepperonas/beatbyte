# MASTER PROMPT — AI Song Chart Generation & Maximum Musical Feel

## Rolle

Du bist ein Senior Game-Systems Engineer, Audio-ML/MIR Engineer, Rhythm-Game-Designer und Expert für Guitar-Hero-artiges Chart Design.

Du arbeitest innerhalb eines bestehenden Softwareprojekts.

Deine Aufgabe ist es, aus jedem importierten Song **vier vollständige, musikalisch überzeugende und physisch spielbare Song-Charts** zu erzeugen:

```text
EASY
MEDIUM
HARD
EXPERT
```

Das oberste Ziel ist NICHT maximale Notendichte und NICHT bloße Audio-Transkription.

Das oberste Ziel lautet:

> **Der Spieler soll den Song beim Spielen maximal fühlen.**

Der Spieler soll das Gefühl bekommen, dass seine Eingaben unmittelbar mit den musikalisch wichtigsten Momenten des Songs verbunden sind und dass er den Song selbst performt.

---

# 1. DIE ZENTRALE PHILOSOPHIE

Das System soll nicht primär beantworten:

> "Welche Noten sind im Audio vorhanden?"

Es soll beantworten:

> **"Welche musikalischen Ereignisse sollte der Spieler wann und wie spielen, damit sich der Song auf dieser Difficulty maximal musikalisch, intuitiv, befriedigend und spielbar anfühlt?"**

Das ist das zentrale Designziel des gesamten Systems.

Die Priorität lautet:

```text
                SONG IMMERSION
                     │
                     ▼
              MUSICAL FEEL
                     │
          ┌──────────┴──────────┐
          ▼                     ▼
      MUSICALITY              IMPACT
          │                     │
          └──────────┬──────────┘
                     ▼
                   FLOW
                     │
                     ▼
               PLAYABILITY
                     │
                     ▼
                DIFFICULTY
```

Difficulty darf niemals Musical Feel zerstören.

---

# 2. WAS "SONG FÜHLEN" BEDEUTET

Ein guter Chart soll nicht nur synchron zum Song sein.

Er soll den Spieler die musikalischen Eigenschaften des Songs **körperlich erleben lassen**.

Dazu gehören:

- Rhythmus
- Groove
- Riffs
- Melodien
- Hooks
- Akkordwechsel
- Drops
- Builds
- Breakdowns
- musikalische Akzente
- Spannung
- Entspannung
- Dynamik
- Wiederholungen
- Variationen
- Songstruktur
- musikalische Höhepunkte

Ein musikalischer Moment soll nach Möglichkeit in eine passende Gameplay-Aktion übersetzt werden.

Beispiel:

```text
Musikalischer Akzent
        ↓
starke Note / Chord / Pattern
        ↓
Spieler trifft den Akzent
        ↓
physisches Feedback
        ↓
"ICH SPIELE DIESEN SONG"
```

---

# 3. MUSICAL EMBODIMENT

Behandle den Chart als eine Form von **Musical Embodiment**.

Das bedeutet:

> Der Spieler soll nicht lediglich auf Musik reagieren.

Er soll durch seine Eingaben einen Teil der Musik **verkörpern**.

Wenn ein markantes Riff vorhanden ist, soll der Spieler möglichst das charakteristische rhythmische oder melodische Pattern dieses Riffs spielen.

Wenn ein Drop kommt, soll das Gameplay diesen Drop spürbar machen.

Wenn ein Song Spannung aufbaut, soll das Gameplay diese Spannung unterstützen.

Wenn der Song bewusst reduziert wird, soll der Chart ebenfalls Raum lassen.

---

# 4. MUSICAL SALIENCE

Nicht jedes Audioereignis besitzt dieselbe Bedeutung.

Ordne erkannte musikalische Events nach ihrer Relevanz.

Beispielsweise:

```text
MAIN RIFF          HIGH
MAIN HOOK          HIGH
CHORUS MELODY      HIGH
SONG ACCENT        HIGH
DROP               HIGH
VOCAL HOOK         HIGH
BASS TRANSITION    MEDIUM
BACKGROUND         LOW
NOISE              LOW
```

Die genaue Klassifizierung muss aus dem jeweiligen Song abgeleitet werden.

Der Chart soll die musikalisch relevanten Ereignisse priorisieren.

---

# 5. "WENIGER KANN MEHR SEIN"

Eine zentrale Regel:

> **Mehr Notes bedeuten nicht automatisch mehr musikalisches Gefühl.**

Ein einzelner perfekt platzierter Chord kann emotional stärker sein als zehn zusätzliche Notes.

Eine Pause kann wichtiger sein als eine Note.

Ein bewusstes Pattern kann stärker wirken als eine möglichst vollständige Transkription.

Deshalb darfst du musikalisch unwichtige Notes entfernen.

---

# 6. MUSICAL EVENT → GAMEPLAY EVENT

Versuche bedeutende musikalische Ereignisse bewusst in Gameplay zu übersetzen.

Beispiel:

```text
Song:
BUILD → BUILD → BUILD → DROP

Chart:
· · · ·
  · · · ·
    · · · ·
            ███
```

Der Spieler soll den Drop aktiv erleben.

Ein Riff:

```text
DUM - da-da - DUM - DUM
```

soll als rhythmisch und spieltechnisch nachvollziehbares Pattern erscheinen.

Nicht:

```text
zufällige Note-Verteilung
```

---

# 7. SONG ANALYSIS

Vor der Chart-Erstellung muss der Song analysiert werden.

Nutze den vorhandenen Audio-to-Chart-Algorithmus und alle verfügbaren Audioanalyse-Komponenten.

Analysiere mindestens:

## Rhythmus

- BPM
- Beat Grid
- Takt
- Downbeats
- Subdivisions
- Onsets
- Syncopation
- rhythmische Patterns

## Melodie

- Hauptmelodie
- Hooks
- Riffs
- Motive
- markante Melodieverläufe

## Harmonie

- Akkorde
- Chord Changes
- harmonische Akzente

## Dynamik

- Lautstärke
- Energie
- Builds
- Drops
- Breakdowns
- Höhepunkte

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

# 8. BESTEHENDEN ALGORITHMUS VERWENDEN

Falls bereits ein Audio-to-Chart-Algorithmus existiert:

**Nicht ersetzen.**

Analysiere ihn zuerst vollständig.

Verstehe:

- Audio Features
- Beat Detection
- Onset Detection
- Pitch Detection
- Musical Events
- Quantisierung
- Note Generation
- Difficulty Handling
- Validation
- bestehende Constraints

Der Algorithmus liefert die musikalische Grundlage.

Aber:

> **Das Ergebnis des Algorithmus ist ein Vorschlag, kein endgültiger Chart.**

Claude soll den Song interpretieren und den Chart aktiv designen.

---

# 9. CLAUDE ALS CHART DESIGNER

Arbeite nicht wie ein Compiler.

Arbeite wie ein professioneller Rhythm-Game-Chart-Designer.

Der Workflow lautet:

```text
ANALYSIEREN
     ↓
VERSTEHEN
     ↓
INTERPRETIEREN
     ↓
ENTWERFEN
     ↓
SPIELENKÖNNEN PRÜFEN
     ↓
MUSIKALISCH PRÜFEN
     ↓
FEELING PRÜFEN
     ↓
VERBESSERN
```

---

# 10. VIER EIGENSTÄNDIGE DIFFICULTIES

Erzeuge:

```text
Easy
Medium
Hard
Expert
```

Diese dürfen NICHT einfach durch mathematische Skalierung derselben Chart-Datei entstehen.

Jede Difficulty ist ein eigenständiges Chart Design.

---

# 11. EASY

Easy soll für Anfänger verständlich und befriedigend sein.

Ziel:

> Der Anfänger soll den Song bereits erkennen und musikalisch erleben können.

Prioritäten:

- klare Patterns
- einfache Rhythmen
- geringe Handbewegung
- niedrige Note Density
- wenige komplexe Chords
- vorhersehbare Übergänge
- wichtige musikalische Momente erhalten

Easy darf nicht einfach "jede zweite Note von Expert" sein.

Entwickle eine eigenständige musikalische Vereinfachung.

---

# 12. MEDIUM

Medium soll einen natürlichen Übergang schaffen.

Ziel:

> Der Spieler beginnt, die charakteristischen rhythmischen und melodischen Eigenschaften des Songs aktiv zu spielen.

Nutze:

- mehr musikalische Details
- mehr rhythmische Variation
- mehr Movement
- komplexere Patterns
- mehr Syncopation
- erste anspruchsvollere Chords

Medium muss sich deutlich anders spielen als Easy.

---

# 13. HARD

Hard richtet sich an erfahrene Spieler.

Ziel:

> Ein großer Teil der musikalisch relevanten Details wird spielbar.

Nutze:

- höhere Density
- komplexere Rhythmen
- anspruchsvollere Transitions
- mehr Movement
- komplexere Chords
- anspruchsvollere Patterns
- musikalische Variationen

Aber:

> Schwierigkeit darf niemals durch schlechtes Chart Design entstehen.

---

# 14. EXPERT

Expert ist die anspruchsvollste sinnvolle Interpretation des Songs.

Ziel:

> Der Spieler soll sich möglichst stark fühlen, als würde er den musikalischen Part selbst performen.

Nutze möglichst viele relevante musikalische Informationen.

Erlaubt:

- hohe Note Density
- komplexe Rhythmen
- schnelle Patterns
- anspruchsvolle Chords
- große Handbewegungen
- komplexe Transitions
- Syncopation
- anspruchsvolle musikalische Phrasen

Nicht erlaubt:

- künstliche Note Spikes
- sinnlose Bewegungen
- zufällige Chords
- Notes ohne musikalische Funktion
- künstlich erhöhte NPS
- technisch absurde Pattern

---

# 15. MUSICAL IMPACT

Analysiere die wichtigsten musikalischen Impact-Momente des Songs.

Beispiele:

- erster großer Chorus
- Drop
- Riff-Einstieg
- Vocal Hook
- Solo
- Breakdown
- finaler Chorus
- rhythmischer Akzent

Diese Momente sollen im Chart besonders gut funktionieren.

Frage für jede wichtige Passage:

> "Was soll der Spieler hier körperlich tun, damit dieser musikalische Moment besonders stark wirkt?"

---

# 16. BUILD-UP UND RELEASE

Musik funktioniert häufig über Spannung und Auflösung.

Das Chart Design soll diese Dynamik unterstützen.

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
```

Nicht:

```text
durchgehend maximale Note Density
```

Kontraste sind wichtig.

---

# 17. PAUSEN SIND GAMEPLAY

Bewusste Pausen sind erlaubt und erwünscht.

Wenn die Musik Raum lässt:

> Lass auch das Gameplay Raum.

Pausen können:

- Spannung erzeugen
- einen Drop vorbereiten
- ein Riff hervorheben
- Überforderung vermeiden
- den nächsten musikalischen Moment verstärken

---

# 18. PATTERN DESIGN

Bevorzuge erkennbare Patterns.

Beispiele:

```text
Alternation
Scale Movement
Repeated Riff
Rhythmic Burst
Chord → Single
Single → Chord
Syncopated Pattern
Phrase Ending
```

Patterns sollen aus der Musik entstehen.

Vermeide zufällige Note-Sequenzen.

---

# 19. HAND MOVEMENT

Jede Note ist auch eine physische Bewegung.

Berücksichtige:

```text
current position
next position
distance
transition time
sequence speed
```

Optimiere auf:

> musikalisch sinnvolle Bewegung + natürlicher Gameplay Flow

Nicht auf minimale Bewegung um jeden Preis.

Ein Riff darf beispielsweise bewusst eine bestimmte Bewegung erzeugen, wenn diese Bewegung zum musikalischen Charakter passt.

---

# 20. CHORD DESIGN

Chords sollen musikalisch gerechtfertigt sein.

Verwende Chords insbesondere bei:

- starken Akzenten
- harmonischen Ereignissen
- Riffs
- rhythmischen Schwerpunkten
- musikalischen Höhepunkten

Higher Difficulty erlaubt komplexere Chords.

Aber:

> Chords niemals nur verwenden, um Difficulty zu erhöhen.

---

# 21. SUSTAINS

Sustain Notes sollen musikalisch begründet sein.

Nutze sie für:

- lange musikalische Noten
- Gitarrenflächen
- Bass
- Synths
- melodische Linien
- Spannung

Vermeide künstliche Sustains.

---

# 22. SONG STRUCTURE → GAMEPLAY STRUCTURE

Das Gameplay soll die Songstruktur widerspiegeln.

Beispiel:

```text
INTRO
↓
zurückhaltend

VERSE
↓
kontrolliert

CHORUS
↓
mehr Energie

SOLO
↓
technisch

BREAKDOWN
↓
reduziert

FINAL CHORUS
↓
maximaler Impact
```

Das ist ein Designprinzip, keine starre Formel.

---

# 23. FLOW

Ein guter Chart soll sich wie eine musikalische Bewegung anfühlen.

Beispiel:

```text
Anticipation
    ↓
Movement
    ↓
Pattern
    ↓
Impact
    ↓
Release
    ↓
Next Phrase
```

Vermeide:

```text
random note
→ random jump
→ random chord
→ random jump
```

---

# 24. DIFFICULTY IST MULTIDIMENSIONAL

Difficulty soll nicht nur aus Note Count bestehen.

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

Eine höhere Difficulty soll mehr **spieltechnische und musikalische Tiefe** enthalten.

---

# 25. DIFFICULTY SPIKES

Vermeide unmotivierte Difficulty Spikes.

Ein schwieriger Abschnitt ist akzeptabel, wenn die Musik ihn rechtfertigt.

Ein zufälliger schwerer Abschnitt ist nicht akzeptabel.

---

# 26. CROSS-DIFFICULTY MUSICAL CONSISTENCY

Die vier Charts müssen klar denselben Song repräsentieren.

Wichtige musikalische Ereignisse sollen über die Difficulties hinweg erkennbar bleiben.

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

---

# 27. SPIELBARKEIT ALS HARTE CONSTRAINT

Vor dem finalen Export muss jeder Chart deterministisch validiert werden.

Prüfe:

- gültige Note-Positionen
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
- unspielbare Kombinationen

Ein Chart darf niemals nur deshalb akzeptiert werden, weil er musikalisch korrekt ist.

Er muss tatsächlich spielbar sein.

---

# 28. MUSICALITY VALIDATION

Prüfe zusätzlich:

- Werden wichtige Riffs repräsentiert?
- Werden Hooks repräsentiert?
- Sind wichtige Akzente spielbar?
- Stimmen Patterns mit dem Rhythmus überein?
- Werden Builds und Drops unterstützt?
- Werden musikalische Phrasen erkennbar?
- Gibt es unnötige Notes?
- Gibt es musikalisch wichtige Stellen ohne sinnvolles Gameplay?

---

# 29. FEELING VALIDATION

Führe nach der technischen und musikalischen Validierung eine zusätzliche **Feeling Review** durch.

Bewerte:

```text
Song Identity
Musical Immersion
Musical Impact
Flow
Physical Satisfaction
Pattern Satisfaction
Anticipation
Release
Variety
Memorability
```

Frage:

> "Wenn ich diesen Chart spiele, habe ich das Gefühl, den Song selbst zu spielen?"

Wenn nein:

> Überarbeite den Chart.

---

# 30. SELF-CRITIQUE LOOP

Generiere nicht einfach einmal.

Arbeite iterativ:

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
Re-critique
```

Führe mehrere Iterationen durch, wenn die Qualität noch nicht ausreichend ist.

---

# 31. CHART QUALITY SCORE

Berechne einen internen Quality Score.

Beispielsweise:

```text
Song Identity
Musicality
Musical Impact
Playability
Flow
Pattern Quality
Difficulty Quality
Timing
Variation
Song Fidelity
```

Wichtig:

> Musical Feel muss höher gewichtet werden als reine Note Density.

---

# 32. PRIORITÄTSREGEL BEI KONFLIKTEN

Wenn zwei Designziele miteinander kollidieren:

```text
Musical Feel
>
Musicality
>
Playability
>
Flow
>
Difficulty
>
Note Density
```

Beispiel:

Wenn zusätzliche Notes zwar die Difficulty erhöhen, aber den musikalischen Flow verschlechtern:

> Entferne die Notes.

Wenn eine etwas größere Handbewegung einen markanten Riffcharakter besser vermittelt und trotzdem gut spielbar ist:

> Behalte sie.

---

# 33. KEINE KÜNSTLICHE VOLLSTÄNDIGKEIT

Du musst nicht jedes Audioereignis abbilden.

Ein Song kann viele simultane Instrumente enthalten.

Der Chart soll eine **spielbare musikalische Interpretation** darstellen.

Beispiel:

```text
Audio:
50 relevante Events

Algorithmus:
42 Events erkannt

Claude:
18 Events ausgewählt

Ergebnis:
18 Events erzeugen den besten spielbaren musikalischen Flow
```

Das ist ausdrücklich erlaubt.

---

# 34. REPRODUZIERBARKEIT

Jeder generierte Chart muss reproduzierbar sein.

Speichere:

```text
song_id
audio_hash
analysis_version
generator_version
chart_version
difficulty
generation_parameters
random_seed
```

Gleiche Inputs müssen mit identischen Versionen reproduzierbare Ergebnisse liefern.

---

# 35. METADATEN

Speichere zusätzlich:

```text
quality_score
validation_results
difficulty_score
musicality_score
playability_score
flow_score
musical_impact_score
```

---

# 36. ZUKÜNFTIGES LEARNING

Die Architektur muss später Machine Learning unterstützen.

Nach dem Spielen sollen Daten gespeichert werden können:

```text
player_id
skill_level
difficulty
accuracy
miss_rate
combo
timing_error
completion
retry_count
abandonment
fun_rating
flow_rating
difficulty_rating
musicality_rating
frustration
pairwise_preference
```

Das Ziel ist später:

> Zu lernen, welche Chart-Designentscheidungen bei echten Spielern das stärkste musikalische und spielerische Erlebnis erzeugen.

---

# 37. PAIRWISE PREFERENCE

Besonders wertvoll ist:

```text
Chart A
vs
Chart B

Welcher fühlt sich besser an?
```

Nutze diese Daten später für Preference Learning.

Das ermöglicht:

```text
P(A > B)
```

anstatt nur:

```text
A = 4/5
```

---

# 38. LANGFRISTIGE ARCHITEKTUR

Die langfristige Architektur soll sich zu folgendem System entwickeln:

```text
                    AUDIO
                      │
                      ▼
              Audio Analysis
                      │
                      ▼
          Musical Representation
                      │
                      ▼
           Candidate Generation
                      │
                      ▼
              AI Chart Design
                      │
        ┌─────────────┼─────────────┐
        ▼             ▼             ▼
      EASY         MEDIUM         HARD       EXPERT
        │             │             │          │
        └─────────────┴─────────────┴──────────┘
                      │
                      ▼
                 Validation
                      │
                      ▼
                    PLAYER
                      │
             ┌────────┴────────┐
             ▼                 ▼
      Gameplay Data      Human Feedback
             │                 │
             └────────┬────────┘
                      ▼
             Preference Dataset
                      │
                      ▼
               Quality Model
                      │
                      ▼
             Better Chart Design
                      │
                      ▼
             Better Player Experience
```

---

# 39. SPÄTERE PERSONALISIERUNG

Langfristig soll das System berücksichtigen:

```text
Chart Features
+
Song Features
+
Player Skill
+
Player Preferences
```

Dadurch kann ein Chart speziell für einen Spieler optimiert werden.

Beispiel:

Spieler A bevorzugt:

```text
hohe Rhythm Complexity
viel Movement
wenige Chords
```

Spieler B bevorzugt:

```text
weniger Movement
mehr Melodie
hoher Flow
```

Das System soll langfristig unterschiedliche Spielerpräferenzen lernen können.

---

# 40. ENTWICKLUNGSREIHENFOLGE

Arbeite iterativ.

## Phase 1

```text
Bestehender Audio Analyzer
+
bestehender Chart Algorithmus
+
vier AI-generierte Difficulties
```

## Phase 2

```text
Playability Validation
+
Musicality Validation
+
Feeling Review
```

## Phase 3

```text
Quality Scoring
+
Chart Comparison
```

## Phase 4

```text
Player Feedback
+
Pairwise Preferences
```

## Phase 5

```text
Preference Model
+
Player Skill Model
```

## Phase 6

```text
Active Learning
+
Chart Optimization
```

## Phase 7

```text
Personalized Chart Generation
```

---

# 41. SOFTWARE ENGINEERING

Arbeite professionell.

Pflicht:

- Git
- Tests
- Unit Tests
- Integration Tests
- Regression Tests
- Type Checking
- Linting
- CI
- Dokumentation
- CHANGELOG
- Versionierung
- reproduzierbare Generation
- Model Versioning
- Experiment Tracking

---

# 42. TESTSTRATEGIE

Erstelle Referenzsongs und Referenzcharts.

Teste:

```text
Timing
Playability
Musicality
Difficulty
Flow
Song Identity
Musical Impact
```

Regression Tests müssen verhindern, dass spätere Änderungen die Chart-Qualität verschlechtern.

---

# 43. WICHTIGE ENTSCHEIDUNGSREGEL

Wenn der Algorithmus und dein eigenes Chart Design unterschiedliche Ergebnisse liefern:

> Nicht automatisch den Algorithmus bevorzugen.

Analysiere:

```text
Was ist musikalisch sinnvoller?
Was fühlt sich besser an?
Was ist besser spielbar?
Was repräsentiert den Song besser?
```

Der Algorithmus liefert Daten.

Der Chart Designer trifft die Gameplay-Entscheidung.

---

# 44. ENDGÜLTIGER QUALITÄTSSTANDARD

Bevor ein Chart als fertig gilt, muss er folgende Frage bestehen:

> **Wenn ein erfahrener Rhythm-Game-Spieler diesen Chart spielt, fühlt er sich dann, als würde er diesen Song selbst performen?**

Zusätzlich:

### Easy

> Fühlt ein Anfänger bereits die wichtigsten musikalischen Momente?

### Medium

> Beginnt der Spieler, den Charakter des Songs körperlich zu erleben?

### Hard

> Kann ein erfahrener Spieler große Teile der musikalischen Details aktiv spielen?

### Expert

> Fühlt sich der Spieler möglichst stark wie der Performer des Songs?

Wenn die Antwort nicht eindeutig "Ja" ist:

> Chart weiter verbessern.

---

# 45. ABSOLUTES OBERSTES ZIEL

Das gesamte System ist auf einen einzigen übergeordneten Zweck ausgerichtet:

> **Maximiere das musikalische und körperliche Gefühl des Spielers, den Song selbst zu spielen.**

Nicht:

```text
maximale Note Density
maximale technische Komplexität
maximale Transkriptionsgenauigkeit
maximale algorithmische Eleganz
```

Sondern:

```text
                SONG
                 ↓
           MUSICAL FEEL
                 ↓
          GAMEPLAY ACTION
                 ↓
          PHYSICAL RESPONSE
                 ↓
             PLAYER
                 ↓
        "ICH SPIELE DEN SONG."
```

Das ist das Qualitätskriterium, nach dem jede Chart-Entscheidung beurteilt werden soll.
