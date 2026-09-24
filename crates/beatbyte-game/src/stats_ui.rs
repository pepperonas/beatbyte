//! One player's statistics, across eight views.
//!
//! Each view answers one question and says so in its subtitle (see
//! `docs/superpowers/specs/2026-09-23-player-analytics-design.md`):
//!
//! - **OVERVIEW** — am I getting better?
//! - **TIMING** — early, late, or just noisy?
//! - **TECHNIQUE** — which frets and patterns break me?
//! - **DIFFICULTY** — where do I play, and how far?
//! - **PROGRESS** — steady, or streaky?
//! - **SONGS** — what should I practise next?
//! - **VERSUS** — how do I stand against the others?
//! - **INSIGHTS** — what matters right now?
//!
//! Run/career arithmetic stays in [`beatbyte_core::stats`] and draws
//! from `history.jsonl`. Note-grain evidence comes from a **read-only**
//! open of `telemetry.db` on a background task (never the gameplay
//! writer). The drawing is [`crate::plot`].

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use beatbyte_core::Difficulty;
use beatbyte_core::player::PlayerId;
use beatbyte_core::stats::{self, Filter, PlayerRun};
use beatbyte_telemetry::analytics::{self, PlayerSnapshot, Scope};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future};
use bevy::ui::Val::Px as px;

use crate::controls::{InputMap, MenuNav};
use crate::history::PlayHistory;
use crate::palette;
use crate::players::Players;
use crate::plot;
use crate::states::AppState;
use crate::telemetry::store_path;
use crate::ui::UiFont;
use crate::ui_kit;

/// Width of the plot area inside the wide panel.
const PLOT_W: f32 = 700.0;
/// Height of a line plot.
const PLOT_H: f32 = 170.0;
/// Width of a horizontal bar's track.
const BAR_W: f32 = 320.0;
/// Height of the timing histogram.
const HIST_H: f32 = 72.0;
/// Height of a song heat strip.
const HEAT_H: f32 = 18.0;

/// Whose statistics the screen shows. Set by the roster before the
/// state change; falls back to whoever is playing.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct StatsFor(pub Option<PlayerId>);

/// The view `BEATBYTE_SHOT_VIEW` asks for, for the harness.
///
/// Without it only the first of eight views could ever be
/// photographed, which is the same blind spot `BEATBYTE_SHOT_ROW`
/// exists to close for a scrolling list. Pure — tested.
#[must_use]
pub fn view_named(raw: &str) -> Option<View> {
    let wanted = raw.to_ascii_lowercase();
    View::ALL
        .into_iter()
        .find(|view| view.label().eq_ignore_ascii_case(&wanted))
}

/// Which of the eight views is on screen.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatsView(pub View);

/// The six views, in tab order (Player Analytics).
///
/// There were eight. PROGRESS drew the same accuracies OVERVIEW
/// already draws, only pooled across difficulties — which is the
/// reading the overview's own comment argues against, because
/// pooling shows a player "getting worse" the day they moved up —
/// and its three tiles were a duplicate count plus two sentences.
/// INSIGHTS was a panel 1150 px wide holding at most five short
/// lines, and those lines are the answer to the overview's own
/// question. Both now live on OVERVIEW; nothing they said was lost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum View {
    /// Progress over time.
    #[default]
    Overview,
    /// Drift and judgment mix.
    Timing,
    /// Frets, patterns, chords — note grain from telemetry.
    Technique,
    /// Where the player plays.
    Difficulty,
    /// What to practise next.
    Songs,
    /// Against the other players.
    Versus,
}

impl View {
    /// All six, in tab order.
    pub const ALL: [View; 6] = [
        View::Overview,
        View::Timing,
        View::Technique,
        View::Difficulty,
        View::Songs,
        View::Versus,
    ];

    /// The tab's label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            View::Overview => "OVERVIEW",
            View::Timing => "TIMING",
            View::Technique => "TECHNIQUE",
            View::Difficulty => "DIFFICULTY",
            View::Songs => "SONGS",
            View::Versus => "VERSUS",
        }
    }

    /// The question the view answers, shown under the title.
    #[must_use]
    pub const fn question(self) -> &'static str {
        match self {
            View::Overview => "AM I GETTING BETTER?",
            View::Timing => "EARLY, LATE, OR JUST NOISY?",
            View::Technique => "WHICH FRETS AND PATTERNS BREAK ME?",
            View::Difficulty => "WHERE DO I PLAY, AND HOW FAR DO I GET?",
            View::Songs => "WHAT SHOULD I PRACTISE NEXT?",
            View::Versus => "HOW DO I STAND AGAINST THE OTHERS?",
        }
    }

    /// Whether this tab redraws when the telemetry snapshot lands.
    ///
    /// OVERVIEW is on the list since it took the findings in: most of
    /// them are read from the snapshot, and without the redraw they
    /// would appear only once the player pressed something.
    #[must_use]
    pub const fn needs_telemetry(self) -> bool {
        matches!(
            self,
            View::Overview | View::Timing | View::Technique | View::Songs
        )
    }

    /// The next view along.
    ///
    /// Stops at the ends rather than wrapping, because every list in
    /// this game does ([`ui_kit::step_cursor`]) — a screen with its
    /// own navigation model is the drift `ui_kit` exists to stop.
    /// Pure — tested.
    #[must_use]
    pub fn step(self, delta: i32) -> View {
        let at = View::ALL.iter().position(|v| *v == self).unwrap_or(0);
        View::ALL[ui_kit::step_cursor(at, View::ALL.len(), delta)]
    }
}

/// How far back the history views look.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TimeWindow {
    /// Last seven days.
    Days7,
    /// Last thirty days.
    Days30,
    /// Last ninety days.
    Days90,
    /// Everything on disk.
    #[default]
    All,
}

impl TimeWindow {
    const ALL: [TimeWindow; 4] = [
        TimeWindow::Days7,
        TimeWindow::Days30,
        TimeWindow::Days90,
        TimeWindow::All,
    ];

    /// Chip label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            TimeWindow::Days7 => "7D",
            TimeWindow::Days30 => "30D",
            TimeWindow::Days90 => "90D",
            TimeWindow::All => "ALL",
        }
    }

    /// Step along the window chips. Pure — tested.
    #[must_use]
    pub fn step(self, delta: i32) -> TimeWindow {
        let at = Self::ALL.iter().position(|w| *w == self).unwrap_or(0);
        Self::ALL[ui_kit::step_cursor(at, Self::ALL.len(), delta)]
    }

    /// Milliseconds retained from `now`, or `None` for All.
    #[must_use]
    pub const fn span_ms(self) -> Option<u64> {
        const DAY: u64 = 86_400_000;
        match self {
            TimeWindow::Days7 => Some(7 * DAY),
            TimeWindow::Days30 => Some(30 * DAY),
            TimeWindow::Days90 => Some(90 * DAY),
            TimeWindow::All => None,
        }
    }
}

/// Difficulty + time window for Stats (Player Analytics P0).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatsFilters {
    /// `None` = every difficulty.
    pub difficulty: Option<Difficulty>,
    /// How far back history views look.
    pub window: TimeWindow,
}

/// Whether a run belongs under the current filters. Pure — tested.
#[must_use]
pub fn run_passes_filters(
    difficulty_id: &str,
    started_ms: u64,
    filters: StatsFilters,
    now_ms: u64,
) -> bool {
    if let Some(want) = filters.difficulty
        && Difficulty::from_id(difficulty_id) != Some(want)
    {
        return false;
    }
    if let Some(span) = filters.window.span_ms()
        && now_ms.saturating_sub(started_ms) > span
    {
        return false;
    }
    true
}

/// The telemetry scope the screen's filters mean.
///
/// ⚠️ This must say exactly what [`run_passes_filters`] says on the
/// history side, or the two halves of one screen answer the same
/// question differently — which is the state it replaces: the chips
/// were drawn over all eight tabs while the snapshot took no filters
/// at all, so on four of them pressing a chip rebuilt the screen and
/// produced a byte-identical answer. A test pins the two together.
///
/// A value that cannot be expressed as SQLite's signed integer
/// excludes rather than includes: `i64::MAX` matches no session and
/// no start time, where a `None` would silently widen the question to
/// everything. Neither can happen with a roster id or a wall clock,
/// and a filter that quietly stops filtering is the bug being fixed.
#[must_use]
pub fn telemetry_scope(player: Option<PlayerId>, filters: StatsFilters, now_ms: u64) -> Scope {
    Scope {
        player_id: player.map(|id| i64::try_from(id).unwrap_or(i64::MAX)),
        difficulty: filters.difficulty.map(crate::telemetry::difficulty_index),
        since_ms: filters
            .window
            .span_ms()
            .map(|span| i64::try_from(now_ms.saturating_sub(span)).unwrap_or(i64::MAX)),
    }
}

/// What a player filter leaves out of the telemetry, said plainly —
/// or `None` when it leaves nothing out.
///
/// ⚠️ Worth a line on the screen because the number is large and the
/// cause is invisible: sessions recorded before runs were attributed
/// name no player, and the one-time adoption that claimed the play
/// history for its player never ran over the telemetry store. On the
/// machine this was written on that is 138 of 144 honest sessions,
/// so a screen that simply showed the remaining six would read as a
/// regression rather than as a filter doing its job.
#[must_use]
pub fn unattributed_note(count: u64) -> Option<String> {
    match count {
        0 => None,
        1 => Some("1 RECORDED RUN NAMES NO PLAYER — NOT COUNTED HERE".to_owned()),
        many => Some(format!(
            "{many} RECORDED RUNS NAME NO PLAYER — NOT COUNTED HERE"
        )),
    }
}

