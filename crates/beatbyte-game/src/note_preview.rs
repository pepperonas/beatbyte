//! The notes of the song under the browser's cursor, scrolling with
//! its preview: a small five-lane highway, and below it how dense the
//! whole song is, so the player SEES how hard a track is before
//! starting it.
//!
//! The browser must stay as quick as it is without it, so:
//!
//! - **Nothing happens while the cursor moves.** The chart is read
//!   only once the cursor has rested for [`LOAD_REST_S`] AND the audio
//!   preview is playing that very song ([`wants_load`]): the same rest
//!   the audio preview counts, not a second timer.
//! - **Reading happens off the frame thread**, one chart at a time,
//!   and the last [`CACHE`] results are kept, so arrowing back to a
//!   song shows its notes at once.
//! - **Drawing happens only while the preview shows**: a fixed pool of
//!   nodes is moved, nothing is spawned, and a value that has not
//!   changed is not written (the screen's change detection stays
//!   quiet).
//!
//! Everything that decides is a pure function here and pinned by the
//! tests at the bottom; the Bevy systems only carry values.

use std::collections::VecDeque;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use beatbyte_audio::SongClock;
use beatbyte_chart::ChartFile;
use beatbyte_core::difficulty::Difficulty;
use beatbyte_core::lane::LaneSet;
use beatbyte_core::note::NoteEvent;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future};

use crate::audio_sys::Music;
use crate::boot::BuiltinSongs;
use crate::config::Settings;
use crate::library::{SongEntry, SongLibrary, SongSource};
use crate::preview::SongPreview;

/// How long the cursor must rest on a song before its chart is read.
/// Longer than [`crate::preview::REST_S`]: the notes follow the sound.
pub const LOAD_REST_S: f32 = 1.0;

/// How far ahead of the strike line the highway looks, in seconds.
pub const LEAD_S: f64 = 2.0;

/// How long a note stays visible past the strike line, in seconds.
pub const TRAIL_S: f64 = 0.25;

/// Columns of the density curve.
pub const BUCKETS: usize = 48;

/// The window the peak density is measured over, in seconds. A chord
/// is one event, and one burst of notes is not the song's pace.
pub const PEAK_WINDOW_S: f64 = 2.0;

/// How many results are kept.
pub const CACHE: usize = 8;

/// How far the device's reported position may lie from where the
/// preview was told to start before the clock trusts it.
pub const READY_TOLERANCE_S: f64 = 1.5;

/// One note as the preview draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewNote {
    /// When it is struck, in song seconds.
    pub time_s: f64,
    /// Which lanes (a chord is one note with several).
    pub lanes: LaneSet,
    /// How long it is held, 0 for a tap.
    pub sustain_s: f64,
}

impl PreviewNote {
    /// When the note's sustain ends.
    #[must_use]
    pub fn end_s(&self) -> f64 {
        self.time_s + self.sustain_s.max(0.0)
    }
}

/// One difficulty, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewTrack {
    /// The notes, by time.
    pub notes: Vec<PreviewNote>,
    /// The density curve, [`BUCKETS`] columns from 0 to 1 (the busiest
    /// column is 1), across the song's whole length.
    pub density: Vec<f32>,
    /// Notes per second between the first note and the last.
    pub avg_nps: f32,
    /// The busiest [`PEAK_WINDOW_S`] of the song, in notes per second.
    pub peak_nps: f32,
}

/// A chart, every difficulty it offers, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewChart {
    /// The difficulties, in the chart's order.
    pub tracks: Vec<(Difficulty, PreviewTrack)>,
    /// The length the density curve spans, in seconds.
    pub duration_s: f64,
}

impl PreviewChart {
    /// The track of one difficulty.
    #[must_use]
    pub fn track(&self, difficulty: Difficulty) -> Option<&PreviewTrack> {
        self.tracks
            .iter()
            .find(|(d, _)| *d == difficulty)
            .map(|(_, t)| t)
    }
}

/// Turn a chart into its preview. `duration_s` is the song's length
/// (the audio's), so the curve's marker lines up with the preview's
/// position; without it the last note's end stands in.
#[must_use]
pub fn build(chart: &ChartFile, duration_s: Option<f64>) -> PreviewChart {
    let converted: Vec<(Difficulty, Vec<PreviewNote>)> = chart
        .charts
        .iter()
        .filter_map(|c| {
            let track = chart.to_track(c.difficulty).ok()?;
            Some((c.difficulty, notes_of(track.events())))
        })
        .collect();
    let last_end = converted
        .iter()
        .flat_map(|(_, notes)| notes.last().map(PreviewNote::end_s))
        .fold(0.0_f64, f64::max);
    let duration_s = duration_s
        .filter(|d| d.is_finite() && *d > 0.0)
        .unwrap_or(last_end)
        .max(last_end)
        .max(1.0);
    PreviewChart {
        tracks: converted
            .into_iter()
            .map(|(d, notes)| (d, summarize(notes, duration_s)))
            .collect(),
        duration_s,
    }
}

