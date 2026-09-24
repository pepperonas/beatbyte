//! Filling the store with a plausible amount of playing, and timing
//! what that does to it.
//!
//! The point is not a number to put on a badge. It is the question
//! ADR-0018 has to be able to answer before anybody plays a thousand
//! songs into this: **does it still work at the size it will actually
//! reach, and how big does it get?**
//!
//! The data is generated to look like the real corpus rather than
//! like a loop: a set of charts, sessions spread over them, and per
//! session a mix of actions, hits, misses, sustains and overstrums in
//! the proportions three measured autopilot runs showed. It is
//! deterministic — the same seed gives the same database — so two
//! runs of the benchmark compare.

use std::time::Instant;

use crate::model::{
    Completion, Detail, Event, EventType, Flags, InputDevice, Outcome, Provenance, Rating,
    SessionRow, micros,
};
use crate::store::Store;
use crate::writer::BATCH_EVENTS;
use crate::{Result, analytics, schema, session_uid};

/// What one benchmark run measured.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchReport {
    /// Sessions written.
    pub sessions: u64,
    /// Events written.
    pub events: u64,
    /// How long the whole fill took, in seconds.
    pub insert_s: f64,
    /// The mean time one batch commit took, in milliseconds.
    pub batch_ms: f64,
    /// The database's size afterwards.
    pub bytes: u64,
    /// Each timed query: what it was, how long it took in
    /// milliseconds, and how many rows it produced.
    pub queries: Vec<(String, f64, usize)>,
}

impl BenchReport {
    /// Events per second during the fill.
    #[must_use]
    pub fn events_per_second(&self) -> f64 {
        if self.insert_s <= 0.0 {
            return 0.0;
        }
        self.events as f64 / self.insert_s
    }

    /// Bytes per stored event.
    #[must_use]
    pub fn bytes_per_event(&self) -> f64 {
        if self.events == 0 {
            return 0.0;
        }
        self.bytes as f64 / self.events as f64
    }
}

/// A tiny deterministic generator — splitmix64, four lines of
/// arithmetic and no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 { 0 } else { self.next() % bound }
    }
}

/// Fill a store with `sessions` runs spread over `charts` charts.
///
/// Returns how many events went in and how long the fill took.
pub fn fill(
    store: &mut Store,
    sessions: usize,
    notes_per_session: usize,
    charts: usize,
    seed: u64,
) -> Result<(u64, f64, f64)> {
    let mut rng = Rng(seed);
    let started = Instant::now();
    let mut written = 0u64;
    let mut batches = 0u64;
    let mut batch_total = 0f64;
    for index in 0..sessions {
        let chart = index % charts.max(1);
        let row = bench_session(index, chart, notes_per_session, charts.max(1));
        let id = store.begin(&row)?;
        let mut sequence = 0u32;
        let mut pending: Vec<(u32, Event)> = Vec::with_capacity(BATCH_EVENTS);
        for note in 0..notes_per_session {
            for event in bench_events(note, &mut rng) {
                pending.push((sequence, event));
                sequence += 1;
                if pending.len() >= BATCH_EVENTS {
                    let at = Instant::now();
                    store.append(id, &pending)?;
                    batch_total += at.elapsed().as_secs_f64();
                    batches += 1;
                    written += pending.len() as u64;
                    pending.clear();
                }
            }
        }
        if !pending.is_empty() {
            let at = Instant::now();
            store.append(id, &pending)?;
            batch_total += at.elapsed().as_secs_f64();
            batches += 1;
            written += pending.len() as u64;
        }
        store.finish(
            id,
            Outcome {
                ended_ms: row.started_ms + 200_000,
                completion: if rng.below(10) < 7 {
                    Completion::Completed
                } else {
                    Completion::Aborted
                },
                dropped: 0,
                practice: false,
            },
        )?;
    }
    let batch_ms = if batches == 0 {
        0.0
    } else {
        batch_total / batches as f64 * 1000.0
    };
    Ok((written, started.elapsed().as_secs_f64(), batch_ms))
}