/// Wall-clock ms for window filters.
#[must_use]
pub fn wall_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Read-only session count from the on-disk store. Pure I/O helper —
/// the async job and its tests share it.
pub fn probe_session_count(path: &Path) -> Result<u64, String> {
    beatbyte_telemetry::Store::open_readonly(path)
        .and_then(|store| store.session_count())
        .map_err(|error| error.to_string())
}

/// Load the Stats-shell snapshot from disk. Shared by the async job
/// and its tests.
pub fn load_player_snapshot(path: &Path, scope: Scope) -> Result<PlayerSnapshot, String> {
    // Nothing recorded yet is the first-run case, not a failure, and
    // it must not reach the player as a SQLite sentence.
    if !path.exists() {
        return Err("NO TELEMETRY RECORDED YET — PLAY A SONG FIRST".to_owned());
    }
    beatbyte_telemetry::Store::open_readonly(path)
        .and_then(|store| analytics::player_snapshot(&store, scope))
        .map_err(|error| format!("TELEMETRY UNREADABLE — {error}").to_uppercase())
}

/// Background snapshot of `telemetry.db` for the note-grain tabs.
#[derive(Resource, Default)]
struct TelemetryProbe {
    /// In-flight job, if any.
    task: Option<Task<Result<PlayerSnapshot, String>>>,
    /// Bumped every time a new probe is requested.
    generation: u64,
    /// Generation the `result` belongs to.
    result_generation: u64,
    /// Last finished probe.
    result: Option<Result<PlayerSnapshot, String>>,
}

impl TelemetryProbe {
    fn request(&mut self, path: Option<std::path::PathBuf>, scope: Scope) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let Some(path) = path else {
            self.task = None;
            self.result_generation = generation;
            self.result = Some(Err("NO PLACE TO KEEP TELEMETRY ON THIS MACHINE".to_owned()));
            return;
        };
        self.task = Some(
            AsyncComputeTaskPool::get().spawn(async move { load_player_snapshot(&path, scope) }),
        );
    }

    fn poll(&mut self) {
        let Some(task) = self.task.as_mut() else {
            return;
        };
        if let Some(outcome) = block_on(future::poll_once(task)) {
            self.task = None;
            self.result_generation = self.generation;
            self.result = Some(outcome);
        }
    }

    fn ready(&self) -> Option<&Result<PlayerSnapshot, String>> {
        if self.task.is_some() || self.result_generation != self.generation {
            return None;
        }
        self.result.as_ref()
    }

    fn snapshot(&self) -> Option<&PlayerSnapshot> {
        self.ready().and_then(|r| r.as_ref().ok())
    }

    /// What to show in place of a reading that is not there.
    ///
    /// ⚠️ The error arm used to discard its reason and print "NO
    /// TELEMETRY STORE" — so a locked or damaged database, or a
    /// permission problem, all claimed the store did not exist,
    /// which is a different and wrong statement. The reason is now
    /// carried through; the common case (nothing recorded yet) is
    /// told apart from a real failure by `load_player_snapshot`.
    fn line(&self) -> String {
        match self.ready() {
            None => "…".to_owned(),
            Some(Ok(snap)) => format!("{} HONEST SESSIONS RECORDED", snap.sessions),
            Some(Err(reason)) => reason.clone(),
        }
    }
}

/// Everything this screen spawns.
#[derive(Component)]
struct StatsScreen;

/// A view tab the mouse can pick — same as LEFT/RIGHT.
#[derive(Component)]
struct ViewTab(View);

/// A difficulty filter chip.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct DifficultyChip(Option<Difficulty>);

/// A time-window filter chip.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct WindowChip(TimeWindow);

/// The panel the views draw into — and the thing that scrolls.
///
/// ⚠️ This was `ui_kit::panel_wide()`, which has no ceiling and does
/// not clip: SONGS with a real library ran clean off the bottom of
/// the window, taking the back button and the footer with it, and
/// VERSUS loops over every other player with no limit at all. Every
/// other list in this game is a `scroll_panel`; this one is not an
/// exception, it was an oversight.
#[derive(Component)]
struct StatsBody;

/// Systems of the statistics screen.
pub struct StatsUiPlugin;

impl Plugin for StatsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StatsFor>()
            .init_resource::<StatsView>()
            .init_resource::<StatsFilters>()
            .init_resource::<TelemetryProbe>()
            .add_systems(
                OnEnter(AppState::Stats),
                (pick_shot_view, kick_telemetry_probe, spawn_stats)
                    .chain()
                    .after(crate::history::HistoryReloaded),
            )
            .add_systems(
                Update,
                (
                    poll_telemetry_probe,
                    stats_nav,
                    stats_scroll,
                    paint_chips,
                    refresh_shell_when_probe_lands,
                )
                    .chain()
                    .run_if(in_state(AppState::Stats)),
            )
            .add_systems(
                OnExit(AppState::Stats),
                (clear_telemetry_probe, despawn_stats).chain(),
            );
    }
}

/// Who the statistics screen is about.
///
/// Bundled because two systems ask it — the probe that starts on
/// entry and the one that restarts it when a filter changes — and
/// because `stats_nav` sits at Bevy's sixteen-parameter cap.
#[derive(bevy::ecs::system::SystemParam)]
struct Viewer<'w> {
    chosen: Res<'w, StatsFor>,
    players: Res<'w, Players>,
}

impl Viewer<'_> {
    /// The roster player the screen is drawn for, if any. Matches
    /// what `spawn_stats` picks, so the telemetry half and the
    /// history half are about the same person.
    fn id(&self) -> Option<PlayerId> {
        self.chosen.0.or(self.players.0.selected)
    }
}

fn kick_telemetry_probe(
    mut probe: ResMut<TelemetryProbe>,
    viewer: Viewer,
    filters: Res<StatsFilters>,
) {
    probe.request(
        store_path(),
        telemetry_scope(viewer.id(), *filters, wall_now_ms()),
    );
}

fn poll_telemetry_probe(mut probe: ResMut<TelemetryProbe>) {
    probe.poll();
}

/// How far one press or one notch moves the panel.
const SCROLL_STEP: f32 = 48.0;

/// Free scrolling, because the panel holds charts and paragraphs
/// rather than rows a cursor could step between (the song-info
/// pattern). UP/DOWN are free here: the tabs are on LEFT/RIGHT.
#[allow(clippy::needless_pass_by_value)] // Bevy system params
fn stats_scroll(
    map: Res<InputMap>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&bevy::input::gamepad::Gamepad>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut bodies: Query<&mut ScrollPosition, With<StatsBody>>,
) {
    let nav = MenuNav::read(&map, &keys, pads.iter());
    let mut delta = 0.0;
    if nav.down {
        delta += SCROLL_STEP;
    }
    if nav.up {
        delta -= SCROLL_STEP;
    }
    for event in wheel.read() {
        delta -= event.y * SCROLL_STEP * 0.5;
    }
    if delta == 0.0 {
        return;
    }
    for mut position in bodies.iter_mut() {
        position.y = (position.y + delta).max(0.0);
    }
}

fn clear_telemetry_probe(mut probe: ResMut<TelemetryProbe>) {
    *probe = TelemetryProbe::default();
}

/// When the async probe finishes on a telemetry shell tab, rebuild so
/// the placeholder text updates without waiting for a key.
fn refresh_shell_when_probe_lands(
    probe: Res<TelemetryProbe>,
    view: Res<StatsView>,
    mut commands: Commands,
    screen: Query<Entity, With<StatsScreen>>,
) {
    if !view.0.needs_telemetry() {
        return;
    }
    if !probe.is_changed() {
        return;
    }
    if probe.task.is_some() || probe.result_generation != probe.generation {
        return;
    }
    for entity in &screen {
        commands.entity(entity).despawn();
    }
    commands.run_system_cached(spawn_stats);
}

/// Open the view the harness asked for, so all eight can be
/// photographed rather than only the first.
fn pick_shot_view(mut view: ResMut<StatsView>) {
    if let Ok(raw) = std::env::var("BEATBYTE_SHOT_VIEW") {
        match view_named(&raw) {
            Some(wanted) => view.0 = wanted,
            None => bevy::log::error!("unknown BEATBYTE_SHOT_VIEW `{raw}`"),
        }
    }
}

/// A trend, said in words rather than left as a slope.
///
/// Per ten runs, because "0.0004 per run" is a number nobody can
/// picture. `None` below three runs — two points always fit a line,
/// and calling that a trend would claim more than the data holds.
/// Pure — tested.
#[must_use]
pub fn trend_line(slope: Option<f64>) -> String {
    let Some(slope) = slope else {
        return "NOT ENOUGH RUNS FOR A TREND".to_owned();
    };
    let per_ten = slope * 10.0 * 100.0;
    if per_ten.abs() < 0.5 {
        return "HOLDING STEADY".to_owned();
    }
    let direction = if per_ten > 0.0 { "UP" } else { "DOWN" };
    format!("{direction} {:.1} POINTS PER 10 RUNS", per_ten.abs())
}

/// How a drift reads to a player. The thresholds are the results
/// screen's, so one run and a hundred runs are described the same
/// way. Pure — tested.
#[must_use]
pub fn drift_line(mean_ms: Option<f64>) -> String {
    let Some(mean) = mean_ms else {
        return "NO TIMING RECORDED YET".to_owned();
    };
    if mean.abs() < 3.0 {
        "ON TIME".to_owned()
    } else if mean < 0.0 {
        format!("{} EARLY ON AVERAGE", plot::millis_abs(mean))
    } else {
        format!("{} LATE ON AVERAGE", plot::millis_abs(mean))
    }
}

