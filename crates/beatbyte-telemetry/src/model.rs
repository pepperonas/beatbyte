//! The telemetry vocabulary: what a session is, what an event is, and
//! the compact integer encodings both are stored in.
//!
//! Everything here is pure — no database, no clock, no I/O — so the
//! encodings can be pinned with plain values. That matters more than
//! usual: an enum whose numbers move silently reinterprets every row
//! ever written, and nothing about a wrong number looks wrong.
//!
//! # Why integers
//!
//! A million rows of `"NOTE_HIT"` and `"PERFECT"` is a million rows of
//! the same two words. Every repeated field is a `u8` with a stable
//! code; the words live here, once, in [`EventType::label`] and
//! friends, which is also what an export writes.

use beatbyte_core::{InputKind, Judgment, Lane, NoteEvent};

// ── Time ────────────────────────────────────────────────────────────

/// Song time as stored: microseconds on the song timeline.
///
/// The engine runs on `f64` seconds (ADR-0004) and keeps doing so;
/// this is the one conversion, at the telemetry boundary. A
/// microsecond is two orders of magnitude under the tightest hit
/// window, so nothing measurable is lost — and an integer cannot
/// drift by an ULP between writing and reading it back, which a
/// float demonstrably does in this repository.
///
/// A non-finite input is `0`: telemetry may not panic, and a NaN
/// timestamp is not evidence of anything.
#[must_use]
pub fn micros(seconds: f64) -> i64 {
    if !seconds.is_finite() {
        return 0;
    }
    let scaled = (seconds * 1e6).round();
    // `as` saturates on the way to an integer in Rust 2021+, but the
    // clamp says the intent rather than relying on that.
    scaled.clamp(i64::MIN as f64, i64::MAX as f64) as i64
}

/// A signed timing offset as stored: microseconds, `i32`.
///
/// `i32` microseconds spans ±35 minutes, which is several orders of
/// magnitude more than any offset a hit window can produce; the
/// narrower type is what keeps an event row small.
#[must_use]
pub fn delta_micros(seconds: f64) -> i32 {
    if !seconds.is_finite() {
        return 0;
    }
    let scaled = (seconds * 1e6).round();
    scaled.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

/// Microseconds back to seconds — what a reader uses.
#[must_use]
pub fn seconds(micros: i64) -> f64 {
    micros as f64 / 1e6
}

// ── Event types ─────────────────────────────────────────────────────

/// What kind of thing happened.
///
/// The codes are **stable on disk** and deliberately sparse: a new
/// event slots into its family's gap without renumbering anything
/// that was written before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum EventType {
    /// A logical action reached the session (fret, strum, hype). The
    /// only evidence that the player did something the engine then
    /// did nothing with.
    Action = 10,
    /// A note event was judged as a hit.
    NoteHit = 20,
    /// A note event's window passed unhit.
    NoteMiss = 21,
    /// A strum matched no note.
    Overstrum = 22,
    /// A sustain tail ended — held out or dropped (see [`Flags::DONE`]).
    SustainEnded = 31,
    /// Hype was activated.
    HypeActivated = 50,
    /// Hype ran out.
    HypeEnded = 51,
    /// The song was paused.
    Paused = 60,
    /// The song resumed.
    Resumed = 61,
    /// The playhead was moved (practice loop, seek).
    Seek = 62,
    /// The rock meter emptied with failing armed.
    Failed = 70,
    /// One sung note was judged.
    VocalNote = 80,
    /// One sung phrase was judged.
    VocalPhrase = 81,
    /// A calibration offset changed mid-session.
    CalibrationChanged = 90,
    /// A physical button edge, before the mapping table. Only written
    /// at [`Detail::Diagnostic`].
    RawInput = 99,
}

impl EventType {
    /// Every event type, in code order — what a reader iterates and
    /// what pins the round trip.
    pub const ALL: [EventType; 15] = [
        EventType::Action,
        EventType::NoteHit,
        EventType::NoteMiss,
        EventType::Overstrum,
        EventType::SustainEnded,
        EventType::HypeActivated,
        EventType::HypeEnded,
        EventType::Paused,
        EventType::Resumed,
        EventType::Seek,
        EventType::Failed,
        EventType::VocalNote,
        EventType::VocalPhrase,
        EventType::CalibrationChanged,
        EventType::RawInput,
    ];