fn notes_of(events: &[NoteEvent]) -> Vec<PreviewNote> {
    events
        .iter()
        .map(|e| PreviewNote {
            time_s: e.time_s,
            lanes: e.lanes,
            sustain_s: e.sustain_s,
        })
        .collect()
}

/// The density figures of one difficulty. `notes` must be sorted.
#[must_use]
pub fn summarize(notes: Vec<PreviewNote>, duration_s: f64) -> PreviewTrack {
    let duration_s = duration_s.max(1.0);
    let mut counts = [0u32; BUCKETS];
    for note in &notes {
        let at = (note.time_s / duration_s * BUCKETS as f64).floor();
        let column = (at.max(0.0) as usize).min(BUCKETS - 1);
        counts[column] += 1;
    }
    let busiest = counts.iter().copied().max().unwrap_or(0).max(1) as f32;
    let density = counts.iter().map(|&c| c as f32 / busiest).collect();
    let avg_nps = match (notes.first(), notes.last()) {
        (Some(first), Some(last)) if notes.len() > 1 => {
            let span = (last.time_s - first.time_s).max(PEAK_WINDOW_S);
            (notes.len() as f64 / span) as f32
        }
        _ => 0.0,
    };
    // The busiest window: two pointers over the sorted times.
    let mut most = 0usize;
    let mut start = 0usize;
    for end in 0..notes.len() {
        while notes[end].time_s - notes[start].time_s >= PEAK_WINDOW_S {
            start += 1;
        }
        most = most.max(end - start + 1);
    }
    PreviewTrack {
        notes,
        density,
        avg_nps,
        peak_nps: (most as f64 / PEAK_WINDOW_S) as f32,
    }
}

/// The notes on the highway at song time `now_s`: struck no longer
/// than [`TRAIL_S`] ago (or still held), and due within [`LEAD_S`].
#[must_use]
pub fn visible(notes: &[PreviewNote], now_s: f64) -> Range<usize> {
    // A sustain that started long ago can still be on screen, so start
    // from where the longest possible tail could begin.
    let earliest = now_s - TRAIL_S - LEAD_S * 4.0;
    let start = notes.partition_point(|n| n.time_s < earliest);
    let end = notes.partition_point(|n| n.time_s <= now_s + LEAD_S);
    let first_shown = (start..end)
        .find(|&i| notes[i].end_s() >= now_s - TRAIL_S)
        .unwrap_or(end);
    first_shown..end.max(first_shown)
}

/// Where a moment sits on a highway `height` tall, measured from the
/// top: `now_s` on the strike line at `strike`, [`LEAD_S`] ahead at 0.
#[must_use]
pub fn y_of(time_s: f64, now_s: f64, strike: f32) -> f32 {
    let ahead = (time_s - now_s) / LEAD_S;
    strike - (ahead as f32) * strike
}

/// Whether the cursor has rested long enough, on the song the audio
/// preview is playing, to read its chart.
#[must_use]
pub fn wants_load(rest_s: f32, playing: Option<usize>, cursor: Option<usize>) -> bool {
    cursor.is_some() && playing == cursor && rest_s >= LOAD_REST_S
}

/// Whether the device's reported position belongs to the preview yet.
/// The preview fades the song in first and seeks after, so the first
/// frames of a new song report the file's start, not the hook.
#[must_use]
pub fn clock_ready(reported_s: f64, expected_start_s: f64) -> bool {
    reported_s.is_finite() && (reported_s - expected_start_s).abs() <= READY_TOLERANCE_S
}

/// Which chart a preview belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PreviewKey {
    /// A chart file in the library.
    File(PathBuf),
    /// A built-in song, by its index.
    Builtin(usize),
}

impl PreviewKey {
    /// The key of a library entry.
    #[must_use]
    pub fn of(entry: &SongEntry) -> PreviewKey {
        match &entry.source {
            SongSource::File { chart_path, .. } => PreviewKey::File(chart_path.clone()),
            SongSource::Builtin(index) => PreviewKey::Builtin(*index),
        }
    }
}

/// The last few results, newest last. A chart that could not be read
/// is kept too (as `None`), so a broken file is not read again on
/// every rest.
#[derive(Debug, Default)]
pub struct PreviewCache {
    entries: VecDeque<(PreviewKey, Option<Arc<PreviewChart>>)>,
}

impl PreviewCache {
    /// The result for `key`, if one is kept: `Some(None)` means the
    /// chart could not be read.
    pub fn get(&mut self, key: &PreviewKey) -> Option<Option<Arc<PreviewChart>>> {
        let at = self.entries.iter().position(|(k, _)| k == key)?;
        // Touched: it moves to the back, away from eviction.
        let entry = self.entries.remove(at)?;
        let found = entry.1.clone();
        self.entries.push_back(entry);
        Some(found)
    }

