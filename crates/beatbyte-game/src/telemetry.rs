//! Recording a run into the gameplay store (ADR-0018).
//!
//! The engine produces a judgment and a signed timing offset for
//! every note and used to throw them away after the session; this
//! module stops the deletion. What it records, why it records so
//! little, and why none of it happens on the frame thread is the
//! ADR's business — this is the wiring.
//!
//! # What used to be here
//!
//! Layer 1 of adaptive charting (ADR-0011) shipped as one JSONL file
//! per session, written on the way out. Those files still exist and
//! are still readable — `beatbyte-cli telemetry import` takes them
//! into the store, once — but nothing writes them any more. Keeping
//! both would have been the parallel architecture the store exists
//! to avoid; the readers (`review`, `dossier`) moved first, and the
//! writer followed, which is the order that cannot lose evidence.

use std::path::PathBuf;

use beatbyte_chart::chart_hash;
use beatbyte_core::telemetry::NoteLine;
use beatbyte_core::{Judgment, SessionEvent};
use bevy::prelude::*;

use crate::gameplay::{PlayerSession, SessionFeedback};
use crate::states::AppState;

/// Where the older per-session files live.
///
/// Nothing writes there now; the legacy importer reads it.
#[must_use]
pub fn telemetry_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("telemetry"))
}

// ── The store (ADR-0018) ────────────────────────────────────────────
//
// The JSONL recorder above is layer 1 as it shipped; this is the
// store that replaces it. Both run for now — the readers (`review`,
// `dossier`) still live on the files, and the migration order that
// cannot lose evidence is: write both, move the readers, then retire
// the writer.

use beatbyte_telemetry::model::{
    Completion, Event, EventType, Flags, InputDevice, Provenance, Rating, SessionRow,
};
use beatbyte_telemetry::{Telemetry, schema_version, session_uid};

use crate::config::TelemetryLevel;
use crate::gameplay::{PlayerDevice, PlayerIndex};
use crate::multiplayer::DeviceId;
use crate::states::GamePhase;

/// Where the store lives.
#[must_use]
pub fn store_path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("telemetry.db"))
}

/// The open store, or the reason there is none.
///
/// Held for the whole process: the worker thread and the database
/// connection outlive any one song, and opening per song would pay
/// the cost of both on every start.
#[derive(Resource, Default)]
pub struct TelemetryStore {
    /// The writer, once it has been opened.
    writer: Option<Telemetry>,
    /// Set after the first failed open, so a broken path is reported
    /// once rather than on every song.
    refused: bool,
}

impl TelemetryStore {
    /// The writer, if one is open.
    #[must_use]
    pub fn writer(&self) -> Option<&Telemetry> {
        self.writer.as_ref()
    }

    /// Open the store if it is wanted and not open yet.
    fn ensure(&mut self, level: TelemetryLevel) -> Option<&Telemetry> {
        if level == TelemetryLevel::Off {
            return None;
        }
        if self.writer.is_none() && !self.refused {
            match store_path() {
                Some(path) => match Telemetry::open(path.clone()) {
                    Ok(writer) => {
                        info!("telemetry: store at {}", path.display());
                        self.writer = Some(writer);
                    }
                    Err(error) => {
                        warn!("telemetry: cannot open {}: {error}", path.display());
                        self.refused = true;
                    }
                },
                None => {
                    warn!("telemetry: no data directory on this platform");
                    self.refused = true;
                }
            }
        }
        self.writer.as_ref()
    }
}

/// What the current run has opened in the store.
#[derive(Resource, Default)]
pub struct StoreRun {
    /// Player slots with an open session.
    slots: Vec<u8>,
    /// What the run is recording.
    detail: Option<beatbyte_telemetry::Detail>,
    /// Whether a rock meter emptied — the one completion state the
    /// history's marker cannot express.
    failed: bool,
    /// The most recently judged note, per slot.
    ///
    /// The session does not position an overstrum — it is a strum
    /// that matched nothing, and nothing is where it happened. The
    /// nearest judged note is how an analysis localizes one to a
    /// passage, and it is also what makes an overstrum countable
    /// against the note it clusters around. The JSONL layer learned
    /// this the hard way: without it, the first log could not place a
    /// flake that turned out to be in the count-in.
    last_judged: Vec<Option<u32>>,
}

impl StoreRun {
    /// Whether a session is open for this slot.
    fn open(&self, slot: u8) -> bool {
        self.slots.contains(&slot)
    }

    /// Remember which note was judged last in this slot.
    fn judged(&mut self, slot: u8, note_index: usize) {
        let slot = slot as usize;
        if self.last_judged.len() <= slot {
            self.last_judged.resize(slot + 1, None);
        }
        self.last_judged[slot] = u32::try_from(note_index).ok();
    }

    /// The note an overstrum in this slot is nearest to.
    fn anchor(&self, slot: u8) -> Option<u32> {
        self.last_judged.get(slot as usize).copied().flatten()
    }

    /// Whether this kind of event is being recorded at all.
    ///
    /// The one place the level is interpreted on the game side, so a
    /// recorder elsewhere cannot escape the setting the player chose.
    #[must_use]
    pub fn records(&self, kind: EventType) -> bool {
        self.detail.is_some_and(|detail| kind.recorded_at(detail))
    }
}

