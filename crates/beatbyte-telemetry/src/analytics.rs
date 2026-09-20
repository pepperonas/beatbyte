//! The questions the store was shaped to answer.
//!
//! Every function here is a **read**. Nothing in this module changes
//! a chart, a score or a setting, and nothing in the game calls it:
//! analytics produce evidence for a person to act on, which is the
//! line ADR-0011 drew and ADR-0018 keeps.
//!
//! # What a single miss means
//!
//! Nothing. A note is a candidate for "too hard" only across many
//! sessions, and even then the reading is confounded by skill,
//! difficulty, device and calibration. Every function that ranks
//! notes therefore takes a minimum sample count and reports the one
//! it had, and [`NoteQuality::confidence`] exists so a caller cannot
//! quote a rate without also seeing how thin it is.
//!
//! Runs the autopilot drove and runs that used practice are excluded
//! from every query in this module: the first is perfect by
//! construction and the second was played at another speed.

use crate::model::{Completion, EventType};
use crate::store::Store;
use crate::{Error, Result};

/// The clause every analytical query shares. Written once so that a
/// new query cannot forget to exclude a perfect robot.
const HONEST_RUNS: &str = "s.autopilot = 0 AND s.practice = 0";

/// How a chart note has actually played, over many sessions.
///
/// The §21 note-quality signal: **computed, never written into the
/// chart**. A chart that carried its own quality score would be a
/// chart that argues with the next measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteQuality {
    /// Index into the played track's events.
    pub note_index: u32,
    /// How many times it was judged at all.
    pub samples: u32,
    /// How often it was hit, `0.0`–`1.0`.
    pub hit_rate: f64,
    /// The middle timing error in milliseconds, signed.
    pub median_offset_ms: f64,
    /// How spread out the timing was, in milliseconds.
    pub offset_spread_ms: f64,
    /// Overstrums that landed on this note as the nearest judged one.
    pub overstrums: u32,
}

impl NoteQuality {
    /// How much this row deserves to be believed, `0.0`–`1.0`.
    ///
    /// A saturating curve on the sample count, not a statistical
    /// confidence interval, and deliberately labelled as such: ten
    /// plays is a hint, fifty is a finding. Pure, so the shape can be
    /// pinned.
    #[must_use]
    pub fn confidence(&self) -> f64 {
        1.0 - (-f64::from(self.samples) / 20.0).exp()
    }
}

/// How every note of one chart version has played.
///
/// One query, one pass: the rows come back ordered by note so the
/// aggregation is a fold rather than a query per note.
pub fn note_quality(
    store: &Store,
    chart_hash: &str,
    difficulty: u8,
    min_samples: u32,
) -> Result<Vec<NoteQuality>> {
    let rows = store.query(
        "SELECT e.note_index, e.event_type, e.delta_us
           FROM gameplay_event e JOIN gameplay_session s USING (session_id)
          WHERE s.chart_hash = ?1 AND s.difficulty = ?2 AND e.note_index IS NOT NULL
            AND e.event_type IN (?3, ?4, ?5)
            AND s.autopilot = 0 AND s.practice = 0
          ORDER BY e.note_index",
        &[
            &chart_hash,
            &difficulty,
            &EventType::NoteHit.code(),
            &EventType::NoteMiss.code(),
            &EventType::Overstrum.code(),
        ],
        |row| {
            let note: u32 = row.get(0)?;
            let kind: u8 = row.get(1)?;
            let delta: Option<i32> = row.get(2)?;
            Ok((note, kind, delta))
        },
    )?;

    let mut out: Vec<NoteQuality> = Vec::new();
    let mut current: Option<(u32, Vec<i32>, u32, u32, u32)> = None;
    let mut finish = |note: u32, mut deltas: Vec<i32>, hits: u32, misses: u32, over: u32| {
        let samples = hits + misses;
        if samples < min_samples {
            return;
        }
        deltas.sort_unstable();
        out.push(NoteQuality {
            note_index: note,
            samples,
            hit_rate: f64::from(hits) / f64::from(samples.max(1)),
            median_offset_ms: median_ms(&deltas),
            offset_spread_ms: spread_ms(&deltas),
            overstrums: over,
        });
    };
    for (note, kind, delta) in rows {
        match current.as_mut() {
            Some((open, ..)) if *open == note => {}
            Some(_) => {
                if let Some((open, deltas, hits, misses, over)) = current.take() {
                    finish(open, deltas, hits, misses, over);
                }
                current = Some((note, Vec::new(), 0, 0, 0));
            }
            None => current = Some((note, Vec::new(), 0, 0, 0)),
        }
        if let Some((_, deltas, hits, misses, over)) = current.as_mut() {
            if kind == EventType::NoteHit.code() {
                *hits += 1;
                if let Some(delta) = delta {
                    deltas.push(delta);
                }
            } else if kind == EventType::NoteMiss.code() {
                *misses += 1;
            } else {
                *over += 1;
            }
        }
    }
    if let Some((note, deltas, hits, misses, over)) = current.take() {
        finish(note, deltas, hits, misses, over);
    }
    Ok(out)
}