    /// The stored code.
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// Read a stored code back. `None` for a code this build does not
    /// know — a reader from an older schema must survive a row a
    /// later one wrote.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<EventType> {
        match code {
            10 => Some(EventType::Action),
            20 => Some(EventType::NoteHit),
            21 => Some(EventType::NoteMiss),
            22 => Some(EventType::Overstrum),
            31 => Some(EventType::SustainEnded),
            50 => Some(EventType::HypeActivated),
            51 => Some(EventType::HypeEnded),
            60 => Some(EventType::Paused),
            61 => Some(EventType::Resumed),
            62 => Some(EventType::Seek),
            70 => Some(EventType::Failed),
            80 => Some(EventType::VocalNote),
            81 => Some(EventType::VocalPhrase),
            90 => Some(EventType::CalibrationChanged),
            99 => Some(EventType::RawInput),
            _ => None,
        }
    }

    /// The name an export writes. Never stored in a row.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            EventType::Action => "action",
            EventType::NoteHit => "note_hit",
            EventType::NoteMiss => "note_miss",
            EventType::Overstrum => "overstrum",
            EventType::SustainEnded => "sustain_ended",
            EventType::HypeActivated => "hype_activated",
            EventType::HypeEnded => "hype_ended",
            EventType::Paused => "paused",
            EventType::Resumed => "resumed",
            EventType::Seek => "seek",
            EventType::Failed => "failed",
            EventType::VocalNote => "vocal_note",
            EventType::VocalPhrase => "vocal_phrase",
            EventType::CalibrationChanged => "calibration_changed",
            EventType::RawInput => "raw_input",
        }
    }

    /// Whether this kind is written at the given detail level.
    ///
    /// The one place the level is interpreted, so a new event type
    /// cannot accidentally escape the setting the player chose.
    #[must_use]
    pub const fn recorded_at(self, detail: Detail) -> bool {
        match self {
            EventType::RawInput => matches!(detail, Detail::Diagnostic),
            EventType::Action => !matches!(detail, Detail::Results),
            _ => true,
        }
    }
}

// ── Detail level ────────────────────────────────────────────────────

/// How much of the input path is kept.
///
/// The blackbox is complete at [`Detail::Actions`]: every decision the
/// engine made and every action it was asked to make one about.
/// [`Detail::Diagnostic`] adds the physical layer — which device,
/// which button — and roughly doubles the row count, so it is what
/// you switch on when a controller is suspect, not what you run with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Detail {
    /// Only what the engine decided: hits, misses, overstrums, holds.
    Results,
    /// …plus every logical action. The default.
    #[default]
    Actions,
    /// …plus the physical button edges behind them.
    Diagnostic,
}

impl Detail {
    /// The stored code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Detail::Results => 0,
            Detail::Actions => 1,
            Detail::Diagnostic => 2,
        }
    }

    /// Read a stored code back; anything unknown is the default.
    #[must_use]
    pub const fn from_code(code: u8) -> Detail {
        match code {
            0 => Detail::Results,
            2 => Detail::Diagnostic,
            _ => Detail::Actions,
        }
    }

    /// The lowercase name used in settings files and on screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Detail::Results => "results",
            Detail::Actions => "actions",
            Detail::Diagnostic => "diagnostic",
        }
    }
}

// ── Actions ─────────────────────────────────────────────────────────

/// A logical gameplay action, as stored.
///
/// Fret edges carry their lane in the code itself (`0..=4` down,
/// `5..=9` up), so an action needs no second column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A fret went down.
    FretDown(Lane),
    /// A fret came up.
    FretUp(Lane),
    /// The strum bar was hit.
    Strum,
    /// Hype activation was asked for.
    Hype,
}