/// Which device a player is holding, as the store names it.
///
/// A guitar controller is a gamepad with a telling name — there is no
/// other way to tell one from a pad, and calling every pad a guitar
/// would make the device column useless for the one question it
/// exists to answer. Pure, so the matching can be pinned.
#[must_use]
pub fn device_kind(device: DeviceId, pad_name: Option<&str>) -> InputDevice {
    match device {
        DeviceId::Keyboard => InputDevice::Keyboard,
        DeviceId::Pad(_) => {
            let name = pad_name.unwrap_or_default().to_ascii_lowercase();
            if [
                "guitar",
                "plorer",
                "wireless legacy",
                "rock band",
                "stratocaster",
            ]
            .iter()
            .any(|hint| name.contains(hint))
            {
                InputDevice::Guitar
            } else {
                InputDevice::Gamepad
            }
        }
    }
}

/// Record one logical action.
///
/// Shared by the human input path and the autopilot's injector: the
/// injector bypasses `gameplay_input` entirely (it owns the session
/// while it plays), and without this the action stream would be the
/// one part of the blackbox a harness run never exercises.
pub fn record_action(
    store: Option<&TelemetryStore>,
    run: Option<&StoreRun>,
    slot: u8,
    kind: beatbyte_core::InputKind,
    song_time_s: f64,
) {
    let (Some(store), Some(run)) = (store, run) else {
        return;
    };
    let Some(writer) = store.writer() else {
        return;
    };
    if !run.records(EventType::Action) {
        return;
    }
    writer.record(
        slot,
        Event::new(
            EventType::Action,
            beatbyte_telemetry::model::micros(song_time_s),
        )
        .acting(beatbyte_telemetry::model::Action::from_input(kind)),
    );
}

/// Open one session per player on the first frame that has any.
///
/// Deliberately in `Update` rather than on state entry: it then needs
/// no ordering agreement with the gameplay spawn chain, and an event
/// that reaches the worker before this does is adopted by the session
/// when it opens (see `beatbyte_telemetry::writer`).
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn begin_store_session(
    mut store: ResMut<TelemetryStore>,
    mut run: ResMut<StoreRun>,
    players: Query<(&PlayerIndex, &PlayerDevice, &PlayerSession)>,
    pads: Query<(Entity, &Name), With<bevy::input::gamepad::Gamepad>>,
    song: Option<Res<crate::boot::LoadedSong>>,
    difficulty: Option<Res<crate::song_select::SelectedDifficulty>>,
    settings: Res<crate::config::Settings>,
    autopilot: Option<Res<crate::autopilot::Autopilot>>,
    roster: Option<Res<crate::players::Players>>,
) {
    if !run.slots.is_empty() || players.is_empty() {
        return;
    }
    let (Some(song), Some(difficulty)) = (song, difficulty) else {
        return;
    };
    let Some(detail) = settings.telemetry.detail() else {
        return;
    };
    let Some(writer) = store.ensure(settings.telemetry) else {
        return;
    };
    let started_ms = beatbyte_telemetry::now_ms();
    let hash = chart_hash(&song.chart);
    let autopilot = autopilot.is_some_and(|autopilot| autopilot.enabled);
    // Only slot one is attributed to a person: the join screen
    // assigns DEVICES, and guessing a name for slot two would put
    // somebody's bad night on a friend's record (the play log makes
    // the same call).
    let player_id = roster.and_then(|players| players.0.selected);
    let mut opened = Vec::new();
    for (index, device, player) in &players {
        let Ok(slot) = u8::try_from(index.0) else {
            continue;
        };
        let pad_name = match device.0 {
            DeviceId::Pad(entity) => pads
                .get(entity)
                .ok()
                .map(|(_, name)| name.as_str().to_owned()),
            DeviceId::Keyboard => None,
        };
        let row = SessionRow {
            uid: session_uid(started_ms, slot),
            started_ms,
            title: song.chart.song.title.clone(),
            artist: song.chart.song.artist.clone(),
            genre: song.chart.song.genre.clone(),
            chart_hash: hash.clone(),
            // Which FILE the chart came from is not carried on the
            // loaded song; the hash is the identity that matters, and
            // a path invented here would be a guess. The importer and
            // the CLI can fill it in where they do know.
            chart_file: None,
            difficulty: difficulty_index(difficulty.0),
            player_slot: slot,
            player_id: if slot == 0 { player_id } else { None },
            provenance: Provenance {
                game: env!("CARGO_PKG_VERSION").to_owned(),
                chart_format: beatbyte_chart::FORMAT_VERSION,
                generator: song
                    .chart
                    .provenance
                    .as_ref()
                    .map(|provenance| provenance.designer.clone()),
                scoring: beatbyte_core::score::SCORING_VERSION,
                // The analysis that made this chart is not recorded
                // anywhere in it yet; a number invented here would be
                // a claim rather than a fact.
                analysis: None,
                vocal: song
                    .vocals
                    .as_ref()
                    .map(|_| beatbyte_chart::vocals::VOCAL_PIPELINE_VERSION),
            },
            telemetry_schema: schema_version(),
            detail,
            input_device: device_kind(device.0, pad_name.as_deref()),
            input_offset_ms: Some(settings.latency_offset_ms),
            video_offset_ms: Some(settings.video_offset_ms),
            mic_offset_ms: song.vocals.as_ref().map(|_| settings.mic_offset_ms),
            tap_mode: settings.tap_mode,
            no_fail: settings.no_fail,
            // Practice is engaged from the pause menu and cannot have
            // happened yet; `finish` settles it.
            practice: false,
            autopilot,
            notes_total: u32::try_from(player.session.track().events().len()).unwrap_or(u32::MAX),
        };
        writer.begin(slot, row);
        opened.push(slot);
    }
    run.slots = opened;
    run.detail = Some(detail);
    run.failed = false;
    run.last_judged.clear();
}