/// One generated session.
fn bench_session(index: usize, chart: usize, notes: usize, charts: usize) -> SessionRow {
    let started_ms = 1_700_000_000_000 + index as u64 * 300_000;
    let genres = ["rock", "pop", "house", "metal"];
    let generators = ["v17", "v18"];
    SessionRow {
        uid: format!("{}-{index:08}", session_uid(started_ms, 0)),
        started_ms,
        title: format!("Song {chart}"),
        artist: format!("Artist {}", chart % 40),
        // Genre and difficulty are deliberately NOT the same cycle:
        // with `chart % 4` for both, every rock chart would be on one
        // difficulty and the generator comparison — which controls for
        // both — would return nothing and be fast for the wrong
        // reason. (It did, in the first run of this benchmark.)
        genre: Some(genres[chart % genres.len()].to_owned()),
        chart_hash: format!("chart{chart:08x}"),
        song_id: None,
        chart_file: None,
        difficulty: ((chart / genres.len()) % 4) as u8,
        player_slot: 0,
        player_id: Some(1 + (index % 3) as u64),
        provenance: Provenance {
            game: "0.18.0".to_owned(),
            chart_format: 1,
            // A generator version is adopted at a point in TIME, so
            // it changes once the whole library has been played
            // through — not per session. With `index % 2` it
            // correlated with the chart's parity and the comparison
            // saw one version per chart, which is the one thing it
            // must never see.
            generator: Some(generators[(index / charts) % generators.len()].to_owned()),
            scoring: 1,
            analysis: None,
            vocal: None,
        },
        telemetry_schema: schema::schema_version(),
        detail: Detail::Actions,
        input_device: if index.is_multiple_of(5) {
            InputDevice::Guitar
        } else {
            InputDevice::Keyboard
        },
        input_offset_ms: Some(-10.0),
        video_offset_ms: Some(0.0),
        mic_offset_ms: None,
        tap_mode: true,
        no_fail: true,
        practice: false,
        autopilot: false,
        notes_total: u32::try_from(notes).unwrap_or(u32::MAX),
    }
}

/// The events one note produces — the proportions three measured
/// autopilot runs actually showed: roughly two actions per judged
/// note, a sustain on four notes in ten, and an overstrum now and
/// then.
fn bench_events(note: usize, rng: &mut Rng) -> Vec<Event> {
    let at = micros(note as f64 * 0.45);
    let mut out = Vec::with_capacity(4);
    out.push(
        Event::new(EventType::Action, at).acting(crate::model::Action::FretDown(
            beatbyte_core::Lane::from_index(note % 5).unwrap_or(beatbyte_core::Lane::One),
        )),
    );
    out.push(Event::new(EventType::Action, at).acting(crate::model::Action::Strum));
    let missed = rng.below(100) < 18;
    if missed {
        out.push(
            Event::new(EventType::NoteMiss, at)
                .about(note)
                .judged(Rating::Miss),
        );
    } else {
        let offset = (rng.below(60) as f64 - 25.0) / 1000.0;
        let rating = match rng.below(10) {
            0..=5 => Rating::Perfect,
            6..=8 => Rating::Great,
            _ => Rating::Good,
        };
        out.push(
            Event::new(EventType::NoteHit, at)
                .about(note)
                .off_by(offset)
                .judged(rating),
        );
    }
    if rng.below(10) < 4 {
        let mut sustain = Event::new(EventType::SustainEnded, at)
            .about(note)
            .flagged(Flags::SUSTAIN);
        if rng.below(10) < 8 {
            sustain = sustain.flagged(Flags::DONE);
        }
        out.push(sustain);
    }
    if rng.below(100) < 2 {
        out.push(Event::new(EventType::Overstrum, at).about(note));
    }
    out
}