impl Action {
    /// The stored code.
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Action::FretDown(lane) => lane as u8,
            Action::FretUp(lane) => 5 + lane as u8,
            Action::Strum => 10,
            Action::Hype => 11,
        }
    }

    /// Read a stored code back.
    #[must_use]
    pub fn from_code(code: u8) -> Option<Action> {
        match code {
            0..=4 => Lane::from_index(code as usize).map(Action::FretDown),
            5..=9 => Lane::from_index((code - 5) as usize).map(Action::FretUp),
            10 => Some(Action::Strum),
            11 => Some(Action::Hype),
            _ => None,
        }
    }

    /// The action behind one of the engine's inputs.
    #[must_use]
    pub fn from_input(kind: InputKind) -> Action {
        match kind {
            InputKind::FretDown(lane) => Action::FretDown(lane),
            InputKind::FretUp(lane) => Action::FretUp(lane),
            InputKind::Strum => Action::Strum,
            InputKind::ActivateHype => Action::Hype,
        }
    }

    /// The name an export writes.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Action::FretDown(lane) => format!("fret{}_down", lane.index() + 1),
            Action::FretUp(lane) => format!("fret{}_up", lane.index() + 1),
            Action::Strum => "strum".to_owned(),
            Action::Hype => "hype".to_owned(),
        }
    }
}

// ── Ratings ─────────────────────────────────────────────────────────

/// A judgment, as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Rating {
    /// Dead on.
    Perfect = 0,
    /// Inside the great window.
    Great = 1,
    /// Inside the good window.
    Good = 2,
    /// Outside every window.
    Miss = 3,
}

impl Rating {
    /// The stored code.
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// Read a stored code back.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Rating> {
        match code {
            0 => Some(Rating::Perfect),
            1 => Some(Rating::Great),
            2 => Some(Rating::Good),
            3 => Some(Rating::Miss),
            _ => None,
        }
    }

    /// The name an export writes — the same words the JSONL layer
    /// used, so an analysis reads the same either way.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Rating::Perfect => "perfect",
            Rating::Great => "great",
            Rating::Good => "good",
            Rating::Miss => "miss",
        }
    }
}

impl From<Judgment> for Rating {
    fn from(judgment: Judgment) -> Rating {
        match judgment {
            Judgment::Perfect => Rating::Perfect,
            Judgment::Great => Rating::Great,
            Judgment::Good => Rating::Good,
            Judgment::Miss => Rating::Miss,
        }
    }
}

// ── Flags ───────────────────────────────────────────────────────────

/// What kind of note this was, packed into the row that is there
/// anyway.
///
/// These describe the *chart*, and the chart could be read instead —
/// which is exactly the argument against storing them. They are
/// stored regardless, deliberately: a redesigned song's old chart
/// version is deleted by the rollover, and an analysis six months
/// from now must still be able to ask whether chords fail more often
/// than single notes. Nothing derivable from the telemetry *itself*
/// is stored (no combo, no score, no early/late — `delta_us` has the
/// sign).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags(pub u16);

impl Flags {
    /// Nothing set.
    pub const NONE: Flags = Flags(0);
    /// More than one lane — a chord.
    pub const CHORD: Flags = Flags(1 << 0);
    /// A hammer-on / pull-off.
    pub const HOPO: Flags = Flags(1 << 1);
    /// The note carries a sustain tail.
    pub const SUSTAIN: Flags = Flags(1 << 2);
    /// A sustain was held to its end (on [`EventType::SustainEnded`]).
    pub const DONE: Flags = Flags(1 << 3);
    /// The event belongs to the vocal part, not the fretboard.
    pub const VOCAL: Flags = Flags(1 << 4);
    /// The run had help the score does not show (the original singer
    /// audible in the room, autopilot, …).
    pub const ASSISTED: Flags = Flags(1 << 5);

    /// Both sets together.
    #[must_use]
    pub const fn with(self, other: Flags) -> Flags {
        Flags(self.0 | other.0)
    }

    /// Whether every bit of `other` is set here.
    #[must_use]
    pub const fn has(self, other: Flags) -> bool {
        self.0 & other.0 == other.0
    }

