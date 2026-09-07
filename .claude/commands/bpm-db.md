Ergänze die bestehende Darstellung um zwei Monitor-Anzeigen auf den Boxen.

## Aufgabe

1. **Linke Box:** Monitor, der die aktuelle **BPM** anzeigt.
2. **Rechte Box:** Monitor, der die aktuellen **Dezibel** anzeigt.
3. Beide Werte werden **am Laptop gemessen** (lokale Audio-Eingabe).

## Bedingung

- Ist **kein Audio-Support vorhanden** (kein Mikrofon-/Audio-Zugriff verfügbar oder verweigert), werden **beide Monitore gar nicht angezeigt** — keine leeren Rahmen, keine Platzhalter, keine Null-Werte.
- Ist Audio-Support vorhanden, erscheinen beide Monitore mit den Live-Werten.

## Vorgehen

1. Prüfe zuerst im vorhandenen Code, wo die Boxen gerendert werden und ob bereits eine Audio-Analyse (BPM/Pegel) existiert — nutze Vorhandenes, statt Neues daneben zu bauen.
2. Beschreibe kurz, was du gefunden hast und wie du die Monitore anbindest.
3. Setze die Änderung um.
4. Verifiziere beide Fälle: mit Audio-Support (Werte sichtbar) und ohne (keine Monitore im DOM).

## Ausgabeformat

- **Befund:** wo die Boxen liegen, welche Audio-Quelle genutzt wird (Datei:Zeile)
- **Änderungen:** Liste der geänderten Dateien mit je einem Satz, was dort passiert
- **Verifikation:** wie die beiden Fälle geprüft wurden, mit dem tatsächlichen Ergebnis

## Regeln

- Keine erfundenen Werte: Wenn eine Messung nicht verfügbar ist, zeige den Monitor nicht, statt einen Schätzwert darzustellen.
- Erfinde keine zusätzlichen Funktionen (keine weiteren Anzeigen, Einstellungen oder Schwellenwerte), die hier nicht verlangt sind.
- Bestehende IDs, Handler und Layout-Struktur unangetastet lassen, soweit nicht zwingend nötig.
- Berichte ehrlich: Was nicht verifiziert wurde, wird als nicht verifiziert benannt.