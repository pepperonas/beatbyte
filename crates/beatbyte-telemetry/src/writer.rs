//! The recording side: a bounded queue, a worker thread, and batched
//! transactions.
//!
//! # The one rule
//!
//! **Nothing on the frame thread touches the database.** Recording an
//! event is: build a small `Copy` struct, bump an atomic, hand it to a
//! bounded channel with `try_send`, return. There is no lock to
//! contend for, no allocation per event beyond the channel's own, and
//! no path that can block on I/O. A full queue drops the event and
//! says so; it never waits.
//!
//! Lifecycle messages — a session opening, closing, a note the player
//! left — travel on a **second, unbounded** channel. They happen a
//! handful of times per song and losing the row that closes a session
//! would cost the whole session's interpretation, so they may not be
//! dropped; but they may not block either, and sharing the bounded
//! queue would mean exactly that (a full queue plus a blocking send
//! is a stalled frame, and in the first draft of this module it was a
//! deadlock the tests found). Two channels, each with one job.
//!
//! # Two channels, one order
//!
//! Nothing orders the event queue against the control channel, so an
//! event CAN reach the worker before the message that opens its
//! session — the game records within the same frame the session
//! starts. Events for a slot that has no session yet are therefore
//! held, not thrown away, and the session that opens next adopts
//! them. (Held to a bound: a slot that never opens stops collecting
//! after one batch, and says so.)
//!
//! # A drop is never silent
//!
//! A dropped event leaves a **gap in the sequence numbers**, because
//! the counter advances whether or not the send succeeded. The gap is
//! at the exact place the loss happened. The count is also stored on
//! the session and clears `telemetry_complete`, so an analysis cannot
//! mistake a hole for a quiet passage.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError, channel, sync_channel,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::model::{Completion, Event, Outcome, SessionRow};
use crate::store::{PlayerNote, SessionId, Store};
use crate::{Result, now_ms};

/// How many events the queue holds before it starts dropping.
///
/// A busy expert chart produces on the order of ten events a second;
/// eight thousand is minutes of head room, which is more than any
/// plausible disk stall.
pub const QUEUE_CAPACITY: usize = 8192;

/// How many events accumulate before the worker commits.
pub const BATCH_EVENTS: usize = 500;

/// …and how long it waits if that many never arrive.
pub const FLUSH_EVERY: Duration = Duration::from_secs(2);

/// How long the worker blocks on the control channel before draining
/// the event queue again and looking at the clock.
const TICK: Duration = Duration::from_millis(25);

/// How many events one pass takes off the queue before the worker
/// looks at the control channel again. Bounded so a flood cannot
/// starve a `Finish`.
const DRAIN_PER_PASS: usize = 4096;

/// How many events a slot may hold while waiting for the message
/// that opens its session. One batch is far more than the tick the
/// two channels can disagree by.
const ORPHAN_LIMIT: usize = BATCH_EVENTS;

/// The most players one run can record. Four is the game's cap; the
/// array is sized past it so a fifth would be a compile-time change
/// rather than a silent mis-attribution.
pub const SLOTS: usize = 8;

/// One recorded event on its way to the worker.
struct Queued {
    slot: u8,
    sequence: u32,
    event: Event,
}

/// Everything that is not an event: rare, small, and never dropped.
enum Control {
    Begin {
        slot: u8,
        row: Box<SessionRow>,
    },
    Note {
        slot: u8,
        written_ms: u64,
        note: Box<PlayerNote>,
    },
    Finish {
        slot: u8,
        outcome: Outcome,
    },
    /// Commit everything and hand back what the player has said about
    /// this slot. The one synchronous call in the whole module, and
    /// it exists for one reason: a harness that wants to prove a
    /// rating LANDED cannot do it against an asynchronous queue.
    Notes {
        slot: u8,
        reply: std::sync::mpsc::Sender<Vec<PlayerNote>>,
    },
    Flush,
    Stop,
}

