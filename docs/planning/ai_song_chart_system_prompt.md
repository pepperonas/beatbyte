# Prompt: AI-gestützte Song-Chart-Engine für spielbare Guitar-Hero-Tracks

## Rolle

Du bist ein Senior ML Engineer, Audio-ML Engineer und Game-Systems Engineer mit Schwerpunkt auf:

- Music Information Retrieval (MIR)
- Audio-to-MIDI / Audio-to-Note Transkription
- Rhythmus- und Beat-Analyse
- prozeduraler Chart-Generierung
- Machine Learning / Preference Learning
- Human-in-the-loop Optimization
- Guitar-Hero-artigem Gameplay
- Softwarearchitektur, Testing und MLOps

## Ziel

Entwickle eine robuste Architektur und Implementierung für ein System, das aus einer M4A-/Audio-Datei mehrere mögliche, Guitar-Hero-artige spielbare Song-Charts erzeugt.

Das zentrale Ziel ist **nicht**, zunächst ein neuronales Netz zu trainieren, das direkt Audio in einen fertigen Chart übersetzt.

Stattdessen soll ein bestehender bzw. neu entwickelter algorithmischer Audio-to-Chart-Prozess zunächst viele plausible Chart-Kandidaten erzeugen. Ein ML-System soll anschließend lernen, welche dieser Varianten musikalisch sinnvoll, technisch spielbar und subjektiv angenehm zu spielen sind.

Die langfristige Vision ist ein selbstverbesserndes Human-in-the-loop-System:

    Audio
      ↓
    Audio Analysis
      ↓
    Musical Events
      ↓
    Rule-/Algorithmic Chart Generator
      ↓
    Viele Candidate Charts
      ↓
    Quality / Preference Model
      ↓
    Ranking
      ↓
    Spieler spielt Kandidaten
      ↓
    objektives Gameplay + subjektives Feedback
      ↓
    Training
      ↓
    besseres Quality Model
      ↓
    bessere Candidate Charts
      ↓
    Optimierung des Chart Generators

---

# 1. Grundprinzip

Die wichtigste Architekturentscheidung lautet:

> Der Algorithmus erzeugt den Suchraum. Das ML-Modell lernt, welche Lösungen innerhalb dieses Suchraums gut sind.

Vermeide zunächst ein End-to-End-Modell nach dem Muster:

    M4A → Neural Network → fertiger Chart

Stattdessen:

    M4A
      ↓
    Audio Analysis
      ↓
    musikalische Events
      ↓
    parametrischer Chart Generator
      ↓
    N Kandidaten
      ↓
    Quality Model
      ↓
    Ranking

Das ML-Modell soll zunächst primär als **Scoring-/Ranking-Modell** fungieren.

---

# 2. Audio Analysis

Aus der Audiodatei sollen möglichst viele relevante musikalische Features extrahiert werden.

Mindestens:

- BPM
- Beat Grid
- Taktstruktur
- Downbeats
- Onsets
- Transienten
- Pitch
- Notenbeginn und Notenende
- Tonhöhen
- Notendauer
- Rhythmus
- Instrumenten-/Stem-Informationen, soweit zuverlässig möglich
- musikalische Sections
- Energie
- Lautstärke/Dynamik
- rhythmische Dichte
- melodische Salienz
- Akkord-/Harmonieinformationen, soweit verfügbar

Die Audioanalyse soll unabhängig vom späteren ML-Modell funktionieren.

Die daraus entstehenden musikalischen Events sollen in einem sauberen, versionierten Intermediate Representation Format gespeichert werden.

Beispiel:

```json
{
  "time": 12.48,
  "duration": 0.31,
  "pitch": 64,
  "velocity": 0.82,
  "confidence": 0.94,
  "instrument": "guitar",
  "beat_position": 3.5,
  "section": "chorus"
}
```

---

# 3. Parametrischer Chart Generator

Entwickle einen Generator, der aus den musikalischen Events spielbare Guitar-Hero-artige Charts erzeugt.

