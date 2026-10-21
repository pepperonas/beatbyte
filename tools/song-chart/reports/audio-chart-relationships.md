# Beziehung zwischen Audio und Referenznoten

Deskriptiver Vergleich nach eingefrorener Generierung. Transkribierte Instrumentenanschläge und Referenz-Ereignisse werden bei ±80 ms eins zu eins gepaart. Dies bewertet die gewählte Instrumentenspur und das Tonhöhenmodell gemeinsam; es sind keine verifizierten Tonhöhenlabels.

| Genre | Songs | Mono-Kontur stimmt | Gleiche Tonhöhe → gleicher Griff | Referenzakkorde mit Mehrton-Beleg |
|---|---:|---:|---:|---:|
| rock | 12 | 65.4% (n=816) | 47.1% (n=257) | 37.5% (n=1602) |
| metal | 12 | 61.1% (n=1544) | 68.8% (n=760) | 29.8% (n=2090) |
| pop | 12 | 64.2% (n=411) | 43.2% (n=176) | 56.7% (n=2035) |
| electronic | 12 | 68.4% (n=1838) | 51.0% (n=679) | 49.5% (n=747) |

Kontur wird nur bei gepaarten monophonen Tönen, höchstens 1 s Abstand und tatsächlich wechselndem Griff ausgewertet. Verschiedene Tonhöhen mit gleichem Spielgriff gehen nicht in die Richtungsquote ein. Diese Auswahl kann die Quote erhöhen; sie beschreibt die verbleibenden Fälle, keine Gesamtgenauigkeit.

Gleiche Tonhöhen können in menschlichen Charts bewusst unterschiedliche Griffe bekommen. Umgekehrt kann ein Spielakkord einen musikalischen Akzent abstrahieren. Der Generator nutzt deshalb Kontur- und Wiederholungsregeln als Gestaltungsprior, keine behauptete exakte Tonhöhe-zu-Taste-Funktion.

Akkord-Beleg bedeutet lediglich mehrere nahe beginnende transkribierte Tonhöhen in der ausgewählten Spur. Trennartefakte, Bass-/Keyboard-Leakage oder fehlende Gitarrenanteile können den Wert verändern. Sustain-Unterschiede vergleichen hörbare Modellhaltedauer mit der bewusst spielerisch verkürzten Referenz; sie sind keine unmittelbare Modellfehlerquote.

Die Detaildatei enthält außerdem die Referenzverteilung auf Beats, Achtel, Triolen und feinere Positionen. Damit lässt sich erkennen, warum ein starres Viertel-/Achtelraster schnelle / synkopierte Songs nicht abdeckt. Diese Auswertung ändert keine eingefrorenen Parameter.
