//! Vocal play: the microphone against the chart.
//!
//! What is drawn is a ribbon of time — the target melody as bars
//! sliding right to left past a playhead, and the pitch the
//! microphone is hearing as a short trace over them. That is the
//! whole readable idea: **where your line is, where you are, and how
//! far apart they are.**
//!
//! ## Two clocks, three subtractions
//!
//! The microphone counts its own samples and the song counts its
//! own. [`beatbyte_audio::mic::CaptureAnchor`] holds where the two
//! were seen together and is reconciled every frame the way
//! `SongClock` is reconciled against the audio device: a big
//! disagreement is a new correspondence and snaps, a small one is
//! two crystals drifting and is eased out.
//!
//! Judgment reads `song_time`; the ribbon is drawn from
//! `visual_time`, which is the same rule the notes and the lyrics
//! already follow.
//!
//! ## What it deliberately does not do
//!
//! Hype, the rock meter, the combo. [`VocalSession`] produces phrase
//! outcomes and those go to the implementation that already exists —
//! a second Hype would be two Hypes.

use std::collections::VecDeque;

use bevy::prelude::*;
use bevy::sprite::Anchor;

use beatbyte_audio::mic::{CaptureAnchor, CapturedFrame, to_song_time};
use beatbyte_core::vocal::{VocalNote, VocalPart};
use beatbyte_core::vocal_session::{
    PhraseOutcome, VocalEvent, VocalGrade, VocalInputFrame, VocalScoreConfig, VocalSession,
};

use super::GameplayScreen;
use crate::audio_sys::GameClock;
use crate::palette;
use crate::states::{AppState, GamePhase};
use crate::ui::UiFont;

/// The ribbon's edges in world units. It sits under the lyric line
/// and over the highway, on a plate of its own so the notes behind it
/// do not read as pitch.
pub const BAND_TOP: f32 = 238.0;
/// The ribbon's lower edge.
pub const BAND_BOTTOM: f32 = 98.0;
/// Its left edge.
pub const BAND_LEFT: f32 = -560.0;
/// Its right edge.
pub const BAND_RIGHT: f32 = 560.0;

/// Seconds of already-sung song kept on screen, left of the playhead.
pub const BEHIND_S: f64 = 1.2;
/// Seconds of what is coming, right of it.
pub const AHEAD_S: f64 = 2.8;

/// How long the live trace remembers, in seconds. Longer than
/// [`BEHIND_S`] would draw a line nobody can see.
pub const TRACE_S: f64 = BEHIND_S;

/// Semitones of air kept above and below what the phrase asks for, so
/// a singer who overshoots can see that they did.
pub const RANGE_PAD: f32 = 3.0;

/// The narrowest visible range, in semitones. A phrase on one note
/// would otherwise be drawn at infinite magnification.
pub const RANGE_MIN: f32 = 12.0;

/// How fast the visible range eases toward what the music wants, in
/// semitones per second. Snapping it at every phrase would make the
/// whole ribbon jump.
pub const RANGE_EASE: f32 = 14.0;

/// Bars the ribbon can draw at once.
const BAR_POOL: usize = 48;
/// Trace pieces it can draw at once.
const TRACE_POOL: usize = 96;

/// Where the ribbon puts a moment and a pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VocalView {
    /// The song time under the playhead.
    pub now_s: f64,
    /// The lowest pitch drawn.
    pub low_midi: f32,
    /// The highest.
    pub high_midi: f32,
}

impl VocalView {
    /// World x for a song time. Pure — tested.
    #[must_use]
    pub fn x_of(&self, song_time_s: f64) -> f32 {
        let span = BEHIND_S + AHEAD_S;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a fraction of the ribbon's width"
        )]
        let along = ((song_time_s - self.now_s + BEHIND_S) / span) as f32;
        BAND_LEFT + along * (BAND_RIGHT - BAND_LEFT)
    }

    /// World y for a pitch. Pure — tested.
    #[must_use]
    pub fn y_of(&self, midi: f32) -> f32 {
        let span = (self.high_midi - self.low_midi).max(1e-3);
        let up = (midi - self.low_midi) / span;
        BAND_BOTTOM + up * (BAND_TOP - BAND_BOTTOM)
    }

    /// Where the playhead stands.
    #[must_use]
    pub fn playhead_x(&self) -> f32 {
        self.x_of(self.now_s)
    }

    /// Whether any of `[from, to]` is on screen. Pure — tested.
    #[must_use]
    pub fn visible(&self, from: f64, to: f64) -> bool {
        to >= self.now_s - BEHIND_S && from <= self.now_s + AHEAD_S
    }
}

/// The pitch range the music wants shown right now: whatever the
/// visible stretch of the part asks for, padded, and never narrower
/// than [`RANGE_MIN`].
///
/// It reads the WHOLE visible window rather than the current phrase,
/// so the range has already opened up by the time a higher phrase
/// arrives instead of lurching when it does. Pure — tested.
#[must_use]
pub fn wanted_range(part: &VocalPart, now_s: f64) -> Option<(f32, f32)> {
    let view = VocalView {
        now_s,
        low_midi: 0.0,
        high_midi: 1.0,
    };
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for phrase in &part.phrases {
        if !view.visible(phrase.start_s, phrase.end_s) {
            continue;
        }
        for note in &phrase.notes {
            if !view.visible(note.start_s, note.end_s) {
                continue;
            }
            if let Some(span) = note.pitch_span() {
                low = low.min(span.low_midi);
                high = high.max(span.high_midi);
            }
        }
    }
    if !low.is_finite() || !high.is_finite() {
        return None;
    }
    Some(pad_range(low, high))
}

/// Widen a bare low/high to something drawable. Pure — tested.
#[must_use]
pub fn pad_range(low: f32, high: f32) -> (f32, f32) {
    let mut low = low - RANGE_PAD;
    let mut high = high + RANGE_PAD;
    let short = RANGE_MIN - (high - low);
    if short > 0.0 {
        low -= short / 2.0;
        high += short / 2.0;
    }
    (low, high)
}

