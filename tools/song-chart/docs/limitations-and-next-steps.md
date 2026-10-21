# Grenzen und begründete nächste Schritte

Der Prototyp trennt Audioerkennung und Spielgestaltung. Damit sind Fehler nachvollziehbar: Ein fehlender Instrumentenanschlag lässt sich nicht durch eine andere Spurzuordnung reparieren. Die Messberichte zeigen, ob der nächste Hebel im Zeitraster, in den Ereignissen oder in der Spielgestaltung liegt.

## Konkrete technische Grenzen

- Die aktuelle kanonische Arbeits-WAV ist mono. Die ursprüngliche Audiodatei wird für das Spiel unverändert kopiert. Für die nächste Trennmodell-Auswertung sollte der vorhandene kanonische Stereo-Decoder eingebunden werden: Stereo kann der Instrumententrennung zusätzliche Information liefern. Dieser Schritt ist im jetzigen Benchmark noch nicht isoliert gemessen.
- `htdemucs/other` trennt keine reine Gitarre ab. Keyboards und weitere harmonische Instrumente können im selben Stem verbleiben. Der Vergleich mit `htdemucs_6s/guitar` zeigt die Grenzen dieses alternativen Pfads, beweist aber keine richtige Instrumentenzuordnung jedes einzelnen Ereignisses.
- Die lokale Basic-Pitch-Nachverarbeitung verlangt standardmäßig mehr als elf Frames pro Note, ungefähr 128 ms. Sehr kurze Gitarrentöne können deshalb fehlen. Wiederholte Anschläge können teilweise über den zusätzlichen Anschlagsstrom zurückgewonnen werden; schnelle Tonhöhenwechsel ohne akzeptierten Ton bleiben schwierig.
- Palm-Mutes und perkussive Gitarrenanschläge haben nicht immer stabile Tonhöhen. Der konservative Tonhöhenbeleg verhindert erfundene Mix-/Drumnoten, kann aber tatsächlich gespielte Gitarrenrhythmen auslassen.
- Spielakkorde sind musikalische Abstraktionen. Mehrere transkribierte Tonhöhen können auch Oktaven / Obertöne / Instrumentenübersprechen repräsentieren. Die aktuelle Gruppierung ist deshalb keine verifizierte Gitarrengriff-Erkennung.
- Das Phrasenfenster von höchstens 16 Beats und Pausenerkennung ist ein brauchbarer Anfang, aber keine semantische Strophen-/Refrain-/Solo-Erkennung. Exakt erkannte Wiederholungen sind zuverlässig wiederholbar; veränderte Orchestrierung kann ihre Erkennung verhindern.

## Priorisierte Fortsetzung nach Hör- und Spielvergleich

1. Auf unabhängig ausgewählten kurzen Ausschnitten die beabsichtigte Instrumentenstimme und hörbaren Anschläge prüfen. Besonders Palm-Mutes, schnelle Soli, Pausen und Keyboard-/Gitarrenwechsel unterscheiden. Bestehende importierte Charts allein sagen nicht, welchen hörbaren Klang ein Charter im Einzelfall gemeint hat.
2. Die Trennung auf Stereo umstellen und mit unveränderter Notenlogik / neuem Prüfkorpus vergleichen. Damit lässt sich der Anteil der Mono-Vereinfachung messen.
3. Auf verlässlich isolierten Instrumentenspuren einen eigenen Anschlagszweig für sehr kurze und tonhöhenarme Strums prüfen. Dessen Zeitpunkte müssen direkt hörbare Instrumentenereignisse tragen; ein Beat-Raster darf sie nicht erfinden.
4. Die Spielgestaltung separat bewerten: Kontur, Wiederholung, Akkordformen und Belastung bei denselben akzeptierten Master-Ereignissen vergleichen. Dafür braucht es einen Spieltest; bloße Referenz-F1 reicht nicht.
5. Erst wenn Herkunft und Qualität der Labels belegt sind, ein lernendes Ereignis-/Pattern-Modell erwägen. Song-/Aufnahmegruppen müssen dabei vollständig zwischen Training, Validierung und Test getrennt bleiben.

Keine dieser Fortsetzungen wird anhand bereits betrachteter Holdout-Ergebnisse als neuer bestätigter Qualitätsgewinn ausgegeben. Änderungen benötigen neue, unabhängige Prüfsongs oder ausdrücklich einen neuen Entwicklungs-/Testzyklus.