/// The colour a difficulty's line wears. Reuses the judgment palette
/// so the four levels read as a ramp rather than four random hues.
#[must_use]
const fn difficulty_colour(difficulty: beatbyte_core::Difficulty) -> Color {
    match difficulty {
        beatbyte_core::Difficulty::Easy => palette::PERFECT,
        beatbyte_core::Difficulty::Medium => palette::GREAT,
        beatbyte_core::Difficulty::Hard => palette::GOOD,
        beatbyte_core::Difficulty::Expert => palette::MISS,
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system
fn spawn_stats(
    mut commands: Commands,
    font: Res<UiFont>,
    players: Res<Players>,
    history: Res<PlayHistory>,
    view: Res<StatsView>,
    chosen: Res<StatsFor>,
    filters: Res<StatsFilters>,
    probe: Res<TelemetryProbe>,
) {
    let Some(id) = chosen.0.or(players.0.selected) else {
        // Reached with nobody chosen: say so rather than drawing an
        // empty frame the player cannot explain.
        commands
            .spawn((ui_kit::screen_root(), StatsScreen))
            .with_children(|root| {
                ui_kit::header(root, &font, "STATISTICS", "NO PLAYER CHOSEN");
                root.spawn(ui_kit::panel()).with_children(|panel| {
                    plot::empty_note(panel, &font, "CREATE A PLAYER ON THE PLAYERS SCREEN FIRST");
                });
                ui_kit::back_button(root, &font, "PLAYERS");
                crate::prompts::device_footer(root, &font, "ESC BACK", "EAST back");
            });
        return;
    };
    let name = players
        .0
        .name_of(id)
        .map_or_else(|| "PLAYER".to_owned(), str::to_owned);
    let now = wall_now_ms();
    let runs: Vec<PlayerRun<'_>> = stats::runs_of(&history.0, id, Filter::default())
        .into_iter()
        .filter(|run| {
            run_passes_filters(&run.entry.difficulty, run.entry.started_ms, *filters, now)
        })
        .collect();
    let summary = stats::summarize(&runs);

    commands
        .spawn((ui_kit::screen_root(), StatsScreen))
        .with_children(|root| {
            ui_kit::header(root, &font, &name.to_uppercase(), view.0.question());
            spawn_tabs(root, &font, view.0);
            spawn_filters(root, &font, *filters);
            if view.0.needs_telemetry()
                && let Some(note) = probe
                    .snapshot()
                    .and_then(|snap| unattributed_note(snap.unattributed))
            {
                root.spawn((
                    Text::new(note),
                    font.text(ui_kit::SMALL),
                    TextColor(ui_kit::dimmed_subtitle()),
                    Node {
                        margin: UiRect::bottom(px(8.0)),
                        ..default()
                    },
                ));
            }
            root.spawn((StatsBody, ui_kit::scroll_panel(ui_kit::PANEL_WIDE)))
                .with_children(|panel| match view.0 {
                    View::Overview => overview(panel, &font, &runs, &summary, probe.snapshot()),
                    View::Timing => timing(panel, &font, &runs, &summary, probe.snapshot()),
                    View::Difficulty => difficulty(panel, &font, &runs),
                    View::Versus => {
                        // Same difficulty + window chips as the other
                        // history views — otherwise "7D · HARD" would
                        // leave VERSUS showing career-wide duels.
                        let versus_entries: Vec<beatbyte_core::history::PlayEntry> = history
                            .0
                            .iter()
                            .filter(|entry| {
                                run_passes_filters(
                                    &entry.difficulty,
                                    entry.started_ms,
                                    *filters,
                                    now,
                                )
                            })
                            .cloned()
                            .collect();
                        versus(panel, &font, &versus_entries, &players, id, &name);
                    }
                    View::Technique => {
                        technique_view(panel, &font, probe.snapshot(), &probe.line())
                    }
                    View::Songs => songs_view(panel, &font, &runs, probe.snapshot(), &probe.line()),
                });
            ui_kit::back_button(root, &font, "PLAYERS");
            crate::prompts::device_footer(
                root,
                &font,
                "LEFT/RIGHT view  UP/DOWN scroll  , . difficulty  - + window  ESC back",
                "D-PAD view + scroll  WEST difficulty  NORTH window  EAST back",
            );
        });
}

/// One chip of a set, from the kit. `marker` is what the press
/// handler reads back, so a chip's value stays typed rather than
/// becoming an opaque id.
fn chip<M: Component>(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    marker: M,
    label: &str,
    chosen: bool,
) {
    let (fg, border, fill) = ui_kit::selection_chip_colours(chosen, false);
    parent
        .spawn((
            marker,
            ui_kit::SelectionChip { chosen },
            Button,
            ui_kit::selection_chip_node(),
            BackgroundColor(fill),
            BorderColor::all(border),
        ))
        .with_children(|chip| {
            chip.spawn((
                Text::new(label.to_owned()),
                font.text(ui_kit::SMALL),
                TextColor(fg),
                TextLayout::default().with_no_wrap(),
            ));
        });
}

/// Hover feedback for every chip on the screen.
///
/// ⚠️ There was none. The tabs and both filter rows were bare words
/// whose only state cue was colour, so a pointer over them said
/// nothing at all and an unselected one did not read as pressable.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)] // Bevy system
fn paint_chips(
    mut chips: Query<
        (
            &ui_kit::SelectionChip,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
            &Children,
        ),
        Changed<Interaction>,
    >,
    mut labels: Query<&mut TextColor>,
) {
    for (chip, interaction, mut background, mut border, children) in chips.iter_mut() {
        let label = children.iter().find(|child| labels.contains(*child));
        let mut text = label.and_then(|child| labels.get_mut(child).ok());
        ui_kit::paint_selection_chip(
            *interaction,
            chip.chosen,
            &mut background,
            &mut border,
            text.as_deref_mut(),
        );
    }
}

/// The row of view tabs.
fn spawn_tabs(parent: &mut ChildSpawnerCommands, font: &UiFont, active: View) {
    parent
        .spawn(Node {
            column_gap: px(6.0),
            margin: UiRect::bottom(px(8.0)),
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(6.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|tabs| {
            for view in View::ALL {
                chip(tabs, font, ViewTab(view), view.label(), view == active);
            }
        });
}

/// Difficulty + window chips under the tabs.
fn spawn_filters(parent: &mut ChildSpawnerCommands, font: &UiFont, filters: StatsFilters) {
    parent
        .spawn(Node {
            column_gap: px(8.0),
            margin: UiRect::bottom(px(12.0)),
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(4.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("DIFF".to_owned()),
                font.text(ui_kit::SMALL),
                TextColor(ui_kit::dimmed_subtitle()),
            ));
            for chip in [
                None,
                Some(Difficulty::Easy),
                Some(Difficulty::Medium),
                Some(Difficulty::Hard),
                Some(Difficulty::Expert),
            ] {
                // ⚠️ One name set. The chip said `MED` and `EXP`
                // while the bar two rows down said `MEDIUM` and
                // `EXPERT`, so filtering to a difficulty renamed it.
                let label =
                    chip.map_or_else(|| "ALL".to_owned(), |d| d.display_name().to_uppercase());
                let on = filters.difficulty == chip;
                self::chip(row, font, DifficultyChip(chip), &label, on);
            }
            row.spawn((
                Text::new("·".to_owned()),
                font.text(ui_kit::SMALL),
                TextColor(ui_kit::dimmed_subtitle()),
            ));
            for window in TimeWindow::ALL {
                let on = filters.window == window;
                chip(row, font, WindowChip(window), window.label(), on);
            }
        });
}

/// A labelled figure, the screen's unit of "one number".
fn tile(parent: &mut ChildSpawnerCommands, font: &UiFont, label: &str, value: &str) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3.0),
            min_width: px(120.0),
            ..default()
        })
        .with_children(|cell| {
            cell.spawn((
                Text::new(label.to_owned()),
                font.text(ui_kit::SMALL),
                TextColor(ui_kit::dimmed_subtitle()),
            ));
            cell.spawn((
                Text::new(value.to_owned()),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT),
            ));
        });
}

/// A row of tiles.
fn tiles(parent: &mut ChildSpawnerCommands, font: &UiFont, cells: &[(&str, String)]) {
    parent
        .spawn(Node {
            column_gap: px(ui_kit::CELL_GAP),
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(10.0),
            margin: UiRect::bottom(px(14.0)),
            ..default()
        })
        .with_children(|row| {
            for (label, value) in cells {
                tile(row, font, label, value);
            }
        });
}

/// A duration, said the way a person would.
#[must_use]
pub fn played_time(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    if minutes < 90 {
        format!("{minutes} MIN")
    } else {
        format!("{:.1} H", seconds / 3600.0)
    }
}