/// Ease the drawn range toward the wanted one. Pure — tested.
#[must_use]
pub fn ease_range(current: (f32, f32), wanted: (f32, f32), dt: f32) -> (f32, f32) {
    let step = RANGE_EASE * dt.clamp(0.0, 0.25);
    let toward = |from: f32, to: f32| {
        let delta = to - from;
        if delta.abs() <= step {
            to
        } else {
            from + delta.signum() * step
        }
    };
    (toward(current.0, wanted.0), toward(current.1, wanted.1))
}

/// Which way the singer is off, if enough to say. Pure — tested.
#[must_use]
pub fn direction(cents: f32, tolerance: f32) -> Option<&'static str> {
    if cents > tolerance {
        Some("v LOWER")
    } else if cents < -tolerance {
        Some("^ HIGHER")
    } else {
        None
    }
}

/// The colour a pitch error reads as. Pure — tested.
#[must_use]
pub fn error_colour(cents: f32, config: &VocalScoreConfig) -> Color {
    match config.grade_for_cents(cents) {
        VocalGrade::Perfect => palette::PERFECT,
        VocalGrade::Great => palette::GREAT,
        VocalGrade::Good => palette::GOOD,
        VocalGrade::Weak | VocalGrade::Miss => palette::MISS,
    }
}

/// How much of the ribbon's motion and flash the player has asked
/// for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VocalLook {
    /// Whether the visible range may move at all.
    pub range_moves: bool,
    /// Whether a phrase's verdict may flash.
    pub banner_flashes: bool,
    /// Whether the bars and the trace need to stand apart by more
    /// than colour.
    pub high_contrast: bool,
}

impl VocalLook {
    /// Read it off the settings.
    ///
    /// **Reduced Motion** freezes the range: the whole ribbon sliding
    /// under a singer is exactly the kind of large-field movement the
    /// setting exists to stop, and a fixed range is perfectly
    /// readable — it is just wider. **Reduced Flashing** keeps the
    /// verdict on screen without the swell. **High Contrast** makes
    /// the target bars brighter and thicker, because a ribbon whose
    /// only distinction between "your line" and "your voice" is hue
    /// is unreadable to a good share of players. Pure — tested.
    #[must_use]
    pub fn of(settings: &crate::config::Settings) -> VocalLook {
        VocalLook {
            range_moves: settings.backdrop_motion,
            banner_flashes: !settings.reduced_flashing,
            high_contrast: settings.high_contrast,
        }
    }

    /// The whole span a frozen ribbon draws, when the range may not
    /// follow the music.
    ///
    /// It has to hold the WHOLE part rather than a comfortable
    /// window: a range that cannot move must fit everything that will
    /// ever be asked for, or the notes it does not fit are pinned to
    /// an edge for the rest of the song. Pure — tested.
    #[must_use]
    pub fn fixed_range(part: &VocalPart) -> (f32, f32) {
        part.pitch_range().map_or((54.0, 72.0), |range| {
            pad_range(range.low_midi, range.high_midi)
        })
    }

    /// How thick a target bar is drawn.
    #[must_use]
    pub fn bar_thickness(self) -> f32 {
        if self.high_contrast { 10.0 } else { 6.0 }
    }

    /// How opaque it is.
    #[must_use]
    pub fn bar_alpha(self) -> f32 {
        if self.high_contrast { 1.0 } else { 0.85 }
    }

    /// How thick the live trace is.
    #[must_use]
    pub fn trace_thickness(self) -> f32 {
        if self.high_contrast { 5.0 } else { 3.0 }
    }
}

/// One remembered instant of the microphone.
#[derive(Debug, Clone, Copy)]
pub struct TracePoint {
    /// When, in song seconds.
    pub song_time_s: f64,
    /// What pitch, fractional MIDI.
    pub midi: f32,
    /// How far from the target there, when there was one.
    pub cents: Option<f32>,
}

/// The singer's state for this run.
#[derive(Resource)]
pub struct VocalRun {
    /// The judgment engine.
    pub session: VocalSession,
    /// Where the capture clock and the song clock agree.
    anchor: Option<CaptureAnchor>,
    /// The microphone's recent pitch, oldest first.
    pub trace: VecDeque<TracePoint>,
    /// The newest frame, for the readout.
    pub last: Option<VocalInputFrame>,
    /// The last phrase's verdict and how long it has been up.
    pub banner: Option<(PhraseOutcome, f32)>,
    /// The drawn pitch range.
    pub range: (f32, f32),
    /// Scratch the tap drains into, so the frame loop allocates
    /// nothing.
    frames: Vec<CapturedFrame>,
    /// Scratch the session writes its events into.
    events: Vec<VocalEvent>,
}

impl VocalRun {
    /// A run holding the singer to `part`.
    #[must_use]
    pub fn new(part: VocalPart, config: VocalScoreConfig) -> VocalRun {
        let range = part
            .pitch_range()
            .map_or((54.0, 72.0), |r| pad_range(r.low_midi, r.high_midi));
        VocalRun {
            session: VocalSession::new(part, config),
            anchor: None,
            trace: VecDeque::with_capacity(TRACE_POOL),
            last: None,
            banner: None,
            range,
            frames: Vec::with_capacity(16),
            events: Vec::with_capacity(16),
        }
    }

    /// The view for a moment.
    #[must_use]
    pub fn view(&self, now_s: f64) -> VocalView {
        VocalView {
            now_s,
            low_midi: self.range.0,
            high_midi: self.range.1,
        }
    }
}

/// How long a phrase's verdict stays up, in seconds.
pub const BANNER_S: f32 = 1.6;

/// The rules this run is judged by: the difficulty's tolerances, with
/// the player's own answer to the octave question.
///
/// The tolerances follow the DIFFICULTY rather than a vocal setting
/// of their own — a player who picked Expert asked for Expert — while
/// the octave is a fact about the singer's voice and not about how
/// hard they want it. Pure — tested.
#[must_use]
pub fn score_config(
    settings: &crate::config::Settings,
    difficulty: beatbyte_core::Difficulty,
) -> VocalScoreConfig {
    VocalScoreConfig {
        mode: settings.vocal_pitch_mode,
        ..VocalScoreConfig::for_difficulty(difficulty)
    }
}