/// The notes of one chart that are missed far more than the rest.
///
/// Sorted worst first, and never longer than the caller asked for —
/// the point is a short list a person can go and listen to.
pub fn problem_notes(
    store: &Store,
    chart_hash: &str,
    difficulty: u8,
    min_samples: u32,
    max_hit_rate: f64,
    limit: usize,
) -> Result<Vec<NoteQuality>> {
    let mut notes: Vec<NoteQuality> = note_quality(store, chart_hash, difficulty, min_samples)?
        .into_iter()
        .filter(|note| note.hit_rate <= max_hit_rate)
        .collect();
    notes.sort_by(|left, right| {
        left.hit_rate
            .partial_cmp(&right.hit_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(right.samples.cmp(&left.samples))
    });
    notes.truncate(limit);
    Ok(notes)
}

/// How one generator version's charts have played.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratorRow {
    /// The generator this row is about (`None` = an import's own
    /// first draft, which has no designer).
    pub generator: Option<String>,
    /// Distinct runs.
    pub sessions: u32,
    /// Notes judged across them.
    pub judged: u32,
    /// How often a judged note was missed, `0.0`–`1.0`.
    pub miss_rate: f64,
    /// Mean absolute timing error in milliseconds.
    pub mean_abs_ms: f64,
    /// How often a run reached the end, `0.0`–`1.0`.
    pub completion_rate: f64,
}

/// Compare generator versions on comparable material.
///
/// `genre` and `difficulty` are the controls: comparing a house track
/// on easy against a metal track on expert measures the songs, not
/// the generator.
pub fn generator_comparison(
    store: &Store,
    genre: Option<&str>,
    difficulty: u8,
) -> Result<Vec<GeneratorRow>> {
    let sql = format!(
        "SELECT s.generator_version,
                COUNT(DISTINCT s.session_id)                          AS sessions,
                COUNT(*)                                              AS judged,
                SUM(CASE WHEN e.event_type = ?1 THEN 1 ELSE 0 END)    AS misses,
                AVG(ABS(COALESCE(e.delta_us, 0)))                     AS mean_abs_us,
                SUM(CASE WHEN s.completion = ?2 THEN 1 ELSE 0 END)    AS completed_rows
           FROM gameplay_session s JOIN gameplay_event e USING (session_id)
          WHERE s.difficulty = ?3 AND {HONEST_RUNS}
            AND e.event_type IN (?4, ?1)
            AND (?5 IS NULL OR s.genre = ?5)
          GROUP BY s.generator_version
          ORDER BY sessions DESC"
    );
    store.query(
        &sql,
        &[
            &EventType::NoteMiss.code(),
            &Completion::Completed.code(),
            &difficulty,
            &EventType::NoteHit.code(),
            &genre,
        ],
        |row| {
            let generator: Option<String> = row.get(0)?;
            let sessions: u32 = row.get(1)?;
            let judged: u32 = row.get(2)?;
            let misses: u32 = row.get(3)?;
            let mean_abs_us: Option<f64> = row.get(4)?;
            let completed_rows: u32 = row.get(5)?;
            Ok(GeneratorRow {
                generator,
                sessions,
                judged,
                miss_rate: f64::from(misses) / f64::from(judged.max(1)),
                mean_abs_ms: mean_abs_us.unwrap_or(0.0) / 1000.0,
                // `completed_rows` counts EVENTS of completed
                // sessions, so the rate is per event; dividing by the
                // judged total makes it the share of judgments that
                // happened inside a finished run, which is what
                // "completion" means for a chart.
                completion_rate: f64::from(completed_rows) / f64::from(judged.max(1)),
            })
        },
    )
}