    /// Keep a result, dropping the oldest beyond [`CACHE`].
    pub fn insert(&mut self, key: PreviewKey, chart: Option<Arc<PreviewChart>>) {
        self.entries.retain(|(k, _)| *k != key);
        self.entries.push_back((key, chart));
        while self.entries.len() > CACHE {
            self.entries.pop_front();
        }
    }

    /// How many results are kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is kept.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Forget everything (the library changed).
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// What the browser's note preview is doing.
#[derive(Resource, Default)]
pub struct NotePreview {
    /// The chart being read, and which song it is for.
    task: Option<(PreviewKey, Task<Option<PreviewChart>>)>,
    /// What is known already.
    cache: PreviewCache,
    /// The chart on screen, and whose it is.
    shown: Option<(PreviewKey, Arc<PreviewChart>)>,
    /// Song time of the audio preview, as the highway reads it.
    clock: SongClock,
    /// The music generation the clock belongs to.
    generation: Option<u64>,
    /// Whether the clock is anchored to the preview's audio.
    anchored: bool,
    /// The song time to draw this frame, when there is one.
    now_s: Option<f64>,
}

impl NotePreview {
    /// The chart on screen.
    #[must_use]
    pub fn shown(&self) -> Option<&PreviewChart> {
        self.shown.as_ref().map(|(_, c)| c.as_ref())
    }

    /// The song time to draw, when the highway is running.
    #[must_use]
    pub fn now_s(&self) -> Option<f64> {
        self.now_s
    }

    /// Whether a chart is being read.
    #[must_use]
    pub fn loading(&self) -> bool {
        self.task.is_some()
    }

    /// Hide what is shown and stop the clock; the cache stays.
    pub fn hide(&mut self) {
        self.shown = None;
        self.anchored = false;
        self.now_s = None;
        self.clock.stop();
    }
}

/// The work a reading task does: everything but the disk read is pure.
fn read_preview(path: PathBuf, duration_s: Option<f64>) -> Option<PreviewChart> {
    let chart = match beatbyte_chart::load_chart_file(&path) {
        Ok(chart) => chart,
        Err(_) if !path.is_file() => {
            let current = crate::library::reresolve_chart(&path)?;
            beatbyte_chart::load_chart_file(&current).ok()?
        }
        Err(_) => return None,
    };
    if chart
        .validate()
        .iter()
        .any(|i| i.severity == beatbyte_chart::Severity::Error)
    {
        return None;
    }
    Some(build(&chart, duration_s))
}

/// Read charts, take finished ones, and keep the highway's clock: one
/// system, because all three share the same state.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub fn drive_note_preview(
    time: Res<Time>,
    settings: Res<Settings>,
    library: Res<SongLibrary>,
    builtins: Res<BuiltinSongs>,
    view: Res<crate::song_select::BrowserView>,
    cursor: Res<crate::song_select::BrowserCursor>,
    audio: Res<SongPreview>,
    music: Res<Music>,
    mut state: ResMut<NotePreview>,
) {
    if library.is_changed() && !library.is_added() {
        state.cache.clear();
    }
    let song = view.order.get(cursor.0).copied();
    let entry = song.and_then(|i| library.entries.get(i));
    let key = entry.map(PreviewKey::of);

    // A finished read goes into the cache, whoever it was for.
    if let Some((_, task)) = state.task.as_mut()
        && let Some(result) = block_on(future::poll_once(task))
        && let Some((done, _)) = state.task.take()
    {
        state.cache.insert(done, result.map(Arc::new));
    }

    // Anything but the song the cursor rests on comes down at once.
    if state.shown.as_ref().map(|(k, _)| k) != key.as_ref() {
        state.hide();
    }
    let (Some(entry), Some(key)) = (entry, key) else {
        return;
    };
    if !wants_load(audio.rest_s(), audio.playing(), song) {
        // The audio preview stopped (or never started): so does the
        // highway, it has nothing to follow.
        if audio.playing() != song {
            state.hide();
        }
        return;
    }

    if state.shown.is_none() {
        match state.cache.get(&key) {
            Some(Some(chart)) => {
                info!(
                    "note preview: \"{}\" ({} difficulties)",
                    crate::ui::font_safe(&entry.title),
                    chart.tracks.len()
                );
                state.shown = Some((key, chart));
            }
            // Known to be unreadable: stay quiet.
            Some(None) => {}
            // One read at a time; a stale one finishes first.
            None if state.task.is_none() => {
                let duration_s = entry.duration_s;
                let task = match &entry.source {
                    SongSource::File { chart_path, .. } => {
                        let path = chart_path.clone();
                        AsyncComputeTaskPool::get()
                            .spawn(async move { read_preview(path, duration_s) })
                    }
                    SongSource::Builtin(index) => {
                        let chart = builtins.0.get(*index).map(|s| s.chart.clone());
                        AsyncComputeTaskPool::get()
                            .spawn(async move { chart.map(|c| build(&c, duration_s)) })
                    }
                };
                state.task = Some((key, task));
            }
            None => {}
        }
    }
    if state.shown.is_none() {
        return;
    }

    // The clock: anchored to the preview's audio once the device
    // reports the hook, then reconciled every frame like gameplay's.
    let mono = time.elapsed_secs_f64();
    let generation = music.0.generation();
    if state.generation != Some(generation) {
        state.generation = Some(generation);
        state.anchored = false;
        state.clock.stop();
    }
    let position = music.0.position_s();
    if !state.anchored {
        if music.0.is_active() && clock_ready(position, crate::preview::start_of(entry)) {
            state.clock.start(mono, position);
            state.anchored = true;
            info!("note preview: following the song from {position:.1}s");
        }
    } else {
        state.clock.reconcile(mono, position);
    }
    state.now_s = state
        .anchored
        .then(|| state.clock.song_time(mono))
        .flatten()
        .map(|t| t + settings.video_offset_s());
}