/// OVERVIEW: the headline numbers and the accuracy line per
/// difficulty.
fn overview(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    runs: &[PlayerRun<'_>],
    summary: &stats::Summary,
    snap: Option<&PlayerSnapshot>,
) {
    // The findings first: they are the short answer to the question
    // in the header, and the numbers below are the working.
    findings(parent, font, runs, summary, snap);
    tiles(
        parent,
        font,
        &[
            ("RUNS", summary.runs.to_string()),
            ("FINISHED", summary.completed.to_string()),
            ("SONGS", summary.songs.to_string()),
            ("PLAYED", played_time(summary.seconds)),
            (
                "BEST ACCURACY",
                summary
                    .best_accuracy
                    .map_or_else(|| "-".to_owned(), plot::percent),
            ),
            (
                "BEST SCORE",
                summary
                    .best_score
                    .map_or_else(|| "-".to_owned(), |score| score.to_string()),
            ),
            (
                "BEST STREAK",
                summary
                    .best_streak
                    .map_or_else(|| "-".to_owned(), |s| s.to_string()),
            ),
        ],
    );

    let points = stats::progression(runs);
    // One line per difficulty: a single line mixing Easy and Expert
    // would show a player "getting worse" the day they moved up.
    let series: Vec<plot::Series> = beatbyte_core::Difficulty::ALL
        .into_iter()
        .filter_map(|difficulty| {
            let values: Vec<f64> = points
                .iter()
                .filter(|p| p.difficulty == Some(difficulty))
                .map(|p| p.accuracy)
                .collect();
            (!values.is_empty()).then(|| plot::Series {
                label: difficulty.display_name().to_uppercase(),
                colour: difficulty_colour(difficulty),
                values,
            })
        })
        .collect();
    let bounds = plot::Bounds::around(points.iter().map(|p| p.accuracy))
        .unwrap_or(plot::Bounds { min: 0.0, max: 1.0 })
        .clamped(0.0, 1.0);
    plot::spawn_line_plot(
        parent,
        font,
        &plot::LinePlot {
            width: PLOT_W,
            height: PLOT_H,
            series: &series,
            bounds,
            rule: None,
            label: plot::percent,
            caption: "FINISHED RUNS, OLDEST FIRST".to_owned(),
        },
    );
    // Trend AND spread are taken per difficulty for the same reason,
    // and the one with the most runs is the one worth stating. The
    // retired PROGRESS tab pooled both across difficulties, so its
    // trend read 5.7 where this one read 10.6 — two numbers for one
    // quantity on one screen. This is the honest one.
    let busiest = series.iter().max_by_key(|s| s.values.len());
    parent.spawn((
        Node {
            margin: UiRect::top(px(10.0)),
            ..default()
        },
        Text::new(busiest.map_or_else(
            || "NO FINISHED RUNS YET".to_owned(),
            |s| {
                format!(
                    "{}: {} · {}",
                    s.label,
                    trend_line(stats::trend(&s.values)),
                    consistency_line(&s.values)
                )
            },
        )),
        font.text(ui_kit::SMALL),
        TextColor(palette::BRAND),
    ));
}

/// TIMING: drift against the zero line, judgment mix, and the
/// telemetry hit histogram when the store has enough samples.
fn timing(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    runs: &[PlayerRun<'_>],
    summary: &stats::Summary,
    snap: Option<&PlayerSnapshot>,
) {
    let telemetry_bias = snap.and_then(|s| s.bias_ms);
    tiles(
        parent,
        font,
        &[
            (
                "AVERAGE DRIFT",
                telemetry_bias
                    .or(summary.mean_offset_ms)
                    .map_or_else(|| "-".to_owned(), plot::millis),
            ),
            (
                "PERFECT SHARE",
                summary
                    .perfect_share
                    .map_or_else(|| "-".to_owned(), plot::percent),
            ),
            (
                "OVERSTRUMS / MIN",
                summary
                    .overstrums_per_min
                    .map_or_else(|| "-".to_owned(), |v| format!("{v:.1}")),
            ),
        ],
    );

    // ⚠️ A tile carries a NUMBER. The drift tile carried the whole
    // sentence "24 MS LATE ON AVERAGE" in a 120-px column, where its
    // neighbours held "38%" and "3.2" — so one tile wrapped to three
    // lines and set the height of the row. The sentence says what
    // the number means and belongs under the row, once.
    parent.spawn((
        Node {
            margin: UiRect::top(px(2.0)),
            ..default()
        },
        Text::new(drift_line(telemetry_bias.or(summary.mean_offset_ms))),
        font.text(ui_kit::SMALL),
        TextColor(ui_kit::dimmed_subtitle()),
    ));

    if let Some(snap) = snap
        && !snap.histogram.is_empty()
    {
        parent.spawn((
            Node {
                margin: UiRect::top(px(8.0)).with_bottom(px(4.0)),
                ..default()
            },
            Text::new("HIT OFFSET HISTOGRAM".to_owned()),
            font.text(ui_kit::SMALL),
            TextColor(ui_kit::dimmed_subtitle()),
        ));
        plot::spawn_histogram(parent, font, &snap.histogram, HIST_H, PLOT_W);
    }

    let drift: Vec<f64> = stats::progression(runs)
        .iter()
        .filter_map(|p| p.mean_offset_ms)
        .collect();
    let series = [plot::Series {
        label: "DRIFT".to_owned(),
        colour: palette::HYPE,
        values: drift.clone(),
    }];
    // The zero line has to be on screen even when every run was
    // late, or "late" has nothing to be late against.
    let bounds = plot::Bounds::around(drift.iter().copied())
        .unwrap_or(plot::Bounds {
            min: -30.0,
            max: 30.0,
        })
        .including(0.0);
    plot::spawn_line_plot(
        parent,
        font,
        &plot::LinePlot {
            width: PLOT_W,
            height: PLOT_H,
            series: &series,
            bounds,
            rule: Some(0.0),
            label: plot::millis,
            caption: "ABOVE THE LINE IS LATE".to_owned(),
        },
    );

    parent.spawn((
        Node {
            margin: UiRect::top(px(16.0)).with_bottom(px(6.0)),
            ..default()
        },
        Text::new("JUDGMENT MIX".to_owned()),
        font.text(ui_kit::SMALL),
        TextColor(ui_kit::dimmed_subtitle()),
    ));
    let (mut perfect, mut great, mut good, mut miss) = (0_u64, 0_u64, 0_u64, 0_u64);
    for run in runs {
        let detail = &run.part.detail;
        perfect += u64::from(detail.perfect.unwrap_or(0));
        great += u64::from(detail.great.unwrap_or(0));
        good += u64::from(detail.good.unwrap_or(0));
        miss += u64::from(detail.miss.unwrap_or(0));
    }
    let total = (perfect + great + good + miss) as f64;
    if total > 0.0 {
        let slice = |count: u64, colour: Color| plot::Slice {
            share: count as f64 / total,
            colour,
        };
        plot::spawn_stack(
            parent,
            &[
                slice(perfect, palette::PERFECT),
                slice(great, palette::GREAT),
                slice(good, palette::GOOD),
                slice(miss, palette::MISS),
            ],
            PLOT_W,
            14.0,
        );
        parent.spawn((
            Node {
                margin: UiRect::top(px(5.0)),
                ..default()
            },
            Text::new(format!(
                "PERFECT {perfect}   GREAT {great}   GOOD {good}   MISS {miss}"
            )),
            font.text(ui_kit::SMALL),
            TextColor(ui_kit::dimmed_subtitle()),
        ));
    } else {
        // Runs older than the judgment counts. Saying so is the
        // honest answer; a bar of zeroes would not be.
        plot::empty_note(parent, font, "NO RUN HERE RECORDED ITS JUDGMENTS YET");
    }
}

/// TECHNIQUE: note-kind hit rates + musical context miss rates.
fn technique_view(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    snap: Option<&PlayerSnapshot>,
    waiting: &str,
) {
    let Some(snap) = snap else {
        plot::empty_note(parent, font, waiting);
        return;
    };
    if snap.technique.is_empty() && snap.context.is_empty() {
        plot::empty_note(
            parent,
            font,
            "NOT ENOUGH HONEST NOTE JUDGMENTS YET — PLAY WITHOUT AUTOPILOT",
        );
        return;
    }
    if !snap.technique.is_empty() {
        parent.spawn((
            Node {
                margin: UiRect::bottom(px(6.0)),
                ..default()
            },
            Text::new("HIT RATE BY NOTE KIND".to_owned()),
            font.text(ui_kit::SMALL),
            TextColor(ui_kit::dimmed_subtitle()),
        ));
        let bars: Vec<plot::Bar> = snap
            .technique
            .iter()
            .map(|row| plot::Bar {
                label: row.label.to_uppercase(),
                value: Some(row.hit_rate),
                colour: palette::GREAT,
                note: plot::with_sample(&plot::percent(row.hit_rate), row.judged),
            })
            .collect();
        plot::spawn_bar_plot(
            parent,
            font,
            &bars,
            plot::Bounds { min: 0.0, max: 1.0 },
            BAR_W,
        );
    }
    if !snap.context.is_empty() {
        parent.spawn((
            Node {
                margin: UiRect::top(px(14.0)).with_bottom(px(6.0)),
                ..default()
            },
            Text::new("MISS RATE BY MUSICAL CONTEXT".to_owned()),
            font.text(ui_kit::SMALL),
            TextColor(ui_kit::dimmed_subtitle()),
        ));
        let bars: Vec<plot::Bar> = snap
            .context
            .iter()
            .take(8)
            .map(|row| plot::Bar {
                label: row.label.to_uppercase(),
                value: Some(row.miss_rate),
                colour: palette::MISS,
                note: plot::with_sample(&plot::percent(row.miss_rate), row.judged),
            })
            .collect();
        plot::spawn_bar_plot(
            parent,
            font,
            &bars,
            plot::Bounds { min: 0.0, max: 1.0 },
            BAR_W,
        );
    }
}

/// How erratic accuracy is across finished runs — lower is steadier.
/// Pure — tested. `None` below three accuracies.
#[must_use]
pub fn consistency_line(accuracies: &[f64]) -> String {
    if accuracies.len() < 3 {
        return "NOT ENOUGH RUNS FOR A CONSISTENCY READ".to_owned();
    }
    let mean = accuracies.iter().sum::<f64>() / accuracies.len() as f64;
    let var = accuracies
        .iter()
        .map(|a| {
            let d = a - mean;
            d * d
        })
        .sum::<f64>()
        / accuracies.len() as f64;
    let std = var.sqrt();
    let points = std * 100.0;
    if points < 2.0 {
        "VERY STEADY".to_owned()
    } else if points < 5.0 {
        format!("STEADY · ±{points:.1} POINTS")
    } else if points < 10.0 {
        format!("STREAKY · ±{points:.1} POINTS")
    } else {
        format!("WILD · ±{points:.1} POINTS")
    }
}

/// SONGS: personal bests + problem notes / heat on the busiest chart.
fn songs_view(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    runs: &[PlayerRun<'_>],
    snap: Option<&PlayerSnapshot>,
    waiting: &str,
) {
    // Bests from the filtered run set's entries.
    let entries: Vec<_> = runs.iter().map(|r| r.entry.clone()).collect();
    let player = runs.first().and_then(|r| r.entry.player);
    if let Some(id) = player {
        let bests = stats::bests(&entries, id);
        let mut ranked: Vec<_> = bests.into_iter().collect();
        ranked.sort_by(|a, b| {
            b.1.accuracy
                .partial_cmp(&a.1.accuracy)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        parent.spawn((
            Node {
                margin: UiRect::bottom(px(6.0)),
                ..default()
            },
            Text::new("PERSONAL BESTS".to_owned()),
            font.text(ui_kit::SMALL),
            TextColor(ui_kit::dimmed_subtitle()),
        ));
        if ranked.is_empty() {
            plot::empty_note(parent, font, "NO FINISHED SONGS IN THIS WINDOW");
        } else {
            let bars: Vec<plot::Bar> = ranked
                .iter()
                .take(8)
                .map(|((title, _artist, difficulty), best)| plot::Bar {
                    label: format!("{} ({})", title.to_uppercase(), difficulty.to_uppercase()),
                    value: Some(best.accuracy),
                    colour: palette::GREAT,
                    note: plot::percent(best.accuracy),
                })
                .collect();
            plot::spawn_bar_plot(
                parent,
                font,
                &bars,
                plot::Bounds { min: 0.0, max: 1.0 },
                BAR_W,
            );
        }
    } else {
        plot::empty_note(parent, font, "NO RUNS IN THIS WINDOW");
    }

    let Some(snap) = snap else {
        parent.spawn((
            Node {
                margin: UiRect::top(px(12.0)),
                ..default()
            },
            Text::new(waiting.to_owned()),
            font.text(ui_kit::SMALL),
            TextColor(ui_kit::dimmed_subtitle()),
        ));
        return;
    };
    if let Some((hash, difficulty, title)) = &snap.busiest {
        parent.spawn((
            Node {
                margin: UiRect::top(px(14.0)).with_bottom(px(6.0)),
                ..default()
            },
            Text::new(format!(
                "MOST PLAYED · {} · {}",
                title.to_uppercase(),
                difficulty_label(*difficulty)
            )),
            font.text(ui_kit::SMALL),
            TextColor(palette::BRAND),
        ));
        let _ = hash;
        if !snap.timeline.is_empty() {
            plot::spawn_heat_strip(parent, font, &snap.timeline, PLOT_W, HEAT_H);
        }
        if !snap.problems.is_empty() {
            parent.spawn((
                Node {
                    margin: UiRect::top(px(10.0)).with_bottom(px(4.0)),
                    ..default()
                },
                Text::new("WEAK NOTES".to_owned()),
                font.text(ui_kit::SMALL),
                TextColor(ui_kit::dimmed_subtitle()),
            ));
            let bars: Vec<plot::Bar> = snap
                .problems
                .iter()
                .take(6)
                .map(|note| plot::Bar {
                    label: format!("NOTE {}", note.note_index),
                    value: Some(note.hit_rate),
                    colour: palette::MISS,
                    note: format!(
                        "{} HIT · {} PLAYS",
                        plot::percent(note.hit_rate),
                        note.samples
                    ),
                })
                .collect();
            plot::spawn_bar_plot(
                parent,
                font,
                &bars,
                plot::Bounds { min: 0.0, max: 1.0 },
                BAR_W,
            );
        }
    }
}

/// Difficulty index as a short label.
fn difficulty_label(code: u8) -> &'static str {
    match code {
        0 => "EASY",
        1 => "MEDIUM",
        2 => "HARD",
        3 => "EXPERT",
        _ => "?",
    }
}

/// The findings block at the top of OVERVIEW: at most five
/// sample-gated sentences, and nothing at all when there is nothing
/// to say.
///
/// This was a tab of its own. It said very little very widely, and
/// what it said answers the overview's question — so it sits above
/// the working rather than two tabs away from it. Silence is the
/// right empty state here: an overview that opened with "NOT ENOUGH
/// EVIDENCE FOR A FINDING YET" over a full career of numbers would
/// be telling the player the opposite of what the screen shows.
fn findings(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    runs: &[PlayerRun<'_>],
    summary: &stats::Summary,
    snap: Option<&PlayerSnapshot>,
) {
    let lines = insight_lines(runs, summary, snap);
    if lines.is_empty() {
        return;
    }
    for line in lines {
        parent.spawn((
            Node {
                margin: UiRect::bottom(px(6.0)),
                ..default()
            },
            Text::new(line),
            font.text(ui_kit::ROW),
            TextColor(palette::TEXT),
        ));
    }
    // A rule between the words and the numbers they came from.
    parent.spawn((
        Node {
            width: Val::Percent(100.0),
            height: px(1.0),
            margin: UiRect::vertical(px(10.0)),
            ..default()
        },
        BackgroundColor(palette::dimmed(palette::TEXT_DIM, 0.25)),
    ));
}

/// Pure insight sentences — tested. Cap five.
#[must_use]
pub fn insight_lines(
    runs: &[PlayerRun<'_>],
    summary: &stats::Summary,
    snap: Option<&PlayerSnapshot>,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(bias) = snap.and_then(|s| s.bias_ms).or(summary.mean_offset_ms)
        && bias.abs() >= 8.0
        && runs.len() >= 3
    {
        // ⚠️ One wording. This said "YOU PLAY EARLY ON AVERAGE
        // (−13 MS)" while the tile three lines below said "13 MS
        // EARLY ON AVERAGE" — the same reading in two grammars and
        // two millisecond forms.
        out.push(format!("YOU PLAY {}", drift_line(Some(bias))));
    }
    if let Some(snap) = snap {
        let single = snap.technique.iter().find(|r| r.label == "singles");
        let chord = snap.technique.iter().find(|r| r.label == "chords");
        if let (Some(s), Some(c)) = (single, chord)
            && s.judged >= 20
            && c.judged >= 20
            && c.hit_rate + 0.08 < s.hit_rate
        {
            out.push(format!(
                "CHORDS BREAK YOU MORE THAN SINGLES ({} VS {})",
                plot::percent(c.hit_rate),
                plot::percent(s.hit_rate)
            ));
        }
        if let Some(held) = snap.technique.iter().find(|r| r.label == "sustains held")
            && held.judged >= 12
            && held.hit_rate < 0.7
        {
            out.push(format!(
                "SUSTAINS SLIP — ONLY {} HELD TO THE END",
                plot::percent(held.hit_rate)
            ));
        }
        if let Some(problem) = snap.problems.first()
            && problem.confidence() >= 0.45
        {
            let title = snap
                .busiest
                .as_ref()
                .map(|b| b.2.to_uppercase())
                .unwrap_or_else(|| "THIS CHART".to_owned());
            out.push(format!(
                "NOTE {} ON {title} IS A WEAK SPOT ({} HIT)",
                problem.note_index,
                plot::percent(problem.hit_rate)
            ));
        }
        if let Some(ctx) = snap
            .context
            .iter()
            .filter(|c| c.judged >= 30)
            .max_by(|a, b| {
                a.miss_rate
                    .partial_cmp(&b.miss_rate)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            && ctx.miss_rate >= 0.35
        {
            out.push(format!(
                "MISSES CLUSTER WHERE THE SONG IS “{}” ({} MISS)",
                ctx.label.to_uppercase(),
                plot::percent(ctx.miss_rate)
            ));
        }
    }
    // ⚠️ No trend line here. There was one, taken over the POOLED
    // accuracies, and the moment the findings moved onto OVERVIEW it
    // sat four lines above the overview's own trend — which is taken
    // per difficulty — saying 5.7 where that one said 10.6. Two
    // numbers for one quantity on one screen is the inconsistency
    // this whole pass exists to remove, and of the two the pooled one
    // is the misleading one: it shows a player getting worse the day
    // they move up a difficulty. The trend stays where the chart it
    // describes is.
    out.truncate(5);
    out
}

/// DIFFICULTY: where the player plays, and how far they get.
fn difficulty(parent: &mut ChildSpawnerCommands, font: &UiFont, runs: &[PlayerRun<'_>]) {
    let split = stats::by_difficulty(runs);
    parent.spawn((
        Node {
            margin: UiRect::bottom(px(8.0)),
            ..default()
        },
        Text::new("BEST ACCURACY".to_owned()),
        font.text(ui_kit::SMALL),
        TextColor(ui_kit::dimmed_subtitle()),
    ));
    let bars: Vec<plot::Bar> = split
        .iter()
        .map(|stat| plot::Bar {
            label: stat.difficulty.display_name().to_uppercase(),
            // No finished run means no accuracy — not a zero. A bar
            // of length zero and a bar that was never earned must not
            // look alike.
            value: stat.best_accuracy,
            colour: difficulty_colour(stat.difficulty),
            note: match stat.best_accuracy {
                Some(best) => format!(
                    "{} RUNS",
                    plot::with_sample(&plot::percent(best), stat.runs as u32)
                ),
                None if stat.runs > 0 => format!("{} RUNS, NONE FINISHED", stat.runs),
                None => "NO RUNS YET".to_owned(),
            },
        })
        .collect();
    plot::spawn_bar_plot(
        parent,
        font,
        &bars,
        plot::Bounds { min: 0.0, max: 1.0 },
        BAR_W,
    );

    parent.spawn((
        Node {
            margin: UiRect::top(px(16.0)).with_bottom(px(8.0)),
            ..default()
        },
        Text::new("RUNS FINISHED".to_owned()),
        font.text(ui_kit::SMALL),
        TextColor(ui_kit::dimmed_subtitle()),
    ));
    let finished: Vec<plot::Bar> = split
        .iter()
        .map(|stat| plot::Bar {
            label: stat.difficulty.display_name().to_uppercase(),
            value: stat.completion(),
            colour: difficulty_colour(stat.difficulty),
            note: stat.completion().map_or_else(
                || "NO RUNS YET".to_owned(),
                |share| {
                    format!(
                        "{} OF {}",
                        plot::with_sample(&plot::percent(share), stat.completed as u32),
                        stat.runs
                    )
                },
            ),
        })
        .collect();
    plot::spawn_bar_plot(
        parent,
        font,
        &finished,
        plot::Bounds { min: 0.0, max: 1.0 },
        BAR_W,
    );
}

/// VERSUS: the same song at the same difficulty, or nothing.
fn versus(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    entries: &[beatbyte_core::history::PlayEntry],
    players: &Players,
    id: PlayerId,
    name: &str,
) {
    let others: Vec<&beatbyte_core::player::Player> =
        players.0.players.iter().filter(|p| p.id != id).collect();
    if others.is_empty() {
        plot::empty_note(
            parent,
            font,
            "NO OTHER PLAYER ON THIS MACHINE — ADD ONE TO COMPARE",
        );
        return;
    }
    for other in others {
        let duels = stats::head_to_head(entries, id, other.id);
        parent.spawn((
            Node {
                margin: UiRect::top(px(10.0)).with_bottom(px(6.0)),
                ..default()
            },
            Text::new(format!(
                "{} VS {}",
                name.to_uppercase(),
                other.name.to_uppercase()
            )),
            font.text(ui_kit::ROW),
            TextColor(palette::TEXT),
        ));
        if duels.is_empty() {
            // The honest empty state, and the reason it is empty:
            // this game will not rank two players who have never
            // played the same thing.
            plot::empty_note(
                parent,
                font,
                "NO SONG BOTH HAVE FINISHED AT THE SAME DIFFICULTY YET",
            );
            continue;
        }
        let ahead = duels.iter().filter(|duel| duel.margin() > 0.0).count();
        parent.spawn((
            Text::new(format!("AHEAD ON {ahead} OF {} SHARED SONGS", duels.len())),
            font.text(ui_kit::SMALL),
            TextColor(palette::BRAND),
        ));
        let rows: Vec<plot::Duel> = duels
            .iter()
            .take(8)
            .map(|duel| {
                // Accuracy is the duel; score margin is the extra
                // evidence when the two are close on accuracy.
                let score_note = if duel.their_score != duel.other_score {
                    format!(
                        " · SCORE {} / {}",
                        plot::plain(duel.their_score as f64),
                        plot::plain(duel.other_score as f64)
                    )
                } else {
                    String::new()
                };
                plot::Duel {
                    label: format!(
                        "{} ({})",
                        duel.title.to_uppercase(),
                        duel.difficulty.to_uppercase()
                    ),
                    margin: duel.margin(),
                    note: format!(
                        "{} / {}{score_note}",
                        plot::percent(duel.theirs),
                        plot::percent(duel.others)
                    ),
                }
            })
            .collect();
        plot::spawn_duel_plot(parent, font, &rows, BAR_W, palette::PERFECT, palette::MISS);
    }
}

/// The three chip rows, bundled: `stats_nav` sits at Bevy's
/// sixteen-parameter cap, and these always travel together.
#[derive(bevy::ecs::system::SystemParam)]
struct ChipPresses<'w, 's> {
    tabs: Query<'w, 's, (&'static ViewTab, &'static Interaction), Changed<Interaction>>,
    difficulty:
        Query<'w, 's, (&'static DifficultyChip, &'static Interaction), Changed<Interaction>>,
    window: Query<'w, 's, (&'static WindowChip, &'static Interaction), Changed<Interaction>>,
}

/// One frame's worth of filter stepping, from either device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FilterSteps {
    /// −1, 0 or +1 along the difficulty ring.
    difficulty: i32,
    /// −1, 0 or +1 along the time windows.
    window: i32,
}

impl FilterSteps {
    /// ⚠️ The keyboard half reads the **typed character**, never the
    /// `KeyCode`. A `KeyCode` is a physical US position: this screen
    /// used `BracketLeft`/`BracketRight` for the window, which on a
    /// German keyboard is `ü` and `+` — the control was simply
    /// unreachable, the same way search once was (`song_select`
    /// learned this first).
    ///
    /// The pad half exists because there was none: a controller
    /// player saw two rows of chips and could reach neither. West
    /// and North are free in the menu table (South confirms, East
    /// goes back), and a single-direction cycle is enough for a ring
    /// of four or five that is drawn on screen.
    fn read<'a>(
        typed: &mut MessageReader<bevy::input::keyboard::KeyboardInput>,
        pads: impl Iterator<Item = &'a bevy::input::gamepad::Gamepad>,
    ) -> FilterSteps {
        let mut steps = FilterSteps::default();
        for event in typed.read() {
            if !event.state.is_pressed() {
                continue;
            }
            let bevy::input::keyboard::Key::Character(text) = &event.logical_key else {
                continue;
            };
            match text.as_str() {
                "," => steps.difficulty -= 1,
                "." => steps.difficulty += 1,
                "-" => steps.window -= 1,
                "+" => steps.window += 1,
                _ => {}
            }
        }
        for pad in pads {
            if pad.just_pressed(bevy::input::gamepad::GamepadButton::West) {
                steps.difficulty += 1;
            }
            if pad.just_pressed(bevy::input::gamepad::GamepadButton::North) {
                steps.window += 1;
            }
        }
        steps
    }
}

/// Tabs, filters, and leaving.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system params
fn stats_nav(
    map: Res<InputMap>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&bevy::input::gamepad::Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    chips: ChipPresses,
    mut back: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        With<ui_kit::BackButton>,
    >,
    mut view: ResMut<StatsView>,
    mut filters: ResMut<StatsFilters>,
    mut probe: ResMut<TelemetryProbe>,
    viewer: Viewer,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
    screen: Query<Entity, With<StatsScreen>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    let mut typed = typed;
    let nav = MenuNav::read(&map, &keys, pads.iter());
    let mut rebuild = false;
    let steps = FilterSteps::read(&mut typed, pads.iter());

    let delta = i32::from(nav.right) - i32::from(nav.left);
    if delta != 0 {
        let next_view = view.0.step(delta);
        if next_view != view.0 {
            view.0 = next_view;
            rebuild = true;
        }
    }
    for (tab, interaction) in &chips.tabs {
        if *interaction == Interaction::Pressed && tab.0 != view.0 {
            view.0 = tab.0;
            rebuild = true;
        }
    }

    if steps.window != 0 {
        filters.window = filters.window.step(steps.window);
        rebuild = true;
    }
    for (chip, interaction) in &chips.window {
        if *interaction == Interaction::Pressed && chip.0 != filters.window {
            filters.window = chip.0;
            rebuild = true;
        }
    }

    if steps.difficulty != 0 {
        filters.difficulty = step_difficulty(filters.difficulty, steps.difficulty);
        rebuild = true;
    }
    for (chip, interaction) in &chips.difficulty {
        if *interaction == Interaction::Pressed && chip.0 != filters.difficulty {
            filters.difficulty = chip.0;
            rebuild = true;
        }
    }

    if rebuild {
        // Filter changes may need a fresher telemetry probe when a
        // shell tab is showing; always safe to re-kick (supersedes).
        if view.0.needs_telemetry() {
            probe.request(
                store_path(),
                telemetry_scope(viewer.id(), *filters, wall_now_ms()),
            );
        }
        for entity in &screen {
            commands.entity(entity).despawn();
        }
        commands.run_system_cached(spawn_stats);
        sounds.write(crate::sfx::UiSound::Navigate);
    }
    if ui_kit::wants_leave(
        nav.back,
        ui_kit::back_pressed(&mut back),
        mouse.just_pressed(MouseButton::Right),
    ) {
        sounds.write(crate::sfx::UiSound::Back);
        next.set(AppState::Players);
    }
}

/// Cycle ALL → Easy → … → Expert. Pure — tested.
#[must_use]
pub fn step_difficulty(current: Option<Difficulty>, delta: i32) -> Option<Difficulty> {
    const ORDER: [Option<Difficulty>; 5] = [
        None,
        Some(Difficulty::Easy),
        Some(Difficulty::Medium),
        Some(Difficulty::Hard),
        Some(Difficulty::Expert),
    ];
    let at = ORDER.iter().position(|d| *d == current).unwrap_or(0);
    ORDER[ui_kit::step_cursor(at, ORDER.len(), delta)]
}

fn despawn_stats(mut commands: Commands, entities: Query<Entity, With<StatsScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_core::player::Roster;

    #[test]
    fn the_views_step_and_stop_at_the_ends_like_every_other_list() {
        assert_eq!(View::Overview.step(1), View::Timing);
        assert_eq!(View::Timing.step(-1), View::Overview);
        // Clamping, not wrapping: `ui_kit::step_cursor` is the one
        // cursor rule in this game, and tabs are not an exception.
        assert_eq!(View::Versus.step(1), View::Versus, "the last wrapped");
        assert_eq!(View::Songs.step(1), View::Versus);
        assert_eq!(View::Overview.step(-1), View::Overview);
        assert_eq!(View::Overview.step(0), View::Overview);
        assert_eq!(View::ALL.len(), 6);
    }

    #[test]
    fn a_view_can_be_named_for_the_harness() {
        assert_eq!(view_named("timing"), Some(View::Timing));
        assert_eq!(view_named("VERSUS"), Some(View::Versus));
        assert_eq!(view_named("technique"), Some(View::Technique));
        assert_eq!(view_named("nonsense"), None);
        // The two that were merged into OVERVIEW are gone from the
        // harness as well as from the screen, or a shot would name a
        // tab that does not exist and quietly get the default.
        assert_eq!(view_named("progress"), None);
        assert_eq!(view_named("insights"), None);
    }

    #[test]
    fn difficulty_and_window_filters_step_and_match_runs() {
        assert_eq!(step_difficulty(None, 1), Some(Difficulty::Easy));
        assert_eq!(
            step_difficulty(Some(Difficulty::Expert), 1),
            Some(Difficulty::Expert)
        );
        assert_eq!(step_difficulty(Some(Difficulty::Easy), -1), None);
        assert_eq!(TimeWindow::All.step(-1), TimeWindow::Days90);
        assert_eq!(TimeWindow::Days7.step(-1), TimeWindow::Days7);

        let filters = StatsFilters {
            difficulty: Some(Difficulty::Hard),
            window: TimeWindow::Days7,
        };
        let now = 1_000_000_000u64;
        assert!(run_passes_filters("hard", now - 1, filters, now));
        assert!(!run_passes_filters("easy", now - 1, filters, now));
        assert!(!run_passes_filters(
            "hard",
            now - 8 * 86_400_000,
            filters,
            now
        ));
        assert!(run_passes_filters(
            "hard",
            now - 1,
            StatsFilters {
                difficulty: None,
                window: TimeWindow::All
            },
            now
        ));
    }

    #[test]
    fn the_readonly_probe_reports_a_missing_store() {
        let path = std::env::temp_dir().join(format!(
            "beatbyte-stats-probe-missing-{}-{}.db",
            std::process::id(),
            wall_now_ms()
        ));
        let _ = std::fs::remove_file(&path);
        assert!(probe_session_count(&path).is_err());
        // ⚠️ And it says so in words a player can act on. This arm
        // used to throw its reason away and print "NO TELEMETRY
        // STORE" for everything — a locked or damaged database made
        // the same claim as an empty machine.
        let reason = load_player_snapshot(&path, Scope::default()).expect_err("no store");
        assert!(
            reason.contains("PLAY A SONG"),
            "a first run reads as a failure: {reason}"
        );
        assert_eq!(reason, reason.to_uppercase(), "the screen speaks in caps");
    }

    #[test]
    fn every_view_states_the_question_it_answers() {
        // A tab labelled with a noun leaves the player to infer what
        // the plot is for; the subtitle is the answer.
        for view in View::ALL {
            assert!(view.question().ends_with('?'), "{}", view.label());
            assert!(!view.label().is_empty());
        }
        assert!(View::Technique.needs_telemetry());
        assert!(View::Timing.needs_telemetry());
        // OVERVIEW took the findings in, and most of them are read
        // from the snapshot.
        assert!(View::Overview.needs_telemetry());
        assert!(!View::Difficulty.needs_telemetry());
        assert!(!View::Versus.needs_telemetry());
    }

    #[test]
    fn consistency_needs_three_runs_and_names_the_spread() {
        assert_eq!(
            consistency_line(&[0.9]),
            "NOT ENOUGH RUNS FOR A CONSISTENCY READ"
        );
        assert_eq!(consistency_line(&[0.9, 0.91, 0.89]), "VERY STEADY");
        let streaky = consistency_line(&[0.95, 0.70, 0.92, 0.68, 0.90]);
        assert!(
            streaky.contains("STREAKY") || streaky.contains("WILD"),
            "{streaky}"
        );
    }

    #[test]
    fn insights_stay_quiet_without_evidence() {
        let summary = stats::Summary::default();
        assert!(insight_lines(&[], &summary, None).is_empty());
    }

    /// ⚠️ The findings sit above OVERVIEW's own trend now, and that
    /// one is taken per difficulty. A pooled trend among the findings
    /// put 5.7 four lines above 10.6 — one quantity, two numbers, one
    /// screen. It is not a formatting slip: pooling shows a player
    /// getting worse the day they move up a difficulty.
    #[test]
    fn a_finding_never_states_a_trend_the_chart_below_states_better() {
        let body = include_str!("stats_ui.rs")
            .split("pub fn insight_lines")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .expect("insight_lines has a body");
        let code: String = body
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        assert!(
            !code.contains("trend_line("),
            "a finding is stating a trend again: the overview draws its own, per difficulty"
        );
    }

    #[test]
    fn a_trend_is_stated_per_ten_runs_or_not_at_all() {
        assert_eq!(trend_line(None), "NOT ENOUGH RUNS FOR A TREND");
        // 0.004 per run = 4 points per 10 runs.
        assert_eq!(trend_line(Some(0.004)), "UP 4.0 POINTS PER 10 RUNS");
        assert_eq!(trend_line(Some(-0.004)), "DOWN 4.0 POINTS PER 10 RUNS");
        // Noise is not a direction.
        assert_eq!(trend_line(Some(0.0001)), "HOLDING STEADY");
    }

    #[test]
    fn drift_reads_as_a_side_and_matches_the_results_screen() {
        // The same 3 ms window the results screen calls "on time",
        // so one run and a hundred are described the same way.
        assert_eq!(drift_line(None), "NO TIMING RECORDED YET");
        assert_eq!(drift_line(Some(1.5)), "ON TIME");
        assert_eq!(drift_line(Some(-2.9)), "ON TIME");
        assert_eq!(drift_line(Some(-18.0)), "18 MS EARLY ON AVERAGE");
        assert_eq!(drift_line(Some(24.0)), "24 MS LATE ON AVERAGE");
    }

    /// An app that can run `spawn_stats` for real: the resources it
    /// reads, a font handle, and nothing else.
    fn wired(history: Vec<beatbyte_core::history::PlayEntry>, roster: Roster) -> App {
        let mut app = App::new();
        app.add_plugins((bevy::MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Font>()
            .insert_resource(crate::players::Players(roster))
            .insert_resource(PlayHistory(history))
            .init_resource::<StatsFor>()
            .init_resource::<StatsView>()
            .init_resource::<StatsFilters>()
            .init_resource::<TelemetryProbe>();
        let font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .reserve_handle();
        app.insert_resource(UiFont::from_handle(font));
        app
    }

    fn a_run(
        player: Option<PlayerId>,
        accuracy: f64,
        completed: bool,
    ) -> beatbyte_core::history::PlayEntry {
        beatbyte_core::history::PlayEntry {
            title: "Africa".to_owned(),
            artist: "Toto".to_owned(),
            difficulty: "medium".to_owned(),
            started_ms: 1_756_000_000_000,
            played_s: 120.0,
            track_s: Some(120.0),
            completed,
            players: 1,
            practice: false,
            autopilot: false,
            score: 4844,
            accuracy,
            source: "file".to_owned(),
            player,
            detail: beatbyte_core::history::RunDetail {
                best_streak: Some(40),
                perfect: Some(80),
                great: Some(10),
                good: Some(5),
                miss: Some(5),
                overstrums: Some(2),
                mean_offset_ms: Some(-12.0),
                ..beatbyte_core::history::RunDetail::default()
            },
            co_players: Vec::new(),
            chart_hash: None,
            genre: None,
            tap_mode: Some(false),
            no_fail: Some(false),
            speed_percent: Some(100),
        }
    }

    /// How many nodes a view builds for a given history.
    fn nodes_for(
        view: View,
        history: Vec<beatbyte_core::history::PlayEntry>,
        roster: &Roster,
    ) -> usize {
        let mut app = wired(history, roster.clone());
        app.world_mut().resource_mut::<StatsView>().0 = view;
        app.add_systems(Update, spawn_stats);
        app.update();
        let screens = app
            .world_mut()
            .query_filtered::<Entity, With<StatsScreen>>()
            .iter(app.world())
            .count();
        assert_eq!(screens, 1, "{} spawned {screens} screens", view.label());
        app.world_mut().query::<&Node>().iter(app.world()).count()
    }

    /// Every view builds for every shape of history, and the data
    /// actually reaches the drawing.
    ///
    /// The locked-screen lesson in reverse: a screenshot proves a
    /// screen renders, but only when somebody can see it. This proves
    /// the four views BUILD — which is where the panics live (an
    /// empty series, a difficulty nobody finished, a player with no
    /// runs at all) — and that a view with runs in it draws
    /// materially MORE than the same view with none. A bare "some
    /// nodes exist" pin passed even with every plot short-circuited
    /// to its empty note, because the tiles and header alone clear
    /// it; the comparison is what bites. Shell tabs (Technique /
    /// Progress / Songs / Insights) only need to build.
    #[test]
    fn every_view_builds_and_a_history_draws_more_than_an_empty_one() {
        let mut roster = Roster::default();
        let id = roster
            .add("Martin", 1)
            .expect("a fresh roster takes a name");
        let full = vec![
            a_run(Some(id), 0.85, true),
            a_run(Some(id), 0.62, true),
            a_run(Some(id), 0.20, false),
        ];
        // VERSUS is excluded here on purpose and pinned separately:
        // with one player in the roster it correctly says "only one
        // player" whatever the history holds, so comparing it here
        // would pin the wrong thing.
        for view in [View::Overview, View::Timing, View::Difficulty] {
            let empty = nodes_for(view, Vec::new(), &roster);
            let drawn = nodes_for(view, full.clone(), &roster);
            assert!(
                drawn > empty,
                "{}: {drawn} nodes with three runs, {empty} with none - \
                 the history is not reaching the drawing",
                view.label()
            );
        }
        // The shapes that have no plot to draw must still build: a
        // player who finished nothing, and one who played nothing.
        for (label, history) in [
            ("runs but none finished", vec![a_run(Some(id), 0.0, false)]),
            ("one finished run", vec![a_run(Some(id), 0.85, true)]),
        ] {
            for view in View::ALL {
                let nodes = nodes_for(view, history.clone(), &roster);
                assert!(nodes > 0, "{} drew nothing for {label}", view.label());
            }
        }
    }

    /// VERSUS draws a duel when there is one to draw.
    ///
    /// Its whole contract is the one the design argued for: compare
    /// like with like, or say nothing. Both halves are pinned here —
    /// a shared song draws more than no shared song, and neither
    /// panics.
    #[test]
    fn versus_draws_a_duel_only_when_two_players_have_met_on_a_song() {
        let mut roster = Roster::default();
        let mine = roster
            .add("Martin", 1)
            .expect("a fresh roster takes a name");
        let theirs = roster.add("Kim", 2).expect("a second name is free");

        let mut apart = a_run(Some(theirs), 0.5, true);
        apart.title = "A different song".to_owned();
        let unshared = vec![a_run(Some(mine), 0.85, true), apart];
        let shared = vec![
            a_run(Some(mine), 0.85, true),
            a_run(Some(theirs), 0.5, true),
        ];

        let without = nodes_for(View::Versus, unshared, &roster);
        let with_duel = nodes_for(View::Versus, shared, &roster);
        assert!(
            with_duel > without,
            "a shared song drew {with_duel} nodes and no shared song {without} -              the duel is not reaching the drawing"
        );
    }

    /// A player the roster does not know must not panic the screen.
    #[test]
    fn a_screen_without_a_player_says_so_instead_of_drawing_nothing() {
        let mut app = wired(Vec::new(), Roster::default());
        app.add_systems(Update, spawn_stats);
        app.update();
        let spawned = app
            .world_mut()
            .query_filtered::<Entity, With<StatsScreen>>()
            .iter(app.world())
            .count();
        assert_eq!(spawned, 1, "an empty roster drew no screen at all");
    }

    /// One session row, varied only in what the filters look at.
    fn scoped_session(
        uid: &str,
        player: Option<u64>,
        difficulty: Difficulty,
        started_ms: u64,
        honest: bool,
    ) -> beatbyte_telemetry::model::SessionRow {
        use beatbyte_telemetry::model::{Detail, InputDevice, Provenance, SessionRow};
        SessionRow {
            uid: uid.to_owned(),
            started_ms,
            title: "Maria".to_owned(),
            artist: "Blondie".to_owned(),
            genre: None,
            chart_hash: "chart-a".to_owned(),
            song_id: None,
            chart_file: None,
            difficulty: crate::telemetry::difficulty_index(difficulty),
            player_slot: 0,
            player_id: player,
            provenance: Provenance {
                game: "0.0.0".to_owned(),
                chart_format: 1,
                generator: None,
                scoring: 1,
                analysis: None,
                vocal: None,
            },
            telemetry_schema: beatbyte_telemetry::schema_version(),
            detail: Detail::Actions,
            input_device: InputDevice::Keyboard,
            input_offset_ms: Some(0.0),
            video_offset_ms: Some(0.0),
            mic_offset_ms: None,
            tap_mode: false,
            no_fail: false,
            practice: !honest,
            autopilot: false,
            notes_total: 1,
        }
    }

    /// ⚠️ The pin S1 exists for: the statistics screen asks one
    /// question of two stores, and the two must agree. The history
    /// side filters runs with [`run_passes_filters`]; the telemetry
    /// side filters rows with the SQL that [`telemetry_scope`]
    /// builds. Before this, the SQL side had no filters at all —
    /// the chips were drawn over every tab and obeyed on half of
    /// them.
    ///
    /// The two encodings differ on purpose (text id in the play log,
    /// integer code in the store), which is exactly why agreeing is
    /// worth pinning rather than assuming.
    #[test]
    fn the_filters_mean_the_same_thing_to_the_history_and_to_the_store() {
        const DAY: u64 = 86_400_000;
        let now = 1_700_000_000_000u64;
        let me = 1u64;
        let them = 2u64;

        // (uid, player, difficulty, age in days, honest)
        let runs: Vec<(&str, Option<u64>, Difficulty, u64, bool)> = vec![
            ("a", Some(me), Difficulty::Easy, 1, true),
            ("b", Some(me), Difficulty::Expert, 3, true),
            ("c", Some(me), Difficulty::Expert, 20, true),
            ("d", Some(me), Difficulty::Hard, 45, true),
            ("e", Some(me), Difficulty::Easy, 200, true),
            ("f", Some(them), Difficulty::Easy, 1, true),
            ("g", Some(them), Difficulty::Expert, 3, true),
            // Nobody's run, and one the screen must never count.
            ("h", None, Difficulty::Easy, 1, true),
            ("i", Some(me), Difficulty::Easy, 1, false),
        ];

        let mut store = beatbyte_telemetry::Store::open_in_memory().expect("a store");
        for (uid, player, difficulty, days, honest) in &runs {
            let started = now - days * DAY;
            store
                .begin(&scoped_session(uid, *player, *difficulty, started, *honest))
                .expect("a session");
        }

        let mut checked = 0usize;
        for difficulty in [
            None,
            Some(Difficulty::Easy),
            Some(Difficulty::Hard),
            Some(Difficulty::Expert),
        ] {
            for window in TimeWindow::ALL {
                let filters = StatsFilters { difficulty, window };
                // The history side: this player's runs, then the
                // same pure rule the Overview tab uses.
                let by_history = runs
                    .iter()
                    .filter(|(_, player, _, _, honest)| *player == Some(me) && *honest)
                    .filter(|(_, _, difficulty, days, _)| {
                        run_passes_filters(difficulty.id(), now - days * DAY, filters, now)
                    })
                    .count() as u64;
                // The store side: the same question in SQL.
                let by_store =
                    analytics::player_snapshot(&store, telemetry_scope(Some(me), filters, now))
                        .expect("a snapshot")
                        .sessions;
                assert_eq!(
                    by_history, by_store,
                    "difficulty {difficulty:?} window {window:?}: \
                     the history counted {by_history} and the store {by_store}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 16, "the filter matrix shrank");

        // And the scope is what excludes them: unfiltered, this
        // player has four honest runs, not the store's eight.
        let all = StatsFilters::default();
        assert_eq!(
            analytics::player_snapshot(&store, telemetry_scope(Some(me), all, now))
                .expect("a snapshot")
                .sessions,
            5,
            "one player's honest runs"
        );
        assert_eq!(
            analytics::player_snapshot(&store, Scope::default())
                .expect("a snapshot")
                .sessions,
            8,
            "every honest run, whoever played it"
        );
    }

    /// ⚠️ Measured on the real store while building S1: 138 of 144
    /// honest sessions name no player, because they were recorded
    /// before runs were attributed and the one-time adoption that
    /// claimed the PLAY HISTORY for its player never ran over the
    /// telemetry store. Filtering by player is right; dropping 96 %
    /// of the evidence without a word is not.
    #[test]
    fn a_reading_says_how_many_runs_name_nobody() {
        const DAY: u64 = 86_400_000;
        let now = 1_700_000_000_000u64;
        let me = 1u64;

        let mut store = beatbyte_telemetry::Store::open_in_memory().expect("a store");
        for (uid, player, difficulty, days, honest) in [
            ("mine", Some(me), Difficulty::Easy, 1, true),
            ("nobody-recent", None, Difficulty::Easy, 1, true),
            ("nobody-old", None, Difficulty::Easy, 200, true),
            ("nobody-hard", None, Difficulty::Hard, 1, true),
            ("nobody-practice", None, Difficulty::Easy, 1, false),
        ] {
            store
                .begin(&scoped_session(
                    uid,
                    player,
                    difficulty,
                    now - days * DAY,
                    honest,
                ))
                .expect("a session");
        }

        let ask = |filters: StatsFilters, who: Option<u64>| {
            analytics::player_snapshot(&store, telemetry_scope(who, filters, now))
                .expect("a snapshot")
        };

        let all = StatsFilters::default();
        assert_eq!(
            ask(all, Some(me)).unattributed,
            3,
            "the three honest orphans"
        );
        assert_eq!(
            ask(all, None).unattributed,
            0,
            "without a player filter they are already counted, so nothing is left out"
        );
        // The count obeys the rest of the scope, or the line would
        // claim runs the window had already excluded anyway.
        assert_eq!(
            ask(
                StatsFilters {
                    window: TimeWindow::Days7,
                    ..all
                },
                Some(me)
            )
            .unattributed,
            2,
            "the 200-day-old orphan is outside the window"
        );
        assert_eq!(
            ask(
                StatsFilters {
                    difficulty: Some(Difficulty::Hard),
                    ..all
                },
                Some(me)
            )
            .unattributed,
            1,
            "only the HARD orphan"
        );
    }

    #[test]
    fn the_unattributed_line_counts_in_the_right_plural_or_stays_quiet() {
        assert_eq!(unattributed_note(0), None, "nothing left out, nothing said");
        assert_eq!(
            unattributed_note(1).as_deref(),
            Some("1 RECORDED RUN NAMES NO PLAYER — NOT COUNTED HERE")
        );
        assert!(
            unattributed_note(138).is_some_and(|line| line.starts_with("138 RECORDED RUNS NAME")),
        );
    }

    #[test]
    fn a_played_time_is_minutes_until_it_is_hours() {
        assert_eq!(played_time(0.0), "0 MIN");
        assert_eq!(played_time(600.0), "10 MIN");
        assert_eq!(played_time(7200.0), "2.0 H");
    }
}