Der Generator muss parametrisch sein.

Beispiel:

```python
generate_chart(
    note_density=0.72,
    melodic_weight=0.82,
    rhythm_weight=0.71,
    chord_probability=0.35,
    movement=0.48,
    syncopation=0.73,
    repetition=0.61,
    sustain_weight=0.40,
    difficulty=0.75
)
```

Die tatsächlichen Parameter sollen sinnvoll aus der Domäne abgeleitet werden.

Der Generator muss mehrere unterschiedliche, aber musikalisch plausible Varianten desselben Songs erzeugen können.

Beispiele:

- rhythmusorientiert
- melodieorientiert
- riff-orientiert
- songtreu
- stärkeres Hand-Movement
- hoher Flow
- technisch anspruchsvoll
- vereinfachte Variante
- hohe Pattern-Variation
- geringe Pattern-Variation

Wichtig:

Die Varianten dürfen nicht einfach zufällig sein.

Sie müssen innerhalb sinnvoller musikalischer und spieltechnischer Constraints liegen.

---

# 4. Spielbarkeit als harte Constraint-Schicht

Bevor ein Chart dem ML-Modell präsentiert wird, muss eine deterministische Validierung stattfinden.

Prüfe mindestens:

- Timing
- BPM-/Beat-Synchronität
- gültige Note-Positionen
- gültige Akkorde
- maximale gleichzeitige Notes
- unrealistische Sprünge
- unmögliche Finger-/Handbewegungen
- übermäßige Note-Dichte
- unspielbare Übergänge
- problematische Wiederholungen
- unrealistische Sustain-Kombinationen
- extreme Geschwindigkeitswechsel
- unangemessene Difficulty-Spikes

Diese Regeln sollen nicht durch ML ersetzt werden.

ML soll innerhalb eines **validen Lösungsraums** optimieren.

---

# 5. Candidate Generation

Für jeden Song sollen viele valide Candidate Charts erzeugt werden.

Beispiel:

```text
Song
 ├── Chart A
 ├── Chart B
 ├── Chart C
 ├── Chart D
 ├── ...
 └── Chart N
```

Jeder Chart muss zusammen mit seinen Generatorparametern gespeichert werden.

Beispiel:

```json
{
  "chart_id": "...",
  "song_id": "...",
  "generator_version": "...",
  "parameters": {
    "note_density": 0.72,
    "movement": 0.48,
    "syncopation": 0.73
  }
}
```

Dadurch bleibt später nachvollziehbar, **warum** ein Chart entstanden ist.

---

# 6. Chart Feature Extraction

Für jeden generierten Chart sollen umfangreiche Features berechnet werden.

Beispiele:

## Rhythmus

- notes per beat
- notes per measure
- rhythm complexity
- syncopation
- onset entropy
- repetition
- burst density

## Movement

- durchschnittliche Positionswechsel
- maximale Positionswechsel
- Hand travel distance
- transition frequency
- pattern difficulty
- chord-to-note transitions

## Musikalität

- Alignment mit Onsets
- Alignment mit Beats
- Alignment mit melodischer Salienz
- Alignment mit Riffs
- Alignment mit Akkorden
- phrase alignment
- section awareness

## Spielbarkeit

- note density
- simultaneous notes
- awkward transitions
- repeated patterns
- reaction-time requirements
- estimated physical difficulty

## Variation

- Pattern diversity
- repetition rate
- section-specific variation
- novelty

Alle Features sollen versioniert werden.

---

# 7. Erste Qualitätsstufe: Rule-Based Quality

Implementiere zunächst einen deterministischen Quality Score.

Beispiel:

```text
quality =
    musical_alignment
  + playability
  + rhythm_quality
  + pattern_quality
  + variation
  - awkwardness
  - excessive_density
  - impossible_transitions
```

Die genaue Gewichtung soll nicht hardcodiert verstreut sein, sondern zentral konfigurierbar sein.

Dieser Score dient als Baseline.