/// The backing a karaoke run plays instead of the song, when there is
/// one.
///
/// `None` is the ordinary case and means exactly what it looks like:
/// play the song. That happens when vocals are off, when this song
/// has no chart, or when its stems are not on disk — and in every one
/// of those the game is what it always was.
///
/// ⚠️ It is `SongAudio::File`, so every path the music thread already
/// has — the loudness gain, the crossfade, the seek — works on it
/// unchanged. The stems were separated from BeatByte's own decode, so
/// the backing sits on the same timeline as the chart.
#[must_use]
pub fn karaoke_audio(
    song: &crate::boot::LoadedSong,
    settings: &crate::config::Settings,
) -> Option<crate::boot::SongAudio> {
    if !settings.vocal_charts || song.vocals.is_none() {
        return None;
    }
    let crate::boot::SongAudio::File(audio_path) = &song.audio else {
        return None;
    };
    let track = beatbyte_audio::stems::karaoke_track(audio_path, settings.original_vocals)?;
    info!("vocals: playing the karaoke backing ({})", track.display());
    Some(crate::boot::SongAudio::File(track))
}

/// Whether a karaoke backing exists for this song at this level.
///
/// The same question [`karaoke_audio`] answers, asked without
/// building anything: at the default level the answer is a manifest
/// read, and above it the mix has already been built by the time
/// this runs.
#[must_use]
fn karaoke_track_exists(
    song: Option<&crate::boot::LoadedSong>,
    settings: &crate::config::Settings,
) -> bool {
    song.is_some_and(|song| karaoke_audio(song, settings).is_some())
}

/// Whether a karaoke backing was actually used for this run.
///
/// Set at the start and read at the end, because what was PLAYED is
/// what decides whether a score is comparable — and the setting
/// alone cannot say. A song with a vocal chart but no stems beside
/// it plays its original mix, the record sings along with the
/// player, and nothing about the configuration shows that.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct KaraokeBacking(pub bool);

/// Whether the original singer is audible in the room.
///
/// A microphone cannot tell a player from a voice coming out of the
/// speakers, so such a run is **assisted** and says so. No pitch-only
/// algorithm can prove otherwise, and a score that quietly compares
/// the two is worse than one that admits which it is.
///
/// ⚠️ Two ways to be assisted, and the second is the one that hides.
/// The obvious one is turning the original vocals up. The other is
/// having no karaoke backing at all — a song whose chart is there but
/// whose stems are not plays its ORIGINAL MIX, with the record
/// singing every note, and the settings look exactly like a clean
/// run. Found by running the gate against a `[GS]` twin, which
/// carries the chart but not the stems. Pure — tested.
#[must_use]
pub fn assisted(settings: &crate::config::Settings, backing_used: bool) -> bool {
    if !settings.vocal_charts {
        return false;
    }
    !backing_used || settings.original_vocals > 0.0
}

/// Wire vocal play into the app.
pub fn register(app: &mut App) {
    // ⚠️ AFTER the ears open. Both run on the same state entry, and
    // Bevy orders same-schedule systems arbitrarily — unordered, this
    // read `Ears` before `open_ears` had filled it about half the
    // time, and vocal play simply did not start on those runs.
    app.add_systems(
        OnEnter(AppState::Gameplay),
        start.after(super::monitors::open_ears),
    )
    .add_systems(OnExit(AppState::Gameplay), stop)
    .add_systems(
        Update,
        (feed, draw)
            .chain()
            .run_if(in_state(GamePhase::Playing).or_else(in_state(GamePhase::Outro))),
    );
}

/// Open the run when this song has a chart to sing and the player
/// asked for vocals.
fn start(
    mut commands: Commands,
    settings: Res<crate::config::Settings>,
    song: Option<Res<crate::boot::LoadedSong>>,
    ears: Res<super::monitors::Ears>,
    selected: Res<crate::song_select::SelectedDifficulty>,
    font: Res<UiFont>,
) {
    if !settings.vocal_charts {
        return;
    }
    let Some(part) = song
        .as_ref()
        .and_then(|song| song.vocals.as_ref())
        .and_then(beatbyte_chart::vocals::VocalChartFile::lead)
        .cloned()
    else {
        return;
    };
    let Some(listener) = ears.0.as_ref() else {
        // No microphone: the song plays, the guitar plays, and the
        // ribbon simply is not there. Never an error.
        info!("vocals: no input device; singing is off for this run");
        return;
    };
    listener.vocals().enable(true);
    let backing = karaoke_track_exists(song.as_deref(), &settings);
    if !backing {
        info!("vocals: no karaoke backing for this song - the run is assisted");
    }
    commands.insert_resource(KaraokeBacking(backing));
    info!(
        "vocals: holding the singer to {} phrases, {} notes",
        part.phrases.len(),
        part.note_count()
    );
    commands.insert_resource(VocalRun::new(part, score_config(&settings, selected.0)));
    spawn_layer(&mut commands, &font);
}

/// Close it, and let the microphone go back to the monitors alone.
fn stop(mut commands: Commands, ears: Res<super::monitors::Ears>) {
    if let Some(listener) = ears.0.as_ref() {
        listener.vocals().enable(false);
    }
    commands.remove_resource::<VocalRun>();
    commands.remove_resource::<KaraokeBacking>();
}