/// Counters both threads read.
#[derive(Default)]
struct Shared {
    /// Events handed over and not yet committed.
    queued: AtomicU32,
    /// Events committed since the writer opened.
    written: AtomicU64,
    /// Events the queue refused, per slot.
    dropped: [AtomicU32; SLOTS],
    /// The next sequence number, per slot.
    sequence: [AtomicU32; SLOTS],
    /// Unix milliseconds of the last commit.
    last_flush_ms: AtomicU64,
    /// How long that commit took, in microseconds.
    last_write_us: AtomicU64,
    /// Sessions in the store after the last commit.
    stored_sessions: AtomicU64,
    /// Events in the store after the last commit.
    stored_events: AtomicU64,
    /// The store's size in bytes after the last commit.
    stored_bytes: AtomicU64,
    /// Failed database operations since the writer opened.
    errors: AtomicU32,
    /// What the last one said.
    last_error: Mutex<Option<String>>,
}

/// What the debug view shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WriterStats {
    /// Events waiting to be committed.
    pub queued: u32,
    /// Events committed since the writer opened.
    pub written: u64,
    /// Events the queue refused since the writer opened.
    pub dropped: u32,
    /// Milliseconds since the last commit.
    pub since_flush_ms: u64,
    /// How long the last commit took, in microseconds.
    pub last_write_us: u64,
    /// Sessions in the store.
    pub sessions: u64,
    /// Events in the store.
    pub events: u64,
    /// The store's size in bytes.
    pub bytes: u64,
    /// Failed database operations.
    pub errors: u32,
    /// What the last failure said.
    pub last_error: Option<String>,
}

/// The handle the game records through.
pub struct Telemetry {
    events: Option<SyncSender<Queued>>,
    control: Option<Sender<Control>>,
    shared: Arc<Shared>,
    worker: Option<std::thread::JoinHandle<()>>,
    path: Option<PathBuf>,
    /// Held only by the test constructor, so nothing drains the queue
    /// and a full one can be observed deterministically. Kept for its
    /// lifetime, never read — dropping it would disconnect the
    /// channel and turn "full" into "gone".
    #[cfg(test)]
    #[allow(dead_code)]
    parked: Option<(Receiver<Queued>, Receiver<Control>)>,
}

impl Telemetry {
    /// Open the store at `path` and start the worker.
    ///
    /// The open happens here, on the calling thread, so a broken or
    /// unwritable database is an error the caller sees instead of a
    /// thread that quietly dies.
    pub fn open(path: PathBuf) -> Result<Telemetry> {
        let store = Store::open(&path)?;
        let (events, event_rx) = sync_channel(QUEUE_CAPACITY);
        let (control, control_rx) = channel();
        let shared = Arc::new(Shared::default());
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("beatbyte-telemetry".to_owned())
            .spawn(move || run(store, &event_rx, &control_rx, &worker_shared))?;
        Ok(Telemetry {
            events: Some(events),
            control: Some(control),
            shared,
            worker: Some(worker),
            path: Some(path),
            #[cfg(test)]
            parked: None,
        })
    }