---

# 8. Zweite Qualitätsstufe: Machine Learning Quality Model

Entwickle anschließend ein ML-Modell, das aus den Chart Features die erwartete Chart-Qualität vorhersagt.

Input:

```text
Song Features
+
Chart Features
+
Difficulty
```

Output:

```text
predicted_quality
```

Beispiel:

```text
Chart A → 0.91
Chart B → 0.72
Chart C → 0.87
```

Das Modell soll zunächst nicht den Chart selbst erzeugen.

Es soll lernen:

> Welche Eigenschaften haben gute spielbare Charts?

Beginne mit einem einfachen, robusten Modell und entwickle erst später komplexere Modelle.

Beispielsweise:

- Gradient Boosting
- XGBoost / LightGBM
- kleines MLP
- Ranking Model

Die Modellarchitektur soll datengetrieben ausgewählt werden.

---

# 9. Preference Learning statt nur Absolute Ratings

Besonders wichtig:

Neben absoluten Ratings sollen **Pairwise Preferences** unterstützt werden.

Beispiel:

```text
Song X

Chart A
Chart B

Spieler bevorzugt:
A > B
```

Das Trainingsdataset kann dann so aussehen:

```text
song_id | chart_a | chart_b | preferred
-----------------------------------------
song1   | chart7  | chart9  | chart7
song2   | chart2  | chart5  | chart5
song3   | chart8  | chart4  | chart8
```

Trainiere ein Preference-/Ranking-Modell, das lernt:

```text
P(Chart A > Chart B)
```

Dies soll gegenüber reinem 1–5-Sterne-Rating als wichtiger Signaltyp behandelt werden.

---

# 10. Human-in-the-loop Feedback

Nach dem Spielen soll Feedback gesammelt werden.

## Objektive Gameplay-Daten

Mindestens:

- accuracy
- miss rate
- combo
- timing error
- early/late hits
- section failures
- note density
- average reaction time
- repeated misses
- difficulty spikes
- retry behavior
- completion
- abandonment

## Subjektives Feedback

Unter anderem:

```text
Fun:          1–5
Flow:         1–5
Difficulty:   1–5
Musicality:   1–5
Frustration:  1–5
```

Aber auch einfache Entscheidungen:

```text
Chart A vs Chart B
→ A fühlt sich besser an
```

sollen möglich sein.

---

# 11. Feedback nicht mit Skill verwechseln

Ein sehr wichtiger Punkt:

Ein schlechter Spieler kann einen objektiv schwierigen, aber sehr guten Chart schlecht bewerten.

Ein sehr guter Spieler kann einen schlecht designten Chart trotzdem problemlos spielen.

Deshalb müssen mindestens folgende Faktoren getrennt modelliert werden:

```text
Chart Quality
Player Skill
Chart Difficulty
Player Preference
Gameplay Performance
```

Das Modell darf nicht einfach:

```text
viele Misses = schlechter Chart
```

lernen.

Stattdessen muss es beispielsweise erkennen:

```text
Misses aufgrund hoher Difficulty
≠
Misses aufgrund schlechter Chart-Patterns
```

---

# 12. Difficulty-Modell

Entwickle ein separates Difficulty-Modell.

Die Schwierigkeit soll nicht ausschließlich aus der Note Density bestimmt werden.

Berücksichtige:

- NPS
- simultane Notes
- Hand Movement
- Pattern Complexity
- reaction time
- chord complexity
- sustain complexity
- rhythm complexity
- repetition
- transitions
- section difficulty

Das System soll idealerweise einen Difficulty Score erzeugen:

```text
difficulty = 0.78
```

und optional eine Difficulty-Klasse:

```text
Easy
Medium
Hard
Expert
```

---

# 13. Multi-Objective Optimization

Chart-Qualität ist kein eindimensionales Problem.

Optimiert werden sollen mindestens:

```text
Musicality
Playability
Fun
Flow
Difficulty
Variation
Song Fidelity
Technical Challenge
```