/// The chart format's difficulty index (0 easy … 3 expert).
#[must_use]
fn difficulty_index(difficulty: beatbyte_core::Difficulty) -> u8 {
    u8::try_from(
        beatbyte_core::Difficulty::ALL
            .iter()
            .position(|value| *value == difficulty)
            .unwrap_or(0),
    )
    .unwrap_or(0)
}

/// Turn one session event into the row that records it.
///
/// Pure, and the only place the mapping lives: an event kind that is
/// deliberately NOT recorded returns `None` here, with the reason
/// written down beside it, rather than being silently absent from a
/// match somewhere.
#[must_use]
pub fn event_for(
    event: &SessionEvent,
    song_time_us: i64,
    note: Option<&beatbyte_core::NoteEvent>,
) -> Option<Event> {
    let flags = note.map_or(Flags::NONE, Flags::of_note);
    match event {
        SessionEvent::NoteHit {
            event_index,
            judgment,
            offset_s,
        } => Some(
            Event::new(
                if *judgment == Judgment::Miss {
                    EventType::NoteMiss
                } else {
                    EventType::NoteHit
                },
                song_time_us,
            )
            .about(*event_index)
            .off_by(*offset_s)
            .judged(Rating::from(*judgment))
            .flagged(flags),
        ),
        SessionEvent::NoteMissed { event_index } => Some(
            Event::new(EventType::NoteMiss, song_time_us)
                .about(*event_index)
                .judged(Rating::Miss)
                .flagged(flags),
        ),
        SessionEvent::Overstrum => Some(Event::new(EventType::Overstrum, song_time_us)),
        SessionEvent::SustainEnded {
            event_index,
            completed,
        } => {
            let mut row = Event::new(EventType::SustainEnded, song_time_us)
                .about(*event_index)
                .flagged(flags);
            if *completed {
                row = row.flagged(Flags::DONE);
            }
            Some(row)
        }
        SessionEvent::HypeActivated => Some(Event::new(EventType::HypeActivated, song_time_us)),
        SessionEvent::HypeEnded => Some(Event::new(EventType::HypeEnded, song_time_us)),
        SessionEvent::Failed => Some(Event::new(EventType::Failed, song_time_us)),
        // Derivable, and therefore not stored (ADR-0018): a sustain
        // starts when a note with a tail is hit, and a phrase is
        // completed or broken by the judgments inside its span.
        SessionEvent::SustainStarted { .. }
        | SessionEvent::PhraseCompleted { .. }
        | SessionEvent::PhraseBroken { .. } => None,
    }
}

/// Record this frame's session events into the store.
fn record_to_store(
    store: Res<TelemetryStore>,
    mut run: ResMut<StoreRun>,
    mut feedback: MessageReader<SessionFeedback>,
    sessions: Query<&PlayerSession>,
    clock: Res<crate::audio_sys::GameClock>,
    time: Res<Time>,
) {
    let Some(writer) = store.writer() else {
        feedback.clear();
        return;
    };
    let now = beatbyte_telemetry::model::micros(clock.song_time(&time).unwrap_or(0.0));
    for message in feedback.read() {
        let Ok(slot) = u8::try_from(message.player_index) else {
            continue;
        };
        if !run.open(slot) {
            continue;
        }
        if matches!(message.event, SessionEvent::Failed) {
            run.failed = true;
        }
        let note = note_of(&sessions, message.player, &message.event);
        if let Some(mut event) = event_for(&message.event, now, note.as_ref()) {
            match message.event {
                // An overstrum has no note of its own; the last one
                // judged is where in the song it happened.
                SessionEvent::Overstrum => {
                    if let Some(anchor) = run.anchor(slot) {
                        event.note_index = Some(anchor);
                    }
                }
                SessionEvent::NoteHit { event_index, .. }
                | SessionEvent::NoteMissed { event_index } => run.judged(slot, event_index),
                _ => {}
            }
            writer.record(slot, event);
        }
    }
}

/// The chart note an event is about, for its shape flags.
fn note_of(
    sessions: &Query<&PlayerSession>,
    player: Entity,
    event: &SessionEvent,
) -> Option<beatbyte_core::NoteEvent> {
    let index = match event {
        SessionEvent::NoteHit { event_index, .. }
        | SessionEvent::NoteMissed { event_index }
        | SessionEvent::SustainEnded { event_index, .. } => *event_index,
        _ => return None,
    };
    sessions
        .get(player)
        .ok()
        .and_then(|session| session.session.track().events().get(index).copied())
}

/// The song was paused.
fn record_pause(
    store: Res<TelemetryStore>,
    run: Res<StoreRun>,
    clock: Res<crate::audio_sys::GameClock>,
    time: Res<Time>,
) {
    mark(&store, &run, EventType::Paused, &clock, &time);
}

/// …and resumed.
fn record_resume(
    store: Res<TelemetryStore>,
    run: Res<StoreRun>,
    clock: Res<crate::audio_sys::GameClock>,
    time: Res<Time>,
) {
    mark(&store, &run, EventType::Resumed, &clock, &time);
}