/// Take the microphone's frames, put them on the song's timeline and
/// judge them.
fn feed(
    mut run: Option<ResMut<VocalRun>>,
    ears: Res<super::monitors::Ears>,
    clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<crate::config::Settings>,
    room: Res<crate::room_stage::RoomStage>,
) {
    let (Some(run), Some(listener)) = (run.as_mut(), ears.0.as_ref()) else {
        return;
    };
    let Some(now) = clock.song_time(&time) else {
        return;
    };
    let tap = listener.vocals();
    run.frames.clear();
    let mut frames = std::mem::take(&mut run.frames);
    tap.drain(&mut frames);

    if let Some(newest) = frames.last() {
        // "Now" on the capture clock is the newest frame's centre
        // plus the delay it took to exist.
        let capture_now = newest.capture_s + tap.known_latency_s();
        match run.anchor.as_mut() {
            Some(anchor) => {
                if anchor.reconcile(capture_now, now) {
                    // A snap means the two clocks were talking about
                    // different songs; whatever is in the trace is
                    // about the old one.
                    run.trace.clear();
                }
            }
            None => {
                run.anchor = Some(CaptureAnchor {
                    capture_s: capture_now,
                    song_time_s: now,
                });
            }
        }
    }
    let Some(anchor) = run.anchor else {
        run.frames = frames;
        return;
    };

    let latency = tap.known_latency_s();
    let offset = settings.mic_offset_ms;
    let mut events = std::mem::take(&mut run.events);
    events.clear();
    for captured in &frames {
        let song_time_s = to_song_time(captured.capture_s, anchor, latency, offset);
        let frame = VocalInputFrame {
            song_time_s,
            midi: captured.pitch.midi(),
            confidence: captured.pitch.clarity,
            rms_dbfs: captured.pitch.rms_dbfs,
            voiced: captured.pitch.voiced,
            clipped: captured.pitch.clipped,
        };
        let cents = frame.midi.and_then(|midi| {
            let note = run.session.part().note_at(song_time_s)?;
            let target = note.target_at(song_time_s)?;
            Some(run.session.config().mode.error_cents(midi, target))
        });
        if let Some(midi) = frame.midi {
            run.trace.push_back(TracePoint {
                song_time_s,
                midi,
                cents,
            });
        }
        run.last = Some(frame);
        run.session.feed(frame, &mut events);
    }
    while run
        .trace
        .front()
        .is_some_and(|point| now - point.song_time_s > TRACE_S)
    {
        run.trace.pop_front();
    }
    for event in events.drain(..) {
        if let VocalEvent::Phrase(outcome) = event {
            run.banner = Some((outcome, 0.0));
            // The room hears the singer too. Same bridge, same
            // vocabulary as a guitar phrase — the lights do not know
            // or care which instrument earned the accent.
            if let Some(post) = crate::room_stage::post_for_vocal(outcome) {
                room.send(post);
            }
        }
    }
    run.events = events;
    run.frames = frames;

    if let Some((_, age)) = run.banner.as_mut() {
        *age += time.delta_secs();
    }
    if run.banner.is_some_and(|(_, age)| age > BANNER_S) {
        run.banner = None;
    }
}

/// A pooled node of the ribbon.
#[derive(Component)]
struct VocalBar(usize);

/// A pooled piece of the live trace.
#[derive(Component)]
struct VocalTrace(usize);

/// The live readout under the ribbon.
#[derive(Component)]
struct VocalReadout;

/// The phrase verdict.
#[derive(Component)]
struct VocalBanner;

/// The playhead.
#[derive(Component)]
struct VocalPlayhead;

fn spawn_layer(commands: &mut Commands, font: &UiFont) {
    let width = BAND_RIGHT - BAND_LEFT;
    let height = BAND_TOP - BAND_BOTTOM;
    let mid = Vec2::new(
        (BAND_LEFT + BAND_RIGHT) / 2.0,
        (BAND_BOTTOM + BAND_TOP) / 2.0,
    );
    // The plate: the highway runs behind this, and a note sliding
    // past would read as pitch without something between them.
    commands.spawn((
        GameplayScreen,
        Sprite {
            color: palette::SURFACE.with_alpha(0.82),
            custom_size: Some(Vec2::new(width, height)),
            ..default()
        },
        Transform::from_xyz(mid.x, mid.y, 4.40),
    ));
    commands.spawn((
        GameplayScreen,
        VocalPlayhead,
        Sprite {
            color: palette::CHROME.with_alpha(0.5),
            custom_size: Some(Vec2::new(2.0, height)),
            ..default()
        },
        Transform::from_xyz(0.0, mid.y, 4.46),
    ));
    for index in 0..BAR_POOL {
        commands.spawn((
            GameplayScreen,
            VocalBar(index),
            Sprite {
                color: palette::TEXT_DIM,
                custom_size: Some(Vec2::new(1.0, 1.0)),
                ..default()
            },
            Visibility::Hidden,
            Transform::from_xyz(0.0, 0.0, 4.50),
        ));
    }
    for index in 0..TRACE_POOL {
        commands.spawn((
            GameplayScreen,
            VocalTrace(index),
            Sprite {
                color: palette::BRAND,
                custom_size: Some(Vec2::new(1.0, 1.0)),
                ..default()
            },
            Visibility::Hidden,
            Transform::from_xyz(0.0, 0.0, 4.60),
        ));
    }
    commands.spawn((
        GameplayScreen,
        VocalReadout,
        Text2d::new(String::new()),
        font.text(10.0),
        TextColor(palette::TEXT_DIM),
        Anchor::TOP_LEFT,
        Transform::from_xyz(BAND_LEFT + 6.0, BAND_BOTTOM - 4.0, 4.70),
    ));
    commands.spawn((
        GameplayScreen,
        VocalBanner,
        Text2d::new(String::new()),
        font.text(13.0),
        TextColor(palette::TEXT),
        Anchor::TOP_RIGHT,
        Transform::from_xyz(BAND_RIGHT - 6.0, BAND_BOTTOM - 4.0, 4.70),
    ));
}

