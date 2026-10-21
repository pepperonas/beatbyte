# Gemessene Ergebnisse

Automatisierter Forschungsprototyp; ein menschlicher Hör-/Spieltest wurde nicht durchgeführt.

## Vollständige Songs

Expert-Ereignisvergleich bei ±50 ms; Makromittel pro erfolgreichem Song. P = Precision, R = Recall. Fehler werden zusätzlich ausgewiesen.

| Modus | Genre | Split | erfolgreich / versucht | P | R | F1 | Mix F1 | Lead-Study F1 |
|---|---|---|---:|---:|---:|---:|---:|
| DSP | electronic | development | 8/8 | 0.490 | 0.451 | 0.435 | 0.523 | 0.478 |
| DSP | electronic | holdout | 4/4 | 0.623 | 0.433 | 0.505 | 0.588 | 0.460 |
| DSP | metal | development | 8/8 | 0.633 | 0.411 | 0.489 | 0.595 | 0.435 |
| DSP | metal | holdout | 4/4 | 0.574 | 0.442 | 0.498 | 0.570 | 0.446 |
| DSP | pop | development | 8/8 | 0.424 | 0.405 | 0.412 | 0.540 | 0.444 |
| DSP | pop | holdout | 4/4 | 0.392 | 0.444 | 0.401 | 0.472 | 0.393 |
| DSP | rock | development | 8/8 | 0.520 | 0.494 | 0.491 | 0.557 | 0.457 |
| DSP | rock | holdout | 4/4 | 0.501 | 0.448 | 0.441 | 0.533 | 0.422 |
| ML | electronic | development | 8/8 | 0.404 | 0.481 | 0.418 | 0.523 | 0.428 |
| ML | electronic | holdout | 4/4 | 0.589 | 0.485 | 0.512 | 0.588 | 0.435 |
| ML | metal | development | 8/8 | 0.571 | 0.349 | 0.412 | 0.595 | 0.391 |
| ML | metal | holdout | 4/4 | 0.495 | 0.481 | 0.488 | 0.570 | 0.436 |
| ML | pop | development | 8/8 | 0.382 | 0.567 | 0.446 | 0.540 | 0.462 |
| ML | pop | holdout | 4/4 | 0.413 | 0.662 | 0.494 | 0.472 | 0.397 |
| ML | rock | development | 8/8 | 0.432 | 0.482 | 0.424 | 0.557 | 0.440 |
| ML | rock | holdout | 4/4 | 0.464 | 0.401 | 0.422 | 0.533 | 0.440 |

Beide vollständigen Durchläufe sind abgeschlossen: je 48 Songs, zusammen 96 Ausgaben.

## Gesamtergebnis

- DSP: 48/48 erfolgreich; Expert-F1 ±50 ms: neuer Generator 0.458, Mix-Baseline 0.549, Lead-Study 0.446. Fehlgeschlagene oder fehlende Songs als 0 gewertet: 0.458.
  Gegen Mix-Baseline: 5/48 Songs mit höherem F1. Median der Generationszeit: 6.0 s (ggf. Caches; Benchmark-/Baselinezeit zusätzlich).
- ML: 48/48 erfolgreich; Expert-F1 ±50 ms: neuer Generator 0.443, Mix-Baseline 0.549, Lead-Study 0.429. Fehlgeschlagene oder fehlende Songs als 0 gewertet: 0.443.
  Gegen Mix-Baseline: 9/48 Songs mit höherem F1. Median der Generationszeit: 66.4 s (ggf. Caches; Benchmark-/Baselinezeit zusätzlich).

Der KI-Prototyp ist im gesamten zeitlichen Referenzvergleich schwächer als der vorhandene Mix-Generator. Ein musikalischer Qualitätsgewinn und ein Produktionsersatz sind damit nicht nachgewiesen. Die Ergebnisse sind ein belastbarer Ausgangspunkt für Instrumenten-/Anschlagsverbesserungen und den vorbereiteten Hör-/Spielvergleich.

## Technische Prüfung

164 Chartdateien einschließlich Referenzen und Vergleichsfassungen geprüft; 0 fehlerhaft. Native Chartvalidierung sowie Integrität vorhandener WAV-/PNG-Vorschauen sind enthalten.
17 gezielte Tests bestanden. Der CLI-Kontrolllauf ist in `reports/cli-smoke.json` dokumentiert.

[Direkte Hör- und Spielvergleiche](listening-cases.md)


## Trennmodell-Auswahl

```json
{
  "development_excerpts": 8,
  "matched_pairs": 8,
  "macro_expert_f1_50ms": {
    "htdemucs": 0.39404526138880785,
    "htdemucs_6s": 0.34649310866377037
  },
  "chosen": "htdemucs",
  "policy": "6s replaces default only with >=4 matched development clips and >0.02 absolute macro-F1 gain. Timing proxy only; no subjective guitar-isolation conclusion."
}
```

## Interpretation

Diese Zahlen prüfen zeitliche Übereinstimmung mit vorhandenen importierten Charts. Sie beweisen weder richtige Gitarrenisolation noch gleichwertiges Spielgefühl. Ein hoher Recall bei geringer Precision bedeutet viele zusätzliche Ereignisse; ein geringer Recall kann fehlende Instrumentenbelege oder eine abweichende musikalische Stimme anzeigen.

Die Herkunft der unteren Schwierigkeitsstufen ist überwiegend unbekannt. Dafür werden keine behaupteten menschlichen Trainingslabels verwendet. `reports/calibration.json` dokumentiert die tatsächlich mögliche Kalibrierung.

Besteht kein klarer Vorteil gegenüber beiden vorhandenen Generatoren auf dem Holdout, ist das Experiment kein nachgewiesener Ersatz. Nächste Änderungen müssen anhand der dokumentierten Fehlertypen entwickelt und auf neuen Prüfsongs validiert werden.

## Direkt ausprobieren

- `outputs/pilot-duvet/chart.json`, `preview.wav`, `chart-preview.png`
- `outputs/pilot-duvet-6s/chart.json`: experimenteller Gitarren-Trennpfad
- `outputs/pilot-boilermaker/chart.json`: Halbtempo-Abweichung zwischen DSP und KI, auf explizitem KI-Raster verarbeitet

Die Audio-/Chart-Ausgaben bleiben lokal im ursprünglichen Testordner und sind nicht Teil dieses Repositorys. Der App-Generator und die USB-Quelldateien wurden nicht geändert.