/// One device-and-offset combination a player has played under.
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationRow {
    /// What they were holding.
    pub device: crate::model::InputDevice,
    /// The offset in force, in milliseconds.
    pub offset_ms: Option<f32>,
    /// How many hits the reading is from.
    pub hits: u32,
    /// The middle timing error in milliseconds, signed.
    pub median_offset_ms: f64,
}

impl CalibrationRow {
    /// The offset that would centre this player's hits, in
    /// milliseconds — a **suggestion**, never applied automatically.
    ///
    /// A positive median means the hits land late, so the offset that
    /// was in force needs to grow by that much. §24: calibration is
    /// never changed from a handful of events, so a caller is
    /// expected to check [`CalibrationRow::hits`] first.
    #[must_use]
    pub fn suggested_offset_ms(&self) -> f64 {
        f64::from(self.offset_ms.unwrap_or(0.0)) + self.median_offset_ms
    }
}

/// Whether a player is consistently early or late.
///
/// Grouped by device and by the offset that was in force, because
/// mixing two calibrations into one median measures neither.
pub fn calibration_bias(
    store: &Store,
    player_id: Option<u64>,
    min_hits: u32,
) -> Result<Vec<CalibrationRow>> {
    let groups = store.query(
        "SELECT s.input_device, s.input_offset_ms, COUNT(*)
           FROM gameplay_event e JOIN gameplay_session s USING (session_id)
          WHERE e.event_type = ?1 AND e.delta_us IS NOT NULL
            AND s.autopilot = 0 AND s.practice = 0
            AND (?2 IS NULL OR s.player_id = ?2)
          GROUP BY s.input_device, s.input_offset_ms
         HAVING COUNT(*) >= ?3",
        &[
            &EventType::NoteHit.code(),
            &player_id.map(|id| id as i64),
            &min_hits,
        ],
        |row| {
            let device: u8 = row.get(0)?;
            let offset: Option<f64> = row.get(1)?;
            let hits: u32 = row.get(2)?;
            Ok((device, offset, hits))
        },
    )?;

    let mut out = Vec::new();
    for (device, offset, hits) in groups {
        // The median, exactly, by asking for the middle row. SQLite
        // has no percentile function and an average is the wrong
        // statistic here: one badly late hit moves a mean and does
        // not move a median.
        let middle = store.query(
            "SELECT e.delta_us
               FROM gameplay_event e JOIN gameplay_session s USING (session_id)
              WHERE e.event_type = ?1 AND e.delta_us IS NOT NULL
                AND s.autopilot = 0 AND s.practice = 0
                AND s.input_device = ?2
                AND (?3 IS NULL OR s.input_offset_ms = ?3)
                AND (?4 IS NULL OR s.player_id = ?4)
              ORDER BY e.delta_us
              LIMIT 1 OFFSET ?5",
            &[
                &EventType::NoteHit.code(),
                &device,
                &offset,
                &player_id.map(|id| id as i64),
                &(i64::from(hits / 2)),
            ],
            |row| row.get::<_, i32>(0),
        )?;
        out.push(CalibrationRow {
            device: crate::model::InputDevice::from_code(device),
            offset_ms: offset.map(|value| value as f32),
            hits,
            median_offset_ms: f64::from(middle.first().copied().unwrap_or(0)) / 1000.0,
        });
    }
    Ok(out)
}