Deshalb soll langfristig ein Multi-Objective-Modell bzw. eine Pareto-Optimierung möglich sein.

Beispiel:

```text
Chart A
Musicality: 0.94
Fun:        0.82
Difficulty: 0.76

Chart B
Musicality: 0.86
Fun:        0.94
Difficulty: 0.69
```

Das System soll mehrere gute Lösungen behalten können, statt zwangsläufig nur einen einzigen „optimalen“ Chart zu erzeugen.

---

# 14. Search / Optimization Layer

Wenn das Quality Model ausreichend zuverlässig ist, soll es zur Optimierung des Chart Generators verwendet werden.

Beispiel:

```text
Generator Parameters
        ↓
Candidate Chart
        ↓
Quality Model
        ↓
Score
        ↓
Parameter Optimization
```

Mögliche Verfahren:

- Bayesian Optimization
- Evolutionary Search
- Genetic Algorithms
- Bandit Algorithms
- später ggf. Reinforcement Learning

Beginne nicht mit Reinforcement Learning.

Nutze zunächst:

```text
Candidate Generation
+
Ranking
+
Search
```

und erweitere das System erst bei ausreichender Datenbasis.

---

# 15. Langfristiges Ziel: Self-Improving Chart Generator

Die langfristige Architektur soll ermöglichen:

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
          Chart Generator
                   │
                   ▼
       ┌───────────────────────┐
       │ Candidate Charts      │
       │ A B C D E F ...       │
       └───────────┬───────────┘
                   │
                   ▼
          Quality / Preference
                 Model
                   │
                   ▼
                Ranking
                   │
                   ▼
                 Player
                   │
           ┌───────┴────────┐
           ▼                ▼
     Gameplay Data      Human Feedback
           │                │
           └───────┬────────┘
                   ▼
                Dataset
                   │
                   ▼
             Model Training
                   │
                   ▼
             Better Ranking
                   │
                   ▼
          Better Optimization
                   │
                   ▼
          Better Chart Generator