    /// The flags a note event implies.
    #[must_use]
    pub fn of_note(note: &NoteEvent) -> Flags {
        let mut flags = Flags::NONE;
        if note.lanes.len() > 1 {
            flags = flags.with(Flags::CHORD);
        }
        if note.kind == beatbyte_core::NoteKind::Hopo {
            flags = flags.with(Flags::HOPO);
        }
        if note.sustain_s > 0.0 {
            flags = flags.with(Flags::SUSTAIN);
        }
        flags
    }
}

// ── Session facts ───────────────────────────────────────────────────

/// Which kind of thing the player held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum InputDevice {
    /// Not established.
    #[default]
    Unknown = 0,
    /// The computer keyboard.
    Keyboard = 1,
    /// A generic gamepad.
    Gamepad = 2,
    /// A guitar controller.
    Guitar = 3,
}

impl InputDevice {
    /// The stored code.
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// Read a stored code back; anything unknown reads as unknown,
    /// which is the honest answer.
    #[must_use]
    pub const fn from_code(code: u8) -> InputDevice {
        match code {
            1 => InputDevice::Keyboard,
            2 => InputDevice::Gamepad,
            3 => InputDevice::Guitar,
            _ => InputDevice::Unknown,
        }
    }

    /// The name an export writes.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            InputDevice::Unknown => "unknown",
            InputDevice::Keyboard => "keyboard",
            InputDevice::Gamepad => "gamepad",
            InputDevice::Guitar => "guitar",
        }
    }
}

/// How a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Completion {
    /// Still open, or the process died before it could be closed.
    #[default]
    Unknown = 0,
    /// Played to the end.
    Completed = 1,
    /// The rock meter emptied.
    Failed = 2,
    /// Left before the end.
    Aborted = 3,
}

impl Completion {
    /// The stored code.
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// Read a stored code back.
    #[must_use]
    pub const fn from_code(code: u8) -> Completion {
        match code {
            1 => Completion::Completed,
            2 => Completion::Failed,
            3 => Completion::Aborted,
            _ => Completion::Unknown,
        }
    }

    /// The name an export writes.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Completion::Unknown => "unknown",
            Completion::Completed => "completed",
            Completion::Failed => "failed",
            Completion::Aborted => "aborted",
        }
    }
}

/// The versions a session was played under.
///
/// Every one of them is a reason two sessions may not be compared
/// blindly. They are a struct of their own so that adding one is a
/// visible change at every construction site rather than a silently
/// defaulted field.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Provenance {
    /// The game version that recorded it.
    pub game: String,
    /// The chart format version the chart was written in.
    pub chart_format: u32,
    /// Who designed this chart version, from the chart's provenance
    /// (`None` for an import's own first draft).
    pub generator: Option<String>,
    /// The scoring rules in force.
    pub scoring: u32,
    /// The analysis pipeline the chart was made from, when the chart
    /// says.
    pub analysis: Option<u32>,
    /// The vocal pipeline, for a run that sang.
    pub vocal: Option<u32>,
}