/// A device and how often its inputs reached the engine and did
/// nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct OrphanRow {
    /// The device.
    pub device: crate::model::InputDevice,
    /// Strums that produced neither a hit nor an overstrum.
    pub orphan_strums: u32,
    /// Strums in total, so the share is readable.
    pub strums: u32,
}

/// Strums that vanished: the input reached the session and the engine
/// produced nothing at all.
///
/// This is the §23 question — a player problem, a controller problem
/// and a mapping problem look identical from the score screen and
/// different from here. It can only be asked of runs recorded at
/// [`crate::model::Detail::Actions`] or above; a run without the
/// action stream simply contributes no strums.
pub fn orphan_strums(store: &Store) -> Result<Vec<OrphanRow>> {
    store.query(
        "SELECT s.input_device,
                SUM(CASE WHEN NOT EXISTS (
                      SELECT 1 FROM gameplay_event r
                       WHERE r.session_id = a.session_id
                         AND r.event_type IN (?2, ?3)
                         AND r.sequence > a.sequence
                         AND r.sequence <= a.sequence + 3
                    ) THEN 1 ELSE 0 END) AS orphans,
                COUNT(*)                 AS strums
           FROM gameplay_event a JOIN gameplay_session s USING (session_id)
          WHERE a.event_type = ?1 AND a.action = ?4
            AND s.autopilot = 0 AND s.practice = 0
          GROUP BY s.input_device",
        &[
            &EventType::Action.code(),
            &EventType::NoteHit.code(),
            &EventType::Overstrum.code(),
            &crate::model::Action::Strum.code(),
        ],
        |row| {
            let device: u8 = row.get(0)?;
            Ok(OrphanRow {
                device: crate::model::InputDevice::from_code(device),
                orphan_strums: row.get(1)?,
                strums: row.get(2)?,
            })
        },
    )
}

/// A histogram of timing errors, in milliseconds per bucket.
///
/// The shape is the diagnosis: a clean bell centred off zero is
/// calibration, two humps is two devices, a long tail is a passage
/// nobody reads in time.
pub fn timing_histogram(
    store: &Store,
    chart_hash: Option<&str>,
    bucket_ms: u32,
) -> Result<Vec<(i32, u32)>> {
    let bucket_us = i64::from(bucket_ms.max(1)) * 1000;
    store.query(
        "SELECT CAST(e.delta_us / ?1 AS INTEGER) AS bucket, COUNT(*)
           FROM gameplay_event e JOIN gameplay_session s USING (session_id)
          WHERE e.event_type = ?2 AND e.delta_us IS NOT NULL
            AND s.autopilot = 0 AND s.practice = 0
            AND (?3 IS NULL OR s.chart_hash = ?3)
          GROUP BY bucket ORDER BY bucket",
        &[&bucket_us, &EventType::NoteHit.code(), &chart_hash],
        |row| {
            let bucket: i32 = row.get(0)?;
            let count: u32 = row.get(1)?;
            Ok((bucket * bucket_ms as i32, count))
        },
    )
}