/// Leaving the browser: nothing keeps running behind it.
pub fn reset_note_preview(mut state: ResMut<NotePreview>) {
    state.task = None;
    state.generation = None;
    state.hide();
}

// ---------------------------------------------------------------- drawing

/// Height of the little highway.
pub const HIGHWAY_H: f32 = 236.0;
/// Where the strike line sits, from the highway's top.
pub const STRIKE: f32 = HIGHWAY_H - 16.0;
/// Width of the preview column beside the panel's text.
pub const BOX_W: f32 = 140.0;
/// Height of one gem.
const GEM_H: f32 = 7.0;
/// Height of the density curve.
pub const DENSITY_H: f32 = 22.0;
/// Gems in the pool: a full window of eighths, chords included.
pub const GEM_POOL: usize = 64;
/// Sustain tails in the pool.
pub const TAIL_POOL: usize = 16;

/// The whole preview: highway, curve and figures. Its column is
/// always laid out (so nothing beside it moves) and only shown once a
/// chart is.
#[derive(Component)]
pub struct NotePreviewBox;
/// One gem of the pool.
#[derive(Component)]
pub struct PreviewGem(pub usize);
/// One sustain tail of the pool.
#[derive(Component)]
pub struct PreviewTail(pub usize);
/// One column of the density curve.
#[derive(Component)]
pub struct DensityBar(pub usize);
/// Where the preview is in the song, on the curve.
#[derive(Component)]
pub struct DensityMarker;
/// The notes-per-second line.
#[derive(Component)]
pub struct NpsLine;

/// A lane's left edge and width, as shares of the highway's width.
fn lane_slot(lane: usize) -> (f32, f32) {
    (lane as f32 * 20.0 + 2.0, 16.0)
}

/// Spawn the preview, hidden, into the browser's panel.
pub fn spawn_box(panel: &mut ChildSpawnerCommands, font: &crate::ui::UiFont) {
    use crate::palette;
    use crate::ui_kit;
    panel
        .spawn((
            NotePreviewBox,
            Visibility::Hidden,
            Node {
                width: px(BOX_W),
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(4),
                ..default()
            },
        ))
        .with_children(|b| {
            b.spawn((
                Node {
                    width: percent(100),
                    height: px(HIGHWAY_H),
                    overflow: Overflow::clip(),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(palette::BACKGROUND.with_alpha(0.85)),
                BorderColor::all(palette::dimmed(palette::TEXT_DIM, 0.3)),
            ))
            .with_children(|h| {
                for (lane, colour) in palette::LANES.iter().enumerate() {
                    let (left, width) = lane_slot(lane);
                    h.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(left),
                            width: percent(width),
                            top: px(0),
                            bottom: px(0),
                            ..default()
                        },
                        BackgroundColor(palette::dimmed(*colour, 0.02)),
                    ));
                }
                h.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        right: px(0),
                        top: px(STRIKE),
                        height: px(2),
                        ..default()
                    },
                    BackgroundColor(palette::dimmed(palette::TEXT, 0.6)),
                ));
                for i in 0..TAIL_POOL {
                    h.spawn((
                        PreviewTail(i),
                        Node {
                            display: Display::None,
                            position_type: PositionType::Absolute,
                            width: percent(4),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ));
                }
                for i in 0..GEM_POOL {
                    h.spawn((
                        PreviewGem(i),
                        Node {
                            display: Display::None,
                            position_type: PositionType::Absolute,
                            height: px(GEM_H),
                            border_radius: BorderRadius::all(px(GEM_H / 2.0)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ));
                }
            });
            b.spawn(Node {
                width: percent(100),
                height: px(DENSITY_H),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexEnd,
                column_gap: px(0),
                ..default()
            })
            .with_children(|d| {
                for i in 0..BUCKETS {
                    d.spawn((
                        DensityBar(i),
                        Node {
                            flex_grow: 1.0,
                            height: percent(0),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ));
                }
                d.spawn((
                    DensityMarker,
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(0),
                        width: px(2),
                        top: px(0),
                        bottom: px(0),
                        ..default()
                    },
                    BackgroundColor(palette::TEXT),
                    ZIndex(1),
                ));
            });
            b.spawn((
                NpsLine,
                Text::new(""),
                font.text(ui_kit::SMALL),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.85)),
            ));
        });
}