/// Everything about a run that is true for the whole run.
///
/// Written once, at the start; only the outcome is filled in at the
/// end. Nothing in here repeats per event — that is the whole point
/// (the event table references the session and the chart, and joins
/// the rest).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    /// A locally unique id for this run, stable across exports.
    pub uid: String,
    /// Unix milliseconds at the start.
    pub started_ms: u64,
    /// Song title, on its own — never joined with the artist.
    pub title: String,
    /// Song artist.
    pub artist: String,
    /// Genre, when the song says.
    pub genre: Option<String>,
    /// Content hash of the exact chart that was played. Chart
    /// identity and chart version in one value.
    pub chart_hash: String,
    /// The chart file that hash came from, for a human reading a row.
    pub chart_file: Option<String>,
    /// Difficulty index (0 easy … 3 expert).
    pub difficulty: u8,
    /// Player slot within the run (solo = 0).
    pub player_slot: u8,
    /// The roster player, when the run was attributed to one.
    pub player_id: Option<u64>,
    /// What the run was played under.
    pub provenance: Provenance,
    /// The schema version of the store at the time of writing.
    pub telemetry_schema: u32,
    /// How much of the input path this run recorded.
    pub detail: Detail,
    /// What the player held.
    pub input_device: InputDevice,
    /// Input calibration in milliseconds. `None` only for a run
    /// imported from a file that never recorded it — a zero would
    /// read as "calibrated to zero", which is a different claim.
    pub input_offset_ms: Option<f32>,
    /// Video offset in milliseconds (presentation only).
    pub video_offset_ms: Option<f32>,
    /// Microphone round-trip offset, for a run that sang.
    pub mic_offset_ms: Option<f32>,
    /// Tap mode: notes hit on fret press alone.
    pub tap_mode: bool,
    /// No Fail armed.
    pub no_fail: bool,
    /// Practice was used at some point in the run.
    pub practice: bool,
    /// The autopilot was driving.
    pub autopilot: bool,
    /// Note events in the played track — the denominator every
    /// completion figure needs.
    pub notes_total: u32,
}

/// How a run ended, written when it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// Unix milliseconds at the end.
    pub ended_ms: u64,
    /// How it ended.
    pub completion: Completion,
    /// How many events were dropped because the queue was full.
    pub dropped: u32,
}

// ── Events ──────────────────────────────────────────────────────────

/// One recorded thing.
///
/// Deliberately flat and mostly empty: a hit uses four of the fields,
/// a pause uses one. A row of `NULL`s costs a bit each in SQLite, and
/// a shape that cannot hold the next event type would cost a
/// migration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    /// Song time in microseconds — `None` when it genuinely is not
    /// known, which is the case for every observation imported from
    /// the older JSONL files: those recorded which note, never when.
    /// A zero would be a lie a time-based query cannot see through.
    pub song_time_us: Option<i64>,
    /// What happened.
    pub kind: EventType,
    /// The note event this is about — an index into the played
    /// track's events, which `(chart_hash, difficulty)` makes
    /// historically unambiguous.
    pub note_index: Option<u32>,
    /// The logical action, for the kinds that have one.
    pub action: Option<Action>,
    /// Signed `input - note` offset in microseconds.
    pub delta_us: Option<i32>,
    /// The judgment earned.
    pub rating: Option<Rating>,
    /// The one general-purpose number: a phrase index, a pitch error
    /// in cents, a seek target in microseconds, a calibration value.
    /// What it means is fixed per [`EventType`] and documented there.
    pub value: Option<i64>,
    /// What kind of note this was.
    pub flags: Flags,
}

impl Event {
    /// A bare event of this kind at this time; fill in what applies.
    #[must_use]
    pub const fn new(kind: EventType, song_time_us: i64) -> Event {
        Event {
            song_time_us: Some(song_time_us),
            kind,
            note_index: None,
            action: None,
            delta_us: None,
            rating: None,
            value: None,
            flags: Flags::NONE,
        }
    }

    /// An event whose place on the song timeline is unknown — what
    /// the legacy importer writes, and nothing else.
    #[must_use]
    pub const fn untimed(kind: EventType) -> Event {
        Event {
            song_time_us: None,
            kind,
            note_index: None,
            action: None,
            delta_us: None,
            rating: None,
            value: None,
            flags: Flags::NONE,
        }
    }

    /// Set the note this event is about.
    #[must_use]
    pub const fn about(mut self, note_index: usize) -> Event {
        self.note_index = Some(note_index as u32);
        self
    }

    /// Set the logical action.
    #[must_use]
    pub const fn acting(mut self, action: Action) -> Event {
        self.action = Some(action);
        self
    }

    /// Set the timing offset, in seconds as the engine has it.
    #[must_use]
    pub fn off_by(mut self, offset_s: f64) -> Event {
        self.delta_us = Some(delta_micros(offset_s));
        self
    }

    /// Set the judgment.
    #[must_use]
    pub const fn judged(mut self, rating: Rating) -> Event {
        self.rating = Some(rating);
        self
    }