/// Time the questions the store exists to answer.
pub fn measure(store: &Store) -> Result<Vec<(String, f64, usize)>> {
    let mut out = Vec::new();
    let first = store.sessions(1)?.first().map(|session| {
        (
            session.id,
            session.row.chart_hash.clone(),
            session.row.difficulty,
        )
    });

    if let Some((id, hash, difficulty)) = first {
        let at = Instant::now();
        let rows = store.events(id)?;
        out.push((
            "every event of one session".to_owned(),
            at.elapsed().as_secs_f64() * 1000.0,
            rows.len(),
        ));

        let at = Instant::now();
        let rows =
            analytics::note_quality(store, &hash, difficulty, 1, analytics::Scope::default())?;
        out.push((
            "note quality of one chart version".to_owned(),
            at.elapsed().as_secs_f64() * 1000.0,
            rows.len(),
        ));
    }

    let at = Instant::now();
    let rows = analytics::generator_comparison(store, Some("rock"), 1)?;
    out.push((
        "generator comparison, one genre".to_owned(),
        at.elapsed().as_secs_f64() * 1000.0,
        rows.len(),
    ));

    let at = Instant::now();
    let rows = analytics::calibration_bias(store, None, 100)?;
    out.push((
        "calibration bias, every player".to_owned(),
        at.elapsed().as_secs_f64() * 1000.0,
        rows.len(),
    ));

    let at = Instant::now();
    let rows = analytics::timing_histogram(store, None, 10, analytics::Scope::default())?;
    out.push((
        "timing histogram, everything".to_owned(),
        at.elapsed().as_secs_f64() * 1000.0,
        rows.len(),
    ));

    let at = Instant::now();
    let rows = analytics::orphan_strums(store)?;
    out.push((
        "strums that did nothing".to_owned(),
        at.elapsed().as_secs_f64() * 1000.0,
        rows.len(),
    ));
    Ok(out)
}

/// Fill a store and measure it.
pub fn run(
    store: &mut Store,
    sessions: usize,
    notes_per_session: usize,
    charts: usize,
    seed: u64,
) -> Result<BenchReport> {
    let (events, insert_s, batch_ms) = fill(store, sessions, notes_per_session, charts, seed)?;
    // The query timings are honest only against the finished
    // database, so the analysis runs after the whole fill.
    let queries = measure(store)?;
    Ok(BenchReport {
        sessions: store.session_count()?,
        events,
        insert_s,
        batch_ms,
        bytes: store.size_bytes()?,
        queries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small, so it belongs in the suite; the shape it proves is the
    /// same one the big run measures.
    #[test]
    fn a_filled_store_holds_what_was_put_in_and_answers_every_question() {
        let mut store = Store::open_in_memory().expect("a store");
        let report = run(&mut store, 12, 40, 3, 7).expect("fills");
        assert_eq!(report.sessions, 12);
        assert!(report.events >= 12 * 40 * 3, "{} events", report.events);
        assert_eq!(
            report.events,
            store.event_count().expect("counts"),
            "the report counts what is actually there"
        );
        assert!(report.bytes > 0);
        assert!(report.bytes_per_event() > 0.0);
        assert_eq!(report.queries.len(), 6, "every question was timed");
        for (name, _, _) in &report.queries {
            assert!(!name.is_empty());
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_database() {
        // Two runs of the benchmark have to be comparable, which they
        // are not if the data differs between them.
        let counts: Vec<u64> = (0..2)
            .map(|_| {
                let mut store = Store::open_in_memory().expect("a store");
                fill(&mut store, 5, 20, 2, 42).expect("fills").0
            })
            .collect();
        assert_eq!(counts[0], counts[1]);
    }

    #[test]
    fn the_generated_data_looks_like_playing_rather_than_like_a_loop() {
        let mut store = Store::open_in_memory().expect("a store");
        fill(&mut store, 20, 60, 4, 3).expect("fills");
        let hits: i64 = store
            .query(
                "SELECT COUNT(*) FROM gameplay_event WHERE event_type = ?1",
                &[&EventType::NoteHit.code()],
                |row| row.get(0),
            )
            .expect("counts")[0];
        let misses: i64 = store
            .query(
                "SELECT COUNT(*) FROM gameplay_event WHERE event_type = ?1",
                &[&EventType::NoteMiss.code()],
                |row| row.get(0),
            )
            .expect("counts")[0];
        assert!(hits > 0 && misses > 0, "{hits} hits, {misses} misses");
        let miss_rate = misses as f64 / (hits + misses) as f64;
        assert!(
            (0.08..0.30).contains(&miss_rate),
            "a plausible miss rate, not everything or nothing: {miss_rate}"
        );
        let ratings: Vec<i64> = store
            .query(
                "SELECT DISTINCT rating FROM gameplay_event WHERE rating IS NOT NULL ORDER BY 1",
                &[],
                |row| row.get(0),
            )
            .expect("reads");
        assert_eq!(ratings.len(), 4, "every judgment occurs: {ratings:?}");
    }
}