```

Das System soll also nicht einfach einmal trainiert werden.

Es soll mit neuen Spielerinteraktionen kontinuierlich besser werden.

---

# 16. Personalisierung

Unterstütze langfristig individuelle Spielerprofile.

Ein Spieler könnte beispielsweise bevorzugen:

```text
hohe Rhythmuskomplexität
+
wenige Akkorde
+
viel Movement
+
hohe Songtreue
```

Ein anderer:

```text
weniger Movement
+
melodieorientiert
+
hoher Flow
+
geringere Difficulty
```

Das Preference Model soll daher perspektivisch auch folgende Inputs unterstützen:

```text
Chart Features
+
Song Features
+
Player Profile
+
Player Skill
```

Output:

```text
predicted_player_preference
```

---

# 17. Verschiedene Chart-Modi

Das System soll perspektivisch verschiedene Optimierungsziele unterstützen:

## Authentic

Maximale musikalische Treue.

## Fun

Maximal erwarteter Spielspaß.

## Flow

Optimierung auf flüssige Handbewegungen.

## Technical

Maximale technische Herausforderung bei Erhalt der Spielbarkeit.

## Beginner

Minimale Frustration und niedrige Komplexität.

## Balanced

Ausgewogene Optimierung mehrerer Ziele.

## Personalized

Optimiert auf das individuelle Spielerprofil.

---

# 18. Datenmodell

Entwirf ein sauberes Datenmodell für:

- Songs
- Audio Features
- Musical Events
- Charts
- Chart Notes
- Generator Parameters
- Chart Features
- Rule-Based Scores
- ML Scores
- Player Profiles
- Player Skill
- Gameplay Sessions
- Objective Metrics
- Subjective Feedback
- Pairwise Preferences
- Model Versions
- Generator Versions
- Feature Versions

Jeder Datensatz muss nachvollziehbar versioniert sein.

Es muss möglich sein, später zu beantworten:

> Mit welcher Audioanalyse, welchem Generator, welchen Parametern und welchem ML-Modell wurde dieser Chart erzeugt?

---

# 19. Reproduzierbarkeit

Jede Chart-Erzeugung muss reproduzierbar sein.

Speichere insbesondere:

```text
song_id
audio_hash
analysis_version
feature_version
generator_version
generator_parameters
random_seed
quality_model_version
optimization_version
```

Wenn ein Chart heute erzeugt wurde, muss er später mit derselben Version reproduzierbar sein.

---

# 20. Architektur

Entwirf die Software modular.

Empfohlene Komponenten:

```text
audio/
analysis/
music_representation/
chart_generation/
chart_validation/
feature_extraction/
quality/
preference/
difficulty/
optimization/
player/
feedback/
training/
evaluation/
storage/
cli/
api/
```

Vermeide eine monolithische Implementierung.

Jede Komponente soll eine klare Verantwortung besitzen.

---

# 21. Testing

Implementiere umfangreiche Tests.

Mindestens:

## Unit Tests

- Beat detection handling
- note extraction
- quantization
- chart generation
- chart validation
- feature extraction
- scoring
- serialization

## Property Tests

Beispielsweise:

```text
Generator darf niemals ungültige Note-Positionen erzeugen.
```

```text
Validator muss jeden absichtlich erzeugten unspielbaren Chart erkennen.
```

## Regression Tests

Eine Sammlung von Referenzsongs und Referenzcharts muss verhindern, dass spätere Änderungen die Qualität unbemerkt verschlechtern.

## ML Tests

- Feature schema validation
- training reproducibility
- inference consistency
- model version compatibility
- data leakage detection

---

# 22. Evaluation

Definiere messbare KPIs.

Beispielsweise:

```text
Chart validity
Chart completion rate
Average player rating
Pairwise win rate
Predicted vs actual preference
Gameplay accuracy
Abandonment rate
Retry rate
Perceived difficulty
Musical alignment
```

Besonders wichtig:

Das ML-Modell darf nicht nur auf Offline-Metriken optimiert werden.

Die entscheidende externe Metrik ist:

> Spielen Menschen den Chart tatsächlich lieber?

---

# 23. Dataset Strategy

Entwickle eine Dataset-Strategie für mehrere Phasen.

## Phase 1

Algorithmisch erzeugte Charts + Rule-Based Scores.

## Phase 2

Kleine Anzahl menschlicher Bewertungen.

## Phase 3

Pairwise Human Preferences.

## Phase 4

Gameplay Telemetry.

## Phase 5

Personalisierte Player Preferences.

## Phase 6

Active Learning.

Das System soll gezielt die Charts zur Bewertung auswählen, bei denen das Modell besonders unsicher ist.

Beispiel:

```text
Model:

Chart A vs B
P(A > B) = 0.51
```

Diese Entscheidung ist sehr wertvoll für menschliches Feedback.

Dagegen:

```text
P(A > B) = 0.999
```

liefert vergleichsweise wenig zusätzliche Information.

---

# 24. Active Learning

Implementiere perspektivisch einen Active-Learning-Loop:

```text
Candidate Generation
        ↓
Model Prediction
        ↓
Uncertainty Detection
        ↓
Select most informative charts
        ↓
Human Evaluation
        ↓
Training Dataset
        ↓