    /// Where the store lives, for the debug view.
    #[must_use]
    pub fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }

    /// Open a session in this slot. Blocking — once per song.
    pub fn begin(&self, slot: u8, row: SessionRow) {
        if slot as usize >= SLOTS {
            return;
        }
        self.shared.sequence[slot as usize].store(0, Ordering::Relaxed);
        self.shared.dropped[slot as usize].store(0, Ordering::Relaxed);
        self.control(Control::Begin {
            slot,
            row: Box::new(row),
        });
    }

    /// Record one event. Never blocks; never allocates beyond the
    /// channel. This is the only call that happens during play.
    pub fn record(&self, slot: u8, event: Event) {
        let Some(sender) = self.events.as_ref() else {
            return;
        };
        let Some(counter) = self.shared.sequence.get(slot as usize) else {
            return;
        };
        // The counter advances even when the send fails: the gap it
        // leaves is the evidence that something was lost here.
        let sequence = counter.fetch_add(1, Ordering::Relaxed);
        match sender.try_send(Queued {
            slot,
            sequence,
            event,
        }) {
            Ok(()) => {
                self.shared.queued.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Full(_)) => {
                if let Some(dropped) = self.shared.dropped.get(slot as usize) {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }

    /// Record something the player said about the run.
    pub fn note(&self, slot: u8, note: PlayerNote) {
        self.control(Control::Note {
            slot,
            written_ms: now_ms(),
            note: Box::new(note),
        });
    }

    /// Commit, then read back what the player has said about a slot.
    ///
    /// Blocking, with a timeout, and deliberately the only call here
    /// that waits: recording is asynchronous, and a caller that needs
    /// to prove a note arrived has nothing else to wait on. `None`
    /// means the worker did not answer in time — never that the note
    /// is absent.
    #[must_use]
    pub fn notes_for(&self, slot: u8, timeout: Duration) -> Option<Vec<PlayerNote>> {
        let (reply, answer) = std::sync::mpsc::channel();
        self.control(Control::Notes { slot, reply });
        answer.recv_timeout(timeout).ok()
    }

    /// Ask the worker to commit what it has.
    pub fn flush(&self) {
        self.control(Control::Flush);
    }

    /// Close a session, carrying its drop count with it.
    ///
    /// `practice` is settled here rather than at the start: it is
    /// engaged from the pause menu and sticky once used.
    pub fn finish(&self, slot: u8, ended_ms: u64, completion: Completion, practice: bool) {
        let dropped = self
            .shared
            .dropped
            .get(slot as usize)
            .map_or(0, |count| count.load(Ordering::Relaxed));
        self.control(Control::Finish {
            slot,
            outcome: Outcome {
                ended_ms,
                completion,
                dropped,
                practice,
            },
        });
    }

    /// Events this slot has lost so far.
    #[must_use]
    pub fn dropped(&self, slot: u8) -> u32 {
        self.shared
            .dropped
            .get(slot as usize)
            .map_or(0, |count| count.load(Ordering::Relaxed))
    }

    /// A snapshot for the debug view.
    #[must_use]
    pub fn stats(&self) -> WriterStats {
        let last_flush = self.shared.last_flush_ms.load(Ordering::Relaxed);
        WriterStats {
            queued: self.shared.queued.load(Ordering::Relaxed),
            written: self.shared.written.load(Ordering::Relaxed),
            dropped: self
                .shared
                .dropped
                .iter()
                .map(|count| count.load(Ordering::Relaxed))
                .sum(),
            since_flush_ms: if last_flush == 0 {
                0
            } else {
                now_ms().saturating_sub(last_flush)
            },
            last_write_us: self.shared.last_write_us.load(Ordering::Relaxed),
            sessions: self.shared.stored_sessions.load(Ordering::Relaxed),
            events: self.shared.stored_events.load(Ordering::Relaxed),
            bytes: self.shared.stored_bytes.load(Ordering::Relaxed),
            errors: self.shared.errors.load(Ordering::Relaxed),
            last_error: self
                .shared
                .last_error
                .lock()
                .ok()
                .and_then(|guard| guard.clone()),
        }
    }

    /// Stop the worker and wait for it to commit what it holds.
    ///
    /// Called from [`Drop`], and separately at application exit so
    /// the wait happens somewhere a log line can still be written.
    pub fn shutdown(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.send(Control::Stop);
        }
        // The event queue is dropped only after the stop is queued:
        // the worker drains what is still in it before it leaves.
        self.events = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    /// Unbounded, so this can never block a frame. A control message
    /// is a handful of bytes a few times per song.
    fn control(&self, command: Control) {
        if let Some(sender) = self.control.as_ref() {
            let _ = sender.send(command);
        }
    }

    /// A writer with nothing on the other end of the queue: the only
    /// way to observe a full one without racing a real worker.
    #[cfg(test)]
    fn parked(capacity: usize) -> Telemetry {
        let (events, event_rx) = sync_channel(capacity);
        let (control, control_rx) = channel();
        Telemetry {
            events: Some(events),
            control: Some(control),
            shared: Arc::new(Shared::default()),
            worker: None,
            path: None,
            parked: Some((event_rx, control_rx)),
        }
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl std::fmt::Debug for Telemetry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Telemetry")
            .field("path", &self.path)
            .field("stats", &self.stats())
            .finish()
    }
}

/// One slot's pending work on the worker side.
#[derive(Default)]
struct Slot {
    session: Option<SessionId>,
    pending: Vec<(u32, Event)>,
}

/// The worker loop.
///
/// One pass: take what is in the event queue, then wait a short tick
/// on the control channel. Control can therefore never be starved by
/// a flood of events, and events are never delayed by more than a
/// tick — which is nothing next to the flush interval they are
/// batched into anyway.
fn run(
    mut store: Store,
    events: &Receiver<Queued>,
    control: &Receiver<Control>,
    shared: &Arc<Shared>,
) {
    let mut slots: HashMap<u8, Slot> = HashMap::new();
    let mut last_flush = Instant::now();
    loop {
        let mut pending = drain_events(events, &mut slots, shared);
        let mut force = false;
        match control.recv_timeout(TICK) {
            Ok(Control::Begin { slot, row }) => {
                // Anything already queued for this slot is taken
                // first, so the decision below is made about all of
                // it rather than about half of it.
                drain_events(events, &mut slots, shared);
                let open = slots.get(&slot).and_then(|slot| slot.session);
                if open.is_some() {
                    // A slot that is still open was never closed; its
                    // writes go down before the new id replaces it, or
                    // they would be attributed to the wrong song.
                    flush(&mut store, &mut slots, shared, &mut last_flush);
                }
                // What is left belongs to the run about to open: the
                // first frame's events can reach the worker before
                // the message that opens the session does.
                let carried = slots
                    .get_mut(&slot)
                    .map(|slot| std::mem::take(&mut slot.pending))
                    .unwrap_or_default();
                match store.begin(&row) {
                    Ok(id) => {
                        slots.insert(
                            slot,
                            Slot {
                                session: Some(id),
                                pending: carried,
                            },
                        );
                    }
                    Err(error) => note_error(shared, &error),
                }
                pending = slots.values().map(|slot| slot.pending.len()).sum();
            }
            Ok(Control::Note {
                slot,
                written_ms,
                note,
            }) => {
                if let Some(id) = slots.get(&slot).and_then(|slot| slot.session)
                    && let Err(error) = store.add_note(id, written_ms, &note)
                {
                    note_error(shared, &error);
                }
            }
            Ok(Control::Finish { slot, outcome }) => {
                // Everything this slot produced goes down before the
                // row that closes it, so a reader never sees a
                // finished session whose last events are still in a
                // queue.
                drain_events(events, &mut slots, shared);
                flush(&mut store, &mut slots, shared, &mut last_flush);
                if let Some(id) = slots.get(&slot).and_then(|slot| slot.session)
                    && let Err(error) = store.finish(id, outcome)
                {
                    note_error(shared, &error);
                }
                pending = 0;
            }
            Ok(Control::Notes { slot, reply }) => {
                drain_events(events, &mut slots, shared);
                flush(&mut store, &mut slots, shared, &mut last_flush);
                let notes = slots
                    .get(&slot)
                    .and_then(|slot| slot.session)
                    .and_then(|id| store.notes(id).ok())
                    .unwrap_or_default();
                let _ = reply.send(notes);
                pending = 0;
            }
            Ok(Control::Flush) => force = true,
            Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => {
                // Take whatever is left in the queue with us.
                drain_events(events, &mut slots, shared);
                flush(&mut store, &mut slots, shared, &mut last_flush);
                return;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if force || pending >= BATCH_EVENTS || last_flush.elapsed() >= FLUSH_EVERY {
            flush(&mut store, &mut slots, shared, &mut last_flush);
        }
    }
}

/// Move what is waiting in the event queue into the per-slot buffers,
/// and report how much is now buffered in total.
fn drain_events(
    events: &Receiver<Queued>,
    slots: &mut HashMap<u8, Slot>,
    shared: &Arc<Shared>,
) -> usize {
    let mut taken = 0;
    while taken < DRAIN_PER_PASS {
        match events.try_recv() {
            Ok(queued) => {
                shared.queued.fetch_sub(1, Ordering::Relaxed);
                slots
                    .entry(queued.slot)
                    .or_default()
                    .pending
                    .push((queued.sequence, queued.event));
                taken += 1;
            }
            Err(_) => break,
        }
    }
    slots.values().map(|slot| slot.pending.len()).sum()
}

/// Commit every slot's pending events in one pass.
fn flush(
    store: &mut Store,
    slots: &mut HashMap<u8, Slot>,
    shared: &Arc<Shared>,
    last_flush: &mut Instant,
) {
    let started = Instant::now();
    let mut wrote = 0u64;
    let mut orphaned = 0usize;
    for slot in slots.values_mut() {
        if slot.pending.is_empty() {
            continue;
        }
        let Some(id) = slot.session else {
            // The session has not opened yet — it is on the other
            // channel and a tick away. Hold what has arrived; the
            // `Begin` that follows adopts it.
            if slot.pending.len() > ORPHAN_LIMIT {
                // Unless it never comes. Then this is a slot nothing
                // will ever claim, and holding it forever would be a
                // leak rather than patience.
                orphaned += slot.pending.len();
                slot.pending.clear();
            }
            continue;
        };
        match store.append(id, &slot.pending) {
            Ok(()) => wrote += slot.pending.len() as u64,
            Err(error) => note_error(shared, &error),
        }
        slot.pending.clear();
    }
    if orphaned > 0 {
        shared.errors.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut last) = shared.last_error.lock() {
            *last = Some(format!(
                "{orphaned} events belonged to a session that never opened"
            ));
        }
    }
    *last_flush = Instant::now();
    if wrote > 0 {
        shared.written.fetch_add(wrote, Ordering::Relaxed);
        shared.last_write_us.store(
            u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }
    shared.last_flush_ms.store(now_ms(), Ordering::Relaxed);
    if let Ok(count) = store.session_count() {
        shared.stored_sessions.store(count, Ordering::Relaxed);
    }
    if let Ok(count) = store.event_count() {
        shared.stored_events.store(count, Ordering::Relaxed);
    }
    if let Ok(bytes) = store.size_bytes() {
        shared.stored_bytes.store(bytes, Ordering::Relaxed);
    }
}

fn note_error(shared: &Arc<Shared>, error: &crate::Error) {
    shared.errors.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut last) = shared.last_error.lock() {
        *last = Some(error.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Detail, EventType, InputDevice, Provenance, micros};
    use crate::schema;

    fn a_session(uid: &str, slot: u8) -> SessionRow {
        SessionRow {
            uid: uid.to_owned(),
            started_ms: 1_700_000_000_000,
            title: "Maria".to_owned(),
            artist: "Blondie".to_owned(),
            genre: None,
            chart_hash: "abc".to_owned(),
            song_id: None,
            chart_file: None,
            difficulty: 1,
            player_slot: slot,
            player_id: None,
            provenance: Provenance {
                game: "0.18.0".to_owned(),
                chart_format: 1,
                generator: None,
                scoring: 1,
                analysis: None,
                vocal: None,
            },
            telemetry_schema: schema::schema_version(),
            detail: Detail::Actions,
            input_device: InputDevice::Keyboard,
            input_offset_ms: Some(0.0),
            video_offset_ms: Some(0.0),
            mic_offset_ms: None,
            tap_mode: true,
            no_fail: true,
            practice: false,
            autopilot: false,
            notes_total: 3,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "beatbyte-telemetry-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("telemetry.db")
    }

    #[test]
    fn a_recorded_run_lands_in_the_store_when_the_writer_stops() {
        let path = scratch("roundtrip");
        {
            let mut writer = Telemetry::open(path.clone()).expect("opens");
            writer.begin(0, a_session("run", 0));
            for index in 0..3u32 {
                writer.record(
                    0,
                    Event::new(EventType::NoteMiss, micros(f64::from(index))).about(index as usize),
                );
            }
            writer.finish(0, 1_700_000_100_000, Completion::Completed, false);
            writer.shutdown();
            let stats = writer.stats();
            assert_eq!(stats.written, 3, "every event was committed");
            assert_eq!(stats.dropped, 0);
        }
        let store = Store::open(&path).expect("reopens");
        let session = store
            .sessions(1)
            .expect("lists")
            .into_iter()
            .next()
            .expect("the one run");
        assert_eq!(session.completion, Completion::Completed);
        assert!(session.complete);
        let events = store.events(session.id).expect("reads");
        assert_eq!(events.len(), 3);
        assert_eq!(
            events
                .iter()
                .map(|(sequence, _)| *sequence)
                .collect::<Vec<_>>(),
            vec![0, 1, 2],
            "sequence numbers are monotone from zero"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
    }

    /// The two channels have no order between them, so an event can
    /// reach the worker before the message that opens its session —
    /// which is exactly what happens in the game, where both are
    /// issued within one frame. The first draft threw those events
    /// away and every recorded run came back empty.
    #[test]
    fn an_event_that_overtakes_its_session_is_still_recorded() {
        let path = scratch("overtake");
        {
            let mut writer = Telemetry::open(path.clone()).expect("opens");
            // Events first, deliberately: the queue is bounded and
            // fast, the control channel is read a tick later.
            for index in 0..5u32 {
                writer.record(0, Event::new(EventType::Overstrum, i64::from(index)));
            }
            writer.begin(0, a_session("late-begin", 0));
            writer.finish(0, 1, Completion::Completed, false);
            writer.shutdown();
        }
        let store = Store::open(&path).expect("reopens");
        let session = store
            .sessions(1)
            .expect("lists")
            .into_iter()
            .next()
            .expect("the run");
        assert_eq!(
            store.events(session.id).expect("reads").len(),
            5,
            "the session that opens adopts what was already waiting"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
    }

    #[test]
    fn two_players_in_one_run_keep_their_own_sessions_and_counters() {
        let path = scratch("twoplayers");
        {
            let mut writer = Telemetry::open(path.clone()).expect("opens");
            writer.begin(0, a_session("p0", 0));
            writer.begin(1, a_session("p1", 1));
            writer.record(0, Event::new(EventType::Overstrum, 0));
            writer.record(1, Event::new(EventType::Overstrum, 0));
            writer.record(1, Event::new(EventType::Overstrum, 1));
            writer.finish(0, 1, Completion::Aborted, false);
            writer.finish(1, 1, Completion::Completed, false);
            writer.shutdown();
        }
        let store = Store::open(&path).expect("reopens");
        let sessions = store.sessions(10).expect("lists");
        assert_eq!(sessions.len(), 2);
        for session in sessions {
            let events = store.events(session.id).expect("reads");
            let expected = if session.row.player_slot == 0 { 1 } else { 2 };
            assert_eq!(
                events.len(),
                expected,
                "slot {} got the other player's events",
                session.row.player_slot
            );
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
    }

    #[test]
    fn the_player_s_own_words_reach_the_run_they_are_about() {
        let path = scratch("notes");
        {
            let mut writer = Telemetry::open(path.clone()).expect("opens");
            writer.begin(0, a_session("run", 0));
            writer.finish(0, 1, Completion::Completed, false);
            // The results screen speaks AFTER the run is closed.
            writer.note(0, PlayerNote::Fun(5));
            writer.note(0, PlayerNote::Comment("the chorus drags".to_owned()));
            writer.shutdown();
        }
        let store = Store::open(&path).expect("reopens");
        let session = store
            .sessions(1)
            .expect("lists")
            .into_iter()
            .next()
            .expect("the run");
        let notes = store.notes(session.id).expect("reads");
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0], PlayerNote::Fun(5));
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
    }

    #[test]
    fn what_the_player_said_can_be_read_back_without_stopping_the_writer() {
        // The harness that proves a rating landed cannot wait on an
        // asynchronous queue, so this is the one call that blocks.
        let path = scratch("readback");
        let mut writer = Telemetry::open(path.clone()).expect("opens");
        writer.begin(0, a_session("run", 0));
        writer.finish(0, 1, Completion::Completed, false);
        writer.note(0, PlayerNote::Fun(5));
        let notes = writer
            .notes_for(0, Duration::from_secs(5))
            .expect("the worker answers");
        assert_eq!(notes, vec![PlayerNote::Fun(5)]);
        // A slot nothing was recorded for answers with nothing rather
        // than with a wait.
        assert_eq!(
            writer.notes_for(3, Duration::from_secs(5)),
            Some(Vec::new())
        );
        writer.shutdown();
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
    }

    #[test]
    fn a_full_queue_drops_the_event_counts_it_and_leaves_a_gap() {
        // Nothing drains this writer, so the queue fills at a known
        // point instead of racing a worker.
        let writer = Telemetry::parked(2);
        for index in 0..6u32 {
            writer.record(0, Event::new(EventType::Overstrum, i64::from(index)));
        }
        assert_eq!(
            writer.dropped(0),
            4,
            "a queue of two takes two and refuses the rest"
        );
        assert_eq!(
            writer.shared.sequence[0].load(Ordering::Relaxed),
            6,
            "the counter advances through a drop — the gap in the \
             stored sequence is what makes the loss visible"
        );
        assert_eq!(writer.stats().dropped, 4);
        // And a drop is never silent: the count travels with the
        // session and clears its completeness flag.
        let outcome = Outcome {
            ended_ms: 1,
            completion: Completion::Completed,
            dropped: writer.dropped(0),
            practice: false,
        };
        assert!(outcome.dropped > 0);
    }

    #[test]
    fn a_run_that_lost_events_is_stored_as_incomplete() {
        let path = scratch("incomplete");
        {
            let mut writer = Telemetry::open(path.clone()).expect("opens");
            writer.begin(0, a_session("run", 0));
            // Simulate the loss the parked test proves happens.
            writer.shared.dropped[0].store(7, Ordering::Relaxed);
            writer.finish(0, 1, Completion::Completed, false);
            writer.shutdown();
        }
        let store = Store::open(&path).expect("reopens");
        let session = store
            .sessions(1)
            .expect("lists")
            .into_iter()
            .next()
            .expect("the run");
        assert_eq!(session.dropped, 7);
        assert!(!session.complete);
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
    }

    #[test]
    fn a_run_the_process_never_closed_reads_as_one() {
        let path = scratch("crash");
        {
            let mut writer = Telemetry::open(path.clone()).expect("opens");
            writer.begin(0, a_session("run", 0));
            writer.record(0, Event::new(EventType::NoteHit, 0));
            // No `finish` — the game went away mid-song.
            writer.shutdown();
        }
        let store = Store::open(&path).expect("reopens");
        let session = store
            .sessions(1)
            .expect("lists")
            .into_iter()
            .next()
            .expect("the run");
        assert_eq!(
            session.ended_ms, None,
            "an unfinished session says so instead of pretending to \
             have ended"
        );
        assert_eq!(session.completion, Completion::Unknown);
        assert_eq!(
            store.events(session.id).expect("reads").len(),
            1,
            "what it did record is still there"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
    }

    #[test]
    fn a_batch_lands_without_being_asked_to() {
        // The periodic flush: more than one batch worth of events and
        // no `flush` call at all.
        let path = scratch("batching");
        let mut writer = Telemetry::open(path.clone()).expect("opens");
        writer.begin(0, a_session("run", 0));
        for index in 0..(BATCH_EVENTS + 20) {
            writer.record(0, Event::new(EventType::Overstrum, index as i64));
        }
        // Give the worker its turn; the batch trigger is a count, so
        // this does not depend on the clock.
        let deadline = Instant::now() + Duration::from_secs(5);
        while writer.stats().written < BATCH_EVENTS as u64 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            writer.stats().written >= BATCH_EVENTS as u64,
            "five hundred events commit on their own: {:?}",
            writer.stats()
        );
        writer.shutdown();
        assert_eq!(writer.stats().written, (BATCH_EVENTS + 20) as u64);
        assert_eq!(writer.stats().queued, 0, "the queue drains");
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
    }

    /// The first draft put control messages on the bounded queue and
    /// sent them blocking. A full queue then made `shutdown` — which
    /// `Drop` calls — wait for a reader that was never coming, and the
    /// whole test binary hung. The two channels are what fixed it;
    /// this is the test that would have caught it.
    #[test]
    fn a_full_queue_cannot_wedge_the_shutdown() {
        let mut writer = Telemetry::parked(1);
        for index in 0..50u32 {
            writer.record(0, Event::new(EventType::Overstrum, i64::from(index)));
        }
        assert!(writer.dropped(0) > 0, "the queue really is full");
        // Every one of these travels on the unbounded control channel
        // and must return immediately even so.
        writer.begin(0, a_session("late", 0));
        writer.note(0, PlayerNote::Fun(3));
        writer.flush();
        writer.finish(0, 1, Completion::Aborted, false);
        writer.shutdown();
    }

    #[test]
    fn a_slot_past_the_cap_is_ignored_rather_than_mis_attributed() {
        let writer = Telemetry::parked(4);
        writer.begin(200, a_session("nope", 200));
        writer.record(200, Event::new(EventType::Overstrum, 0));
        assert_eq!(writer.dropped(200), 0);
        assert_eq!(
            writer.stats().dropped,
            0,
            "an impossible slot records nothing and blames nobody"
        );
    }
}
