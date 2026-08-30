# beat-bytes — Gameplay-Mechanik-Spezifikation (Guitar-Hero-Modell)

> Referenzdokument, vom Nutzer am 2026-08-30 übergeben. Zweck:
> Vollständigkeitsprüfung von Implementierungsplänen. Jeder Punkt ist
> **[HARD]** (Regel) oder **[TUNE]** (kalibrierbarer Wert).
>
> Abgleich mit dem Ist-Stand: siehe die Notiz am Ende — einiges davon
> existiert bereits exakt so, einiges ist ein bewusstes Delta.

## 1. Zeit- und Positionsmodell

- **[HARD]** Charts sind **tickbasiert**, nicht sekundenbasiert. `resolution` = Ticks pro Viertelnote (üblich: 192 in `.chart`, 480 in `.mid`).
- **[HARD]** Ein **SyncTrack** hält BPM-Änderungen und Taktarten. Tick→Sekunde ist eine stückweise lineare Funktion über alle BPM-Segmente, **einmal vorberechnet**, nicht pro Frame integriert.
- **[HARD]** Sekunde→Tick wird für Editor, Scrubbing und Whammy gebraucht; beide Richtungen exakt invers.
- **[HARD]** **Drei** unabhängige Offsets: `chart_offset` (Audio-Start↔Tick 0), `audio_latency` (verschiebt das **Trefferfenster**), `video_latency` (verschiebt die **Darstellung**). Audio- und Video-Kalibrierung sind getrennt.
- **[HARD]** Maßgebliche Uhr ist die **Audio-Clock** (Sample-Position), nicht die Frame-Zeit; zwischen Callbacks wird interpoliert.
- **[TUNE]** Scroll-Speed ist rein visuell.

## 2. Chart-Datenmodell

```
Note { tick: u32, length: u32, fret: Fret, flags: NoteFlags }
Fret  = Green | Red | Yellow | Blue | Orange | Open
Flags = FORCE (Auto-Typ invertieren) | TAP | ...
```

- **[HARD]** Notes am gleichen Tick = **Chord** = ein Trefferobjekt.
- **[HARD]** `length == 0` → Einzelnote; `> 0` → Sustain.
- **[HARD]** Weitere Spuren: Star-Power-Phrasen, Solo-Marker, Sektionen, Lyrics optional.
- **[HARD]** Schwierigkeitsgrade sind **eigenständige Tracks** (Easy 3 Bünde, Medium 4, Hard/Expert 5).
- **[HARD]** Open Notes belegen die ganze Bahn, nie Teil eines Chords (hier: verbieten).

## 3. Note-Typ-Auflösung (Strum / HOPO / Tap)

Typ wird **beim Laden einmalig** aufgelöst, nicht zur Laufzeit.

- Auto-HOPO wenn ALLE gelten: Abstand zur Vorgängernote `< hopo_threshold` (Ticks) · kein Chord · Bund ≠ Vorgänger (bzw. nicht im Vorgänger-Chord) · es gibt eine Vorgängernote.
- **[TUNE]** `hopo_threshold` Standard **65/192** einer Viertelnote; Alternativen 1/8…1/32; pro Chart überschreibbar.
- **[HARD]** Threshold **exklusiv** (`<`, nicht `<=`).
- **[HARD]** `FORCE` **invertiert** das Auto-Ergebnis. Chord+Force → Chord-HOPO (Entscheidung dokumentieren).
- **[HARD]** `TAP`: kein Strum nötig UND keine Vorgänger-Bedingung; darf auch gestrummt werden. Unterschied zu HOPO: HOPO verlangt getroffene Vorgängernote.
- **[HARD]** Laufzeit: HOPO darf immer gestrummt werden · Vorgänger verfehlt → HOPO degradiert zu Strum · nach Overstrum ist die Kette gebrochen → nächste Note strummen.

## 4. Input-Modell

- **[HARD]** Bundzustand = **Bitmaske** (5 Bit), Zustandsabfrage statt Einzel-Events.
- **[HARD]** Strum = diskretes Event (Up/Down gleichwertig).
- **[HARD]** Whammy = Achse (0.0–1.0), pro Frame.
- **[HARD]** SP-Aktivierung: eigener Button (+ optional Tilt).
- **[HARD]** Bundwechsel während Sustain getrennt vom Note-Hit-Check auswerten.

## 5. Trefferlogik

- **[TUNE]** Fenster **±70 ms**, symmetrisch, konfigurierbar. **[HARD]** zeitbasiert (ms), nie tickbasiert.
- **[HARD]** **Note-Pointer**: nur die früheste ungespielte Note ist Kandidat; kein Skip. Fenster verpasst → Miss, Pointer weiter.
- **[HARD]** Einzelnote: geforderter Bund gedrückt und **kein höherer**; tiefere erlaubt (**Anchoring**). Chord: exakte Maske. Open: kein Bund.
- **[TUNE]** **Strum-Leniency**: Strum ~60 ms vor der Note wird gepuffert (kein Overstrum, wenn die Note noch kommt); umgekehrt Bund ~60 ms nach Strum.
- **[HARD]** Overstrum: Combo/Multiplier weg, Rock-Meter-Abzug, aktiver Sustain gedroppt; zählt NICHT in die Trefferquote.
- **[HARD]** **Anti-Ghosting** bei HOPO/Tap-Läufen; **[TUNE]** „Infinite Frets"-Option.

## 6. Sustains

- **[HARD]** Halten nach Treffer; Punkte kontinuierlich. **[TUNE]** ~25/Beat, Multiplier gilt.
- **[TUNE]** Drop-Leniency (~1/8 Beat) und End-Leniency (~50 ms).
- **[HARD]** **Extended Sustains** (Sustain läuft weiter, während auf anderem Bund neue Notes gespielt werden) sind der Normalfall. **Disjoint Chord Sustains**: pro Note eigener Hold-State.
- **[HARD]** Overstrum droppt alle aktiven Sustains. Whammy stört den Hold nicht.