/// How many columns of the curve lie behind song time `now_s`.
#[must_use]
pub fn played_columns(now_s: f64, duration_s: f64) -> usize {
    let share = (now_s / duration_s.max(1.0)).clamp(0.0, 1.0);
    ((share * BUCKETS as f64).floor() as usize).min(BUCKETS)
}

/// The line under the curve.
#[must_use]
pub fn nps_line(track: &PreviewTrack) -> String {
    format!("AVG {:.1} · PEAK {:.1} /S", track.avg_nps, track.peak_nps)
}

type BoxOnly = (
    With<NotePreviewBox>,
    Without<PreviewGem>,
    Without<PreviewTail>,
    Without<DensityBar>,
    Without<DensityMarker>,
);
type GemOnly = (
    Without<NotePreviewBox>,
    Without<PreviewTail>,
    Without<DensityBar>,
    Without<DensityMarker>,
);
type TailOnly = (
    Without<NotePreviewBox>,
    Without<PreviewGem>,
    Without<DensityBar>,
    Without<DensityMarker>,
);
type BarOnly = (
    Without<NotePreviewBox>,
    Without<PreviewGem>,
    Without<PreviewTail>,
    Without<DensityMarker>,
);
type MarkerOnly = (
    With<DensityMarker>,
    Without<NotePreviewBox>,
    Without<PreviewGem>,
    Without<PreviewTail>,
    Without<DensityBar>,
);

/// Write a node field only when it differs, so an unchanged frame
/// leaves change detection quiet.
macro_rules! put {
    ($node:expr, $field:ident, $value:expr) => {{
        let value = $value;
        if $node.$field != value {
            $node.$field = value;
        }
    }};
}