/// Sessions whose recording is known to have a hole in it.
///
/// A reader that filters on this is reading whole runs; a reader that
/// does not at least knows the number exists.
pub fn incomplete_sessions(store: &Store) -> Result<Vec<(String, u32)>> {
    store.query(
        "SELECT uid, dropped_events FROM gameplay_session
          WHERE telemetry_complete = 0 ORDER BY started_ms DESC",
        &[],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
}

/// The middle of a sorted slice of microsecond offsets, in
/// milliseconds.
fn median_ms(sorted: &[i32]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let middle = sorted.len() / 2;
    let micros = if sorted.len() % 2 == 1 {
        f64::from(sorted[middle])
    } else {
        (f64::from(sorted[middle - 1]) + f64::from(sorted[middle])) / 2.0
    };
    micros / 1000.0
}

/// The spread of a sorted slice: the middle 80 %, in milliseconds.
///
/// Not a standard deviation — timing errors are not normal and one
/// hit taken during a sneeze should not widen the number by a third.
fn spread_ms(sorted: &[i32]) -> f64 {
    if sorted.len() < 2 {
        return 0.0;
    }
    let low = sorted[sorted.len() / 10];
    let high = sorted[(sorted.len() * 9 / 10).min(sorted.len() - 1)];
    f64::from(high - low) / 1000.0
}

/// Turn a database error into a readable one at a call site that has
/// no better context to add.
#[must_use]
pub fn describe(error: &Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Action, Detail, Event, InputDevice, Outcome, Provenance, Rating, SessionRow, micros,
    };
    use crate::schema;
    use crate::store::SessionId;

    fn session(uid: &str, generator: Option<&str>, device: InputDevice, offset: f32) -> SessionRow {
        SessionRow {
            uid: uid.to_owned(),
            started_ms: 1_700_000_000_000,
            title: "Maria".to_owned(),
            artist: "Blondie".to_owned(),
            genre: Some("rock".to_owned()),
            chart_hash: "chart-a".to_owned(),
            chart_file: None,
            difficulty: 1,
            player_slot: 0,
            player_id: Some(1),
            provenance: Provenance {
                game: "0.18.0".to_owned(),
                chart_format: 1,
                generator: generator.map(str::to_owned),
                scoring: 1,
                analysis: None,
                vocal: None,
            },
            telemetry_schema: schema::schema_version(),
            detail: Detail::Actions,
            input_device: device,
            input_offset_ms: Some(offset),
            video_offset_ms: Some(0.0),
            mic_offset_ms: None,
            tap_mode: true,
            no_fail: true,
            practice: false,
            autopilot: false,
            notes_total: 4,
        }
    }

    /// Four plays of the same chart: note 2 is missed by everyone,
    /// note 0 is hit by everyone.
    fn a_played_chart() -> (Store, Vec<SessionId>) {
        let mut store = Store::open_in_memory().expect("a store");
        let mut ids = Vec::new();
        for run in 0..4u32 {
            let id = store
                .begin(&session(
                    &format!("run{run}"),
                    Some("v17"),
                    InputDevice::Keyboard,
                    -10.0,
                ))
                .expect("begins");
            let mut events = Vec::new();
            let mut sequence = 0u32;
            for note in 0..4u32 {
                sequence += 1;
                // Note 2 is always missed; note 3 half the time.
                let missed = note == 2 || (note == 3 && run % 2 == 0);
                let event = if missed {
                    Event::new(EventType::NoteMiss, micros(f64::from(note))).about(note as usize)
                } else {
                    Event::new(EventType::NoteHit, micros(f64::from(note)))
                        .about(note as usize)
                        .off_by(0.012 + f64::from(run) * 0.001)
                        .judged(Rating::Great)
                };
                events.push((sequence, event));
            }
            store.append(id, &events).expect("appends");
            store
                .finish(
                    id,
                    Outcome {
                        ended_ms: 1_700_000_100_000,
                        completion: Completion::Completed,
                        dropped: 0,
                    },
                )
                .expect("finishes");
            ids.push(id);
        }
        (store, ids)
    }

    #[test]
    fn a_note_everybody_misses_rises_to_the_top() {
        let (store, _) = a_played_chart();
        let ranked = problem_notes(&store, "chart-a", 1, 3, 0.6, 10).expect("ranks");
        assert_eq!(ranked.len(), 2, "two notes are under the threshold");
        assert_eq!(ranked[0].note_index, 2, "the one nobody hits comes first");
        assert!((ranked[0].hit_rate - 0.0).abs() < 1e-9);
        assert_eq!(ranked[0].samples, 4);
        assert_eq!(ranked[1].note_index, 3);
        assert!((ranked[1].hit_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_note_with_too_little_evidence_is_not_reported_at_all() {
        let (store, _) = a_played_chart();
        let ranked = problem_notes(&store, "chart-a", 1, 99, 1.0, 10).expect("ranks");
        assert!(
            ranked.is_empty(),
            "four plays are not ninety-nine, and a thin reading must \
             not be presented as a finding"
        );
    }

    #[test]
    fn the_autopilot_is_kept_out_of_every_reading() {
        let (mut store, _) = a_played_chart();
        let mut robot = session("robot", Some("v17"), InputDevice::Keyboard, -10.0);
        robot.autopilot = true;
        let id = store.begin(&robot).expect("begins");
        // The robot hits the note nobody else can.
        store
            .append(
                id,
                &(0..40u32)
                    .map(|index| {
                        (
                            index,
                            Event::new(EventType::NoteHit, micros(2.0))
                                .about(2)
                                .off_by(0.0)
                                .judged(Rating::Perfect),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .expect("appends");
        let ranked = problem_notes(&store, "chart-a", 1, 3, 0.6, 10).expect("ranks");
        assert_eq!(
            ranked.first().map(|note| note.note_index),
            Some(2),
            "a chart that a robot plays perfectly is not an easy chart"
        );
        assert_eq!(ranked[0].samples, 4, "and the robot's plays do not count");
    }

    #[test]
    fn a_generator_is_compared_on_comparable_material() {
        let (mut store, _) = a_played_chart();
        // A second generator on the same genre and difficulty, which
        // never misses.
        let id = store
            .begin(&session("v18", Some("v18"), InputDevice::Keyboard, -10.0))
            .expect("begins");
        store
            .append(
                id,
                &(0..4u32)
                    .map(|note| {
                        (
                            note,
                            Event::new(EventType::NoteHit, micros(f64::from(note)))
                                .about(note as usize)
                                .off_by(0.002)
                                .judged(Rating::Perfect),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .expect("appends");
        let rows = generator_comparison(&store, Some("rock"), 1).expect("compares");
        let v17 = rows
            .iter()
            .find(|row| row.generator.as_deref() == Some("v17"))
            .expect("v17 is there");
        let v18 = rows
            .iter()
            .find(|row| row.generator.as_deref() == Some("v18"))
            .expect("v18 is there");
        assert!(v18.miss_rate < v17.miss_rate);
        assert!(v18.mean_abs_ms < v17.mean_abs_ms);
        assert_eq!(v17.sessions, 4);
        assert_eq!(v18.sessions, 1);
        // A genre nobody played returns nothing rather than everything.
        assert!(
            generator_comparison(&store, Some("polka"), 1)
                .expect("compares")
                .is_empty()
        );
    }

    #[test]
    fn a_constant_bias_shows_up_as_a_median_and_a_suggestion() {
        let (store, _) = a_played_chart();
        let rows = calibration_bias(&store, Some(1), 4).expect("reads");
        assert_eq!(rows.len(), 1, "one device, one offset");
        let row = &rows[0];
        assert_eq!(row.device, InputDevice::Keyboard);
        assert_eq!(row.hits, 10);
        assert!(
            (12.0..=16.0).contains(&row.median_offset_ms),
            "every hit was 12–15 ms late: {}",
            row.median_offset_ms
        );
        assert!(
            row.suggested_offset_ms() > f64::from(row.offset_ms.unwrap_or(0.0)),
            "hits that land late need a larger offset, not a smaller one"
        );
        assert!(
            calibration_bias(&store, Some(1), 1000)
                .expect("reads")
                .is_empty(),
            "a bias is not reported from a handful of notes"
        );
        assert!(
            calibration_bias(&store, Some(99), 1)
                .expect("reads")
                .is_empty(),
            "and it is per player"
        );
    }

    #[test]
    fn a_strum_that_did_nothing_is_visible_and_one_that_hit_is_not() {
        let mut store = Store::open_in_memory().expect("a store");
        let id = store
            .begin(&session("run", None, InputDevice::Guitar, 0.0))
            .expect("begins");
        store
            .append(
                id,
                &[
                    // A strum that hit.
                    (
                        1,
                        Event::new(EventType::Action, micros(1.0)).acting(Action::Strum),
                    ),
                    (
                        2,
                        Event::new(EventType::NoteHit, micros(1.0))
                            .about(0)
                            .judged(Rating::Perfect),
                    ),
                    // A strum that overstrummed — the engine reacted.
                    (
                        3,
                        Event::new(EventType::Action, micros(2.0)).acting(Action::Strum),
                    ),
                    (4, Event::new(EventType::Overstrum, micros(2.0))),
                    // A strum that vanished.
                    (
                        5,
                        Event::new(EventType::Action, micros(3.0)).acting(Action::Strum),
                    ),
                    (
                        6,
                        Event::new(EventType::Action, micros(3.1)).acting(Action::Strum),
                    ),
                ],
            )
            .expect("appends");
        let rows = orphan_strums(&store).expect("reads");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].device, InputDevice::Guitar);
        assert_eq!(rows[0].strums, 4);
        assert_eq!(
            rows[0].orphan_strums, 2,
            "two strums reached the engine and produced nothing: {rows:?}"
        );
    }

    #[test]
    fn the_timing_histogram_buckets_where_the_hits_landed() {
        let (store, _) = a_played_chart();
        let histogram = timing_histogram(&store, Some("chart-a"), 10).expect("reads");
        assert!(!histogram.is_empty());
        let total: u32 = histogram.iter().map(|(_, count)| count).sum();
        assert_eq!(total, 10, "every hit is in exactly one bucket");
        assert!(
            histogram.iter().all(|(edge, _)| *edge >= 10),
            "12–15 ms late belongs in the +10 ms bucket, not around \
             zero: {histogram:?}"
        );
    }

    #[test]
    fn a_run_with_a_hole_can_be_found_again() {
        let mut store = Store::open_in_memory().expect("a store");
        let id = store
            .begin(&session("holey", None, InputDevice::Keyboard, 0.0))
            .expect("begins");
        store
            .finish(
                id,
                Outcome {
                    ended_ms: 1,
                    completion: Completion::Completed,
                    dropped: 12,
                },
            )
            .expect("finishes");
        let rows = incomplete_sessions(&store).expect("reads");
        assert_eq!(rows, vec![("holey".to_owned(), 12)]);
    }

    #[test]
    fn confidence_grows_with_evidence_and_never_reaches_certainty() {
        let quality = |samples| NoteQuality {
            note_index: 0,
            samples,
            hit_rate: 0.5,
            median_offset_ms: 0.0,
            offset_spread_ms: 0.0,
            overstrums: 0,
        };
        assert!(quality(1).confidence() < 0.1);
        assert!(quality(20).confidence() > 0.5);
        assert!(quality(100).confidence() > 0.99);
        assert!(
            quality(u32::MAX).confidence() < 1.0 + f64::EPSILON,
            "a saturating curve, not a promise"
        );
        assert!(quality(5).confidence() < quality(50).confidence());
    }

    #[test]
    fn the_middle_and_the_spread_are_the_middle_and_the_spread() {
        assert!((median_ms(&[1000, 2000, 3000]) - 2.0).abs() < 1e-9);
        assert!((median_ms(&[1000, 3000]) - 2.0).abs() < 1e-9);
        assert!((median_ms(&[]) - 0.0).abs() < 1e-9);
        // One wild value moves a mean and must not move the middle.
        let mut wild: Vec<i32> = (0..100).map(|index| index * 100).collect();
        wild.push(9_000_000);
        wild.sort_unstable();
        assert!((median_ms(&wild) - 5.0).abs() < 0.2, "{}", median_ms(&wild));
        assert!(spread_ms(&wild) < 10.0, "the middle 80 % is not the tail");
        assert!((spread_ms(&[42]) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn an_error_describes_itself() {
        let error = Error::Import("no header".to_owned());
        assert!(describe(&error).contains("no header"));
    }
}