## 7. Scoring

- **[TUNE]** 50 Punkte/Note (Chord = 50 × n). **[HARD]** Multiplier alle 10 Treffer bis 4×; Miss/Overstrum → 1×.
- **[HARD]** Streak zählt **Trefferobjekte** (Chord = 1), Punkte zählen **Notes** (Chord = n).
- **[HARD]** SP aktiv → ×2 (bis 8×). **[TUNE]** Solo-Bonus 100 × getroffene Notes.
- **[HARD]** Getrennt: `notes_hit`, `notes_total`, `max_streak`, `overstrums`; Quote = hit/total ohne Overstrums.
- **[TUNE]** Sterne (3–5, Gold) als Prozent eines Base-Scores.

## 8. Star Power

- **[HARD]** Phrasen: alle Notes treffen → +25 % (4 Phrasen = voll). Aktivierbar ab 50 %. Aktiv: ×2, Meter läuft **musiksynchron** leer (≈ 8 Takte **[TUNE]**). Whammy in SP-Phrasen lädt. Stacking mit Obergrenze. Rock Meter steigt schneller.

## 9. Rock Meter

- **[HARD]** Start ~50 %, steigt/fällt pro Treffer/Fehler; leer → Fail. **[TUNE]** Fall-Rate je Schwierigkeit; Easy oft ohne Fail. **[HARD]** No-Fail-Modus markiert Scoring als ungültig.

## 10. Edge-Case-Checkliste

| # | Fall | Erwartet |
|---|---|---|
| 1 | Erste Note im HOPO-Threshold zum Songstart | Kein HOPO |
| 2 | Zwei identische Bünde kurz nacheinander | Kein Auto-HOPO |
| 3 | HOPO nach verfehlter Note | Strummen |
| 4 | HOPO nach Overstrum | Strummen |
| 5 | Tap nach verfehlter Note | Ohne Strum treffbar |
| 6 | Chord mit Force | Chord-HOPO (dokumentieren) |
| 7 | Sustain + neue Note auf anderem Bund | Extended Sustain läuft |
| 8 | Chord-Sustain ungleicher Längen | Pro Note eigener Hold-State |
| 9 | Bund kurz losgelassen im Sustain | Drop-Leniency |
| 10 | Strum 50 ms vor der Note | Puffer, kein Overstrum |
| 11 | Extremer BPM-Wechsel | Fenster bleibt in ms konstant |
| 12 | Zwei Notes im selben Fenster | Nur die frühere, kein Skip |
| 13 | Open Note bei gehaltenen Bünden | Miss |
| 14 | Anchoring | Einzelnote gültig, Chord nicht |
| 15 | Ghost-Input im HOPO-Lauf | Kein Treffer, kein Miss |
| 16 | SP-Aktivierung im Sustain | Sustain bleibt, Multiplier springt |
| 17 | Pause / Fokusverlust | Clock + Input sauber resync |
| 18 | Negativer Chart-Offset | Notes vor Audio-Start rendern |

## 11. Chart-Editor

- **[HARD]** Editor nutzt die **identische** Typ-Auflösung (gemeinsame Bibliothek, keine Zweitimplementierung). Snap-Raster 1/1…1/64 + frei. Visuelle Kennzeichnung aller Typen. Force/Tap manuell + Anzeige des Auto-Ergebnisses. BPM-Anchors inkl. Tap-Tempo. Waveform, Scrubbing, Metronom. Undo/Redo. Validator vor Export. **[TUNE]** `.chart`/`.mid`-Import/Export.

## 12. Konstanten (zentral)

```rust
hit_window_ms:            70.0
strum_leniency_ms:        60.0
hopo_threshold_fraction:  65.0 / 192.0
sustain_drop_leniency_ms: 100.0
sustain_end_leniency_ms:  50.0
note_base_points:         50
sustain_points_per_beat:  25
multiplier_step:          10
multiplier_max:           4
sp_phrase_fill:           0.25
sp_activation_threshold:  0.50
sp_duration_beats:        32.0
```

Alle Werte in **einer** Config-Struktur, nichts hardcodiert im Trefferpfad.

---

## Abgleich mit dem Ist-Stand (2026-08-30, geprüft am Code)

**Existiert bereits so:** Anchoring exakt nach §5.3 (session.rs) ·
`TempoMap` mit BPM-Wechseln, vorberechnet · Score-Konstanten fast 1:1
(§7/§8/§12: 50 Punkte, 25/Beat, 10er-Multiplier bis 4×, Hype ×2, 25 %
Phrase, 50 % Aktivierung, 32 Beats) — zentral in `ScoreConfig` +
`TimingWindows`, per Drift-Test an die Doku gebunden · Chord = ein
Event · Overstrum getrennt von der Quote · Audio-Clock maßgeblich
(ADR-0004/0005) · HOPO mit Degradierung.

**Bewusste Deltas / offene Punkte:** Chart-Format v1 ist
**sekundenbasiert** (Tick-SyncTrack wäre Format v2 — vor dem
1.0-Freeze entscheiden) · EIN Latenz-Offset statt Audio/Video getrennt
· Tap ist globaler Modus, keine per-Note-Flags, kein FORCE · keine
Open Notes, kein Whammy · Rock Meter bewusst geparkt (Roadmap P3) ·
`sustain: Option<ActiveSustain>` = ein aktiver Sustain, keine
Extended/Disjoint Sustains · Strum-Puffer: nicht als eigener
Mechanismus gefunden, zu verifizieren.

© 2026 Martin Pfeffer | celox.io