/// Write one bare event to every open slot.
fn mark(
    store: &TelemetryStore,
    run: &StoreRun,
    kind: EventType,
    clock: &crate::audio_sys::GameClock,
    time: &Time,
) {
    let Some(writer) = store.writer() else {
        return;
    };
    if !run.records(kind) {
        return;
    }
    let now = beatbyte_telemetry::model::micros(clock.song_time(time).unwrap_or(0.0));
    for slot in &run.slots {
        writer.record(*slot, Event::new(kind, now));
    }
}

/// The playhead moved by more than playing could explain.
///
/// Observed rather than reported from the call sites, deliberately:
/// a practice loop, the blind test's handover and the MC set's swap
/// all seek, and each of them would have to remember to say so. What
/// they have in common is visible from here — the song clock moving
/// further in one frame than the frame lasted.
///
/// The threshold is a second, well above the clock's own corrections
/// (a 30 ms snap, a 10 % slew) and above the count-in handover's step
/// back, so a correction is never recorded as a seek.
fn watch_for_seeks(
    store: Res<TelemetryStore>,
    run: Res<StoreRun>,
    clock: Res<crate::audio_sys::GameClock>,
    time: Res<Time>,
    mut last: Local<Option<f64>>,
) {
    let Some(now) = clock.song_time(&time) else {
        *last = None;
        return;
    };
    let previous = last.replace(now);
    let Some(previous) = previous else {
        return;
    };
    let expected = f64::from(time.delta_secs());
    if (now - previous - expected).abs() < SEEK_THRESHOLD_S {
        return;
    }
    let Some(writer) = store.writer() else {
        return;
    };
    if !run.records(EventType::Seek) {
        return;
    }
    let event = Event::new(EventType::Seek, beatbyte_telemetry::model::micros(previous))
        .valued(beatbyte_telemetry::model::micros(now));
    for slot in &run.slots {
        writer.record(*slot, event);
    }
}

/// How far the playhead has to move in one frame before it counts as
/// a seek rather than as the clock correcting itself.
const SEEK_THRESHOLD_S: f64 = 1.0;

/// Close every session this run opened.
#[allow(clippy::needless_pass_by_value)] // Bevy system params
fn finish_store_session(
    mut commands: Commands,
    store: Res<TelemetryStore>,
    mut run: ResMut<StoreRun>,
    completed: Option<Res<crate::history::RunCompleted>>,
    practice: Option<Res<crate::gameplay::PracticeState>>,
) {
    let Some(writer) = store.writer() else {
        run.slots.clear();
        commands.insert_resource(LastRun::default());
        return;
    };
    // A run that failed is not a run that was abandoned, and the
    // history's completion marker cannot say which: it is simply
    // absent for both.
    let completion = if run.failed {
        Completion::Failed
    } else if completed.is_some() {
        Completion::Completed
    } else {
        Completion::Aborted
    };
    let practice = practice.is_some_and(|practice| practice.used);
    let ended_ms = beatbyte_telemetry::now_ms();
    let closed = std::mem::take(&mut run.slots);
    for slot in &closed {
        writer.finish(*slot, ended_ms, completion, practice);
    }
    // A practised run is recorded and marked, but it is not offered
    // for a verdict: what a player would be rating is the speed.
    commands.insert_resource(LastRun {
        slots: if practice { Vec::new() } else { closed },
    });
    run.detail = None;
    run.failed = false;
    run.last_judged.clear();
}

/// What the run that just ended left open for the player to talk to.
///
/// The results screen speaks after the session is closed, so it
/// cannot ask the live run which slots it had. Empty means "no
/// feedback is offered": nothing was recorded, or the run used
/// practice — feedback about a practice run is feedback about the
/// speed, not about the chart.
#[derive(Resource, Default)]
pub struct LastRun {
    /// The slots whose sessions are still there to be annotated.
    pub slots: Vec<u8>,
}

impl LastRun {
    /// Whether there is anything to say something about.
    #[must_use]
    pub fn open(&self) -> bool {
        !self.slots.is_empty()
    }
}

/// Record what the player said about the run that just ended.
///
/// Takes the older layer's vocabulary because that is what the
/// results screen speaks, and translates: one sentence, one rating
/// or one verdict is the same fact whichever shape it arrives in.
pub fn append_feedback(store: &TelemetryStore, last: &LastRun, line: &NoteLine) {
    let Some(writer) = store.writer() else {
        return;
    };
    let note = match line {
        NoteLine::Fun { fun } => beatbyte_telemetry::PlayerNote::Fun(*fun),
        NoteLine::Comment { comment } => beatbyte_telemetry::PlayerNote::Comment(comment.clone()),
        NoteLine::Versus { versus, parent } => beatbyte_telemetry::PlayerNote::Versus {
            better: versus == "better",
            parent: parent.clone(),
        },
        // The observation kinds are recorded as they happen; only the
        // player's own words arrive this way.
        _ => return,
    };
    for slot in &last.slots {
        writer.note(*slot, note.clone());
    }
}

/// Stop the worker and let it commit what it holds.
///
/// `Drop` does this too, but an explicit stop happens somewhere a log
/// line can still be written, and before Bevy tears the world down.
fn close_store(mut store: ResMut<TelemetryStore>, mut exit: MessageReader<AppExit>) {
    if exit.read().next().is_none() {
        return;
    }
    if let Some(writer) = store.writer.as_mut() {
        let stats = writer.stats();
        writer.shutdown();
        info!(
            "telemetry: {} events written, {} dropped, {} in the store",
            stats.written, stats.dropped, stats.events
        );
    }
}