    /// Set the general-purpose value.
    #[must_use]
    pub const fn valued(mut self, value: i64) -> Event {
        self.value = Some(value);
        self
    }

    /// Add flags.
    #[must_use]
    pub const fn flagged(mut self, flags: Flags) -> Event {
        self.flags = self.flags.with(flags);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_conversion_survives_what_a_clock_can_hand_it() {
        assert_eq!(micros(1.5), 1_500_000);
        assert_eq!(micros(-0.000_001), -1);
        assert_eq!(
            micros(0.000_000_4),
            0,
            "rounds, never truncates toward -inf"
        );
        // A clock that has not started, a stem that failed to decode,
        // a division by a zero tempo: all of them reach a recorder as
        // a non-finite float, and none of them may panic or store a
        // number that later reads as a real timestamp.
        assert_eq!(micros(f64::NAN), 0);
        assert_eq!(
            micros(f64::INFINITY),
            0,
            "an infinity is not a late timestamp, it is no timestamp"
        );
        assert_eq!(micros(f64::NEG_INFINITY), 0);
        assert_eq!(delta_micros(f64::NAN), 0);
        assert_eq!(delta_micros(f64::INFINITY), 0);
        // A finite number that is merely absurd still saturates
        // rather than wrapping into a plausible small one.
        assert_eq!(micros(1e30), i64::MAX);
        assert_eq!(micros(-1e30), i64::MIN);
        assert_eq!(delta_micros(-0.018), -18_000, "18 ms early");
        // ±35 minutes is the i32 range; anything past it saturates
        // rather than wrapping into a plausible small number.
        assert_eq!(delta_micros(1e9), i32::MAX);
        assert_eq!(delta_micros(-1e9), i32::MIN);
        assert!((seconds(1_500_000) - 1.5).abs() < 1e-12);
    }

    #[test]
    fn every_event_code_reads_back_as_itself() {
        for kind in EventType::ALL {
            assert_eq!(
                EventType::from_code(kind.code()),
                Some(kind),
                "{} does not round-trip",
                kind.label()
            );
        }
        // Distinct codes: two types sharing one would silently merge
        // every row of the newer into the older.
        let mut codes: Vec<u8> = EventType::ALL.iter().map(|k| k.code()).collect();
        codes.sort_unstable();
        let count = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), count, "two event types share a code");
        assert_eq!(
            EventType::from_code(200),
            None,
            "a code from a later schema must read as unknown, not as \
             some other event"
        );
    }

    /// The codes are the schema. This pins the actual numbers, not
    /// merely that they round-trip: a reordering of the enum would
    /// pass the round-trip test and reinterpret every row on disk.
    #[test]
    fn the_stored_codes_are_the_ones_the_documents_name() {
        assert_eq!(EventType::Action.code(), 10);
        assert_eq!(EventType::NoteHit.code(), 20);
        assert_eq!(EventType::NoteMiss.code(), 21);
        assert_eq!(EventType::Overstrum.code(), 22);
        assert_eq!(EventType::SustainEnded.code(), 31);
        assert_eq!(EventType::RawInput.code(), 99);
        assert_eq!(Rating::Perfect.code(), 0);
        assert_eq!(Rating::Miss.code(), 3);
        assert_eq!(Action::Strum.code(), 10);
        assert_eq!(Action::Hype.code(), 11);
        assert_eq!(Action::FretDown(Lane::One).code(), 0);
        assert_eq!(Action::FretUp(Lane::One).code(), 5);
        assert_eq!(Action::FretUp(Lane::Five).code(), 9);
    }

    #[test]
    fn every_action_reads_back_as_itself() {
        let all: Vec<Action> = Lane::ALL
            .iter()
            .map(|lane| Action::FretDown(*lane))
            .chain(Lane::ALL.iter().map(|lane| Action::FretUp(*lane)))
            .chain([Action::Strum, Action::Hype])
            .collect();
        for action in all {
            assert_eq!(Action::from_code(action.code()), Some(action));
        }
        assert_eq!(Action::from_code(12), None);
    }

    #[test]
    fn an_action_is_the_input_it_came_from() {
        assert_eq!(
            Action::from_input(InputKind::FretDown(Lane::Three)),
            Action::FretDown(Lane::Three)
        );
        assert_eq!(
            Action::from_input(InputKind::ActivateHype),
            Action::Hype,
            "the activation gesture is one action, whichever gesture \
             the player used"
        );
    }

    #[test]
    fn a_judgment_becomes_its_rating_and_keeps_its_word() {
        assert_eq!(Rating::from(Judgment::Great).label(), "great");
        assert_eq!(Rating::from(Judgment::Miss), Rating::Miss);
        for code in 0..4u8 {
            assert!(Rating::from_code(code).is_some());
        }
        assert_eq!(Rating::from_code(4), None);
    }

    #[test]
    fn the_detail_level_decides_what_is_written_and_nothing_else_does() {
        // Results: the engine's decisions only.
        assert!(EventType::NoteHit.recorded_at(Detail::Results));
        assert!(!EventType::Action.recorded_at(Detail::Results));
        assert!(!EventType::RawInput.recorded_at(Detail::Results));
        // Actions (the default): the whole logical path.
        assert!(EventType::Action.recorded_at(Detail::Actions));
        assert!(
            !EventType::RawInput.recorded_at(Detail::Actions),
            "the physical layer is opt-in; a default that records \
             which buttons a person pressed is not a default"
        );
        // Diagnostic: everything.
        for kind in EventType::ALL {
            assert!(kind.recorded_at(Detail::Diagnostic), "{}", kind.label());
        }
        assert_eq!(
            Detail::from_code(Detail::Diagnostic.code()),
            Detail::Diagnostic
        );
        assert_eq!(
            Detail::from_code(200),
            Detail::Actions,
            "an unreadable setting falls back to the default, not to \
             the most talkative level"
        );
    }

    #[test]
    fn note_flags_describe_the_note_and_combine() {
        use beatbyte_core::{LaneSet, NoteKind};
        let chord = NoteEvent {
            time_s: 1.0,
            lanes: LaneSet::from_iter([Lane::One, Lane::Two]),
            sustain_s: 0.5,
            kind: NoteKind::Hopo,
        };
        let flags = Flags::of_note(&chord);
        assert!(flags.has(Flags::CHORD));
        assert!(flags.has(Flags::HOPO));
        assert!(flags.has(Flags::SUSTAIN));
        assert!(!flags.has(Flags::VOCAL));
        let single = NoteEvent {
            time_s: 1.0,
            lanes: LaneSet::from_iter([Lane::One]),
            sustain_s: 0.0,
            kind: NoteKind::Strum,
        };
        assert_eq!(Flags::of_note(&single), Flags::NONE);
        // Each flag its own bit — an overlap would make one note kind
        // indistinguishable from another.
        let bits = [
            Flags::CHORD,
            Flags::HOPO,
            Flags::SUSTAIN,
            Flags::DONE,
            Flags::VOCAL,
            Flags::ASSISTED,
        ];
        for (index, bit) in bits.iter().enumerate() {
            for other in &bits[index + 1..] {
                assert_eq!(bit.0 & other.0, 0, "two flags share a bit");
            }
        }
    }

    #[test]
    fn an_event_builds_up_from_its_parts() {
        let event = Event::new(EventType::NoteHit, micros(12.5))
            .about(42)
            .off_by(-0.018)
            .judged(Rating::Great)
            .flagged(Flags::CHORD);
        assert_eq!(event.song_time_us, Some(12_500_000));
        assert_eq!(event.note_index, Some(42));
        assert_eq!(event.delta_us, Some(-18_000));
        assert_eq!(event.rating, Some(Rating::Great));
        assert!(event.flags.has(Flags::CHORD));
        assert_eq!(event.action, None, "a hit carries no action of its own");
        assert_eq!(
            Event::untimed(EventType::NoteMiss).song_time_us,
            None,
            "an imported observation says it does not know when, \
             rather than claiming the start of the song"
        );
    }
}