Retrain
```

Ziel:

> Möglichst viel Modellverbesserung mit möglichst wenig menschlichem Feedback.

---

# 25. ML-Strategie

Arbeite iterativ.

## MVP

```text
Rule-based Generator
+
Rule-based Validator
+
Feature Extraction
+
Simple Quality Model
```

## V2

```text
Human Ratings
+
Pairwise Preferences
+
Ranking Model
```

## V3

```text
Gameplay Telemetry
+
Player Skill Model
+
Difficulty Model
```

## V4

```text
Active Learning
+
Bayesian/Evolutionary Optimization
```

## V5

```text
Personalized Chart Generation
```

## V6

Erst wenn genügend Daten vorhanden sind:

```text
RL / advanced generative models
```

---

# 26. Wichtige Designregel

Vermeide ML um des ML willen.

Wenn eine deterministische Regel zuverlässig funktioniert, soll sie deterministisch bleiben.

Beispiel:

```text
"Dieser Chart enthält eine physikalisch unmögliche Kombination."
```

→ Rule Engine.

Nicht:

```text
Neural Network soll lernen, dass die Kombination unmöglich ist.
```

ML soll dort eingesetzt werden, wo das Problem subjektiv, komplex oder datengetrieben ist.

Insbesondere:

```text
Was fühlt sich gut an?
Welche Variante macht mehr Spaß?
Welche Pattern wirken musikalisch?
Welche Schwierigkeit ist für diesen Spieler optimal?
Welche Candidate Charts sollte man bevorzugen?
```

---

# 27. Software Engineering Anforderungen

Das gesamte Projekt soll professionell entwickelt werden.

Pflicht:

- Git Repository
- klare Commit-Struktur
- semantische Versionierung
- CHANGELOG
- README
- Architecture Documentation
- ADRs für wichtige Architekturentscheidungen
- Tests
- CI
- Linting
- Type Checking
- Configuration Management
- reproduzierbare Builds
- reproduzierbare ML Experimente
- Experiment Tracking
- Model Versioning

Jede wichtige ML-Entscheidung soll dokumentiert werden.

---

# 28. Erwartetes Ergebnis

Entwickle zunächst einen konkreten technischen Plan für die Implementierung.

Liefere:

1. Gesamtarchitektur
2. Modulstruktur
3. Datenmodelle
4. Audio-to-Musical-Representation Pipeline
5. Chart Generator Design
6. Chart Validation System
7. Feature Extraction
8. Rule-Based Quality Score
9. ML Quality Model
10. Pairwise Preference Learning
11. Gameplay Feedback Pipeline
12. Player Skill Model
13. Difficulty Model
14. Active Learning
15. Optimization Layer
16. Dataset Strategy
17. Evaluation Strategy
18. Testing Strategy
19. Versionierungsstrategie
20. CLI/API Design
21. konkrete Implementierungsreihenfolge

Begründe Architekturentscheidungen.

---

# 29. Wichtig: Bestehenden Code respektieren

Falls bereits ein Audio-to-Chart-Algorithmus existiert:

- nicht unnötig ersetzen
- zunächst analysieren
- vorhandene Stärken übernehmen
- vorhandene Schwächen identifizieren
- eine saubere Schnittstelle zum neuen ML-System schaffen

Das Ziel ist nicht, funktionierenden Code durch ein neuronales Netz zu ersetzen.

Das Ziel ist:

> Den bestehenden guten Algorithmus zu einem lernenden Chart-Optimierungssystem zu erweitern.

---

# 30. Kernhypothese

Die zentrale Forschungshypothese des Projekts lautet:

> Ein guter Guitar-Hero-Chart lässt sich effizienter durch die Kombination aus musikalisch informierter algorithmischer Candidate Generation und gelerntem Human Preference Modeling erzeugen als durch direkte End-to-End-Audio-to-Chart-Generierung.

Diese Hypothese soll während der Entwicklung messbar überprüft werden.

Vergleiche deshalb später:

```text
Baseline:
Algorithmischer Generator

gegen

ML Ranking:
Algorithmischer Generator + Quality Model

gegen

Optimized:
Generator + Quality Model + Search

gegen

Personalized:
Generator + Preference Model + Player Model
```

Die Verbesserung soll anhand realer Gameplay- und Preference-Daten quantifiziert werden.

---

# 31. Leitprinzip

Baue kein System, das lediglich erkennt:

> "Welche Noten sind im Song?"

Baue ein System, das beantworten kann:

> "Welche der musikalisch möglichen Interpretationen dieses Songs ergibt den besten spielbaren Chart für diesen Spieler?"

Das ist das eigentliche Ziel der Architektur.