/// The store's systems.
pub struct TelemetryStorePlugin;

impl Plugin for TelemetryStorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TelemetryStore>()
            .init_resource::<StoreRun>()
            .init_resource::<LastRun>()
            // Also registered by the gameplay plugin; declaring it
            // here too costs nothing and removes a hidden ordering
            // dependency between two plugins that are otherwise
            // independent.
            .add_message::<SessionFeedback>()
            .add_systems(
                Update,
                begin_store_session.run_if(in_state(AppState::Gameplay)),
            )
            .add_systems(
                Update,
                record_to_store
                    .after(crate::gameplay::drain_feedback)
                    .run_if(in_state(AppState::Gameplay)),
            )
            .add_systems(Update, watch_for_seeks.run_if(in_state(GamePhase::Playing)))
            .add_systems(OnEnter(GamePhase::Paused), record_pause)
            .add_systems(OnExit(GamePhase::Paused), record_resume)
            .add_systems(OnExit(AppState::Gameplay), finish_store_session)
            .add_systems(Update, close_store);
    }
}

// ── Vocal telemetry (ADR-0018 §25) ──────────────────────────────────
//
// Results only. No microphone audio is stored, ever: a sung note
// becomes a cent error, an onset error and a grade, and the samples
// it was measured from are gone by the next frame. That is a privacy
// rule and a size rule at once — a minute of raw input would outweigh
// a thousand sessions of everything else.

/// Turn one vocal verdict into the row that records it.
///
/// Pure, so the mapping can be pinned without a microphone. A note
/// carries its own place in the part ([`beatbyte_core::VocalPart::flat_index`]),
/// its pitch error in cents and its onset error; a phrase carries its
/// index and its grade. Coverage and stability are deliberately not
/// stored per note: the grade already weighs them, and two more
/// columns on the hottest table would buy a number nobody has asked a
/// question about yet.
#[must_use]
pub fn vocal_event(
    event: &beatbyte_core::VocalEvent,
    part: &beatbyte_core::VocalPart,
    song_time_s: f64,
    assisted: bool,
) -> Option<Event> {
    let mut flags = Flags::VOCAL;
    if assisted {
        flags = flags.with(Flags::ASSISTED);
    }
    let stamp = beatbyte_telemetry::model::micros(song_time_s);
    match event {
        beatbyte_core::VocalEvent::Note {
            phrase,
            note,
            outcome,
        } => {
            let index = part.flat_index(*phrase, *note)?;
            let mut row = Event::new(EventType::VocalNote, stamp)
                .about(index)
                .judged(vocal_rating(outcome.grade))
                .flagged(flags);
            if let Some(cents) = outcome.mean_abs_cents {
                row = row.valued(cents.round() as i64);
            }
            if let Some(onset) = outcome.onset_error_s {
                row = row.off_by(f64::from(onset));
            }
            Some(row)
        }
        beatbyte_core::VocalEvent::Phrase(outcome) => Some(
            Event::new(EventType::VocalPhrase, stamp)
                .about(outcome.index)
                .judged(vocal_rating(outcome.grade))
                // The phrase's own number: how many of its notes were
                // hit. The total is in the chart, so the share is a
                // join and not a second column.
                .valued(outcome.notes_hit as i64)
                .flagged(flags),
        ),
    }
}

/// A sung grade as the store's rating.
///
/// Five grades into four: `Weak` and `Good` both land on `Good`,
/// because the store's scale is the one the fretboard judges on and
/// inventing a fifth for one instrument would make every cross-
/// instrument query lie. The exact grade is recoverable from the
/// cents and the coverage anyway.
#[must_use]
fn vocal_rating(grade: beatbyte_core::VocalGrade) -> Rating {
    match grade {
        beatbyte_core::VocalGrade::Miss => Rating::Miss,
        beatbyte_core::VocalGrade::Weak | beatbyte_core::VocalGrade::Good => Rating::Good,
        beatbyte_core::VocalGrade::Great => Rating::Great,
        beatbyte_core::VocalGrade::Perfect => Rating::Perfect,
    }
}

/// Record one vocal row against the singing player.
///
/// Slot zero: one microphone, one singer. A duet is a format change
/// away and would bring its own slot with it.
pub fn record_vocal(store: Option<&TelemetryStore>, run: Option<&StoreRun>, event: Event) {
    let (Some(store), Some(run)) = (store, run) else {
        return;
    };
    let Some(writer) = store.writer() else {
        return;
    };
    if !run.records(event.kind) || !run.open(0) {
        return;
    }
    writer.record(0, event);
}

#[cfg(test)]
mod store_tests {
    use super::*;
    use beatbyte_core::{Lane, LaneSet, NoteEvent, NoteKind};

