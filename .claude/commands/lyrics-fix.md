Rolle: Du bist Debugging-Spezialist für die Lyrics-Highlighting-Funktion (Synchronisation von Songtext-Zeilen zur Wiedergabe) in diesem Projekt.

Problem: Manche Lyrics-Zeilen werden nie gehighlightet. Reproduzierbares Beispiel: bei "Smells Like Teen Spirit" bleibt die Zeile "a denial, a denial" ohne Highlight.

Auftrag: Finde die tatsächliche Ursache, behebe sie so, dass die Korrektur nicht nur diesen einen Song betrifft, sondern generisch für alle vorhandenen und zukünftig hinzugefügten Lieder greift.

Vorgehen:
1. Reproduzieren: Zeige, dass der Fehler bei "Smells Like Teen Spirit" / "a denial, a denial" auftritt, bevor du etwas änderst.
2. Ursache lokalisieren: Verfolge den Weg vom Lyrics-Datensatz über Parsing/Matching bis zur Highlight-Logik. Benenne die konkrete Stelle (Datei + Zeile) und erkläre den Mechanismus. Rate nicht — belege die Diagnose mit Code oder Ausgabe.
3. Betroffene Fälle bestimmen: Prüfe, welche anderen Songs/Zeilen dasselbe Muster treffen, statt anzunehmen, es sei ein Einzelfall.
4. Generisch fixen: Behebe die Ursache, keine Sonderfall-Behandlung für einzelne Songtitel oder -zeilen. Der Fix muss auch für später ergänzte Lieder greifen.
5. Verifizieren: Zeige nach dem Fix, dass die Beispielzeile gehighlightet wird und keine bisher funktionierende Zeile kaputtgegangen ist (Test oder ausgeführte Prüfung mit Ausgabe).

Regeln:
- Wenn du die Ursache nicht eindeutig belegen kannst, sag das explizit, statt einen plausibel klingenden Fix zu raten.
- Ändere nur, was zur Ursache gehört.
- Falls mehrere unabhängige Ursachen existieren, behandle sie einzeln und benenne sie getrennt.

Ausgabeformat:
## Reproduktion
Was beobachtet wurde, wie geprüft.

## Ursache
Datei:Zeile + Erklärung des Mechanismus, mit Beleg (Codeausschnitt oder Ausgabe).

## Betroffene Fälle
Welche weiteren Zeilen/Songs betroffen sind (oder: keine weiteren, mit Begründung).

## Fix
Was geändert wurde und warum das generisch wirkt.

## Verifikation
Ausgeführte Prüfungen mit tatsächlicher Ausgabe.

## Offene Punkte
Nicht Verifiziertes oder bewusst Ausgelassenes — falls nichts, "keine".