//! Reading beat grids out of a Rekordbox XML export.
//!
//! Rekordbox writes a `<TEMPO Inizio="…" Bpm="…" Battito="…"/>` per
//! grid marker inside each `<TRACK>`: `Inizio` is the marker's time
//! in seconds, `Bpm` the tempo from there on, `Battito` the beat's
//! position in the bar (1 = downbeat). That is everything a grid
//! needs, and it is the format a DJ already has for this material.
//!
//! Parsed with a small scanner rather than an XML crate: three
//! attributes off one element type does not justify a dependency,
//! and the workspace keeps its dependency list short on purpose.
//!
//! ⚠️ Only what the file states is used. Nothing is inferred about
//! markers the export does not contain.

use crate::eval::GroundTruth;

/// One grid marker as Rekordbox states it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marker {
    /// Seconds into the track.
    pub start_s: f64,
    /// Tempo from this marker on.
    pub bpm: f64,
    /// Position in the bar, 1–4 (1 = downbeat).
    pub beat_in_bar: u8,
}

/// Pull one attribute out of an element's text. Pure — tested.
#[must_use]
pub fn attribute(element: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = element.find(&needle)? + needle.len();
    let rest = &element[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

/// Every `<TEMPO …>` marker in a Rekordbox XML document, in file
/// order. Malformed markers are skipped rather than fatal — an
/// export with one bad row should still yield a usable grid.
/// Pure — tested.
#[must_use]
pub fn markers(xml: &str) -> Vec<Marker> {
    let mut found = Vec::new();
    for chunk in xml.split("<TEMPO").skip(1) {
        let Some(end) = chunk.find('>') else { continue };
        let element = &chunk[..end];
        let (Some(start), Some(bpm)) = (
            attribute(element, "Inizio").and_then(|v| v.parse::<f64>().ok()),
            attribute(element, "Bpm").and_then(|v| v.parse::<f64>().ok()),
        ) else {
            continue;
        };
        if !start.is_finite() || !bpm.is_finite() || bpm <= 0.0 {
            continue;
        }
        found.push(Marker {
            start_s: start,
            bpm,
            beat_in_bar: attribute(element, "Battito")
                .and_then(|v| v.parse::<u8>().ok())
                .unwrap_or(1),
        });
    }
    found
}

/// Expand markers into a full grid up to `duration_s`.
///
/// Each marker's tempo runs until the next one — Rekordbox's own
/// semantics. Downbeats are derived from `Battito`: the marker
/// states which beat of the bar it sits on, so bar 1 is a count
/// away, never a guess. Pure — tested.
#[must_use]
pub fn grid(markers: &[Marker], duration_s: f64) -> Option<GroundTruth> {
    let first = markers.first()?;
    let mut beats = Vec::new();
    let mut downbeats = Vec::new();
    for (index, marker) in markers.iter().enumerate() {
        let until = markers
            .get(index + 1)
            .map_or(duration_s, |next| next.start_s.min(duration_s));
        let period = 60.0 / marker.bpm;
        // `Battito` counts from 1; a marker on beat 3 means the next
        // downbeat is two beats away.
        let mut position = i64::from(marker.beat_in_bar.clamp(1, 4)) - 1;
        let mut time = marker.start_s;
        while time < until {
            beats.push(time);
            if position.rem_euclid(4) == 0 {
                downbeats.push(time);
            }
            position += 1;
            time += period;
        }
    }
    if beats.is_empty() {
        return None;
    }
    Some(GroundTruth {
        bpm: first.bpm,
        first_downbeat_ms: downbeats.first().copied().unwrap_or(first.start_s) * 1000.0,
        beats,
        downbeats,
        boundaries: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic export — the shape Rekordbox writes, with made-up
    /// numbers. No real library file is checked in.
    const EXPORT: &str = r#"<DJ_PLAYLISTS><COLLECTION>
        <TRACK Name="Synthetic" AverageBpm="125.00">
            <TEMPO Inizio="0.500" Bpm="125.00" Metro="4/4" Battito="1"/>
        </TRACK></COLLECTION></DJ_PLAYLISTS>"#;

    #[test]
    fn a_marker_is_read_exactly_as_written() {
        let found = markers(EXPORT);
        assert_eq!(found.len(), 1);
        assert!((found[0].start_s - 0.5).abs() < 1e-9);
        assert!((found[0].bpm - 125.0).abs() < 1e-9);
        assert_eq!(found[0].beat_in_bar, 1);
    }

    #[test]
    fn the_grid_starts_where_the_marker_says() {
        let truth = grid(&markers(EXPORT), 10.0).expect("a grid");
        assert!((truth.beats[0] - 0.5).abs() < 1e-9);
        // 125 BPM = 0.48 s per beat.
        assert!((truth.beats[1] - 0.98).abs() < 1e-9);
        // Battito 1 = this marker IS a downbeat, and every fourth
        // beat after it.
        assert!((truth.downbeats[0] - 0.5).abs() < 1e-9);
        assert!((truth.downbeats[1] - (0.5 + 4.0 * 0.48)).abs() < 1e-9);
    }

    #[test]
    fn battito_places_bar_one_without_guessing() {
        // A marker on beat 3 means the downbeat is two beats later —
        // the whole reason the attribute is read instead of assuming
        // every marker is a bar line.
        let xml = r#"<TEMPO Inizio="0.000" Bpm="120.00" Battito="3"/>"#;
        let truth = grid(&markers(xml), 4.0).expect("a grid");
        assert!((truth.downbeats[0] - 1.0).abs() < 1e-9, "two beats in");
    }

    #[test]
    fn a_damaged_row_costs_only_itself() {
        let xml = r#"<TEMPO Inizio="oops" Bpm="125"/>
                     <TEMPO Inizio="1.0" Bpm="125" Battito="1"/>
                     <TEMPO Inizio="2.0" Bpm="0"/>"#;
        assert_eq!(markers(xml).len(), 1, "only the intact marker survives");
    }

    #[test]
    fn a_tempo_change_switches_period_at_the_marker() {
        let xml = r#"<TEMPO Inizio="0.0" Bpm="120.00" Battito="1"/>
                     <TEMPO Inizio="2.0" Bpm="60.00" Battito="1"/>"#;
        let truth = grid(&markers(xml), 4.0).expect("a grid");
        // 0.0,0.5,1.0,1.5 at 120, then 2.0,3.0 at 60.
        assert!(truth.beats.contains(&1.5));
        assert!(truth.beats.contains(&3.0));
        assert!(
            !truth.beats.contains(&2.5),
            "the second tempo rules after 2 s"
        );
    }
}