    #[test]
    fn a_guitar_is_a_pad_with_a_telling_name() {
        let pad = Entity::from_raw_u32(3).expect("a valid entity index");
        assert_eq!(
            device_kind(DeviceId::Keyboard, None),
            InputDevice::Keyboard,
            "the keyboard is never a pad, whatever is plugged in"
        );
        assert_eq!(
            device_kind(DeviceId::Pad(pad), Some("Xbox 360 Controller")),
            InputDevice::Gamepad
        );
        for name in [
            "Guitar Hero X-plorer",
            "X-PLORER",
            "Rock Band Stratocaster",
            "wireless legacy guitar",
        ] {
            assert_eq!(
                device_kind(DeviceId::Pad(pad), Some(name)),
                InputDevice::Guitar,
                "{name} is a guitar"
            );
        }
        assert_eq!(
            device_kind(DeviceId::Pad(pad), None),
            InputDevice::Gamepad,
            "a nameless pad is a pad, not a guess"
        );
    }

    fn a_note(lanes: &[Lane], sustain: f64, kind: NoteKind) -> NoteEvent {
        NoteEvent {
            time_s: 1.0,
            lanes: LaneSet::from_lanes(lanes.iter().copied()),
            sustain_s: sustain,
            kind,
        }
    }

    #[test]
    fn a_judgment_becomes_a_row_that_carries_the_note_s_shape() {
        let chord = a_note(&[Lane::One, Lane::Two], 0.5, NoteKind::Hopo);
        let event = event_for(
            &SessionEvent::NoteHit {
                event_index: 12,
                judgment: Judgment::Great,
                offset_s: -0.018,
            },
            5_000_000,
            Some(&chord),
        )
        .expect("a hit is recorded");
        assert_eq!(event.kind, EventType::NoteHit);
        assert_eq!(event.note_index, Some(12));
        assert_eq!(event.delta_us, Some(-18_000));
        assert_eq!(event.rating, Some(Rating::Great));
        assert!(event.flags.has(Flags::CHORD));
        assert!(event.flags.has(Flags::HOPO));
        assert!(event.flags.has(Flags::SUSTAIN));
    }

    #[test]
    fn a_hit_judged_as_a_miss_is_recorded_as_a_miss() {
        // The session emits `NoteHit { judgment: Miss }` for a strum
        // that landed outside every window. Recording it as a hit
        // would make the miss rate of every chart wrong.
        let event = event_for(
            &SessionEvent::NoteHit {
                event_index: 3,
                judgment: Judgment::Miss,
                offset_s: 0.2,
            },
            0,
            None,
        )
        .expect("recorded");
        assert_eq!(event.kind, EventType::NoteMiss);
        assert_eq!(event.rating, Some(Rating::Miss));
    }

    #[test]
    fn a_sustain_says_whether_it_was_held_out() {
        let held = event_for(
            &SessionEvent::SustainEnded {
                event_index: 7,
                completed: true,
            },
            0,
            None,
        )
        .expect("recorded");
        assert!(held.flags.has(Flags::DONE));
        let dropped = event_for(
            &SessionEvent::SustainEnded {
                event_index: 7,
                completed: false,
            },
            0,
            None,
        )
        .expect("recorded");
        assert!(!dropped.flags.has(Flags::DONE));
    }

    #[test]
    fn what_is_derivable_is_not_recorded() {
        // ADR-0018: a sustain starts when a note with a tail is hit,
        // and a phrase is completed or broken by the judgments inside
        // its span. Storing them would be storing the same fact twice
        // and inviting the two copies to disagree.
        for event in [
            SessionEvent::SustainStarted { event_index: 1 },
            SessionEvent::PhraseCompleted { phrase_index: 0 },
            SessionEvent::PhraseBroken { phrase_index: 0 },
        ] {
            assert!(
                event_for(&event, 0, None).is_none(),
                "{event:?} is derivable and must not be stored"
            );
        }
        // …and what is NOT derivable is.
        for event in [
            SessionEvent::Overstrum,
            SessionEvent::HypeActivated,
            SessionEvent::HypeEnded,
            SessionEvent::Failed,
        ] {
            assert!(event_for(&event, 0, None).is_some(), "{event:?}");
        }
    }

    fn a_part() -> beatbyte_core::VocalPart {
        use beatbyte_core::{VocalKind, VocalNote, VocalPhrase, VocalRole};
        let note = |start: f64, end: f64| VocalNote {
            start_s: start,
            end_s: end,
            kind: VocalKind::Pitched,
            target_midi: Some(60.0),
            contour: Vec::new(),
            confidence: 1.0,
            token_range: None,
        };
        beatbyte_core::VocalPart {
            id: "lead".to_owned(),
            role: VocalRole::Lead,
            name: None,
            phrases: vec![
                VocalPhrase {
                    start_s: 0.0,
                    end_s: 2.0,
                    confidence: 1.0,
                    tokens: Vec::new(),
                    notes: vec![note(0.0, 1.0), note(1.0, 2.0)],
                },
                VocalPhrase {
                    start_s: 3.0,
                    end_s: 4.0,
                    confidence: 1.0,
                    tokens: Vec::new(),
                    notes: vec![note(3.0, 4.0)],
                },
            ],
        }
    }

    fn an_outcome(grade: beatbyte_core::VocalGrade) -> beatbyte_core::NoteOutcome {
        beatbyte_core::NoteOutcome {
            grade,
            kind: beatbyte_core::VocalKind::Pitched,
            mean_abs_cents: Some(23.4),
            onset_error_s: Some(0.042),
            coverage: 0.9,
            stability: Some(0.8),
            score: 0.7,
            points: 100,
        }
    }

