## Rolle

Du arbeitest am bestehenden Smart-Home-Stack auf **raspi5**. Die App **dB-Analyse** (`/app/disco/stats`, im Repo `disco-controller`, `public/stats.html`) misst Pegel und steuert bereits Lightstrips über einen Schwellwert-Mechanismus. Du erweiterst genau diesen vorhandenen Mechanismus — du baust keinen zweiten daneben.

## Ziel

Die Bühnenbeleuchtung soll bei **Schwellwert-Überschreitung highlighten** (kurz stärker leuchten). Zusätzlich bekommen **Bühne, Zuschauer und Boxen** je einen Lightstrip, der immer mal wieder mit einem **Comet-** oder **Snizzle-/Glimmer-Effekt weiß strahlt**. Die Schwellwert-Erkennung soll **dynamisch** laufen.

## Schrittfolge

1. **Erst lesen, dann bauen.** Sieh dir den bestehenden Schwellwert- und Strip-Ansteuerungspfad der dB-Analyse an (Erkennung → Flanke → Ausgabe an den Strip) und beschreibe ihn kurz, bevor du etwas änderst. Nenne dabei die konkreten Dateien und Funktionen, an denen du ansetzt.
2. **Offene Punkte benennen.** Was aus der Anforderung nicht eindeutig aus dem Code hervorgeht (z. B. wie „Bühne", „Zuschauer" und „Boxen" physisch/logisch auf Strips oder LED-Bereiche abgebildet werden, ob neue Hardware dazukommt oder vorhandene Strips segmentiert werden), fragst du am Ende gebündelt nach. Blockiere nicht: arbeite alles ab, was ohne diese Antworten machbar ist, und benenne deine Annahmen explizit.
3. **Highlight bei Überschreitung** an den bestehenden Schwellwert-Mechanismus anhängen (Bühnenbeleuchtung leuchtet kurz stärker).
4. **Weiß-Effekte** (Comet und Snizzle/Glimmer) für Bühne, Zuschauer und Boxen ergänzen — sporadisch, nicht dauerhaft.
5. **Dynamische Schwellwert-Erkennung** umsetzen; orientiere dich dabei an dem, was die dB-Analyse dafür bereits tut, statt eine eigene Logik zu erfinden.
6. **Verifizieren.** Vorhandene Tests laufen lassen; live gegen die echte Kette prüfen, soweit möglich. Berichte, was du tatsächlich gemessen hast, und was du nicht prüfen konntest.

## Regeln

- **Keine Behauptungen ohne Beleg.** Wenn du eine Datei, Funktion, Route oder Konfigurationsoption nennst, hast du sie vorher gelesen. Rate keine Pfade, Endpunkte oder Feldnamen.
- **Bestehendes Verhalten bleibt unangetastet**, soweit die Anforderung es nicht ausdrücklich ändert. Neue Felder additiv; ohne die neuen Effekte verhält sich die Kette wie vorher.
- Alles, was du nicht verifizieren konntest, wird als solches gekennzeichnet — nicht als erledigt gemeldet.

## Ausgabeformat

1. **Bestandsaufnahme** — der vorhandene Mechanismus in wenigen Sätzen, mit Datei- und Funktionsangaben (`pfad/datei.py:zeile`).
2. **Plan** — was wo geändert wird, je Punkt eine Zeile.
3. **Umsetzung** — die Änderungen.
4. **Verifikation** — was gemessen/getestet wurde, mit Ergebnis; getrennt davon, was ungeprüft blieb.
5. **Annahmen & offene Fragen** — gebündelt am Ende.