#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system: every parameter is a world handle"
)]
fn draw(
    run: Option<ResMut<VocalRun>>,
    clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<crate::config::Settings>,
    mut bars: Query<(&VocalBar, &mut Sprite, &mut Transform, &mut Visibility), Without<VocalTrace>>,
    mut trace: Query<
        (&VocalTrace, &mut Sprite, &mut Transform, &mut Visibility),
        Without<VocalBar>,
    >,
    mut readout: Query<&mut Text2d, (With<VocalReadout>, Without<VocalBanner>)>,
    mut banner: Query<(&mut Text2d, &mut TextColor), With<VocalBanner>>,
) {
    let Some(mut run) = run else {
        return;
    };
    let Some(drawn_now) = clock.visual_time(&time, &settings) else {
        return;
    };
    let look = VocalLook::of(&settings);
    // The range follows the music, eased: a phrase that jumps an
    // octave must not make the whole ribbon jump with it. Under
    // Reduced Motion it does not follow at all — a fixed range that
    // holds the whole song is wider but perfectly readable, and the
    // ribbon sliding under a singer is exactly the large-field
    // movement that setting exists to stop.
    if look.range_moves {
        if let Some(wanted) = wanted_range(run.session.part(), drawn_now) {
            run.range = ease_range(run.range, wanted, time.delta_secs());
        }
    } else {
        run.range = VocalLook::fixed_range(run.session.part());
    }
    let view = run.view(drawn_now);
    let config = *run.session.config();

    let mut slot = 0usize;
    let mut placed: Vec<(usize, Vec2, Vec2, Color)> = Vec::new();
    'outer: for phrase in &run.session.part().phrases {
        if !view.visible(phrase.start_s, phrase.end_s) {
            continue;
        }
        for note in &phrase.notes {
            if !view.visible(note.start_s, note.end_s) {
                continue;
            }
            if slot >= BAR_POOL {
                break 'outer;
            }
            if let Some((centre, size, colour)) = bar_of(note, &view, look) {
                placed.push((slot, centre, size, colour));
                slot += 1;
            }
        }
    }
    for (bar, mut sprite, mut transform, mut visibility) in &mut bars {
        match placed.iter().find(|(index, ..)| *index == bar.0) {
            Some((_, centre, size, colour)) => {
                sprite.color = *colour;
                sprite.custom_size = Some(*size);
                transform.translation.x = centre.x;
                transform.translation.y = centre.y;
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }

    // The live trace, as pieces between remembered points.
    let points: Vec<(Vec2, Color)> = run
        .trace
        .iter()
        .map(|point| {
            (
                Vec2::new(view.x_of(point.song_time_s), view.y_of(point.midi)),
                point
                    .cents
                    .map_or(palette::CHROME, |cents| error_colour(cents, &config)),
            )
        })
        .collect();
    for (piece, mut sprite, mut transform, mut visibility) in &mut trace {
        let Some(pair) = points.get(piece.0..piece.0 + 2) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let (from, colour) = pair[0];
        let (to, _) = pair[1];
        let delta = to - from;
        let len = delta.length();
        if len <= f32::EPSILON || len > 200.0 {
            // A jump that long is a gap in the singing, not a slide.
            *visibility = Visibility::Hidden;
            continue;
        }
        sprite.color = colour;
        sprite.custom_size = Some(Vec2::new(len, look.trace_thickness()));
        transform.translation.x = from.x + delta.x / 2.0;
        transform.translation.y = from.y + delta.y / 2.0;
        transform.rotation = Quat::from_rotation_z(delta.y.atan2(delta.x));
        *visibility = Visibility::Inherited;
    }

    if let Ok(mut text) = readout.single_mut() {
        text.0 = readout_line(&run, drawn_now, &config);
    }
    if let Ok((mut text, mut colour)) = banner.single_mut() {
        match run.banner {
            Some((outcome, _)) => {
                text.0 = format!(
                    "{}  x{}  {}",
                    outcome.grade.label(),
                    outcome.multiplier,
                    outcome.points
                );
                // Reduced Flashing keeps the verdict and drops the
                // swell: the words are the information, the flash was
                // only ever the celebration.
                let swell = if look.banner_flashes {
                    let (_, age) = run.banner.unwrap_or((outcome, BANNER_S));
                    1.0 - (age / BANNER_S).clamp(0.0, 1.0) * 0.35
                } else {
                    1.0
                };
                colour.0 = (match outcome.grade {
                    VocalGrade::Perfect => palette::PERFECT,
                    VocalGrade::Great => palette::GREAT,
                    VocalGrade::Good => palette::GOOD,
                    VocalGrade::Weak | VocalGrade::Miss => palette::MISS,
                })
                .with_alpha(swell);
            }
            None => text.0.clear(),
        }
    }
}

/// Where a note's bar goes, and what colour. `None` when it carries
/// no pitch to draw. Pure — tested.
#[must_use]
pub fn bar_of(note: &VocalNote, view: &VocalView, look: VocalLook) -> Option<(Vec2, Vec2, Color)> {
    let midi = note
        .target_midi
        .or_else(|| note.contour.first().map(|point| point.midi))?;
    let left = view.x_of(note.start_s).max(BAND_LEFT);
    let right = view.x_of(note.end_s).min(BAND_RIGHT);
    let width = right - left;
    if width <= 0.5 {
        return None;
    }
    let y = view.y_of(midi);
    // Clamped rather than dropped: a note above or below the drawn
    // range still has to be visible as something to reach for, and
    // the range is easing toward it anyway.
    let y = y.clamp(BAND_BOTTOM + 3.0, BAND_TOP - 3.0);
    let colour = if note.kind.is_pitched() {
        palette::TEXT_DIM
    } else {
        // Rap and speech are not a pitch to hit, so they are not
        // drawn as one.
        palette::HYPE
    };
    Some((
        Vec2::new(left + width / 2.0, y),
        Vec2::new(width, look.bar_thickness()),
        colour.with_alpha(look.bar_alpha()),
    ))
}

/// The line under the ribbon: what is being heard, and which way it
/// is off. Pure — tested.
#[must_use]
pub fn readout_line(run: &VocalRun, now_s: f64, config: &VocalScoreConfig) -> String {
    let Some(frame) = run.last else {
        return "LISTENING".to_owned();
    };
    if !frame.voiced {
        return "NO VOCAL".to_owned();
    }
    if frame.clipped {
        return "TOO LOUD".to_owned();
    }
    let Some(midi) = frame.midi else {
        return "NO VOCAL".to_owned();
    };
    let Some(cents) = run
        .session
        .part()
        .note_at(now_s)
        .and_then(|note| note.target_at(now_s))
        .map(|target| config.mode.error_cents(midi, target))
    else {
        return format!("{:.0} Hz", beatbyte_core::vocal::midi_to_hz(midi));
    };
    match direction(cents, config.perfect_cents) {
        Some(way) => format!("{way}  {cents:+.0}c"),
        None => format!("ON PITCH  {cents:+.0}c"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_core::vocal::{VocalKind, VocalPhrase, VocalPitchPoint, VocalRole};

    fn note(start: f64, end: f64, midi: f32) -> VocalNote {
        VocalNote {
            start_s: start,
            end_s: end,
            kind: VocalKind::Pitched,
            target_midi: Some(midi),
            contour: Vec::new(),
            confidence: 1.0,
            token_range: None,
        }
    }

    fn part(notes: Vec<VocalNote>) -> VocalPart {
        let start = notes.first().map_or(0.0, |n| n.start_s);
        let end = notes.last().map_or(1.0, |n| n.end_s);
        VocalPart {
            id: "lead".to_owned(),
            role: VocalRole::Lead,
            name: None,
            phrases: vec![VocalPhrase {
                start_s: start,
                end_s: end,
                confidence: 1.0,
                tokens: Vec::new(),
                notes,
            }],
        }
    }

    fn plain() -> VocalLook {
        VocalLook {
            range_moves: true,
            banner_flashes: true,
            high_contrast: false,
        }
    }

    fn view(now: f64) -> VocalView {
        VocalView {
            now_s: now,
            low_midi: 54.0,
            high_midi: 72.0,
        }
    }

    #[test]
    fn the_playhead_stands_where_the_lookahead_puts_it() {
        let v = view(10.0);
        let x = v.playhead_x();
        let along = (x - BAND_LEFT) / (BAND_RIGHT - BAND_LEFT);
        let wanted = (BEHIND_S / (BEHIND_S + AHEAD_S)) as f32;
        assert!(
            (along - wanted).abs() < 1e-4,
            "playhead at {along} of the way"
        );
        // Time runs left to right: what is coming is to the right.
        assert!(v.x_of(11.0) > x);
        assert!(v.x_of(9.5) < x);
        // The edges are the window's edges.
        assert!((v.x_of(10.0 + AHEAD_S) - BAND_RIGHT).abs() < 1e-3);
        assert!((v.x_of(10.0 - BEHIND_S) - BAND_LEFT).abs() < 1e-3);
    }

    #[test]
    fn a_higher_pitch_is_drawn_higher() {
        let v = view(0.0);
        assert!(v.y_of(72.0) > v.y_of(54.0), "the ribbon is upside down");
        assert!((v.y_of(54.0) - BAND_BOTTOM).abs() < 1e-3);
        assert!((v.y_of(72.0) - BAND_TOP).abs() < 1e-3);
        // Halfway up the range is halfway up the band.
        let middle = (BAND_BOTTOM + BAND_TOP) / 2.0;
        assert!((v.y_of(63.0) - middle).abs() < 1e-3);
        // A degenerate range does not divide by zero.
        let flat = VocalView {
            now_s: 0.0,
            low_midi: 60.0,
            high_midi: 60.0,
        };
        assert!(flat.y_of(60.0).is_finite());
    }

    #[test]
    fn only_what_is_in_the_window_is_drawn() {
        let v = view(10.0);
        assert!(v.visible(10.0, 10.5), "right under the playhead");
        assert!(v.visible(12.0, 12.5), "coming up");
        assert!(v.visible(9.5, 9.8), "just sung");
        assert!(!v.visible(20.0, 21.0), "a minute away");
        assert!(!v.visible(0.0, 1.0), "long gone");
        // A note that straddles an edge is still partly on screen.
        assert!(v.visible(9.0, 9.5), "ends inside the window");
        assert!(v.visible(12.5, 20.0), "starts inside it");
    }

    #[test]
    fn the_range_follows_the_whole_window_not_just_the_phrase() {
        // A high phrase two seconds out must already be opening the
        // range, or it arrives by making everything jump.
        let p = VocalPart {
            id: "lead".to_owned(),
            role: VocalRole::Lead,
            name: None,
            phrases: vec![
                VocalPhrase {
                    start_s: 0.0,
                    end_s: 5.0,
                    confidence: 1.0,
                    tokens: Vec::new(),
                    notes: vec![note(0.0, 5.0, 57.0)],
                },
                VocalPhrase {
                    start_s: 6.0,
                    end_s: 9.0,
                    confidence: 1.0,
                    tokens: Vec::new(),
                    notes: vec![note(6.0, 9.0, 69.0)],
                },
            ],
        };
        let (_, high) = wanted_range(&p, 2.0).expect("a visible phrase");
        // 63 is the minimum window centred on the near phrase's 57,
        // not the far phrase leaking in: its note is at 69.
        assert!(
            high < 69.0,
            "the far phrase is already in the range: {high}"
        );
        let (_, high) = wanted_range(&p, 4.0).expect("both phrases visible");
        assert!(
            high > 70.0,
            "the coming phrase is not in the range yet: {high}"
        );
        // Nothing visible: nothing to say.
        assert_eq!(wanted_range(&p, 100.0), None);
    }

    #[test]
    fn a_single_note_is_not_drawn_at_infinite_magnification() {
        let (low, high) = pad_range(60.0, 60.0);
        assert!(
            (high - low - RANGE_MIN).abs() < 1e-3,
            "one note got a {} semitone window",
            high - low
        );
        assert!(
            (low + high) / 2.0 - 60.0 < 1e-3,
            "and it is centred on the note"
        );
        // A wide phrase keeps its width plus the air.
        let (low, high) = pad_range(55.0, 75.0);
        assert!((low - 52.0).abs() < 1e-3);
        assert!((high - 78.0).abs() < 1e-3);
    }

    #[test]
    fn the_range_eases_rather_than_jumping() {
        let from = (54.0, 66.0);
        let to = (60.0, 84.0);
        let one_frame = ease_range(from, to, 1.0 / 60.0);
        assert!(one_frame.0 > from.0 && one_frame.0 < to.0, "{one_frame:?}");
        assert!(one_frame.1 > from.1 && one_frame.1 < to.1, "{one_frame:?}");
        // And it gets there rather than crawling for ever.
        let mut range = from;
        for _ in 0..200 {
            range = ease_range(range, to, 1.0 / 60.0);
        }
        assert_eq!(range, to);
        // A long frame does not teleport the ribbon.
        let stalled = ease_range(from, to, 5.0);
        assert!(stalled.1 < to.1, "a dropped frame snapped the range");
    }

    #[test]
    fn the_arrow_points_the_way_the_singer_has_to_move() {
        // Positive cents = sharp = the singer is ABOVE the target, so
        // the instruction is to go lower. Getting this backwards is
        // the single most confusing thing this HUD could do.
        assert_eq!(direction(60.0, 20.0), Some("v LOWER"));
        assert_eq!(direction(-60.0, 20.0), Some("^ HIGHER"));
        assert_eq!(direction(5.0, 20.0), None, "inside the band, say nothing");
        assert_eq!(direction(-5.0, 20.0), None);
        assert_eq!(direction(20.0, 20.0), None, "exactly on the edge is in");
    }

    #[test]
    fn the_colour_says_how_far_off_without_reading_a_number() {
        let config = VocalScoreConfig::default();
        assert_eq!(error_colour(0.0, &config), palette::PERFECT);
        assert_eq!(error_colour(-30.0, &config), palette::GREAT);
        assert_eq!(error_colour(60.0, &config), palette::GOOD);
        assert_eq!(error_colour(300.0, &config), palette::MISS);
        // Symmetric: sharp and flat by the same amount read the same.
        assert_eq!(error_colour(60.0, &config), error_colour(-60.0, &config));
    }

    #[test]
    fn a_bar_covers_the_time_its_note_covers() {
        let v = view(10.0);
        let (centre, size, _) =
            bar_of(&note(10.0, 11.0, 63.0), &v, plain()).expect("a visible note");
        assert!((centre.y - v.y_of(63.0)).abs() < 1e-3);
        let expected = v.x_of(11.0) - v.x_of(10.0);
        assert!((size.x - expected).abs() < 1e-3, "{size:?}");
        // A note running off the left edge is clipped, not moved.
        let (centre, size, _) =
            bar_of(&note(5.0, 10.2, 63.0), &v, plain()).expect("partly visible");
        assert!(centre.x - size.x / 2.0 >= BAND_LEFT - 1e-3);
        // One that has gone entirely is not drawn.
        assert!(bar_of(&note(1.0, 2.0, 63.0), &v, plain()).is_none());
    }

    #[test]
    fn a_note_out_of_the_drawn_range_is_pinned_to_the_edge_not_dropped() {
        // It is still something to reach for, and the range is
        // already easing toward it.
        let v = view(10.0);
        let (centre, _, _) = bar_of(&note(10.0, 11.0, 96.0), &v, plain()).expect("a high note");
        assert!(centre.y <= BAND_TOP, "drawn off the top of the band");
        assert!(centre.y > BAND_BOTTOM);
        let (centre, _, _) = bar_of(&note(10.0, 11.0, 30.0), &v, plain()).expect("a low note");
        assert!(centre.y >= BAND_BOTTOM);
    }

    #[test]
    fn rap_is_not_drawn_as_a_pitch_to_hit() {
        let v = view(10.0);
        let mut rap = note(10.0, 11.0, 60.0);
        rap.kind = VocalKind::Rap;
        rap.target_midi = None;
        rap.contour = vec![VocalPitchPoint {
            offset_s: 0.0,
            midi: 60.0,
            confidence: 1.0,
        }];
        let (_, _, colour) = bar_of(&rap, &v, plain()).expect("rap is still drawn");
        assert!((colour.alpha() - plain().bar_alpha()).abs() < 1e-6);
        assert_ne!(colour, palette::TEXT_DIM.with_alpha(0.85));
    }

    #[test]
    fn the_readout_says_what_is_wrong_in_the_words_that_help() {
        let config = VocalScoreConfig::default();
        let mut run = VocalRun::new(part(vec![note(10.0, 11.0, 60.0)]), config);
        assert_eq!(readout_line(&run, 10.5, &config), "LISTENING");

        let frame = |midi: Option<f32>, voiced: bool, clipped: bool| VocalInputFrame {
            song_time_s: 10.5,
            midi,
            confidence: 0.9,
            rms_dbfs: -18.0,
            voiced,
            clipped,
        };
        run.last = Some(frame(None, false, false));
        assert_eq!(readout_line(&run, 10.5, &config), "NO VOCAL");

        run.last = Some(frame(Some(60.0), true, true));
        assert_eq!(
            readout_line(&run, 10.5, &config),
            "TOO LOUD",
            "clipping is an input-gain problem and says so"
        );

        run.last = Some(frame(Some(60.0), true, false));
        assert!(readout_line(&run, 10.5, &config).starts_with("ON PITCH"));

        run.last = Some(frame(Some(61.0), true, false));
        let line = readout_line(&run, 10.5, &config);
        assert!(line.starts_with("v LOWER"), "{line}");
        assert!(line.contains("+100"), "{line}");

        // Between notes there is no target, so it reports what it
        // hears rather than an error against nothing.
        run.last = Some(frame(Some(60.0), true, false));
        let line = readout_line(&run, 50.0, &config);
        assert!(line.ends_with("Hz"), "{line}");
    }

    #[test]
    fn a_run_is_assisted_exactly_when_the_original_singer_is_audible() {
        // No pitch-only algorithm can prove who made a matching note
        // when the record is playing in the room, so the honest thing
        // is to say which kind of run it was.
        let off = crate::config::Settings::default();
        assert!(!assisted(&off, true), "the default is not assisted");
        let backing_only = crate::config::Settings {
            vocal_charts: true,
            original_vocals: 0.0,
            ..crate::config::Settings::default()
        };
        assert!(
            !assisted(&backing_only, true),
            "a backing track is not help"
        );
        let with_singer = crate::config::Settings {
            original_vocals: 0.05,
            ..backing_only.clone()
        };
        assert!(
            assisted(&with_singer, true),
            "five percent is still audible"
        );

        // ⚠️ The way that HIDES: no backing at all. The song's own
        // mix plays, the record sings every note, and the settings
        // look exactly like a clean run. Found by running the gate
        // against a `[GS]` twin, which carries the chart but not the
        // stems.
        assert!(
            assisted(&backing_only, false),
            "a run against the original mix is not a clean vocal score"
        );

        // Vocals off: there is no vocal run to assist.
        let no_vocals = crate::config::Settings {
            vocal_charts: false,
            original_vocals: 1.0,
            ..crate::config::Settings::default()
        };
        assert!(!assisted(&no_vocals, false));
    }

    #[test]
    fn the_difficulty_sets_the_tolerances_and_the_player_sets_the_octave() {
        use beatbyte_core::Difficulty;
        use beatbyte_core::vocal::PitchMode;
        let strict = crate::config::Settings {
            vocal_pitch_mode: PitchMode::Strict,
            ..crate::config::Settings::default()
        };
        let config = score_config(&strict, Difficulty::Expert);
        assert_eq!(
            config.mode,
            PitchMode::Strict,
            "the setting did not reach it"
        );
        assert_eq!(
            config.perfect_cents,
            VocalScoreConfig::for_difficulty(Difficulty::Expert).perfect_cents,
            "the difficulty's tolerances were not kept"
        );
        // And the default forgives the octave.
        let plain = crate::config::Settings::default();
        assert_eq!(
            score_config(&plain, Difficulty::Easy).mode,
            PitchMode::OctaveIndependent
        );
        assert!(
            score_config(&plain, Difficulty::Easy).perfect_cents
                > score_config(&plain, Difficulty::Expert).perfect_cents
        );
    }

    #[test]
    fn reduced_motion_freezes_a_range_that_still_holds_the_whole_song() {
        // A frozen range that does not fit everything would pin
        // whatever it misses to an edge for the rest of the song, so
        // it has to hold the WHOLE part rather than a comfortable
        // window.
        let p = VocalPart {
            id: "lead".to_owned(),
            role: VocalRole::Lead,
            name: None,
            phrases: vec![
                VocalPhrase {
                    start_s: 0.0,
                    end_s: 2.0,
                    confidence: 1.0,
                    tokens: Vec::new(),
                    notes: vec![note(0.0, 2.0, 50.0)],
                },
                VocalPhrase {
                    start_s: 60.0,
                    end_s: 62.0,
                    confidence: 1.0,
                    tokens: Vec::new(),
                    notes: vec![note(60.0, 62.0, 80.0)],
                },
            ],
        };
        let (low, high) = VocalLook::fixed_range(&p);
        assert!(low < 50.0, "the low phrase is off the bottom: {low}");
        assert!(
            high > 80.0,
            "the phrase a minute away is off the top: {high}"
        );
        // An empty part still has something drawable.
        let empty = VocalPart {
            id: "lead".to_owned(),
            role: VocalRole::Lead,
            name: None,
            phrases: Vec::new(),
        };
        let (low, high) = VocalLook::fixed_range(&empty);
        assert!(high > low);
    }

    #[test]
    fn high_contrast_makes_the_line_stand_apart_by_more_than_colour() {
        let plain = plain();
        let strong = VocalLook {
            high_contrast: true,
            ..plain
        };
        assert!(strong.bar_thickness() > plain.bar_thickness());
        assert!(strong.trace_thickness() > plain.trace_thickness());
        assert!(strong.bar_alpha() > plain.bar_alpha());
        // And it reaches the drawing, not just the struct.
        let v = view(10.0);
        let (_, thin, _) = bar_of(&note(10.0, 11.0, 63.0), &v, plain).expect("a bar");
        let (_, thick, _) = bar_of(&note(10.0, 11.0, 63.0), &v, strong).expect("a bar");
        assert!(thick.y > thin.y, "{thick:?} against {thin:?}");
    }

    #[test]
    fn the_settings_reach_the_look() {
        let settings = crate::config::Settings {
            backdrop_motion: true,
            reduced_flashing: false,
            high_contrast: false,
            ..crate::config::Settings::default()
        };
        let look = VocalLook::of(&settings);
        assert!(look.range_moves && look.banner_flashes && !look.high_contrast);

        let settings = crate::config::Settings {
            backdrop_motion: false,
            reduced_flashing: true,
            high_contrast: true,
            ..crate::config::Settings::default()
        };
        let look = VocalLook::of(&settings);
        assert!(!look.range_moves, "reduced motion did not reach the ribbon");
        assert!(!look.banner_flashes, "reduced flashing did not reach it");
        assert!(look.high_contrast);
    }

    #[test]
    fn a_run_starts_with_a_range_the_song_can_be_drawn_in() {
        let run = VocalRun::new(
            part(vec![note(0.0, 1.0, 55.0), note(2.0, 3.0, 67.0)]),
            VocalScoreConfig::default(),
        );
        assert!(run.range.0 < 55.0 && run.range.1 > 67.0, "{:?}", run.range);
        // An empty part still has a drawable range rather than a
        // division by zero.
        let empty = VocalRun::new(
            VocalPart {
                id: "lead".to_owned(),
                role: VocalRole::Lead,
                name: None,
                phrases: Vec::new(),
            },
            VocalScoreConfig::default(),
        );
        assert!(empty.range.1 > empty.range.0);
    }
}