    #[test]
    fn a_sung_note_is_stored_as_cents_and_a_grade_and_never_as_audio() {
        let part = a_part();
        let event = vocal_event(
            &beatbyte_core::VocalEvent::Note {
                phrase: 1,
                note: 0,
                outcome: an_outcome(beatbyte_core::VocalGrade::Great),
            },
            &part,
            7.5,
            false,
        )
        .expect("a sung note is recorded");
        assert_eq!(event.kind, EventType::VocalNote);
        assert_eq!(
            event.note_index,
            Some(2),
            "the note's own number in the part, counted through the              phrase boundary"
        );
        assert_eq!(event.value, Some(23), "the pitch error, in cents");
        assert_eq!(event.delta_us, Some(42_000), "and how late the onset was");
        assert_eq!(event.rating, Some(Rating::Great));
        assert!(event.flags.has(Flags::VOCAL));
        assert!(!event.flags.has(Flags::ASSISTED));
        assert_eq!(event.song_time_us, Some(7_500_000));
    }

    #[test]
    fn a_run_the_record_sang_along_to_says_so_on_every_row() {
        let part = a_part();
        let event = vocal_event(
            &beatbyte_core::VocalEvent::Phrase(beatbyte_core::PhraseOutcome {
                index: 1,
                score: 0.8,
                grade: beatbyte_core::VocalGrade::Good,
                notes: 4,
                notes_hit: 3,
                points: 400,
                streak: 2,
                multiplier: 2,
            }),
            &part,
            9.0,
            true,
        )
        .expect("a phrase is recorded");
        assert_eq!(event.kind, EventType::VocalPhrase);
        assert_eq!(event.note_index, Some(1));
        assert_eq!(event.value, Some(3), "how many of its notes were hit");
        assert!(event.flags.has(Flags::ASSISTED));
        assert!(event.flags.has(Flags::VOCAL));
    }

    #[test]
    fn five_sung_grades_land_on_the_four_the_store_has() {
        use beatbyte_core::VocalGrade;
        assert_eq!(vocal_rating(VocalGrade::Miss), Rating::Miss);
        assert_eq!(vocal_rating(VocalGrade::Weak), Rating::Good);
        assert_eq!(
            vocal_rating(VocalGrade::Good),
            Rating::Good,
            "weak and good share a rung; inventing a fifth would make              every cross-instrument query lie"
        );
        assert_eq!(vocal_rating(VocalGrade::Great), Rating::Great);
        assert_eq!(vocal_rating(VocalGrade::Perfect), Rating::Perfect);
    }

    #[test]
    fn a_note_that_is_not_in_the_part_is_recorded_as_nothing() {
        let part = a_part();
        assert!(
            vocal_event(
                &beatbyte_core::VocalEvent::Note {
                    phrase: 9,
                    note: 0,
                    outcome: an_outcome(beatbyte_core::VocalGrade::Perfect),
                },
                &part,
                1.0,
                false,
            )
            .is_none(),
            "a row whose note cannot be named is a row nothing can join"
        );
    }

    #[test]
    fn a_difficulty_is_its_index_in_the_chart_format() {
        assert_eq!(difficulty_index(beatbyte_core::Difficulty::Easy), 0);
        assert_eq!(difficulty_index(beatbyte_core::Difficulty::Medium), 1);
        assert_eq!(difficulty_index(beatbyte_core::Difficulty::Hard), 2);
        assert_eq!(difficulty_index(beatbyte_core::Difficulty::Expert), 3);
    }

    /// The whole path in a real (headless) app: a loaded song, a
    /// player, the plugin's own systems, and a store on disk that has
    /// to contain the run afterwards.
    mod wired {
        use super::super::*;
        use beatbyte_chart::schema::{ChartDef, ChartFile, ChartNote, SongMeta};
        use beatbyte_core::{
            Difficulty, ScoreConfig, TempoMap, TimingWindows, Track, TrackSession,
        };

        fn scratch(name: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "beatbyte-game-telemetry-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a directory");
            dir.join("telemetry.db")
        }

        fn a_song() -> crate::boot::LoadedSong {
            crate::boot::LoadedSong {
                chart: ChartFile {
                    format_version: 1,
                    song: SongMeta {
                        title: "Maria".to_owned(),
                        artist: "Blondie".to_owned(),
                        audio: "maria.mp3".to_owned(),
                        bpm: 120.0,
                        offset_s: 0.0,
                        preview_start_s: None,
                        duration_s: Some(200.0),
                        genre: Some("rock".to_owned()),
                    },
                    charts: vec![ChartDef {
                        difficulty: Difficulty::Medium,
                        lanes: 5,
                        notes: vec![ChartNote {
                            time: 1.0,
                            lane: 0,
                            len: 0.0,
                            hopo: false,
                        }],
                        phrases: Vec::new(),
                    }],
                    provenance: None,
                    audio_trim: None,
                    grid: None,
                },
                audio: crate::boot::SongAudio::File(std::path::PathBuf::from("maria.mp3")),
                lyrics: None,
                vocals: None,
                lyric_offset_ms: 0,
            }
        }