/// Draw the preview: move the pool, refill the curve when the song or
/// the difficulty changes, hide everything when there is nothing.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system
pub fn paint_note_preview(
    state: Res<NotePreview>,
    selected: Res<crate::song_select::SelectedDifficulty>,
    library: Res<SongLibrary>,
    view: Res<crate::song_select::BrowserView>,
    cursor: Res<crate::song_select::BrowserCursor>,
    mut boxes: Query<&mut Visibility, BoxOnly>,
    mut gems: Query<(&PreviewGem, &mut Node, &mut BackgroundColor), GemOnly>,
    mut tails: Query<(&PreviewTail, &mut Node, &mut BackgroundColor), TailOnly>,
    mut bars: Query<(&DensityBar, &mut Node, &mut BackgroundColor), BarOnly>,
    mut marker: Query<&mut Node, MarkerOnly>,
    mut nps: Query<&mut Text, With<NpsLine>>,
    mut drawn: Local<Option<(usize, Difficulty)>>,
) {
    use crate::palette;
    let entry = view
        .order
        .get(cursor.0)
        .and_then(|i| library.entries.get(*i));
    let track = state.shown().and_then(|chart| {
        let difficulty = crate::song_browser_view::effective(entry?, selected.0);
        let track = chart
            .track(difficulty)
            .or_else(|| chart.tracks.first().map(|(_, t)| t))?;
        Some((chart, difficulty, track))
    });
    let Some((chart, difficulty, track)) = track else {
        for mut shown in &mut boxes {
            if *shown != Visibility::Hidden {
                *shown = Visibility::Hidden;
            }
        }
        *drawn = None;
        return;
    };
    for mut shown in &mut boxes {
        if *shown != Visibility::Inherited {
            *shown = Visibility::Inherited;
        }
    }

    let now = state.now_s();
    // The column the preview is in: behind it the curve is lit, ahead
    // of it dim, so where the song stands reads at a glance.
    let reached = now.map_or(0, |t| played_columns(t, chart.duration_s));
    for (bar, mut node, mut colour) in &mut bars {
        let v = track.density.get(bar.0).copied().unwrap_or(0.0);
        put!(node, height, percent(v * 100.0));
        let tone = if bar.0 < reached {
            palette::BRAND
        } else {
            palette::dimmed(palette::BRAND, 0.3)
        };
        if colour.0 != tone {
            colour.0 = tone;
        }
    }

    // The figures: once per chart and difficulty.
    let key = (std::ptr::from_ref(chart) as usize, difficulty);
    if *drawn != Some(key) {
        *drawn = Some(key);
        let line = nps_line(track);
        for mut text in &mut nps {
            if text.0 != line {
                text.0.clone_from(&line);
            }
        }
    }

    if let Some(now) = now {
        let share = ((now / chart.duration_s).clamp(0.0, 1.0) * 100.0) as f32;
        for mut node in &mut marker {
            put!(node, left, percent(share));
        }
    }

    // The highway: the notes in view, onto the pool, the rest hidden.
    let mut gem_at = 0;
    let mut tail_at = 0;
    let mut placed: Vec<(f32, f32, f32, Color)> = Vec::with_capacity(GEM_POOL);
    let mut held: Vec<(f32, f32, f32, Color)> = Vec::with_capacity(TAIL_POOL);
    if let Some(now) = now {
        for note in &track.notes[visible(&track.notes, now)] {
            let y = y_of(note.time_s, now, STRIKE);
            for lane in note.lanes.iter() {
                let (left, width) = lane_slot(lane.index());
                let colour = palette::lane_color(lane);
                if note.sustain_s > 0.0 && tail_at < TAIL_POOL {
                    let end = y_of(note.end_s(), now, STRIKE).max(0.0);
                    let top = end.min(y);
                    held.push((left + width / 2.0 - 2.0, top, (y - top).max(0.0), colour));
                    tail_at += 1;
                }
                if gem_at < GEM_POOL {
                    placed.push((left, y - GEM_H / 2.0, width, colour));
                    gem_at += 1;
                }
            }
        }
    }
    for (gem, mut node, mut colour) in &mut gems {
        if let Some(&(left, top, width, tint)) = placed.get(gem.0) {
            put!(node, display, Display::Flex);
            put!(node, left, percent(left));
            put!(node, width, percent(width));
            put!(node, top, px(top.round()));
            if colour.0 != tint {
                colour.0 = tint;
            }
        } else {
            put!(node, display, Display::None);
        }
    }
    for (tail, mut node, mut colour) in &mut tails {
        if let Some(&(left, top, height, tint)) = held.get(tail.0) {
            let tint = palette::dimmed(tint, 0.6);
            put!(node, display, Display::Flex);
            put!(node, left, percent(left));
            put!(node, top, px(top.round()));
            put!(node, height, px(height.round()));
            if colour.0 != tint {
                colour.0 = tint;
            }
        } else {
            put!(node, display, Display::None);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use beatbyte_core::lane::Lane;

    fn tap(time_s: f64) -> PreviewNote {
        PreviewNote {
            time_s,
            lanes: LaneSet::single(Lane::One),
            sustain_s: 0.0,
        }
    }

    #[test]
    fn the_chart_is_read_only_after_a_rest_on_the_song_that_plays() {
        assert!(!wants_load(0.99, Some(3), Some(3)), "not rested yet");
        assert!(wants_load(1.0, Some(3), Some(3)));
        // The audio preview is still on the old song, or none at all.
        assert!(!wants_load(5.0, Some(2), Some(3)));
        assert!(!wants_load(5.0, None, Some(3)));
        assert!(!wants_load(5.0, None, None), "an empty list reads nothing");
    }

    #[test]
    fn arrowing_through_the_list_never_asks_for_a_chart() {
        // The audio preview restarts its rest on every move, so a list
        // scrolled at any human speed never reaches the threshold.
        let mut audio = SongPreview::default();
        let mut asked = 0;
        for frame in 0..600 {
            let song = Some(frame / 20); // a new song every 20 frames
            crate::preview::step(&mut audio, song, 1.0 / 60.0, true);
            if wants_load(audio.rest_s(), audio.playing(), song) {
                asked += 1;
            }
        }
        assert_eq!(asked, 0);
    }

    #[test]
    fn the_window_holds_exactly_the_notes_in_view() {
        let notes: Vec<_> = (0..20).map(|i| tap(f64::from(i) * 0.5)).collect();
        let range = visible(&notes, 4.0);
        let shown: Vec<f64> = notes[range].iter().map(|n| n.time_s).collect();
        // TRAIL_S behind (3.75) to LEAD_S ahead (6.0), both inclusive.
        assert_eq!(shown, vec![4.0, 4.5, 5.0, 5.5, 6.0]);
        assert!(visible(&notes, -10.0).is_empty(), "before the song");
        assert!(visible(&notes, 100.0).is_empty(), "after the song");
    }

    #[test]
    fn a_held_note_stays_in_view_until_its_tail_has_passed() {
        let mut long = tap(1.0);
        long.sustain_s = 3.0;
        let notes = vec![long, tap(10.0)];
        assert_eq!(visible(&notes, 3.5), 0..1, "still held at 3.5 s");
        assert_eq!(visible(&notes, 4.5).len(), 0, "the tail ended at 4.0");
    }

    #[test]
    fn a_note_falls_from_the_top_to_the_strike_line() {
        assert_eq!(y_of(5.0, 5.0, 200.0), 200.0, "now is on the line");
        assert_eq!(y_of(5.0 + LEAD_S, 5.0, 200.0), 0.0, "lead ahead is the top");
        assert!(y_of(4.9, 5.0, 200.0) > 200.0, "past notes go below");
    }

    #[test]
    fn the_curve_counts_every_note_once_and_tops_out_at_one() {
        let notes: Vec<_> = (0..96).map(|i| tap(f64::from(i))).collect();
        let track = summarize(notes, 96.0);
        assert_eq!(track.density.len(), BUCKETS);
        assert!(
            track.density.iter().all(|d| *d == 1.0),
            "even notes, flat curve"
        );
        let mut busy: Vec<_> = (0..10).map(|i| tap(f64::from(i) * 0.1)).collect();
        busy.push(tap(50.0));
        let track = summarize(busy, 96.0);
        assert_eq!(track.density[0], 1.0);
        assert!(track.density[1..].iter().all(|d| *d <= 0.1));
    }

    #[test]
    fn a_chord_is_one_note_and_one_burst_is_not_the_pace() {
        // One event per beat at 2 per second for a minute, one of them
        // a five-lane chord: the chord counts once.
        let mut notes: Vec<_> = (0..120).map(|i| tap(f64::from(i) * 0.5)).collect();
        notes[60].lanes = LaneSet::from_lanes(Lane::ALL);
        let track = summarize(notes, 60.0);
        assert!((track.peak_nps - 2.0).abs() < 0.01, "{}", track.peak_nps);
        assert!((track.avg_nps - 2.0).abs() < 0.05, "{}", track.avg_nps);
        // Eight notes in half a second over an otherwise empty song:
        // the peak is the window's pace, not eight per half second.
        let burst: Vec<_> = (0..8).map(|i| tap(10.0 + f64::from(i) / 16.0)).collect();
        let track = summarize(burst, 60.0);
        assert_eq!(track.peak_nps, 4.0, "8 notes / 2 s window");
    }

    #[test]
    fn the_curve_lights_up_to_where_the_song_stands() {
        assert_eq!(played_columns(0.0, 96.0), 0);
        assert_eq!(played_columns(48.0, 96.0), BUCKETS / 2);
        assert_eq!(played_columns(500.0, 96.0), BUCKETS, "never past the end");
        assert_eq!(played_columns(-3.0, 96.0), 0);
    }

    #[test]
    fn the_clock_waits_for_the_seek_to_land() {
        // The song fades in at its start, the seek to the hook follows.
        assert!(!clock_ready(0.02, 47.3));
        assert!(clock_ready(47.31, 47.3));
        assert!(!clock_ready(f64::NAN, 47.3));
        // A preview that starts at the top is ready at once.
        assert!(clock_ready(0.0, 0.0));
    }

    #[test]
    fn the_cache_keeps_the_newest_and_remembers_failures() {
        let mut cache = PreviewCache::default();
        let chart = Arc::new(PreviewChart {
            tracks: vec![],
            duration_s: 1.0,
        });
        for i in 0..CACHE {
            cache.insert(PreviewKey::Builtin(i), Some(chart.clone()));
        }
        // Touch the oldest, then add one: the second-oldest goes.
        assert!(cache.get(&PreviewKey::Builtin(0)).is_some());
        cache.insert(PreviewKey::Builtin(99), None);
        assert_eq!(cache.len(), CACHE);
        assert!(cache.get(&PreviewKey::Builtin(1)).is_none(), "evicted");
        assert!(
            cache.get(&PreviewKey::Builtin(0)).is_some(),
            "touched, kept"
        );
        assert_eq!(cache.get(&PreviewKey::Builtin(99)), Some(None), "a failure");
    }

    /// The systems themselves, in an app: what the player sees is the
    /// box showing and hiding, and the pool staying still when idle.
    mod wired {
        use super::*;
        use crate::boot::{LoadedSong, SongAudio};
        use crate::song_select::{BrowserCursor, BrowserView, SelectedDifficulty};
        use beatbyte_chart::{ChartDef, ChartNote, SongMeta};

        fn chart() -> ChartFile {
            let notes = |step: f64| -> Vec<ChartNote> {
                (0..400)
                    .map(|i| ChartNote {
                        time: f64::from(i) * step,
                        lane: (i % 5) as u8,
                        len: 0.0,
                        hopo: false,
                    })
                    .collect()
            };
            let def = |difficulty, step| ChartDef {
                difficulty,
                lanes: 5,
                notes: notes(step),
                phrases: Vec::new(),
            };
            ChartFile {
                format_version: 1,
                song: SongMeta {
                    title: "T".to_owned(),
                    artist: "A".to_owned(),
                    audio: "none.m4a".to_owned(),
                    bpm: 120.0,
                    offset_s: 0.0,
                    preview_start_s: Some(40.0),
                    duration_s: Some(200.0),
                    genre: None,
                },
                charts: vec![def(Difficulty::Easy, 0.5), def(Difficulty::Expert, 0.25)],
                provenance: None,
                audio_trim: None,
                grid: None,
                rules: None,
            }
        }

        fn app() -> App {
            AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
            let mut app = App::new();
            let song = LoadedSong {
                chart: chart(),
                audio: SongAudio::File(PathBuf::from("none.m4a")),
                lyrics: None,
                vocals: None,
                lyric_offset_ms: 0,
                song_id: None,
            };
            let entry = |i: usize| SongEntry {
                loudness: None,
                title: format!("Song {i}"),
                artist: "A".to_owned(),
                bpm: 120.0,
                duration_s: Some(200.0),
                difficulties: vec![Difficulty::Easy, Difficulty::Expert],
                note_counts: vec![],
                genre: None,
                preview_start_s: Some(40.0),
                source: SongSource::Builtin(i),
                song_id: None,
                has_lyrics: false,
                polish: crate::library::Polish::default(),
            };
            app.init_resource::<Time>()
                .init_resource::<SongPreview>()
                .init_resource::<NotePreview>()
                .init_resource::<BrowserCursor>()
                .init_resource::<BrowserView>()
                .insert_resource(SelectedDifficulty(Difficulty::Easy))
                .insert_resource(BuiltinSongs(vec![song.clone(), song]))
                .insert_resource(SongLibrary {
                    entries: vec![entry(0), entry(1)],
                })
                .insert_resource(Music(beatbyte_audio::playback::spawn_music_thread()))
                .insert_resource(Settings {
                    song_preview: true,
                    music_volume: 0.8,
                    ..Settings::default()
                })
                .add_systems(
                    Update,
                    (
                        crate::preview::drive_preview,
                        drive_note_preview,
                        paint_note_preview,
                    )
                        .chain(),
                );
            app.world_mut().resource_mut::<BrowserView>().order = vec![0, 1];
            let font = crate::ui::UiFont::from_handle(Handle::default());
            app.world_mut()
                .commands()
                .spawn(Node::default())
                .with_children(|panel| spawn_box(panel, &font));
            app.update();
            app
        }

        fn frame(app: &mut App, dt: f32) {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(dt));
            app.update();
        }

        fn visible(app: &mut App) -> bool {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&Visibility, With<NotePreviewBox>>();
            *q.single(world).unwrap() != Visibility::Hidden
        }

        fn nps(app: &mut App) -> String {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&Text, With<NpsLine>>();
            q.single(world).unwrap().0.clone()
        }

        /// Rest on the current song until its chart is on screen.
        fn rest_until_shown(app: &mut App) {
            for _ in 0..400 {
                frame(app, 0.05);
                if visible(app) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            panic!("the preview never showed");
        }

        #[test]
        fn a_rest_shows_the_notes_and_a_move_hides_them_at_once() {
            let mut app = app();
            // The audio preview has started, but the rest is short.
            frame(&mut app, crate::preview::REST_S + 0.05);
            assert!(!app.world().resource::<NotePreview>().loading());
            assert!(!visible(&mut app), "not before LOAD_REST_S");
            rest_until_shown(&mut app);
            assert_eq!(nps(&mut app), "AVG 2.0 · PEAK 2.0 /S", "the Easy track");

            app.world_mut().resource_mut::<BrowserCursor>().0 = 1;
            frame(&mut app, 0.016);
            assert!(!visible(&mut app), "the next song shows nothing yet");
        }

        #[test]
        fn another_difficulty_redraws_without_reading_again() {
            let mut app = app();
            rest_until_shown(&mut app);
            app.world_mut().resource_mut::<SelectedDifficulty>().0 = Difficulty::Expert;
            frame(&mut app, 0.016);
            assert!(!app.world().resource::<NotePreview>().loading());
            assert_eq!(nps(&mut app), "AVG 4.0 · PEAK 4.0 /S", "the Expert track");
        }

        #[test]
        fn the_pool_is_left_alone_while_nothing_shows() {
            let mut app = app();
            let world = app.world_mut();
            let mut changed = world.query_filtered::<(), (With<PreviewGem>, Changed<Node>)>();
            frame(&mut app, 0.016);
            frame(&mut app, 0.016);
            let world = app.world_mut();
            assert_eq!(
                changed.iter(world).count(),
                0,
                "an idle frame writes no gem"
            );
        }

        #[test]
        fn the_notes_in_view_land_on_the_pool() {
            let mut app = app();
            rest_until_shown(&mut app);
            // Draw a moment directly: the clock needs a real device.
            app.world_mut().resource_mut::<NotePreview>().now_s = Some(10.0);
            app.world_mut()
                .run_system_cached(paint_note_preview)
                .unwrap();
            let world = app.world_mut();
            let mut gems = world.query::<(&PreviewGem, &Node)>();
            let shown = gems
                .iter(world)
                .filter(|(_, n)| n.display != Display::None)
                .count();
            // Easy steps 0.5 s: 9.75..=12.0 holds 10.0 to 12.0, 5 notes.
            assert_eq!(shown, 5);

            // The same moment again writes nothing: unchanged values
            // are not assigned.
            let mut changed = world.query_filtered::<(), (With<PreviewGem>, Changed<Node>)>();
            changed.iter(world).count(); // the reader's baseline
            app.world_mut().clear_trackers();
            app.world_mut()
                .run_system_cached(paint_note_preview)
                .unwrap();
            let world = app.world_mut();
            assert_eq!(
                changed.iter(world).count(),
                0,
                "a still frame writes no gem"
            );
        }
    }
}