        fn app_with(level: TelemetryLevel, path: &std::path::Path) -> App {
            let mut app = App::new();
            app.add_plugins(bevy::state::app::StatesPlugin)
                // What `DefaultPlugins` registers in the real app.
                .add_message::<AppExit>()
                .init_resource::<Time>()
                .init_resource::<crate::config::Settings>()
                .init_resource::<crate::audio_sys::GameClock>()
                .init_state::<AppState>()
                .init_state::<GamePhase>()
                .add_plugins(TelemetryStorePlugin);
            app.world_mut()
                .resource_mut::<crate::config::Settings>()
                .telemetry = level;
            app.insert_resource(a_song());
            app.insert_resource(crate::song_select::SelectedDifficulty(Difficulty::Medium));
            app.world_mut()
                .resource_mut::<crate::audio_sys::GameClock>()
                .begin(0.0, 0.0);
            let track = Track::new(
                Difficulty::Medium,
                TempoMap::constant(120.0, 0.0),
                vec![],
                vec![],
            )
            .expect("an empty track is a track");
            app.world_mut().spawn((
                PlayerIndex(0),
                PlayerDevice(DeviceId::Keyboard),
                PlayerSession {
                    session: TrackSession::new(
                        track,
                        TimingWindows::default(),
                        ScoreConfig::default(),
                    ),
                    frame_events: Vec::new(),
                    spawn_cursor: 0,
                },
            ));
            // The store opens where the test says, not in the player's
            // data directory.
            app.insert_resource(TelemetryStore {
                writer: Telemetry::open(path.to_path_buf()).ok(),
                ..TelemetryStore::default()
            });
            app.world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(AppState::Gameplay);
            app.update();
            app
        }

        fn one_player(app: &mut App) -> Entity {
            let mut query = app
                .world_mut()
                .query_filtered::<Entity, With<PlayerSession>>();
            let entity = query.iter(app.world()).next();
            entity.expect("the one player")
        }

        fn stored(path: &std::path::Path) -> Vec<beatbyte_telemetry::StoredSession> {
            beatbyte_telemetry::Store::open(path)
                .expect("reopens")
                .sessions(10)
                .expect("lists")
        }

        #[test]
        fn a_run_opens_a_session_that_names_what_it_was_played_under() {
            let path = scratch("session");
            let mut app = app_with(TelemetryLevel::Actions, &path);
            // Two frames: the session opens on the first that has a
            // player, and the event goes down on the second.
            let player = one_player(&mut app);
            app.world_mut().write_message(SessionFeedback {
                player,
                player_index: 0,
                event: SessionEvent::Overstrum,
            });
            app.update();
            app.world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(AppState::SongSelect);
            app.update();
            app.world_mut().remove_resource::<TelemetryStore>();

            let sessions = stored(&path);
            assert_eq!(sessions.len(), 1, "one player, one session");
            let session = &sessions[0];
            assert_eq!(session.row.title, "Maria");
            assert_eq!(session.row.artist, "Blondie");
            assert_eq!(session.row.genre.as_deref(), Some("rock"));
            assert_eq!(session.row.difficulty, 1);
            assert_eq!(session.row.input_device, InputDevice::Keyboard);
            assert_eq!(
                session.row.provenance.scoring,
                beatbyte_core::score::SCORING_VERSION
            );
            assert_eq!(session.row.provenance.game, env!("CARGO_PKG_VERSION"));
            assert_eq!(
                session.completion,
                Completion::Aborted,
                "leaving without the completion marker is leaving"
            );
            assert!(session.complete, "nothing was dropped");
            let store = beatbyte_telemetry::Store::open(&path).expect("reopens");
            let events = store.events(session.id).expect("reads");
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].1.kind, EventType::Overstrum);
            let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
        }

        /// An overstrum is a strum that matched nothing, so it has
        /// no note of its own — and without one it cannot be placed
        /// in the song at all. The last judged note is the anchor.
        #[test]
        fn an_overstrum_is_anchored_to_the_note_it_followed() {
            let path = scratch("anchor");
            let mut app = app_with(TelemetryLevel::Actions, &path);
            let player = one_player(&mut app);
            for event in [
                SessionEvent::NoteHit {
                    event_index: 41,
                    judgment: Judgment::Perfect,
                    offset_s: 0.0,
                },
                SessionEvent::Overstrum,
                SessionEvent::NoteMissed { event_index: 42 },
                SessionEvent::Overstrum,
            ] {
                app.world_mut().write_message(SessionFeedback {
                    player,
                    player_index: 0,
                    event,
                });
            }
            app.update();
            app.world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(AppState::SongSelect);
            app.update();
            app.world_mut().remove_resource::<TelemetryStore>();

            let store = beatbyte_telemetry::Store::open(&path).expect("reopens");
            let session = store.sessions(1).expect("lists").remove(0);
            let anchors: Vec<Option<u32>> = store
                .events(session.id)
                .expect("reads")
                .into_iter()
                .filter(|(_, event)| event.kind == EventType::Overstrum)
                .map(|(_, event)| event.note_index)
                .collect();
            assert_eq!(
                anchors,
                vec![Some(41), Some(42)],
                "each overstrum is placed at the note it followed"
            );
            let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
        }

        #[test]
        fn nothing_is_recorded_when_the_player_turned_it_off() {
            let path = scratch("off");
            let mut app = app_with(TelemetryLevel::Off, &path);
            let player = one_player(&mut app);
            app.world_mut().write_message(SessionFeedback {
                player,
                player_index: 0,
                event: SessionEvent::Overstrum,
            });
            app.update();
            app.world_mut().remove_resource::<TelemetryStore>();
            assert!(
                stored(&path).is_empty(),
                "OFF means no session at all, not an empty one"
            );
            let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
        }
    }
}
