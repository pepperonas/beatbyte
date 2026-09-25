//! Autopilot: the game plays itself, perfectly.
//!
//! Enabled with `BEATBYTE_AUTOPILOT=1`. The autopilot drives the real
//! screens (menu → gameplay → results) and feeds the real judgment
//! engine frame by frame — an end-to-end validation of the whole loop
//! that no unit test can substitute for, and the only way an
//! unattended machine can "play" the game. At the results screen it
//! logs the outcome and exits with success/failure.
//!
//! This is deliberately *not* compiled out in release: it doubles as a
//! soak-test harness and a demo attract mode later.

use beatbyte_core::session::NoteState;
use beatbyte_core::{GameInput, InputKind, LaneSet};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::audio_sys::GameClock;
use crate::gameplay::{LastResults, PlayerSession};
use crate::states::AppState;

/// Set the moment any autopilot verdict (pass OR fail) is written.
/// `run()` refuses a clean exit without one: every silent way the
/// event loop can die (window closed, loop torn down, invisible-
/// window quirks) has at some point produced a fake exit-0 "pass".
pub static VERDICT_DELIVERED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn deliver(app_exit: &mut MessageWriter<AppExit>, exit: AppExit) {
    VERDICT_DELIVERED.store(true, std::sync::atomic::Ordering::Relaxed);
    app_exit.write(exit);
}

/// Whether autopilot is enabled (checked once at startup).
#[derive(Resource, Clone, Copy)]
pub struct Autopilot {
    /// Enabled?
    pub enabled: bool,
}

/// `BEATBYTE_AUTOPILOT_FAIL=1`: the fail drill. The autopilot plays
/// NOTHING, No Fail is switched off for the run in memory, and the
/// verdict inverts — the run passes only if the rock meter emptied,
/// the session failed, and the results say so. The one automated
/// path through the failure flow.
#[derive(Resource)]
struct FailDrill;

/// Whether the note injector owns this run's inputs, so real devices
/// are muted while it plays.
///
/// The injector plays every note itself, stamped — a key, a pad
/// button or a click from the desk is not part of the run and can
/// only add strums the chart never asked for. Measured in this
/// project's recorded telemetry: seven of 305 autopilot sessions
/// carried overstrums, and in every one of them EVERY note was still
/// hit perfectly at 0.0 ms. More strums than notes is something the
/// injector cannot produce — it strums at most once per pending
/// event — so those came from the room, and one of them failed a
/// playtest that passed untouched minutes later.
///
/// Key-play mode is the exception: there the autopilot IS the
/// keyboard, and muting the key path would mute the drill.
#[must_use]
pub fn injector_owns_input(enabled: bool, key_play: bool) -> bool {
    enabled && !key_play
}

/// Marker for [`injector_owns_input`]: while it exists, the gameplay
/// input system (`gameplay::input::gameplay_input`) ignores real
/// devices. Named, not linked: the system is `pub(super)`, so a link
/// to it fails the `-D warnings` rustdoc build CI runs.
#[derive(Resource)]
pub struct InjectorOwnsInput;

/// Install [`InjectorOwnsInput`] when the run calls for it.
///
/// The wiring lives in its own function so a test can watch the
/// decision reach the world: a predicate that is right while nothing
/// installs its marker would leave both halves green and the run
/// still open to the room.
fn own_inputs(app: &mut App, enabled: bool, key_play: bool) {
    if injector_owns_input(enabled, key_play) {
        app.insert_resource(InjectorOwnsInput);
    }
}

/// Frets the autopilot currently holds, per player.
#[derive(Resource, Default)]
struct AutopilotHands {
    held: Vec<LaneSet>,
    /// Index of the next event to play, per player.
    next_event: Vec<usize>,
    /// Time spent on the results screen.
    results_time: f32,
}

impl AutopilotHands {
    fn ensure(&mut self, players: usize) {
        self.held.resize(players, LaneSet::EMPTY);
        self.next_event.resize(players, 0);
    }
}

/// Plugin wiring the autopilot systems.
pub struct AutopilotPlugin;

impl Plugin for AutopilotPlugin {
    fn build(&self, app: &mut App) {
        let enabled = std::env::var_os("BEATBYTE_AUTOPILOT").is_some();
        let key_play = std::env::var_os("BEATBYTE_AUTOPILOT_KEYS").is_some();
        app.insert_resource(Autopilot { enabled })
            .init_resource::<AutopilotHands>();
        own_inputs(app, enabled, key_play);

        // Screen photography works without the autopilot: the screens
        // it cannot reach are exactly the ones that need it.
        if let Ok(raw) = std::env::var("BEATBYTE_SHOT_STATE") {
            match shot_state(&raw) {
                Some(target) => {
                    app.insert_resource(ShotState(target)).add_systems(
                        Update,
                        (enter_shot_state, reopen_shot_search, quit_after_shot),
                    );
                    if let Some(dir) = std::env::var_os("BEATBYTE_SHOT_DIR") {
                        let dir = std::path::PathBuf::from(dir);
                        if std::fs::create_dir_all(&dir).is_ok() {
                            app.insert_resource(ShotDir(dir))
                                .add_systems(Update, autopilot_screenshots);
                        }
                    }
                }
                None => error!("unknown BEATBYTE_SHOT_STATE `{raw}`"),
            }
        }
        if enabled && std::env::var_os("BEATBYTE_AUTOPILOT_FAIL").is_some() {
            app.insert_resource(FailDrill)
                .add_systems(Startup, arm_failure_for_drill);
        }
        if enabled {
            app.add_systems(
                Update,
                (
                    autopilot_menu.run_if(in_state(AppState::MainMenu)),
                    autopilot_song_select.run_if(in_state(AppState::SongSelect)),
                    autopilot_edit.run_if(in_state(AppState::Editor)),
                    // After the text field and before the editor's keys:
                    // injected key events then reach the field and the
                    // key state in the same frame, as real ones do (run
                    // before the field, the Backspaces that cleared it
                    // arrived a frame later as Delete).
                    autopilot_edit_mouse
                        .run_if(in_state(AppState::Editor))
                        .after(crate::editor_ui::editor_typing)
                        .before(crate::editor_ui::editor_input),
                    autopilot_results.run_if(in_state(AppState::Results)),
                    autopilot_drop.run_if(in_state(AppState::SongSelect)),
                    fail_if_window_vanishes,
                ),
            )
            .add_systems(OnEnter(AppState::Gameplay), autopilot_reset)
            .add_systems(Update, autopilot_reset_on_swap);
            if std::env::var_os("BEATBYTE_AUTOPILOT_LOOP").is_some() {
                // Section-loop validation: arm the loop, watch song
                // time actually wrap twice, and verify the section's
                // notes reopened for judgment.
                app.add_systems(
                    Update,
                    autopilot_loop_check.run_if(in_state(AppState::Gameplay)),
                );
            }
            if std::env::var_os("BEATBYTE_AUTOPILOT_SPEED").is_some() {
                // Practice-speed validation: measure the SLOPE of
                // song time against wall time mid-run. Because the
                // clock reconciles against the device, a music
                // stream running at the wrong pace would drag the
                // measured slope with it — this one number checks
                // audio pace and clock rate together.
                app.add_systems(
                    Update,
                    autopilot_speed_check.run_if(in_state(crate::states::GamePhase::Playing)),
                );
            }
            if std::env::var_os("BEATBYTE_AUTOPILOT_RATE").is_some() {
                // Results-feedback validation (A5): press a REAL
                // digit key on the results screen (and RIGHT when
                // the chart has a parent version), then read the
                // session log back and verify the lines landed.
                app.add_systems(
                    PreUpdate,
                    autopilot_rate
                        .after(bevy::input::InputSystems)
                        .run_if(in_state(AppState::Results)),
                )
                .add_systems(Update, autopilot_rate_return);
            }
            if std::env::var_os("BEATBYTE_AUTOPILOT_PAUSE").is_some() {
                // Pause-menu validation: mid-song, drive the pause
                // overlay with REAL keys — pause, step to the SFX
                // row, adjust down and back up, resume — and verify
                // the setting actually moved each time. The run then
                // continues to the normal flawless-finish verdict,
                // proving the pause round-trip cost nothing.
                app.add_systems(
                    PreUpdate,
                    autopilot_pause
                        .after(bevy::input::InputSystems)
                        .run_if(in_state(AppState::Gameplay)),
                );
            }
            if std::env::var_os("BEATBYTE_AUTOPILOT_DELETE").is_some() {
                // Deletion validation drives the browser with REAL
                // arrow/backspace keys; song selection stays passive.
                app.add_systems(
                    PreUpdate,
                    autopilot_delete
                        .after(bevy::input::InputSystems)
                        .run_if(in_state(AppState::SongSelect)),
                );
            }
            if std::env::var_os("BEATBYTE_AUTOPILOT_ALIGN").is_some() {
                // Alignment validation: real arrow keys to the song,
                // real `K`, then the status row and the file on disk
                // are the verdict. Song selection stays passive.
                app.add_systems(
                    PreUpdate,
                    autopilot_align
                        .after(bevy::input::InputSystems)
                        .run_if(in_state(AppState::SongSelect)),
                );
            }
            if std::env::var_os("BEATBYTE_AUTOPILOT_TASTE").is_some() {
                // Blind-test validation: real arrow keys to the song,
                // a real `T`, then the built test is the verdict —
                // and on the results screen a real arrow answers it
                // and the session log has to show the line.
                app.add_systems(
                    PreUpdate,
                    autopilot_taste
                        .after(bevy::input::InputSystems)
                        .run_if(in_state(AppState::SongSelect)),
                )
                .add_systems(
                    PreUpdate,
                    autopilot_taste_verdict
                        .after(bevy::input::InputSystems)
                        .run_if(in_state(AppState::Results)),
                );
            }
            if std::env::var_os("BEATBYTE_AUTOPILOT_MODEL").is_some() {
                // Model-download validation: into the settings screen,
                // real arrow keys to the LYRICS MODEL row, real Enter,
                // then the row's own state machine is the verdict.
                app.add_systems(PreUpdate, autopilot_model.after(bevy::input::InputSystems));
            }
            if key_play {
                // Keyboard-path validation: press REAL KeyCodes on
                // ButtonInput, so InputMap resolution, gameplay_input
                // routing and judgment run exactly as for a human.
                // Frame-quantized, so Greats are legitimate — misses
                // and overstrums are not.
                app.add_systems(
                    PreUpdate,
                    autopilot_key_play
                        .after(bevy::input::InputSystems)
                        .run_if(in_state(crate::states::GamePhase::Playing)),
                );
            } else {
                // Judgment is input-stamp-driven, so playing *before*
                // the session advances makes the autopilot exact and
                // frame-rate independent (hitches cannot cause
                // misses — the stamps carry the truth).
                app.add_systems(
                    Update,
                    autopilot_play
                        .before(crate::gameplay::advance_sessions)
                        .run_if(in_state(crate::states::GamePhase::Playing)),
                );
            }

            // Optional: capture screenshots at interesting moments
            // (`BEATBYTE_SHOT_DIR=<dir>`), for README/docs material.
            if let Some(dir) = std::env::var_os("BEATBYTE_SHOT_DIR") {
                let dir = std::path::PathBuf::from(dir);
                if let Err(error) = std::fs::create_dir_all(&dir) {
                    error!("cannot create screenshot dir {}: {error}", dir.display());
                } else {
                    app.insert_resource(ShotDir(dir))
                        .add_systems(Update, autopilot_screenshots);
                }
            }
        }
    }
}

/// Boot straight into one screen, photograph it and quit.
///
/// The autopilot walks menu → song select → gameplay → results, so
/// those screens photograph themselves. Settings, controls,
/// calibration and the input tester are reachable only by hand —
/// which made them the screens least likely to be checked after a
/// change, exactly backwards. `BEATBYTE_SHOT_STATE=settings` together
/// with `BEATBYTE_SHOT_DIR` opens one, shoots it and exits.
#[must_use]
pub fn shot_state(raw: &str) -> Option<AppState> {
    match raw
        .to_ascii_lowercase()
        .replace(['-', '_', ' '], "")
        .as_str()
    {
        "menu" | "mainmenu" => Some(AppState::MainMenu),
        "songselect" | "browser" => Some(AppState::SongSelect),
        "settings" => Some(AppState::Settings),
        "controls" => Some(AppState::Controls),
        "calibration" => Some(AppState::Calibration),
        "inputtest" => Some(AppState::InputTest),
        "join" | "multiplayer" | "multiplayersetup" => Some(AppState::MultiplayerSetup),
        "about" => Some(AppState::About),
        "players" | "roster" => Some(AppState::Players),
        "stats" | "statistics" => Some(AppState::Stats),
        "achievements" | "awards" => Some(AppState::Achievements),
        "songinfo" | "info" | "document" => Some(AppState::SongInfo),
        _ => None,
    }
}

/// The screen [`shot_state`] resolved from the environment.
#[derive(Resource)]
struct ShotState(AppState);

/// Enter the requested screen once the app is genuinely up.
///
/// Waits for the main menu rather than switching at startup: boot
/// inserts the song library and the built-in songs, and a screen that
/// needs them — the browser does — panics on a missing resource if it
/// is entered before boot has run.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn enter_shot_state(
    target: Res<ShotState>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut cursor: ResMut<crate::song_select::BrowserCursor>,
    mut view: ResMut<crate::song_select::BrowserView>,
    mut settings_cursor: ResMut<crate::settings_ui::SettingsCursor>,
    mut awards: ResMut<crate::achievements_ui::AchievementsView>,
    // ⚠️ Optional: a system's parameters are validated BEFORE its
    // body runs, so a plain `Res` here fails on every frame before
    // boot inserts the library — the early return below never gets
    // the chance to save it.
    library: Option<Res<crate::library::SongLibrary>>,
    info: Option<Res<crate::song_info::Showing>>,
    mut commands: Commands,
    mut done: Local<bool>,
) {
    if *done || *state.get() != AppState::MainMenu {
        return;
    }
    *done = true;
    // Photograph a list at a chosen row, not only at its first one.
    // A scrolling list looks identical to a short one until something
    // moves the selection past the fold, so without this the scroll
    // could only be argued about, not seen.
    if let Ok(raw) = std::env::var("BEATBYTE_SHOT_ROW")
        && let Ok(row) = raw.parse::<usize>()
    {
        cursor.0 = row;
        // The settings list scrolls too, and a row below its fold
        // was as unphotographable as a song below the browser's.
        settings_cursor.0 = row;
        // So does the achievements list, and it is the one where the
        // fold hides the thing most worth seeing: every hidden
        // achievement sits in the last two categories, so a covered
        // `? ? ?` row could not be photographed at all.
        awards.row = row;
    }
    // Photograph the browser under a chosen sort - the active-column
    // marker only exists when a sort is active, so without this it
    // could only be argued about, not seen.
    if let Ok(raw) = std::env::var("BEATBYTE_SHOT_SORT") {
        match crate::song_select::SortMode::from_label(&raw) {
            Some(mode) => view.sort = mode,
            None => error!("unknown BEATBYTE_SHOT_SORT `{raw}`"),
        }
    }
    // Photograph the browser mid-search - the search prompt, the
    // first-match selection and the "no match" hint only exist while
    // a filter is typed, so without this they could only be argued
    // about, not seen.
    if let Ok(raw) = std::env::var("BEATBYTE_SHOT_SEARCH") {
        view.searching = true;
        view.filter = raw.to_lowercase();
    }
    // The document screen is the one that cannot be entered cold:
    // the browser hands it what to show. For a photograph, hand it
    // the first song in the library that HAS a document — an empty
    // panel would be a picture of nothing.
    if target.0 == AppState::SongInfo && info.is_none() {
        match library
            .iter()
            .flat_map(|library| library.entries.iter())
            .filter_map(|entry| match &entry.source {
                crate::library::SongSource::File { chart_path, .. } => chart_path.parent(),
                crate::library::SongSource::Builtin(_) => None,
            })
            .find_map(crate::song_info::Showing::read)
        {
            Some(showing) => commands.insert_resource(showing),
            None => {
                error!("BEATBYTE_SHOT_STATE=songinfo: no song here has a document yet");
                return;
            }
        }
    }
    next.set(target.0);
}

/// `BEATBYTE_SHOT_SEARCH` wants the search OPEN in the picture, and
/// the browser closes the field whenever it is entered (a field that
/// swallows every letter read as a broken screen when a player came
/// back from a song). So the field is opened again on the first
/// frame the browser is on screen — the filter it typed survived
/// the entry, only the open state did not.
fn reopen_shot_search(
    target: Res<ShotState>,
    state: Res<State<AppState>>,
    mut view: ResMut<crate::song_select::BrowserView>,
    mut done: Local<bool>,
) {
    if *done || *state.get() != target.0 || target.0 != AppState::SongSelect {
        return;
    }
    if std::env::var_os("BEATBYTE_SHOT_SEARCH").is_some() {
        view.searching = true;
    }
    *done = true;
}

/// Leave once the screen has been on display long enough to be
/// photographed (the shot itself waits out the 0.25 s transition
/// fade, so this has to outlast that plus the save).
fn quit_after_shot(
    target: Res<ShotState>,
    state: Res<State<AppState>>,
    time: Res<Time>,
    mut elapsed: Local<f32>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if *state.get() != target.0 {
        return;
    }
    *elapsed += time.delta_secs();
    if *elapsed > 2.0 {
        app_exit.write(AppExit::Success);
    }
}

/// Where autopilot screenshots go.
#[derive(Resource)]
struct ShotDir(std::path::PathBuf);

/// The song times `BEATBYTE_SHOT_TIMES` asks for, parsed once. Pure
/// parsing — tested via [`parse_shot_times`].
fn shot_times() -> Vec<f64> {
    static TIMES: std::sync::OnceLock<Vec<f64>> = std::sync::OnceLock::new();
    TIMES
        .get_or_init(|| {
            std::env::var("BEATBYTE_SHOT_TIMES")
                .map(|raw| parse_shot_times(&raw))
                .unwrap_or_default()
        })
        .clone()
}

/// `17.5,39` → `[17.5, 39.0]`; junk entries are skipped, never fatal.
#[must_use]
pub fn parse_shot_times(raw: &str) -> Vec<f64> {
    raw.split(',')
        .filter_map(|part| part.trim().parse::<f64>().ok())
        .filter(|at| at.is_finite() && *at >= 0.0)
        .collect()
}

/// The file name a requested moment is saved under.
#[must_use]
pub fn shot_name(at: f64) -> String {
    format!("gameplay-t{at}")
}

/// Which requested moment this frame belongs to, if any.
///
/// A moment stays claimable for a second after its time so that a
/// frame arrives even on a machine that stutters. That window is far
/// wider than the moments themselves need to be: photographing a
/// 300 ms effect means asking for times a few hundredths apart, and
/// their windows all overlap. So a moment that has already been
/// photographed steps aside for the next one instead of swallowing
/// its window — which is the whole difference between one picture of
/// an impulse and a series of them.
#[must_use]
pub fn next_shot(times: &[f64], now: f64, already: impl Fn(f64) -> bool) -> Option<f64> {
    times
        .iter()
        .copied()
        .filter(|&at| (at..at + 1.0).contains(&now))
        .find(|&at| !already(at))
}

/// Take one screenshot per named moment of the run (state screens
/// wait out the transition fade first).
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn autopilot_screenshots(
    mut commands: Commands,
    dir: Res<ShotDir>,
    state: Res<State<AppState>>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    players: Query<&crate::gameplay::PlayerSession>,
    phase: Option<Res<State<crate::states::GamePhase>>>,
    mut taken: Local<std::collections::HashSet<&'static str>>,
    mut in_state_for: Local<(Option<AppState>, f32)>,
) {
    // Track how long the current state has been active.
    if in_state_for.0 != Some(*state.get()) {
        *in_state_for = (Some(*state.get()), 0.0);
    } else {
        in_state_for.1 += time.delta_secs();
    }
    if in_state_for.1 < 0.6 {
        return;
    }
    let moment = match state.get() {
        AppState::MainMenu => Some("menu"),
        AppState::SongSelect => Some("songselect"),
        AppState::MultiplayerSetup => Some("join"),
        AppState::Gameplay
            if phase
                .as_deref()
                .is_some_and(|p| *p.get() == crate::states::GamePhase::Outro) =>
        {
            Some("gameplay-yourock")
        }
        AppState::Gameplay
            if phase
                .as_deref()
                .is_some_and(|p| *p.get() == crate::states::GamePhase::Paused) =>
        {
            // The pause menu is UI too — the invisible-settings bug
            // shipped because nothing ever photographed it.
            Some("gameplay-paused")
        }
        AppState::Gameplay => {
            let now = game_clock.song_time(&time).unwrap_or(0.0);
            // An energy phrase is worth its own frame: the fixed
            // moments below fall between phrases on every song in the
            // library, so nothing automated ever pictured a marked
            // note or a phrase band.
            let in_phrase = players.iter().any(|player| {
                player
                    .session
                    .track()
                    .phrases()
                    .iter()
                    .any(|phrase| phrase.contains(now))
            });
            let hype = players
                .iter()
                .any(|player| player.session.performance().hype_active());
            // Extra moments on request (`BEATBYTE_SHOT_TIMES=17.5,39`):
            // a frame within a second after each listed song time,
            // named by it — for looking at a lyric lead-in, a
            // countdown, a line mid-fill, whatever a change touched.
            let requested = next_shot(&shot_times(), now, |at| {
                taken.contains(shot_name(at).as_str())
            });
            if let Some(at) = requested {
                // One leak per requested moment: the set of names is
                // `&'static str`, and a run asks for a handful.
                let name: &'static str = Box::leak(shot_name(at).into_boxed_str());
                Some(name)
            } else if hype {
                Some("gameplay-hype")
            } else if in_phrase {
                Some("gameplay-phrase")
            } else if (24.0..26.0).contains(&now) {
                Some("gameplay")
            } else if (44.0..46.0).contains(&now) {
                Some("gameplay-late")
            } else {
                None
            }
        }
        AppState::Results => Some("results"),
        AppState::Settings => Some("settings"),
        AppState::Controls => Some("controls"),
        AppState::Calibration => Some("calibration"),
        AppState::InputTest => Some("inputtest"),
        AppState::About => Some("about"),
        AppState::Players => Some("players"),
        AppState::Stats => Some("stats"),
        AppState::Achievements => Some("achievements"),
        AppState::SongInfo => Some("songinfo"),
        _ => None,
    };
    if let Some(name) = moment
        && !taken.contains(name)
    {
        taken.insert(name);
        let path = dir.0.join(format!("beatbyte-{name}.png"));
        info!("autopilot: capturing screenshot {}", path.display());
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
}

/// Head into the song browser shortly after the menu appears.
fn autopilot_menu(
    time: Res<Time>,
    mut delay: Local<f32>,
    music: Res<crate::audio_sys::Music>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    *delay += time.delta_secs();
    if *delay > 0.8 {
        *delay = 0.0;
        // Quieter than a played run; the mute gate rides on top, so
        // toggling `M` mid-run keeps working.
        music.0.set_volume(0.5);
        info!("autopilot: opening song select");
        next_state.set(AppState::SongSelect);
    }
}

/// The editor drill's bookkeeping: the real chart it must not change,
/// and the scratch folder it edits instead.
#[derive(Resource)]
pub struct EditDrill {
    /// The player's chart the drill copied.
    pub original: std::path::PathBuf,
    /// Its bytes before the drill.
    pub original_bytes: Vec<u8>,
    /// Where the copy lives.
    pub scratch: std::path::PathBuf,
}

/// In editor-validation mode (`BEATBYTE_AUTOPILOT_EDIT=1`), drive the
/// real editor: add a note, undo, redo, save, verify, exit.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn autopilot_edit(
    time: Res<Time>,
    mut delay: Local<f32>,
    mut edits_done: Local<bool>,
    mut keys_ok: Local<Option<bool>>,
    mut looped: Local<Option<LoopCheck>>,
    state: Option<ResMut<crate::editor_ui::EditorState>>,
    music: Res<crate::audio_sys::Music>,
    mut game_clock: ResMut<crate::audio_sys::GameClock>,
    clicks: Res<crate::editor_ui::AuditionClicks>,
    settings: Res<crate::config::Settings>,
    drill: Option<Res<EditDrill>>,
    mouse: Option<Res<MouseDrill>>,
    mut commands: Commands,
    mut app_exit: MessageWriter<AppExit>,
) {
    let Some(mut state) = state else {
        return;
    };
    *delay += time.delta_secs();
    if *delay < 1.0 {
        return;
    }
    // Phase 2: audition ran for ~4 s — the metronome overlay must
    // have ticked (E3). Then deliver the verdict.
    if *edits_done {
        if *delay < 5.0 {
            return;
        }
        // Phase 3: the loop at half speed — the playhead must wrap,
        // never run past the end, and move at half the wall's pace
        // (the clock AND the music slowed, together).
        if clicks.0 >= 3 && looped.is_none() {
            let start = state.cursor_s;
            state.previewing = true;
            crate::editor_ui::toggle_preview(&mut state, &music, &mut game_clock, &settings, 0.0);
            state.speed = 0.5;
            state.loop_region = beatbyte_editor::playback::LoopRegion::between(start, start + 0.8);
            state.looping = true;
            state.cursor_s = start;
            let now = time.elapsed_secs_f64();
            crate::editor_ui::toggle_preview(&mut state, &music, &mut game_clock, &settings, now);
            *looped = Some(LoopCheck {
                wall0: now,
                song0: start,
                ..LoopCheck::default()
            });
            return;
        }
        if let Some(check) = looped.as_mut()
            && !check.done
        {
            let wall = time.elapsed_secs_f64() - check.wall0;
            let song = state.cursor_s;
            // The MUSIC's own position, inside the first pass: it must
            // run at the same half speed as the clock, or the notes
            // drift off the audio (the clock alone cannot show that).
            if check.device.is_none() && wall >= 0.4 {
                check.device = Some((wall, music.0.position_s(), None));
            }
            if let Some((w0, p0, None)) = check.device
                && wall >= 1.3
            {
                check.device = Some((w0, p0, Some((music.0.position_s() - p0) / (wall - w0))));
            }
            if song + 0.3 < check.last {
                check.wraps += 1;
                if check.slope.is_none() {
                    check.slope = Some((check.last - check.song0) / check.last_wall.max(1e-6));
                }
            }
            if check.last_wall < 2.0
                && wall >= 2.0
                && let Some(dir) = std::env::var_os("BEATBYTE_SHOT_DIR")
            {
                let path = std::path::PathBuf::from(dir).join("beatbyte-editor-loop.png");
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
            }
            check.max = check.max.max(song);
            check.last = song;
            check.last_wall = wall;
            if wall < 4.0 {
                return;
            }
            check.done = true;
        }
        music.0.stop();
        game_clock.clock.stop();
        if let Some(drill) = &drill {
            let _ = std::fs::remove_dir_all(&drill.scratch);
        }
        let loop_ok = looped.as_ref().is_some_and(|check| {
            let end = check.song0 + 0.8;
            let slope = check.slope.unwrap_or(0.0);
            info!(
                "autopilot: loop at 50 %: {} wrap(s), furthest {:.3} s (end {:.3}), pace {:.2}",
                check.wraps, check.max, end, slope
            );
            let device = check.device.and_then(|d| d.2).unwrap_or(0.0);
            info!("autopilot: the music itself ran at {device:.2}");
            check.wraps >= 1
                && check.max <= end + 0.1
                && (slope - 0.5).abs() < 0.1
                && (device - 0.5).abs() < 0.1
        });
        if !loop_ok {
            error!("autopilot: editor validation FAILED — the loop at half speed");
            deliver(&mut app_exit, AppExit::error());
            return;
        }
        if clicks.0 >= 3 {
            info!("autopilot: editor validation PASSED ({} clicks)", clicks.0);
            deliver(&mut app_exit, AppExit::Success);
        } else {
            error!(
                "autopilot: editor validation FAILED — audition ticked {} times (need >= 3)",
                clicks.0
            );
            deliver(&mut app_exit, AppExit::error());
        }
        return;
    }
    // Phase 1b: the keyboard-side edits are done; the mouse drill runs
    // one gesture per frame, then the audition starts.
    if let Some(keys) = *keys_ok {
        let Some(mouse) = mouse.as_deref() else {
            return;
        };
        if !mouse.done {
            return;
        }
        if !(keys && mouse.ok) {
            error!("autopilot: editor validation FAILED — the mouse drill");
            deliver(&mut app_exit, AppExit::error());
            return;
        }
        // Start the audition (preview from the cursor) and let phase 2
        // assert the metronome overlay; four seconds of it.
        music.0.set_volume(0.3);
        music.0.play_file(state.audio_path.clone());
        music.0.set_song_gain(crate::loudness::song_gain_for(
            &crate::boot::SongAudio::File(state.audio_path.clone()),
            &settings,
        ));
        music.0.seek_s(state.cursor_s);
        game_clock.begin(time.elapsed_secs_f64(), state.cursor_s);
        state.previewing = true;
        *edits_done = true;
        *delay = 1.0;
        return;
    }
    use beatbyte_editor::EditOp;
    let difficulty = state.session.difficulty;
    let note = beatbyte_chart::ChartNote {
        time: 0.123,
        lane: 0,
        len: 0.0,
        hopo: false,
    };
    let mut ok = true;
    // Idempotence: an earlier cycle SAVED its probe note into this
    // chart (found the hard way — the next run's add collided).
    // Sweep both probe slots before starting.
    for (time, lane) in [(0.123, 0u8), (0.321, 3u8)] {
        let leftover = state.session.chart().chart_for(difficulty).and_then(|d| {
            d.notes
                .iter()
                .copied()
                .find(|n| n.lane == lane && (n.time - time).abs() < 1e-6)
        });
        if let Some(note) = leftover {
            ok &= state
                .session
                .edit(EditOp::RemoveNote { difficulty, note })
                .is_ok();
        }
    }
    ok &= state
        .session
        .edit(EditOp::AddNote { difficulty, note })
        .is_ok();
    let after_add = state
        .session
        .chart()
        .chart_for(difficulty)
        .map(|d| d.notes.len());
    ok &= state.session.undo();
    ok &= state.session.redo();
    let after_redo = state
        .session
        .chart()
        .chart_for(difficulty)
        .map(|d| d.notes.len());
    ok &= after_add == after_redo;
    // Move the added note, then undo/redo the move — position must
    // track exactly (E1: invertible MoveNote through the real session).
    let note_at = |state: &crate::editor_ui::EditorState, time: f64, lane: u8| {
        state
            .session
            .chart()
            .chart_for(difficulty)
            .is_some_and(|d| {
                d.notes
                    .iter()
                    .any(|n| n.lane == lane && (n.time - time).abs() < 1e-9)
            })
    };
    ok &= state
        .session
        .edit(EditOp::MoveNote {
            difficulty,
            from_time: 0.123,
            from_lane: 0,
            to_time: 0.321,
            to_lane: 3,
        })
        .is_ok();
    ok &= note_at(&state, 0.321, 3) && !note_at(&state, 0.123, 0);
    ok &= state.session.undo();
    ok &= note_at(&state, 0.123, 0) && !note_at(&state, 0.321, 3);
    ok &= state.session.redo();
    ok &= note_at(&state, 0.321, 3);
    // Put it back so the on-disk count/undo checks stay meaningful.
    ok &= state.session.undo();
    // Batch: two adds as ONE undo step (E2 — bulk ops compose
    // primitives and undo exactly once).
    let depth_before = state.session.undo_depth();
    ok &= state
        .session
        .edit_batch(vec![
            EditOp::AddNote {
                difficulty,
                note: beatbyte_chart::ChartNote {
                    time: 0.777,
                    lane: 1,
                    len: 0.0,
                    hopo: false,
                },
            },
            EditOp::AddNote {
                difficulty,
                note: beatbyte_chart::ChartNote {
                    time: 0.888,
                    lane: 2,
                    len: 0.0,
                    hopo: false,
                },
            },
        ])
        .is_ok();
    ok &= state.session.undo_depth() == depth_before + 1;
    ok &= note_at(&state, 0.777, 1) && note_at(&state, 0.888, 2);
    ok &= state.session.undo();
    ok &= !note_at(&state, 0.777, 1) && !note_at(&state, 0.888, 2);
    // A star-power phrase: added, undone, redone — then left in, so the
    // save below has a real change to write.
    let phrase = beatbyte_chart::ChartPhrase {
        start: 0.2,
        end: 0.9,
    };
    let phrases = |state: &crate::editor_ui::EditorState| {
        state
            .session
            .chart()
            .chart_for(difficulty)
            .map_or(0, |d| d.phrases.len())
    };
    let before_phrase = phrases(&state);
    ok &= state
        .session
        .edit(EditOp::AddPhrase { difficulty, phrase })
        .is_ok();
    ok &= state.session.undo() && phrases(&state) == before_phrase;
    ok &= state.session.redo() && phrases(&state) == before_phrase + 1;
    ok &= state.session.is_valid();
    // Save through the editor's own path: a NEW version beside the
    // copy, made active, marked as hand-made.
    let status = crate::editor_ui::save(&mut state);
    info!("autopilot: editor save — {status}");
    let written = state.chart_path.clone();
    ok &= written.file_name().is_some_and(|n| n == "chart.v2.json");
    let back = beatbyte_chart::load_chart_file(&written).ok();
    ok &= back
        .as_ref()
        .and_then(|chart| chart.chart_for(difficulty).map(|d| d.notes.len()))
        == after_redo;
    ok &= back
        .as_ref()
        .is_some_and(beatbyte_chart::versions::is_hand_edited);
    ok &= std::fs::read_to_string(written.with_file_name("chart-active.json"))
        .is_ok_and(|pointer| pointer.contains("chart.v2.json"));
    // ⚠️ The player's real chart is untouched, byte for byte.
    if let Some(drill) = &drill {
        let untouched = std::fs::read(&drill.original).is_ok_and(|b| b == drill.original_bytes);
        if !untouched {
            error!(
                "autopilot: the editor drill CHANGED the real chart {}",
                drill.original.display()
            );
        }
        ok &= untouched;
    }
    *keys_ok = Some(ok);
    if ok {
        commands.insert_resource(MouseDrill::default());
    } else {
        error!("autopilot: editor validation FAILED");
        deliver(&mut app_exit, AppExit::error());
    }
}

/// The editor drill's loop check: what the playhead did.
#[derive(Default)]
pub struct LoopCheck {
    wall0: f64,
    song0: f64,
    last: f64,
    last_wall: f64,
    max: f64,
    wraps: u32,
    slope: Option<f64>,
    /// (wall, music position, measured pace) of the music thread.
    device: Option<(f64, f64, Option<f64>)>,
    done: bool,
}

/// The editor drill's mouse half: where it is and what it found.
#[derive(Resource, Default)]
pub struct MouseDrill {
    step: usize,
    /// Where the probe note is (time, lane) as the drill goes.
    probe: (f64, u8),
    /// The empty spot the box starts from.
    empty: Vec2,
    notes_before: usize,
    depth_before: usize,
    zoom_pivot: Option<(f32, f64, f64)>,
    notes_seen: usize,
    under: (f64, u8),
    phrases_before: usize,
    home: Option<(beatbyte_core::Difficulty, Vec<beatbyte_chart::ChartNote>)>,
    pasted: (f64, u8),
    typed_to: f64,
    /// Whether every check passed.
    pub ok: bool,
    /// Whether the drill is over.
    pub done: bool,
}

/// The star-power phrases of the difficulty being edited.
fn phrase_count(state: &crate::editor_ui::EditorState) -> usize {
    state
        .session
        .chart()
        .chart_for(state.session.difficulty)
        .map_or(0, |def| def.phrases.len())
}

/// A key press as the window delivers it.
fn key_event(
    window: Entity,
    key_code: KeyCode,
    logical_key: bevy::input::keyboard::Key,
) -> bevy::input::keyboard::KeyboardInput {
    bevy::input::keyboard::KeyboardInput {
        key_code,
        logical_key,
        state: bevy::input::ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    }
}

/// Drive the REAL pointer code, one gesture per frame: place a note
/// with a click, drag it to another time and lane, drag its length,
/// box-select it, right-click empty space and then the note, zoom with
/// Cmd + wheel — and undo back to where it started. Runs before
/// `editor_pointer`, so what it presses is what that system reads.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
#[allow(clippy::too_many_lines)] // one script, one step per frame
fn autopilot_edit_mouse(
    mut commands: Commands,
    drill: Option<ResMut<MouseDrill>>,
    state: Option<ResMut<crate::editor_ui::EditorState>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut wheel: MessageWriter<bevy::input::mouse::MouseWheel>,
    mut typed: MessageWriter<bevy::input::keyboard::KeyboardInput>,
    mut closes: MessageWriter<bevy::window::WindowCloseRequested>,
    windows: Query<Entity, With<bevy::window::PrimaryWindow>>,
    mut chip_actions: ResMut<crate::editor_ui::ChipActions>,
) {
    use beatbyte_editor::view;
    let (Some(mut drill), Some(mut state)) = (drill, state) else {
        return;
    };
    if drill.done {
        return;
    }
    let point = |commands: &mut Commands, at: Vec2| {
        commands.insert_resource(crate::editor_ui::pointer::InjectedPointer(at));
    };
    let note_at = |state: &crate::editor_ui::EditorState, key: (f64, u8)| {
        state
            .notes()
            .iter()
            .copied()
            .find(|n| view::same_note(key, n))
    };
    let check = |drill: &mut MouseDrill, what: &str, passed: bool| {
        if passed {
            info!("autopilot: mouse {what}: ok");
        } else {
            error!("autopilot: mouse {what}: FAILED");
            drill.ok = false;
        }
    };
    let v = state.view;
    let xy = |lane: u8, time: f64| Vec2::new(view::lane_x(lane) as f32, v.y_of(time) as f32);
    let step = drill.step;
    drill.step += 1;
    match step {
        0 => {
            drill.ok = true;
            // A quiet spot: lanes 3 and 4 free for three seconds.
            let busy = |t: f64| {
                state
                    .notes()
                    .iter()
                    .any(|n| n.lane >= 3 && (n.time - t).abs() < 2.5)
            };
            let mut t = 2.0;
            while busy(t) && t < 600.0 {
                t += 0.5;
            }
            state.snap_on = true;
            state.division = 4;
            state.follow = false;
            state.selection.clear();
            state.view.center_s = t + 0.5;
            state.view.px_per_s = 150.0;
            state.dirty_view = true;
            drill.probe = (t, 4);
            drill.notes_before = state.notes().len();
            drill.depth_before = state.session.undo_depth();
            let target = Vec2::new(view::lane_x(4) as f32, state.view.y_of(t) as f32);
            point(&mut commands, target);
        }
        // Click: a note, snapped, selected.
        1 => buttons.press(MouseButton::Left),
        2 => buttons.release(MouseButton::Left),
        3 => {
            let placed = state.snap_with(drill.probe.0, false);
            let found = note_at(&state, (placed, 4));
            check(&mut drill, "click places a snapped note", found.is_some());
            check(
                &mut drill,
                "the placed note is selected",
                state.selection.len() == 1,
            );
            drill.probe = (placed, 4);
            point(&mut commands, xy(4, placed));
            buttons.press(MouseButton::Left);
        }
        // Drag: half a second later, one lane left.
        4 => point(&mut commands, xy(3, drill.probe.0 + 0.5)),
        5 => buttons.release(MouseButton::Left),
        6 => {
            let expected = state.snap_with(drill.probe.0 + 0.5, false);
            let passed =
                note_at(&state, (expected, 3)).is_some() && note_at(&state, drill.probe).is_none();
            check(&mut drill, "drag moves the note in time and lane", passed);
            drill.probe = (expected, 3);
            // The length tab above the head.
            let head = xy(3, expected);
            point(
                &mut commands,
                Vec2::new(head.x, head.y + (view::HEAD_REACH + 4.0) as f32),
            );
            buttons.press(MouseButton::Left);
        }
        7 => point(&mut commands, xy(3, drill.probe.0 + 1.0)),
        8 => buttons.release(MouseButton::Left),
        9 => {
            let want = state.snap_with(drill.probe.0 + 1.0, false) - drill.probe.0;
            let len = note_at(&state, drill.probe).map_or(-1.0, |n| n.len);
            check(
                &mut drill,
                "dragging the tab sets the length",
                (len - want).abs() < 1e-6,
            );
            // An empty spot for the box: lane 0, below the note.
            let mut empty = Vec2::new(view::lane_x(0) as f32, v.y_of(drill.probe.0) as f32 - 30.0);
            for _ in 0..40 {
                let hit = view::hit_test(
                    state.notes(),
                    &state.view,
                    f64::from(empty.x),
                    f64::from(empty.y),
                );
                if matches!(hit, view::Hit::Lane { .. }) {
                    break;
                }
                empty.y -= 7.0;
            }
            drill.empty = empty;
            // ⚠️ Nothing selected before the box: the length drag had
            // selected the note, and a box that selected nothing still
            // passed (the mutation probe showed it).
            state.selection.clear();
            point(&mut commands, empty);
            buttons.press(MouseButton::Left);
        }
        10 => {
            let far = xy(4, drill.probe.0);
            point(&mut commands, Vec2::new(far.x, far.y + 30.0));
            // A picture of the box being drawn, when asked for (this
            // frame renders it; the next one releases it).
            if let Some(dir) = std::env::var_os("BEATBYTE_SHOT_DIR") {
                let path = std::path::PathBuf::from(dir).join("beatbyte-editor-box.png");
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
            }
        }
        11 => buttons.release(MouseButton::Left),
        12 => {
            let probe = drill.probe;
            check(
                &mut drill,
                "a box selects the note",
                state.selection.iter().any(|k| {
                    view::same_note(
                        *k,
                        &beatbyte_chart::ChartNote {
                            time: probe.0,
                            lane: probe.1,
                            len: 0.0,
                            hopo: false,
                        },
                    )
                }),
            );
            let empty = drill.empty;
            point(&mut commands, empty);
            buttons.press(MouseButton::Right);
        }
        13 => buttons.release(MouseButton::Right),
        14 => {
            let passed =
                state.selection.is_empty() && state.menu.is_some_and(|menu| menu.target.is_none());
            check(
                &mut drill,
                "right-click on empty space opens the space menu, selection cleared",
                passed,
            );
            // A click beside the menu closes it and places nothing.
            drill.notes_seen = state.notes().len();
            buttons.press(MouseButton::Left);
        }
        15 => buttons.release(MouseButton::Left),
        16 => {
            let passed = state.menu.is_none() && state.notes().len() == drill.notes_seen;
            check(&mut drill, "a click beside the menu only closes it", passed);
            point(&mut commands, xy(drill.probe.1, drill.probe.0));
            buttons.press(MouseButton::Right);
        }
        17 => {
            buttons.release(MouseButton::Right);
            if let Some(dir) = std::env::var_os("BEATBYTE_SHOT_DIR") {
                let path = std::path::PathBuf::from(dir).join("beatbyte-editor-menu.png");
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
            }
        }
        18 => {
            let probe = drill.probe;
            let passed = state
                .menu
                .and_then(|menu| menu.target)
                .is_some_and(|key| key.1 == probe.1 && (key.0 - probe.0).abs() < 1e-6);
            check(&mut drill, "right-click on a note opens its menu", passed);
            // The menu's Delete, exactly as a click on it arrives.
            chip_actions.0.push(crate::editor_ui::actions::DELETE);
        }
        19 => {
            let passed = note_at(&state, drill.probe).is_none()
                && state.notes().len() == drill.notes_before
                && state.menu.is_none();
            check(
                &mut drill,
                "the menu's Delete deletes the note and closes",
                passed,
            );
            // Cmd/Ctrl + wheel: zoom around the pointer.
            let pivot_y = 40.0f32;
            point(&mut commands, Vec2::new(view::lane_x(2) as f32, pivot_y));
            drill.zoom_pivot = Some((
                pivot_y,
                state.view.time_at(f64::from(pivot_y)),
                state.view.px_per_s,
            ));
            keys.press(KeyCode::ControlLeft);
            if let Ok(window) = windows.single() {
                wheel.write(bevy::input::mouse::MouseWheel {
                    unit: bevy::input::mouse::MouseScrollUnit::Line,
                    x: 0.0,
                    y: 1.0,
                    window,
                    phase: bevy::input::touch::TouchPhase::Moved,
                });
            }
        }
        20 => {
            keys.release(KeyCode::ControlLeft);
            if let Some((pivot_y, pivot_time, zoom)) = drill.zoom_pivot {
                check(
                    &mut drill,
                    "cmd-wheel zooms in around the pointer",
                    state.view.px_per_s > zoom * 1.1
                        && (state.view.time_at(f64::from(pivot_y)) - pivot_time).abs() < 1e-3,
                );
            }
            // Copy and paste: the deleted note back (undo), copied,
            // pasted two seconds later with its length.
            let back = state.session.undo();
            let passed = back && note_at(&state, drill.probe).is_some();
            check(&mut drill, "undo brings the deleted note back", passed);
            state.selection = vec![drill.probe];
            chip_actions.0.push(crate::editor_ui::actions::COPY);
        }
        21 => {
            state.cursor_s = drill.probe.0 + 2.0;
            chip_actions.0.push(crate::editor_ui::actions::PASTE);
        }
        22 => {
            let at = state.snap_with(drill.probe.0 + 2.0, false);
            let original = note_at(&state, drill.probe);
            let pasted = note_at(&state, (at, drill.probe.1));
            let passed = pasted.is_some_and(|p| {
                original.is_some_and(|o| (p.len - o.len).abs() < 1e-9 && p.len > 0.0)
            }) && state.selection.len() == 1;
            check(
                &mut drill,
                "copy and paste carry the note and its length",
                passed,
            );
            drill.pasted = (at, drill.probe.1);
            // Type its time: open the field, then real key events.
            chip_actions.0.push(crate::editor_ui::actions::EDIT_TIME);
        }
        23 => {
            let passed = state.field.is_some();
            check(
                &mut drill,
                "the time field opens on the selected note",
                passed,
            );
            let target = format!("{:.3}", drill.pasted.0 + 1.0);
            // Replace the shown value: backspace it away, then type.
            if let Ok(window) = windows.single() {
                let shown = state.field.as_ref().map_or(0, |(_, text)| text.len());
                for _ in 0..shown {
                    typed.write(key_event(
                        window,
                        KeyCode::Backspace,
                        bevy::input::keyboard::Key::Backspace,
                    ));
                }
                for c in target.chars() {
                    typed.write(key_event(
                        window,
                        KeyCode::Digit0,
                        bevy::input::keyboard::Key::Character(c.to_string().into()),
                    ));
                }
                typed.write(key_event(
                    window,
                    KeyCode::Enter,
                    bevy::input::keyboard::Key::Enter,
                ));
            }
            drill.typed_to = target.parse().unwrap_or(0.0);
        }
        24 => {}
        25 => {
            let lane = drill.pasted.1;
            info!(
                "autopilot: typed field: field {:?}, status `{}`, wanted {:.3} lane {lane}, selected {:?}",
                state.field, state.status, drill.typed_to, state.selection
            );
            let passed = state.field.is_none()
                && note_at(&state, (drill.typed_to, lane)).is_some()
                && note_at(&state, drill.pasted).is_none()
                && !state.previewing;
            check(
                &mut drill,
                "a typed time moves the note exactly, Enter does not play",
                passed,
            );
            // A star-power phrase over the selected note.
            drill.phrases_before = phrase_count(&state);
            chip_actions.0.push(crate::editor_ui::actions::PHRASE);
        }
        26 => {
            let passed = phrase_count(&state) == drill.phrases_before + 1;
            check(
                &mut drill,
                "Y lays a star-power phrase over the selection",
                passed,
            );
            // The next difficulty: this one must stay exactly as it is.
            drill.home = Some((state.session.difficulty, state.notes().to_vec()));
            chip_actions.0.push(crate::editor_ui::actions::LEVEL);
        }
        27 => {
            let passed = drill.home.as_ref().is_some_and(|(home, notes)| {
                state.session.difficulty != *home
                    && state
                        .session
                        .chart()
                        .chart_for(*home)
                        .is_some_and(|def| def.notes == *notes)
            });
            check(
                &mut drill,
                "Q switches difficulty and leaves the other untouched",
                passed,
            );
            if let Some((home, _)) = drill.home.clone() {
                let _ = state.session.set_difficulty(home);
                state.selection.clear();
            }
            // A note under the typed note's tail: a warning to find.
            let under = (drill.typed_to + 0.2, drill.pasted.1);
            let difficulty = state.session.difficulty;
            let added = state
                .session
                .edit(beatbyte_editor::EditOp::AddNote {
                    difficulty,
                    note: beatbyte_chart::ChartNote {
                        time: under.0,
                        lane: under.1,
                        len: 0.0,
                        hopo: false,
                    },
                })
                .is_ok();
            check(&mut drill, "a note under a held note can be placed", added);
            state.dirty_view = true;
            state.cursor_s = 0.0;
            drill.under = under;
        }
        // The warnings are drawn (and counted) after this system; W
        // is pressed the frame after.
        28 => chip_actions.0.push(crate::editor_ui::actions::NEXT_WARNING),
        29 => {
            let under = drill.under;
            let passed = state.warnings.iter().any(|w| {
                w.lane == under.1
                    && (w.time - under.0).abs() < 1e-6
                    && matches!(w.kind, beatbyte_editor::lint::Kind::UnderTail { .. })
            }) && (state.cursor_s - under.0).abs() < 1e-6
                && state.selection == vec![under];
            check(
                &mut drill,
                "a note under a tail is warned about and W jumps to it",
                passed,
            );
            // Quitting with unsaved edits: the first request only warns.
            if let Ok(window) = windows.single() {
                closes.write(bevy::window::WindowCloseRequested { window });
            }
        }
        30 => {
            let passed = state.exit_armed > 0.0 && state.status.contains("quit again");
            check(
                &mut drill,
                "quitting with unsaved edits only warns the first time",
                passed,
            );
            state.exit_armed = 0.0;
            // Everything the drill did undoes, back to the start.
            let mut undone = 0;
            while state.session.undo_depth() > drill.depth_before && state.session.undo() {
                undone += 1;
            }
            let passed = undone == 7
                && state.notes().len() == drill.notes_before
                && phrase_count(&state) == drill.phrases_before;
            check(
                &mut drill,
                "the drill's edits undo exactly (7 steps)",
                passed,
            );
            commands.remove_resource::<crate::editor_ui::pointer::InjectedPointer>();
            drill.done = true;
        }
        _ => drill.done = true,
    }
}

/// Pick the demo song and start it (optionally with simulated
/// multiplayer via `BEATBYTE_AUTOPILOT_PLAYERS=N`).
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn autopilot_song_select(
    mut commands: Commands,
    time: Res<Time>,
    mut delay: Local<f32>,
    mut waited: Local<f32>,
    queue: Option<Res<crate::import::ImportQueue>>,
    library: Res<crate::library::SongLibrary>,
    builtins: Res<crate::boot::BuiltinSongs>,
    mut roster: ResMut<crate::multiplayer::PlayerRoster>,
    mut selected: ResMut<crate::song_select::SelectedDifficulty>,
    mut practice: ResMut<crate::gameplay::PracticeState>,
    mut next_state: ResMut<NextState<AppState>>,
    twins: Option<Res<crate::study_twin::StudyQueue>>,
    chore: Option<Res<crate::chore::Chore>>,
) {
    *delay += time.delta_secs();
    if *delay > 0.6 {
        *delay = 0.0;
        // Editor mode: open the first file-based song instead.
        if std::env::var_os("BEATBYTE_AUTOPILOT_EDIT").is_some() {
            // `BEATBYTE_AUTOPILOT_SONG` names the song, as for a played
            // run; without it the first file-based one.
            let wanted = std::env::var("BEATBYTE_AUTOPILOT_SONG")
                .ok()
                .map(|s| s.to_lowercase());
            let file_entry = library
                .entries
                .iter()
                .filter(|entry| {
                    wanted
                        .as_ref()
                        .is_none_or(|w| entry.title.to_lowercase().contains(w.as_str()))
                })
                .find_map(|entry| {
                    if let crate::library::SongSource::File {
                        chart_path,
                        audio_path,
                    } = &entry.source
                    {
                        Some((entry, chart_path.clone(), audio_path.clone()))
                    } else {
                        None
                    }
                });
            let Some((entry, chart_path, audio_path)) = file_entry else {
                error!("autopilot: no file-based song to edit");
                std::process::exit(1);
            };
            let difficulty = entry
                .difficulties
                .first()
                .copied()
                .unwrap_or(beatbyte_core::Difficulty::Medium);
            // ⚠️ The drill SAVES, and it used to save into the player's
            // real chart. It edits a copy in a scratch folder instead;
            // the audio is only read. The verdict checks that the real
            // file did not change by a byte.
            let scratch =
                std::env::temp_dir().join(format!("beatbyte-editor-drill-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&scratch);
            let copy = scratch.join(beatbyte_chart::versions::BASE_CHART);
            let original = std::fs::read(&chart_path).unwrap_or_default();
            if std::fs::create_dir_all(&scratch)
                .and_then(|()| std::fs::write(&copy, &original))
                .is_err()
            {
                error!("autopilot: cannot prepare the editor drill's scratch copy");
                std::process::exit(1);
            }
            commands.insert_resource(EditDrill {
                original: chart_path.clone(),
                original_bytes: original,
                scratch: scratch.clone(),
            });
            match crate::editor_ui::open_editor(&mut commands, &copy, &audio_path, difficulty) {
                Ok(()) => {
                    info!("autopilot: editing \"{}\"", entry.title);
                    next_state.set(AppState::Editor);
                }
                Err(reason) => {
                    error!("autopilot: cannot open editor: {reason}");
                    std::process::exit(1);
                }
            }
            return;
        }
        // Deletion and alignment modes own the browser — never start
        // a song.
        if std::env::var_os("BEATBYTE_AUTOPILOT_DELETE").is_some()
            || std::env::var_os("BEATBYTE_AUTOPILOT_ALIGN").is_some()
            || std::env::var_os("BEATBYTE_AUTOPILOT_TASTE").is_some()
        {
            return;
        }
        // In drop mode, let the WHOLE batch finish before starting a
        // song — the batch summary is part of what is being proven.
        if std::env::var_os("BEATBYTE_AUTOPILOT_DROP").is_some()
            && queue.as_ref().is_none_or(|q| q.total == 0 || q.active())
        {
            *waited += time.delta_secs() + 0.6;
            if *waited > 300.0 {
                error!("autopilot: import batch never finished");
                std::process::exit(1);
            }
            return;
        }
        // With twins asked for, the import's guitar-study job is part
        // of what is being proven too: wait it out (a separation is
        // minutes), and a job that ERRED fails the run — a skip with a
        // reason (no demucs, too little tonal evidence) is a result.
        if std::env::var_os("BEATBYTE_AUTOPILOT_TWINS").is_some()
            && std::env::var_os("BEATBYTE_AUTOPILOT_DROP").is_some()
        {
            let busy = twins.as_ref().is_some_and(|t| !t.pending.is_empty())
                || chore.as_ref().is_some_and(|c| c.running());
            if busy {
                *waited += time.delta_secs() + 0.6;
                if *waited > 900.0 {
                    error!("autopilot: the guitar-study job never finished");
                    std::process::exit(1);
                }
                return;
            }
            if let Some(chore) = chore.as_ref().filter(|c| c.ran) {
                if chore.ok {
                    info!("autopilot: twin job PASSED — {}", chore.line);
                } else {
                    error!("autopilot: twin job FAILED — {}", chore.line);
                    std::process::exit(1);
                }
            }
        }
        // MC-set mode: comma-separated title needles queue a set and
        // start it as one continuous performance.
        if let Ok(raw) = std::env::var("BEATBYTE_AUTOPILOT_MC") {
            let mut songs = Vec::new();
            for needle in raw.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                match select_song(&library.entries, Some(needle)) {
                    Ok(entry) => match crate::song_select::prepare_song(entry, &builtins) {
                        Ok(song) => songs.push(song),
                        Err(reason) => {
                            error!("autopilot: cannot load {needle:?}: {reason}");
                            std::process::exit(1);
                        }
                    },
                    Err(reason) => {
                        error!("autopilot: {reason}");
                        std::process::exit(1);
                    }
                }
            }
            let Some(first) = songs.first().cloned() else {
                error!("autopilot: BEATBYTE_AUTOPILOT_MC names no songs");
                std::process::exit(1);
            };
            let players: usize = std::env::var("BEATBYTE_AUTOPILOT_PLAYERS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1)
                .clamp(1, crate::multiplayer::MAX_PLAYERS);
            roster.devices = vec![crate::multiplayer::DeviceId::Keyboard; players];
            info!(
                "autopilot: starting an MC set of {} song(s) with {players} player(s)",
                songs.len()
            );
            commands.insert_resource(crate::mc::McSet { songs, position: 0 });
            commands.insert_resource(first);
            next_state.set(AppState::Gameplay);
            return;
        }
        let selector = std::env::var("BEATBYTE_AUTOPILOT_SONG").ok();
        let entry = match select_song(&library.entries, selector.as_deref()) {
            Ok(entry) => entry,
            Err(reason) => {
                // A pending drop-import legitimately needs time to
                // appear in the library — keep polling for a while.
                if std::env::var_os("BEATBYTE_AUTOPILOT_DROP").is_some() {
                    *waited += time.delta_secs() + 0.6;
                    if *waited > 300.0 {
                        error!("autopilot: import never appeared: {reason}");
                        std::process::exit(1);
                    }
                    return;
                }
                error!("autopilot: {reason}");
                std::process::exit(1);
            }
        };
        let wanted = std::env::var("BEATBYTE_AUTOPILOT_DIFFICULTY").ok();
        match resolve_difficulty(wanted.as_deref(), &entry.difficulties) {
            Ok(Some(difficulty)) => selected.0 = difficulty,
            Ok(None) => {}
            Err(reason) => {
                error!("autopilot: {reason}");
                std::process::exit(1);
            }
        }
        if let Ok(raw) = std::env::var("BEATBYTE_AUTOPILOT_SPEED") {
            match raw.parse::<u32>() {
                Ok(percent) if (50..=150).contains(&percent) => {
                    practice.speed_percent = percent;
                    info!("autopilot: practice speed {percent}%");
                }
                _ => {
                    error!("autopilot: BEATBYTE_AUTOPILOT_SPEED must be 50-150");
                    std::process::exit(1);
                }
            }
        }
        let players: usize = std::env::var("BEATBYTE_AUTOPILOT_PLAYERS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1)
            .clamp(1, crate::multiplayer::MAX_PLAYERS);
        roster.devices = vec![crate::multiplayer::DeviceId::Keyboard; players];
        match crate::song_select::prepare_song(entry, &builtins) {
            Ok(song) => {
                info!(
                    "autopilot: starting \"{}\" on {} with {players} player(s)",
                    entry.title, selected.0
                );
                commands.insert_resource(song);
                next_state.set(AppState::Gameplay);
            }
            Err(reason) => error!("autopilot: cannot start song: {reason}"),
        }
    }
}

/// The run is only a PASS if the autopilot itself says so. If the
/// window disappears mid-run (display sleep, WM kill), the app must
/// exit with an error — with the default exit condition it exited 0
/// and a killed run was indistinguishable from a flawless one
/// (happened: macOS display sleep at 1 AM, "Monitor removed",
/// 66-second song "passed" in 18).
fn fail_if_window_vanishes(
    mut seen: Local<bool>,
    windows: Query<(), With<bevy::window::PrimaryWindow>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let present = !windows.is_empty();
    if *seen && !present {
        error!("autopilot: window vanished before a verdict — failing the run");
        deliver(&mut app_exit, AppExit::error());
    }
    *seen |= present;
}

/// Whether a run ended where a run should end.
///
/// The song-end check fires once every note is resolved AND the clock
/// is past the content; a healthy run therefore lands within a couple
/// of seconds of the content end. Landing far beyond it means the
/// clock was somewhere else entirely. Pure — tested.
#[must_use]
pub fn song_ended_sanely(finished_at_s: f64, content_end_s: f64) -> bool {
    // Generous: the outro, a long sustain tail and a slow practice
    // rate all push the end out a little. A hijacked clock misses by
    // minutes, not by seconds.
    finished_at_s <= content_end_s + 10.0
}

/// `BEATBYTE_AUTOPILOT_RATE=<1-5>`: on the results screen, press the
/// real digit key (and ArrowRight when the played chart carries
/// provenance), then parse the session log back and verify the
/// feedback lines actually landed. Failing to find them fails the
/// run loudly.
fn autopilot_rate(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    store: Res<crate::telemetry::TelemetryStore>,
    last_run: Res<crate::telemetry::LastRun>,
    song: Option<Res<crate::boot::LoadedSong>>,
    mut frame: Local<u32>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let Some(rating) = std::env::var("BEATBYTE_AUTOPILOT_RATE")
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        .filter(|v| (1..=5).contains(v))
    else {
        error!("autopilot: BEATBYTE_AUTOPILOT_RATE must be 1-5");
        std::process::exit(1);
    };
    let digit = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
    ][usize::from(rating) - 1];
    let has_parent = song.as_ref().is_some_and(|s| s.chart.provenance.is_some());
    match *frame {
        4 => keys.press(digit),
        5 => keys.release(digit),
        8 if has_parent => keys.press(KeyCode::ArrowRight),
        9 if has_parent => keys.release(KeyCode::ArrowRight),
        14 => {
            if !last_run.open() {
                error!("autopilot: rate drill FAILED — no open session to rate into");
                deliver(&mut app_exit, AppExit::error());
                *frame += 1;
                return;
            }
            let Some(writer) = store.writer() else {
                error!("autopilot: rate drill FAILED — the store is not open");
                deliver(&mut app_exit, AppExit::error());
                *frame += 1;
                return;
            };
            for slot in &last_run.slots {
                // Recording is asynchronous; this is the one call
                // that waits for the worker, and it exists so a drill
                // can prove the rating LANDED rather than that it was
                // sent.
                let Some(notes) = writer.notes_for(*slot, std::time::Duration::from_secs(5)) else {
                    error!("autopilot: rate drill FAILED — the writer did not answer");
                    deliver(&mut app_exit, AppExit::error());
                    *frame += 1;
                    return;
                };
                let fun_ok = notes.iter().any(
                    |note| matches!(note, beatbyte_telemetry::PlayerNote::Fun(v) if *v == rating),
                );
                let versus_ok = !has_parent
                    || notes.iter().any(|note| {
                        matches!(
                            note,
                            beatbyte_telemetry::PlayerNote::Versus { better, .. } if *better
                        )
                    });
                if !fun_ok || !versus_ok {
                    error!(
                        "autopilot: rate drill FAILED — slot {slot} lacks fun={fun_ok} versus={versus_ok}"
                    );
                    deliver(&mut app_exit, AppExit::error());
                    *frame += 1;
                    return;
                }
            }
            info!(
                "autopilot: rate drill PASSED — fun {rating} (versus: {}) landed in {} session(s)",
                if has_parent { "better" } else { "no parent" },
                last_run.slots.len()
            );
        }
        // Then leave the screen the way a player does and prove the
        // navigation lands in the BROWSER, not the main menu (user
        // request: back to the last active submenu). This supersedes
        // the end-of-song verdict in this drill variant — reaching
        // the results at all already required the full song.
        18 => keys.press(KeyCode::Enter),
        19 => keys.release(KeyCode::Enter),
        _ => {}
    }
    *frame += 1;
}

/// The second half of the rate drill: after its Enter, the app must
/// land on the song browser. Runs in every state so it can see where
/// the navigation actually went.
fn autopilot_rate_return(
    state: Res<State<AppState>>,
    mut left_results: Local<bool>,
    mut settle: Local<u32>,
    mut app_exit: MessageWriter<AppExit>,
) {
    match state.get() {
        AppState::Results => {
            *left_results = true; // armed once the drill reached results
        }
        AppState::SongSelect if *left_results => {
            // A couple of frames of grace for the transition.
            *settle += 1;
            if *settle > 3 {
                info!("autopilot: rate drill return PASSED — results lead back to the browser");
                deliver(&mut app_exit, AppExit::Success);
            }
        }
        AppState::MainMenu if *left_results => {
            error!("autopilot: rate drill return FAILED — landed on the main menu");
            deliver(&mut app_exit, AppExit::error());
        }
        _ => {}
    }
}

/// `BEATBYTE_AUTOPILOT_TASTE=<title>`: drive the blind taste test
/// from the browser with REAL keys — arrows to the song, then `T` —
/// and verify the test that came out of it is actually blind: two
/// DIFFERENT charts, both sides scheduled, one shared window.
///
/// A test built from one chart twice, or from a window of zero
/// length, would still play and still ask its question; the answer
/// would simply mean nothing. That is the failure this drill exists
/// to make loud.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn autopilot_taste(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    library: Res<crate::library::SongLibrary>,
    view: Res<crate::song_select::BrowserView>,
    cursor: Res<crate::song_select::BrowserCursor>,
    taste: Option<Res<crate::taste::TasteTest>>,
    status: Res<crate::import::ImportStatus>,
    mut selected: ResMut<crate::song_select::SelectedDifficulty>,
    mut frame: Local<u32>,
    mut downs: Local<Option<u32>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let Some(target) = std::env::var("BEATBYTE_AUTOPILOT_TASTE").ok() else {
        return;
    };

    let Some(index) = title_match(
        library.entries.iter().map(|entry| entry.title.as_str()),
        &target,
    ) else {
        error!("autopilot: no song matching `{target}` to taste-test");
        deliver(&mut app_exit, AppExit::error());
        return;
    };
    // The song-start system resolves BEATBYTE_AUTOPILOT_DIFFICULTY for
    // an ordinary run but leaves the browser alone in this mode, so
    // the drill honours it itself — once, before the first key. (The
    // first study-vs-old run asked for hard and got medium.)
    if *frame == 0
        && let Ok(wanted) = std::env::var("BEATBYTE_AUTOPILOT_DIFFICULTY")
    {
        match resolve_difficulty(Some(&wanted), &library.entries[index].difficulties) {
            Ok(Some(difficulty)) => selected.0 = difficulty,
            Ok(None) => {}
            Err(reason) => {
                error!("autopilot: {reason}");
                deliver(&mut app_exit, AppExit::error());
                return;
            }
        }
    }
    // The browser shows the library SORTED: the arrow count is the
    // song's row in the view minus where the cursor already sits,
    // fixed on the first frame (the cursor moves under the presses).
    let downs = *downs.get_or_insert_with(|| {
        let row = view.order.iter().position(|&i| i == index).unwrap_or(0);
        row.saturating_sub(cursor.0) as u32
    });
    let step = *frame / 2;
    let pressing = (*frame).is_multiple_of(2);
    if step < downs {
        if pressing {
            keys.press(KeyCode::ArrowDown);
        } else {
            keys.release(KeyCode::ArrowDown);
        }
    } else if step == downs {
        if pressing {
            keys.press(KeyCode::KeyT);
        } else {
            keys.release(KeyCode::KeyT);
        }
    } else if step > downs + 2 {
        let Some(test) = taste else {
            error!(
                "autopilot: taste drill FAILED — T built no test ({})",
                status.0
            );
            deliver(&mut app_exit, AppExit::error());
            *frame += 1;
            return;
        };
        let same = test.versions[0].hash == test.versions[1].hash;
        let span = test.window.1 - test.window.0;
        let both_sides = test.order[0] != test.order[1];
        if same || span <= 0.0 || !both_sides || test.played != 0 {
            error!(
                "autopilot: taste drill FAILED — same chart {same}, window {span:.1}s, order \
                 {:?}, played {}",
                test.order, test.played
            );
            deliver(&mut app_exit, AppExit::error());
            *frame += 1;
            return;
        }
        info!(
            "autopilot: taste drill PASSED — {} against {}, {:.1}s from {:.1}s",
            test.versions[0].name, test.versions[1].name, span, test.window.0
        );
        // The run itself now plays both sides; the verdict arm on the
        // results screen finishes the drill.
    }
    *frame += 1;
}

/// The second half of the taste drill: on the results screen, press a
/// real arrow to name a side, then read the session log back and
/// verify the pairwise verdict actually landed in it.
fn autopilot_taste_verdict(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    store: Res<crate::telemetry::TelemetryStore>,
    last_run: Res<crate::telemetry::LastRun>,
    taste: Option<Res<crate::taste::TasteTest>>,
    mut frame: Local<u32>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if std::env::var_os("BEATBYTE_AUTOPILOT_TASTE").is_none() {
        return;
    }
    match *frame {
        4 => keys.press(KeyCode::ArrowRight),
        5 => keys.release(KeyCode::ArrowRight),
        10 => {
            let Some(test) = taste else {
                error!("autopilot: taste verdict FAILED — the test is gone");
                deliver(&mut app_exit, AppExit::error());
                *frame += 1;
                return;
            };
            // RIGHT means "the second one", and the log names the
            // chart that played second — so the line must read
            // "better" against the FIRST side's hash.
            let want = test.versions[test.order[0]].hash.clone();
            if !last_run.open() {
                error!("autopilot: taste verdict FAILED — no open session");
                deliver(&mut app_exit, AppExit::error());
                *frame += 1;
                return;
            }
            let Some(writer) = store.writer() else {
                error!("autopilot: taste verdict FAILED — the store is not open");
                deliver(&mut app_exit, AppExit::error());
                *frame += 1;
                return;
            };
            for slot in &last_run.slots {
                let Some(notes) = writer.notes_for(*slot, std::time::Duration::from_secs(5)) else {
                    error!("autopilot: taste verdict FAILED — the writer did not answer");
                    deliver(&mut app_exit, AppExit::error());
                    *frame += 1;
                    return;
                };
                let landed = notes.iter().any(|note| {
                    matches!(
                        note,
                        beatbyte_telemetry::PlayerNote::Versus { better, parent }
                            if *better && *parent == want
                    )
                });
                if !landed {
                    error!(
                        "autopilot: taste verdict FAILED — slot {slot} has no `better` against \
                         {want}"
                    );
                    deliver(&mut app_exit, AppExit::error());
                    *frame += 1;
                    return;
                }
            }
            info!(
                "autopilot: taste verdict PASSED — \"the second one\" recorded against {want} in \
                 {} session(s)",
                last_run.slots.len()
            );
            deliver(&mut app_exit, AppExit::Success);
        }
        _ => {}
    }
    *frame += 1;
}

/// `BEATBYTE_AUTOPILOT_SPEED=<50-150>`: verify mid-run that song
/// time advances at the practice rate — slope over ≥8 wall seconds,
/// tolerance ±5 %. The autopilot's own judgment cannot see a wrong
/// rate (injector and notes share the clock), so the slope is the
/// probe.
fn autopilot_speed_check(
    time: Res<Time>,
    game_clock: Res<crate::audio_sys::GameClock>,
    practice: Res<crate::gameplay::PracticeState>,
    mut anchor: Local<Option<(f64, f64)>>,
    mut done: Local<bool>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if *done {
        return;
    }
    let Some(song_now) = game_clock.song_time(&time) else {
        return;
    };
    // Anchor once the count-in is over — its start transition would
    // dilute the slope.
    if song_now < 0.5 {
        return;
    }
    let wall_now = f64::from(time.elapsed_secs());
    let Some((wall_anchor, song_anchor)) = *anchor else {
        *anchor = Some((wall_now, song_now));
        return;
    };
    let wall_dt = wall_now - wall_anchor;
    if wall_dt < 8.0 {
        return;
    }
    let slope = (song_now - song_anchor) / wall_dt;
    let wanted = practice.rate();
    *done = true;
    if (slope - wanted).abs() <= wanted * 0.05 {
        info!("autopilot: speed check PASSED — song advances at {slope:.3}x (wanted {wanted:.2}x)");
    } else {
        error!("autopilot: speed check FAILED — song advances at {slope:.3}x, wanted {wanted:.2}x");
        deliver(&mut app_exit, AppExit::error());
    }
}

/// `BEATBYTE_AUTOPILOT_LOOP=<from>,<to>`: arm the practice section
/// loop and prove it — song time must wrap (jump backwards) twice,
/// and after a wrap a note inside the section must be judgeable
/// again. Delivers its own verdict; the song never ends while it
/// loops, so the normal end-of-song verdict cannot.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn autopilot_loop_check(
    mut practice: ResMut<crate::gameplay::PracticeState>,
    game_clock: Res<crate::audio_sys::GameClock>,
    players: Query<&crate::gameplay::PlayerSession>,
    time: Res<Time>,
    mut armed: Local<bool>,
    mut last_now: Local<Option<f64>>,
    mut wraps: Local<u32>,
    mut waited: Local<f32>,
    mut done: Local<bool>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if *done {
        return;
    }
    let bounds = std::env::var("BEATBYTE_AUTOPILOT_LOOP")
        .ok()
        .and_then(|raw| {
            let (from, to) = raw.split_once(',')?;
            Some((
                from.trim().parse::<f64>().ok()?,
                to.trim().parse::<f64>().ok()?,
            ))
        });
    let Some((from, to)) = bounds else {
        error!("autopilot: BEATBYTE_AUTOPILOT_LOOP must be `<from>,<to>` in seconds");
        std::process::exit(1);
    };
    if !*armed {
        practice.set_loop_bound(false, from);
        practice.set_loop_bound(true, to);
        if practice.loop_span().is_none() {
            error!("autopilot: loop {from},{to} does not arm (span too short?)");
            std::process::exit(1);
        }
        *armed = true;
        info!("autopilot: loop drill armed for {from:.1}-{to:.1}s");
    }
    *waited += time.delta_secs();
    if *waited > 120.0 {
        error!("autopilot: loop drill FAILED — no second wrap within 120s");
        deliver(&mut app_exit, AppExit::error());
        *done = true;
        return;
    }
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    if let Some(last) = *last_now
        && now < last - 1.0
    {
        *wraps += 1;
        info!("autopilot: loop wrap {} ({last:.2}s -> {now:.2}s)", *wraps);
        // After a wrap, a note inside the section must be judgeable
        // again — the rewind is the half that silently breaking
        // would turn the loop into a spectator ride.
        let reopened = players.iter().any(|player| {
            let track = player.session.track();
            track
                .events()
                .iter()
                .enumerate()
                .filter(|(_, event)| event.time_s >= from && event.time_s <= to)
                .any(|(index, _)| {
                    matches!(
                        player.session.note_state(index),
                        Some(beatbyte_core::session::NoteState::Pending)
                    )
                })
        });
        if !reopened {
            error!("autopilot: loop drill FAILED — no note in the section reopened");
            deliver(&mut app_exit, AppExit::error());
            *done = true;
            return;
        }
        if *wraps >= 2 {
            info!("autopilot: loop drill PASSED — wrapped twice, section notes reopened");
            deliver(&mut app_exit, AppExit::Success);
            *done = true;
        }
    }
    *last_now = Some(now);
}

/// The cameras that share the primary window: the 2D camera and the
/// 3D stage camera (their HDR settings must agree, or the HDR
/// camera's pass is silently dropped).
type WindowCameras = bevy::prelude::Or<(
    bevy::prelude::With<bevy::camera::Camera2d>,
    (
        bevy::prelude::With<bevy::camera::Camera3d>,
        bevy::prelude::With<crate::gameplay::stage3d::Stage3d>,
    ),
)>;

/// `BEATBYTE_AUTOPILOT_PAUSE`: exercise the pause menu with real
/// keys mid-song. Escape pauses, ArrowDown reaches the SFX row, two
/// ArrowLefts and two ArrowRights step the volume down and back up
/// (checked against the exact clamp model after every leg), Escape
/// resumes. Every checkpoint mismatch fails the run loudly; the
/// normal end-of-song verdict then proves the round-trip cost
/// nothing.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn autopilot_pause(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    phase: Res<State<crate::states::GamePhase>>,
    overlays: Query<&bevy::ui::ComputedNode, With<crate::gameplay::PauseOverlay>>,
    cameras_2d: Query<&bevy::camera::Camera, With<bevy::camera::Camera2d>>,
    hdr_cameras: Query<Has<bevy::camera::Hdr>, WindowCameras>,
    stage_cameras: Query<
        (),
        (
            With<bevy::camera::Camera3d>,
            With<crate::gameplay::stage3d::Stage3d>,
        ),
    >,
    settings: Res<crate::config::Settings>,
    time: Res<Time>,
    mut warmup: Local<f32>,
    mut frame: Local<u32>,
    mut baseline: Local<Option<f32>>,
    mut done: Local<bool>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if *done {
        return;
    }
    if baseline.is_none() {
        *warmup += time.delta_secs();
        if *warmup < 1.2 || *phase.get() != crate::states::GamePhase::Playing {
            return;
        }
        *baseline = Some(settings.sfx_volume);
        info!(
            "autopilot: pause drill starts at sfx {:.2}",
            settings.sfx_volume
        );
    }
    let Some(start) = *baseline else { return };
    let step = |value: f32, direction: f32| (0.1f32.mul_add(direction, value)).clamp(0.0, 1.0);
    let after_down = step(step(start, -1.0), -1.0);
    let after_up = step(step(after_down, 1.0), 1.0);
    // One scripted key per stride: press, release, then idle frames
    // so state transitions and menu systems settle in between. The
    // number of ArrowDowns comes from the MENU, not from a memory of
    // its layout — a drill with a hard-coded row index went stale
    // the day rows were inserted above its target.
    const STRIDE: u32 = 8;
    let downs = crate::gameplay::sfx_row_position();
    let mut script = vec![KeyCode::Escape];
    script.extend(std::iter::repeat_n(KeyCode::ArrowDown, downs));
    script.extend([
        KeyCode::ArrowLeft,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ArrowRight,
        KeyCode::Escape,
    ]);
    let after_down_check = 1 + downs + 1; // after the second LEFT
    let after_up_check = after_down_check + 2; // after the second RIGHT
    let resume_check = after_up_check + 1;
    let action = (*frame / STRIDE) as usize;
    let tick = *frame % STRIDE;
    if action >= script.len() {
        *done = true;
        info!(
            "autopilot: pause drill PASSED — sfx {start:.2} -> {after_down:.2} -> {:.2}, resumed",
            settings.sfx_volume
        );
        return;
    }
    match tick {
        0 => keys.press(script[action]),
        1 => keys.release(script[action]),
        _ if tick == STRIDE - 1 => {
            let mut fail = |what: String| {
                error!("autopilot: pause drill FAILED — {what}");
                deliver(&mut app_exit, AppExit::error());
            };
            match action {
                0 if *phase.get() != crate::states::GamePhase::Paused => {
                    fail("escape did not pause".to_owned());
                }
                // The menu must not just exist — it must LAY OUT. A
                // second on-screen camera with no marked UI default
                // once left every gameplay UI root at zero size:
                // entities present, drill green, player staring at
                // an invisible menu.
                0 if !overlays.iter().any(|node| node.size().x > 0.0) => {
                    fail("the pause overlay laid out to zero size — invisible menu".to_owned());
                }
                // Exactly one camera may clear the window: with the
                // 3D stage on screen the 2D camera must LOAD the
                // frame (else it wipes the whole stage — shipped
                // once, hidden behind the round style's HDR bloom),
                // and without it the 2D camera must clear again.
                // Cameras sharing one window must agree on HDR: a
                // mixed SDR/HDR pair silently drops the HDR pass —
                // the stage vanished under the 8-bit style exactly
                // so, hidden behind one settings combination.
                0 if {
                    let mut flags = hdr_cameras.iter();
                    let first = flags.next();
                    flags.any(|hdr| Some(hdr) != first)
                } =>
                {
                    fail(
                        "cameras disagree on HDR — the mixed pair drops the stage's pass"
                            .to_owned(),
                    );
                }
                0 if cameras_2d.iter().any(|camera| {
                    matches!(camera.clear_color, bevy::camera::ClearColorConfig::None)
                        != !stage_cameras.is_empty()
                }) =>
                {
                    fail(
                        "the 2D camera's clear does not match the one-camera-clears rule"
                            .to_owned(),
                    );
                }
                a if a == after_down_check && (settings.sfx_volume - after_down).abs() > 1e-4 => {
                    fail(format!(
                        "two LEFTs on the sfx row left {:.3}, expected {after_down:.3}",
                        settings.sfx_volume
                    ));
                }
                a if a == after_up_check && (settings.sfx_volume - after_up).abs() > 1e-4 => {
                    fail(format!(
                        "two RIGHTs on the sfx row left {:.3}, expected {after_up:.3}",
                        settings.sfx_volume
                    ));
                }
                a if a == resume_check && *phase.get() != crate::states::GamePhase::Playing => {
                    fail("escape did not resume".to_owned());
                }
                _ => {}
            }
        }
        _ => {}
    }
    *frame += 1;
}

/// Resolve the difficulty the autopilot plays.
///
/// `BEATBYTE_AUTOPILOT_DIFFICULTY` names it (`easy`/`medium`/`hard`/
/// `expert`); unset keeps the default. An unknown name or one the
/// selected song does not offer is a loud error — a harness that
/// silently plays the wrong difficulty validates nothing.
fn resolve_difficulty(
    wanted: Option<&str>,
    offered: &[beatbyte_core::Difficulty],
) -> Result<Option<beatbyte_core::Difficulty>, String> {
    let Some(wanted) = wanted else {
        return Ok(None);
    };
    let lowered = wanted.to_lowercase();
    let Some(difficulty) = beatbyte_core::Difficulty::ALL
        .iter()
        .copied()
        .find(|d| d.id() == lowered)
    else {
        return Err(format!(
            "unknown difficulty `{wanted}` (easy/medium/hard/expert)"
        ));
    };
    if !offered.contains(&difficulty) {
        return Err(format!("the selected song offers no {difficulty} chart"));
    }
    Ok(Some(difficulty))
}

/// Find a song by title: an exact match first, a substring second.
///
/// The exactness matters on a real library. Every `[GS]` twin's title
/// CONTAINS its original's, so a plain substring search can never
/// reach the original — on this machine's library that made the blind
/// test unrunnable for every song in it, because the only songs with
/// several chart versions are ones that also have a twin. Pure —
/// tested, in both directions.
#[must_use]
pub fn title_match<'a>(titles: impl Iterator<Item = &'a str>, target: &str) -> Option<usize> {
    let needle = target.to_lowercase();
    let lowered: Vec<String> = titles.map(str::to_lowercase).collect();
    lowered
        .iter()
        .position(|title| *title == needle)
        .or_else(|| lowered.iter().position(|title| title.contains(&needle)))
}

/// Resolve which library song the autopilot plays.
///
/// `BEATBYTE_AUTOPILOT_SONG` selects it: a number is an index into the
/// library, anything else a case-insensitive substring of the title
/// (first match in library order). Unset picks the first entry. A
/// selector that matches nothing is an error — a harness that silently
/// plays the wrong song validates nothing.
fn select_song<'a>(
    entries: &'a [crate::library::SongEntry],
    selector: Option<&str>,
) -> Result<&'a crate::library::SongEntry, String> {
    if entries.is_empty() {
        return Err("empty song library".to_owned());
    }
    let Some(selector) = selector else {
        return Ok(&entries[0]);
    };
    if let Ok(index) = selector.parse::<usize>() {
        return entries.get(index).ok_or_else(|| {
            format!(
                "song index {index} out of range (library has {} entr(ies))",
                entries.len()
            )
        });
    }
    title_match(entries.iter().map(|entry| entry.title.as_str()), selector)
        .and_then(|index| entries.get(index))
        .ok_or_else(|| {
            let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
            format!("no song title contains \"{selector}\" (library: {titles:?})")
        })
}

/// `BEATBYTE_AUTOPILOT_DROP=<path>`: simulate dropping that file onto
/// the window once the browser is up — the import pipeline (copy,
/// analyze, chart, rescan) runs exactly as for a human gesture.
fn autopilot_drop(
    mut sent: Local<bool>,
    mut drops: MessageWriter<bevy::window::FileDragAndDrop>,
    window: Query<Entity, With<bevy::window::PrimaryWindow>>,
) {
    if *sent {
        return;
    }
    let Some(path) = std::env::var_os("BEATBYTE_AUTOPILOT_DROP") else {
        *sent = true;
        return;
    };
    let Ok(window) = window.single() else {
        return;
    };
    *sent = true;
    // Newline-separated: a multi-file gesture arrives as several
    // events in ONE frame, exactly like a real drop of many files.
    for entry in path.to_string_lossy().split('\n') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        info!("autopilot: dropping {entry:?} onto the window");
        drops.write(bevy::window::FileDragAndDrop::DroppedFile {
            window,
            path_buf: std::path::PathBuf::from(entry),
        });
    }
}

/// The arrow key and the number of presses that move the browser's
/// cursor from `cursor` to `row`. Pure — tested.
///
/// ⚠️ Signed: a target ABOVE the cursor needs `ArrowUp`. A count that
/// saturates at zero instead leaves the cursor where it is and the
/// drill acts on whatever song sits there.
#[must_use]
pub fn arrows_to(row: usize, cursor: usize) -> (KeyCode, u32) {
    if row >= cursor {
        (KeyCode::ArrowDown, (row - cursor) as u32)
    } else {
        (KeyCode::ArrowUp, (cursor - row) as u32)
    }
}

/// How long a drill waits in the browser before its first key: the
/// screen's entry fade swallows keys pressed into it.
const BROWSER_SETTLE_S: f32 = 0.8;

/// What the delete drill fixed on its first frame.
#[derive(Default)]
struct DeletePlan {
    /// The song to delete, by where it lives — not by title.
    source: Option<crate::library::SongSource>,
    /// The key and presses that reach it from the cursor.
    arrows: Option<(KeyCode, u32)>,
}

/// `BEATBYTE_AUTOPILOT_DELETE=<title>`: arrow to the matching entry
/// with real key presses, ask with the real Backspace, answer with the
/// real `Y`, and succeed once the song left the library.
///
/// ⚠️ Three things the first version got wrong. The answer had become
/// `Y` (Backspace only ever ASKS since a player deleted songs while
/// clearing text — `song_select::delete_step`), and the drill still
/// pressed Backspace twice: it could not have passed since. The other
/// two are fixed the way the align drill does it. It counted arrows in LIBRARY order, but the
/// browser shows the library sorted, so it deleted whichever song sat
/// that many rows down. And it asked "is it gone?" by title — after
/// deleting "Maria" a title search still finds "[GS] Maria", and the
/// drill would never see the deletion it made. The target is now
/// fixed on the first frame by its SOURCE, and the rows come from the
/// browser's own order.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn autopilot_delete(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    library: Res<crate::library::SongLibrary>,
    view: Res<crate::song_select::BrowserView>,
    cursor: Res<crate::song_select::BrowserCursor>,
    time: Res<Time>,
    mut frame: Local<u32>,
    mut plan: Local<DeletePlan>,
    mut waited: Local<f32>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let Some(target) = std::env::var("BEATBYTE_AUTOPILOT_DELETE").ok() else {
        return;
    };
    *waited += time.delta_secs();
    if *waited > 30.0 {
        error!(
            "autopilot: delete timed out (target still present: {:?})",
            plan.source
        );
        deliver(&mut app_exit, AppExit::error());
        return;
    }
    if *waited < BROWSER_SETTLE_S {
        return;
    }
    if plan.source.is_none() {
        let Some(index) = title_match(
            library.entries.iter().map(|entry| entry.title.as_str()),
            &target,
        ) else {
            error!("autopilot: no song matching `{target}` to delete");
            deliver(&mut app_exit, AppExit::error());
            return;
        };
        let row = view.order.iter().position(|&i| i == index).unwrap_or(0);
        plan.source = Some(library.entries[index].source.clone());
        plan.arrows = Some(arrows_to(row, cursor.0));
    }
    let present = library
        .entries
        .iter()
        .any(|entry| Some(&entry.source) == plan.source.as_ref());
    if !present {
        info!("autopilot: delete validation PASSED");
        deliver(&mut app_exit, AppExit::Success);
        return;
    }
    let Some((arrow, presses)) = plan.arrows else {
        return;
    };
    // Alternate press/release frames: the arrows to reach the entry,
    // then Backspace to ask, a short pause, `Y` to answer.
    let step = *frame / 2;
    let pressing = (*frame).is_multiple_of(2);
    let key = if step < presses {
        Some(arrow)
    } else if step == presses {
        Some(KeyCode::Backspace)
    } else if step == presses + 8 {
        Some(KeyCode::KeyY)
    } else {
        None
    };
    if let Some(key) = key {
        if pressing {
            keys.press(key);
        } else {
            keys.release(key);
        }
    }
    *frame += 1;
}

/// `BEATBYTE_AUTOPILOT_ALIGN=<title-substring>`: arrow down to the
/// matching entry with real key presses, press the real `K`, and
/// succeed once the status row reports the alignment written AND
/// the `words.json` sits beside the song's audio. Every refusal the
/// browser would show a player fails the run with that same line.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn autopilot_align(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    library: Res<crate::library::SongLibrary>,
    view: Res<crate::song_select::BrowserView>,
    cursor: Res<crate::song_select::BrowserCursor>,
    smart: Res<crate::smart_lyrics::SmartLyrics>,
    status: Res<crate::import::ImportStatus>,
    time: Res<Time>,
    mut frame: Local<u32>,
    mut downs: Local<Option<u32>>,
    mut waited: Local<f32>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let Some(target) = std::env::var("BEATBYTE_AUTOPILOT_ALIGN").ok() else {
        return;
    };
    let Some(index) = title_match(
        library.entries.iter().map(|entry| entry.title.as_str()),
        &target,
    ) else {
        error!("autopilot: no song matching `{target}` to align");
        deliver(&mut app_exit, AppExit::error());
        return;
    };
    *waited += time.delta_secs();
    if *waited > 240.0 {
        error!("autopilot: alignment timed out (status: {})", status.0);
        deliver(&mut app_exit, AppExit::error());
        return;
    }
    // The browser shows the library SORTED: the arrow count is the
    // song's row in the view minus where the cursor already sits,
    // fixed on the first frame (the cursor moves under the presses).
    let downs = *downs.get_or_insert_with(|| {
        let row = view.order.iter().position(|&i| i == index).unwrap_or(0);
        row.saturating_sub(cursor.0) as u32
    });
    let step = *frame / 2;
    let pressing = (*frame).is_multiple_of(2);
    if step < downs {
        if pressing {
            keys.press(KeyCode::ArrowDown);
        } else {
            keys.release(KeyCode::ArrowDown);
        }
    } else if step == downs {
        if pressing {
            keys.press(KeyCode::KeyK);
        } else {
            keys.release(KeyCode::KeyK);
        }
    } else if step > downs + 2 && !smart.is_aligning() {
        // K was pressed and nothing runs: either it finished, or it
        // was refused. The status row says which.
        if status.0.starts_with("aligned ") {
            let written = match &library.entries[index].source {
                crate::library::SongSource::File { audio_path, .. } => {
                    beatbyte_chart::lyrics::words_path(audio_path).is_file()
                }
                crate::library::SongSource::Builtin(_) => false,
            };
            if written {
                info!("autopilot: alignment PASSED — {}", status.0);
                deliver(&mut app_exit, AppExit::Success);
            } else {
                error!("autopilot: status says aligned but no words.json beside the song");
                deliver(&mut app_exit, AppExit::error());
            }
        } else {
            error!("autopilot: alignment did not run — {}", status.0);
            deliver(&mut app_exit, AppExit::error());
        }
    }
    *frame += 1;
}

/// `BEATBYTE_AUTOPILOT_MODEL=1`: open the settings screen, arrow
/// down to the LYRICS MODEL row with real keys, press the real Enter
/// when the row says the model is missing, and succeed once it says
/// INSTALLED. Run under a scratch `HOME` to exercise the download
/// itself (378 MB from the release asset); with the model already
/// there it passes on the probe alone.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn autopilot_model(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    state: Res<State<AppState>>,
    mut next_state: ResMut<NextState<AppState>>,
    smart: Res<crate::smart_lyrics::SmartLyrics>,
    time: Res<Time>,
    mut frame: Local<u32>,
    mut waited: Local<f32>,
    mut settled: Local<f32>,
    mut pressed_enter: Local<bool>,
    mut app_exit: MessageWriter<AppExit>,
) {
    use crate::smart_lyrics::ModelState;
    *waited += time.delta_secs();
    if *waited > 600.0 {
        error!(
            "autopilot: model download timed out ({})",
            smart.model_text()
        );
        deliver(&mut app_exit, AppExit::error());
        return;
    }
    if *state.get() != AppState::Settings {
        if *state.get() == AppState::MainMenu && *waited > 1.0 {
            next_state.set(AppState::Settings);
        }
        return;
    }
    // Keys pressed into the screen's entry fade are dropped: the
    // first run of this drill arrowed into the void and confirmed a
    // row it never meant to. Wait it out, as the screenshot harness
    // does.
    *settled += time.delta_secs();
    if *settled < 0.8 {
        return;
    }
    // Enter is released whatever the state has become since the
    // press (the confirm turns the row to DOWNLOADING in the same
    // frame).
    if keys.pressed(KeyCode::Enter) {
        keys.release(KeyCode::Enter);
        if !*pressed_enter {
            *pressed_enter = true;
            info!("autopilot: confirmed the LYRICS MODEL row");
        }
        return;
    }
    match &smart.model {
        ModelState::NotInBuild => {
            error!("autopilot: this build has no aligner (build with --features ml)");
            deliver(&mut app_exit, AppExit::error());
        }
        ModelState::Installed => {
            info!("autopilot: model PASSED — {}", smart.model_text());
            deliver(&mut app_exit, AppExit::Success);
        }
        ModelState::Failed(reason) => {
            error!("autopilot: model download failed — {reason}");
            deliver(&mut app_exit, AppExit::error());
        }
        ModelState::Missing | ModelState::Damaged if !*pressed_enter => {
            let downs = crate::settings_ui::Row::LyricsModel.index() as u32;
            let step = *frame / 2;
            let pressing = (*frame).is_multiple_of(2);
            if step < downs {
                if pressing {
                    keys.press(KeyCode::ArrowDown);
                } else {
                    keys.release(KeyCode::ArrowDown);
                }
            } else if pressing {
                keys.press(KeyCode::Enter);
            }
            *frame += 1;
        }
        _ => {}
    }
}

fn autopilot_reset(mut hands: ResMut<AutopilotHands>) {
    *hands = AutopilotHands::default();
}

/// The MC set swaps songs WITHOUT re-entering the gameplay state, so
/// the `OnEnter` reset above never fires — the hands kept feeding
/// song A's consumed plan into song B and missed all 81 of its
/// notes (found by the first MC verification run).
fn autopilot_reset_on_swap(
    mut swaps: MessageReader<crate::mc::McSwapped>,
    mut hands: ResMut<AutopilotHands>,
) {
    if swaps.read().count() > 0 {
        *hands = AutopilotHands::default();
    }
}

/// Play through the real keyboard: the default bindings (A S D F G +
/// arrow-down strum, Space for Hype) are pressed and released on
/// `ButtonInput<KeyCode>` at the notes' times. One event per frame —
/// a lag spike shifts a stamp into Great territory, never into a
/// phantom input.
fn autopilot_key_play(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    players: Query<&PlayerSession>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut cursor: Local<usize>,
    mut strum_hot: Local<bool>,
    mut started: Local<bool>,
) {
    const FRETS: [KeyCode; 5] = [
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
        KeyCode::KeyF,
        KeyCode::KeyG,
    ];
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    let Some(player) = players.iter().next() else {
        return;
    };
    // `BEATBYTE_AUTOPILOT_NO_STRUM=1`: fret presses only — proves tap
    // mode end to end (and, without tap mode, that strums are truly
    // required). With tap mode ACTIVE the injector never strums on
    // its own either: the fret press already hits, and the strum on
    // top would be a phantom overstrum (seen: 106 of them).
    let strum =
        std::env::var_os("BEATBYTE_AUTOPILOT_NO_STRUM").is_none() && !player.session.tap_mode();
    if !*started {
        *cursor = 0;
        *started = true;
        info!(
            "autopilot: key-play active (strum={strum}, tap_mode={})",
            player.session.tap_mode()
        );
    }
    // Strum released the frame after it was pressed, so the next
    // press registers as a fresh just_pressed. Space IS the strum
    // key now (two-hand keyboard split); Hype moved to Enter.
    if *strum_hot {
        keys.release(KeyCode::Space);
        *strum_hot = false;
    }
    if keys.pressed(KeyCode::Enter) {
        keys.release(KeyCode::Enter);
    }

    // Press slightly EARLY: the stamp lands a frame-quantum before
    // the note, comfortably inside Perfect, and a scheduler hitch has
    // that much more slack before the hit slides out of the window.
    const PRESS_LEAD_S: f64 = 0.02;
    let events = player.session.track().events();
    if *cursor >= events.len() {
        return;
    }
    let event = events[*cursor];
    if event.time_s - PRESS_LEAD_S > now {
        return;
    }
    // Skip events the session already resolved, and events a hitch
    // pushed out of the hit window — WITHOUT strumming, or the miss
    // (honest) gains a phantom overstrum (an injector artifact).
    let window = player.session.windows().good_s;
    if !matches!(player.session.note_state(*cursor), Some(NoteState::Pending))
        || now - event.time_s > window
    {
        *cursor += 1;
        return;
    }
    for (index, key) in FRETS.iter().enumerate() {
        let lane = beatbyte_core::Lane::from_index(index).map(|l| event.lanes.contains(l));
        match lane {
            Some(true) => {
                // Tap mode hits on the press EDGE — a still-held key
                // from the previous note has no edge left, so re-tap
                // (release + press in one frame = a fresh press).
                // With a strum that edge is irrelevant, and releasing
                // would cut a running sustain.
                if !strum && keys.pressed(*key) {
                    keys.release(*key);
                }
                keys.press(*key);
            }
            _ => {
                if keys.pressed(*key) {
                    keys.release(*key);
                }
            }
        }
    }
    if strum {
        keys.press(KeyCode::Space);
        *strum_hot = true;
    }
    if player.session.performance().hype_meter()
        >= player
            .session
            .performance()
            .config()
            .hype_activation_threshold
    {
        keys.press(KeyCode::Enter);
    }
    *cursor += 1;
}

/// How much further than the frame itself song time may step before
/// the autopilot calls it a teleport. Practice speed runs up to
/// 150 %, and a reconcile snap corrects tens of milliseconds; half a
/// second past the frame is neither.
const TELEPORT_S: f64 = 0.5;

/// Whether a song-time step of `advanced` seconds over a frame of
/// `frame_s` wall seconds is a teleport. Pure — tested.
#[must_use]
pub fn teleported(advanced: f64, frame_s: f64) -> bool {
    advanced > frame_s.max(0.0) * 1.5 + TELEPORT_S
}

/// Play every note event exactly on time through the real session
/// API — for every player.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn autopilot_play(
    mut players: Query<(&crate::gameplay::PlayerIndex, &mut PlayerSession)>,
    mut hands: ResMut<AutopilotHands>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    fail_drill: Option<Res<FailDrill>>,
    // The injector owns the session while it plays, so the human
    // input path never sees these: without recording here, the action
    // stream would be the one part of the blackbox no harness run
    // exercises. Optional so a headless drill needs no store.
    store: Option<Res<crate::telemetry::TelemetryStore>>,
    run: Option<Res<crate::telemetry::StoreRun>>,
    mut last_now: Local<Option<f64>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    // The fail drill plays nothing: every note is missed on purpose.
    if fail_drill.is_some() {
        return;
    }
    let Some(now) = game_clock.song_time(&time) else {
        *last_now = None;
        return;
    };
    // The autopilot hits by STAMP, so a clock that teleports forward
    // is invisible to the verdict: every note before the landing
    // point is played in that one frame, perfectly, and the run
    // passes with a highway that stayed empty for three minutes.
    // That is exactly how the count-in teleport shipped. A backward
    // jump is a loop wrap or an MC handover and stays legal — and so
    // is a long FRAME: the clock is wall time, so a one-second stall
    // moves song time one second, which is not a teleport. Measured
    // against the frame's own length, not against a constant.
    if let Some(last) = *last_now
        && teleported(now - last, time.delta_secs_f64())
    {
        error!(
            "autopilot: song time jumped {last:.3} -> {now:.3} in one frame — the clock teleported"
        );
        deliver(&mut app_exit, AppExit::error());
    }
    *last_now = Some(now);
    hands.ensure(players.iter().count());
    let store = store.as_deref();
    let run = run.as_deref();

    for (index, mut player) in &mut players {
        let slot = index.0;
        let recorded_slot = u8::try_from(slot).unwrap_or(u8::MAX);
        let player = &mut *player;
        while hands.next_event[slot] < player.session.track().events().len() {
            let event_index = hands.next_event[slot];
            let event = player.session.track().events()[event_index];
            if event.time_s > now {
                break;
            }
            if !matches!(
                player.session.note_state(event_index),
                Some(NoteState::Pending)
            ) {
                hands.next_event[slot] += 1;
                continue;
            }
            let stamp = event.time_s;
            // Release frets not needed anymore, press the event's frets.
            for lane in hands.held[slot].iter() {
                if !event.lanes.contains(lane) {
                    crate::telemetry::record_action(
                        store,
                        run,
                        recorded_slot,
                        InputKind::FretUp(lane),
                        stamp,
                    );
                    player.session.handle(
                        GameInput {
                            time_s: stamp,
                            kind: InputKind::FretUp(lane),
                        },
                        &mut player.frame_events,
                    );
                }
            }
            for lane in event.lanes.iter() {
                if !hands.held[slot].contains(lane) {
                    crate::telemetry::record_action(
                        store,
                        run,
                        recorded_slot,
                        InputKind::FretDown(lane),
                        stamp,
                    );
                    player.session.handle(
                        GameInput {
                            time_s: stamp,
                            kind: InputKind::FretDown(lane),
                        },
                        &mut player.frame_events,
                    );
                }
            }
            hands.held[slot] = event.lanes;
            // In tap mode the fret presses above already hit the
            // note (the session would absorb one strum on top, but
            // there is nothing to gain). Only strum while the event
            // is still pending (which also stays correct for HOPOs
            // in classic mode).
            if matches!(
                player.session.note_state(event_index),
                Some(NoteState::Pending)
            ) {
                crate::telemetry::record_action(store, run, recorded_slot, InputKind::Strum, stamp);
                player.session.handle(
                    GameInput {
                        time_s: stamp,
                        kind: InputKind::Strum,
                    },
                    &mut player.frame_events,
                );
            }
            // Fire Hype the moment it becomes available.
            if player.session.performance().hype_meter()
                >= player
                    .session
                    .performance()
                    .config()
                    .hype_activation_threshold
            {
                crate::telemetry::record_action(
                    store,
                    run,
                    recorded_slot,
                    InputKind::ActivateHype,
                    stamp,
                );
                player.session.handle(
                    GameInput {
                        time_s: stamp,
                        kind: InputKind::ActivateHype,
                    },
                    &mut player.frame_events,
                );
            }
            hands.next_event[slot] += 1;
        }
    }
}

/// Switch No Fail off in memory for the fail drill. Startup runs long
/// before a session is built, and nothing on the autopilot's path
/// persists settings, so the user's own No Fail is untouched on disk.
fn arm_failure_for_drill(mut settings: ResMut<crate::config::Settings>) {
    settings.no_fail = false;
    info!("autopilot: fail drill — No Fail off for this run, no inputs will be played");
}

/// Log the outcome and exit.
fn autopilot_results(
    time: Res<Time>,
    mut hands: ResMut<AutopilotHands>,
    results: Option<Res<LastResults>>,
    fail_drill: Option<Res<FailDrill>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    hands.results_time += time.delta_secs();
    if hands.results_time < 1.0 {
        return;
    }
    let Some(results) = results else {
        error!("autopilot: reached results without a LastResults resource");
        deliver(&mut app_exit, AppExit::error());
        return;
    };
    if results.players.is_empty() {
        error!("autopilot: results carry no players");
        deliver(&mut app_exit, AppExit::error());
        return;
    }
    if fail_drill.is_some() {
        // The inverted verdict: the run must have FAILED, the results
        // must say so, and the history's newest line must not call it
        // completed. The line is read back from disk, because the
        // in-memory completion marker is consumed on the way out of
        // gameplay and would be absent either way here.
        let perf = &results.players[0].performance;
        let logged_incomplete = crate::history::load()
            .last()
            .is_some_and(|entry| !entry.completed && entry.title == results.title);
        let ok = results.failed && perf.failed() && perf.meter() <= 0.0 && logged_incomplete;
        if ok {
            info!(
                "autopilot: fail drill PASSED — meter empty after {} misses, run marked failed, \
                 logged as not completed",
                perf.counts().miss
            );
            deliver(&mut app_exit, AppExit::Success);
        } else {
            error!(
                "autopilot: fail drill FAILED — failed={} perf.failed={} meter={:.2} logged_incomplete={}",
                results.failed,
                perf.failed(),
                perf.meter(),
                logged_incomplete
            );
            deliver(&mut app_exit, AppExit::error());
        }
        return;
    }
    let mut all_ok = true;
    // The song has to have actually PLAYED. Judgment alone cannot say
    // so: the injector stamps its inputs with each note's own time, so
    // it scores a flawless run even against a clock that jumped to
    // the end — which is precisely how a browser preview's position
    // once ended a 63-second song ten milliseconds in, with 98
    // perfects and a PASSED verdict (v0.14.2).
    if !song_ended_sanely(results.finished_at_s, results.content_end_s) {
        error!(
            "autopilot: the song ended at {:.1}s but its content ends at {:.1}s — \
             the clock did not follow the music",
            results.finished_at_s, results.content_end_s
        );
        all_ok = false;
    }
    for player in &results.players {
        let perf = &player.performance;
        let counts = perf.counts();
        info!(
            "autopilot: P{} \"{}\" ({}) — score {}, accuracy {:.1}%, \
             streak {}, perfect {}, great {}, good {}, miss {}, overstrums {}",
            player.index + 1,
            results.title,
            results.difficulty,
            perf.score(),
            perf.accuracy() * 100.0,
            perf.best_streak(),
            counts.perfect,
            counts.great,
            counts.good,
            counts.miss,
            perf.overstrums()
        );
        // A perfect autopilot must produce a perfect run; anything
        // else is a gameplay bug worth failing loudly over.
        all_ok &= counts.miss == 0 && perf.overstrums() == 0 && counts.total() > 0;
    }
    if all_ok {
        info!("autopilot: run PASSED");
        deliver(&mut app_exit, AppExit::Success);
    } else {
        error!("autopilot: run FAILED — misses or overstrums in a perfect run");
        deliver(&mut app_exit, AppExit::error());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::shot_state;
    use super::title_match;
    use crate::states::AppState;

    /// ⚠️ The drills count rows in the browser's order FROM the
    /// cursor, and a target above it is reached going up. A count that
    /// saturated at zero left the cursor where it was, and the delete
    /// drill would have deleted whatever song sat there.
    #[test]
    fn the_arrows_reach_a_row_from_the_cursor_either_way() {
        use bevy::input::keyboard::KeyCode;
        assert_eq!(super::arrows_to(5, 2), (KeyCode::ArrowDown, 3));
        assert_eq!(super::arrows_to(2, 5), (KeyCode::ArrowUp, 3));
        assert_eq!(super::arrows_to(4, 4), (KeyCode::ArrowDown, 0));
    }

    #[test]
    fn an_exact_title_beats_a_twin_that_merely_contains_it() {
        // Every `[GS]` twin's title CONTAINS its original's, and the
        // twins come first in library order — so a plain substring
        // search could never reach the original. On the author's
        // library that made the blind test unrunnable for every song
        // in it, because the only songs with several chart versions
        // are exactly the ones that also have a twin.
        let titles = ["[GS] Life Is a Flower", "Life Is a Flower", "Lifeboat"];
        assert_eq!(
            title_match(titles.into_iter(), "Life Is a Flower"),
            Some(1),
            "an exact title is the one that was asked for"
        );
        // …and a substring still works when nothing matches exactly.
        assert_eq!(title_match(titles.into_iter(), "flower"), Some(0));
        assert_eq!(title_match(titles.into_iter(), "boat"), Some(2));
        assert_eq!(title_match(titles.into_iter(), "nothing here"), None);
        // Case is not part of the question.
        assert_eq!(title_match(titles.into_iter(), "LIFE IS A FLOWER"), Some(1));
    }

    #[test]
    fn the_injector_owns_the_inputs_unless_the_autopilot_plays_by_key() {
        // The marker in the world is the whole mechanism, so the test
        // reads the world rather than the predicate.
        let owned = |enabled, key_play| {
            let mut app = bevy::app::App::new();
            super::own_inputs(&mut app, enabled, key_play);
            app.world()
                .get_resource::<super::InjectorOwnsInput>()
                .is_some()
        };
        assert!(
            owned(true, false),
            "the note injector plays every note itself: the desk must not join in"
        );
        assert!(
            !owned(true, true),
            "key-play IS the keyboard path — muting it would mute the drill"
        );
        assert!(
            !owned(false, false),
            "without the autopilot the player owns their own inputs"
        );
        assert!(
            !owned(false, true),
            "and the key-play switch alone means nothing"
        );
    }

    #[test]
    fn shot_times_parse_leniently() {
        assert_eq!(
            super::parse_shot_times("17.5, 39,x,-2,inf"),
            vec![17.5, 39.0]
        );
        assert!(super::parse_shot_times("").is_empty());
    }

    #[test]
    fn moments_a_tenth_apart_each_get_their_own_frame() {
        // Photographing an impulse: three moments inside one window.
        let times = [18.80, 18.87, 19.00];
        let mut shot: Vec<f64> = Vec::new();
        let mut frames = Vec::new();
        // One frame every 10 ms across the impulse.
        for step in 0..30u16 {
            let now = 18.78 + f64::from(step) * 0.01;
            if let Some(at) = super::next_shot(&times, now, |at| shot.contains(&at)) {
                shot.push(at);
                frames.push(at);
            }
        }
        assert_eq!(
            frames,
            vec![18.80, 18.87, 19.00],
            "a moment already photographed must step aside, or the first \
             one swallows the second's window for a whole second"
        );
    }

    #[test]
    fn a_moment_is_claimable_for_a_second_and_not_before_its_time() {
        let times = [30.0];
        assert_eq!(super::next_shot(&times, 29.99, |_| false), None);
        assert_eq!(super::next_shot(&times, 30.0, |_| false), Some(30.0));
        assert_eq!(super::next_shot(&times, 30.99, |_| false), Some(30.0));
        assert_eq!(super::next_shot(&times, 31.01, |_| false), None);
    }

    #[test]
    fn shot_state_reaches_every_screen_the_autopilot_cannot() {
        // These four are the reason the hook exists: no automated run
        // visits them, so nothing would notice them breaking.
        assert_eq!(shot_state("settings"), Some(AppState::Settings));
        assert_eq!(shot_state("controls"), Some(AppState::Controls));
        assert_eq!(shot_state("calibration"), Some(AppState::Calibration));
        assert_eq!(shot_state("inputtest"), Some(AppState::InputTest));
    }

    #[test]
    fn shot_state_is_forgiving_about_spelling() {
        for spelling in ["INPUT-TEST", "input_test", "Input Test", "inputtest"] {
            assert_eq!(
                shot_state(spelling),
                Some(AppState::InputTest),
                "`{spelling}` should resolve"
            );
        }
    }

    #[test]
    fn an_unknown_screen_is_rejected_rather_than_guessed() {
        // Silently falling back to the main menu would photograph the
        // wrong screen and look like a pass.
        assert_eq!(shot_state("gameplay"), None);
        assert_eq!(shot_state(""), None);
    }

    use super::select_song;
    use crate::library::{SongEntry, SongSource};

    fn entry(title: &str) -> SongEntry {
        SongEntry {
            loudness: None,
            title: title.to_owned(),
            artist: "Tests".to_owned(),
            bpm: 120.0,
            duration_s: None,
            difficulties: vec![],
            note_counts: vec![],
            genre: None,
            preview_start_s: None,
            source: SongSource::Builtin(0),
            has_lyrics: false,
            polish: crate::library::Polish::default(),
            song_id: None,
        }
    }

    #[test]
    fn a_teleport_is_a_jump_the_frame_cannot_explain() {
        use super::teleported;
        // A normal frame, a fast frame, practice at 150 %: fine.
        assert!(!teleported(0.016, 0.016));
        assert!(!teleported(0.025, 0.016));
        // A one-second STALL moves song time one second: not a
        // teleport, the clock is wall time (this fired on a real run
        // at 26.27 -> 27.28 before the frame was taken into account).
        assert!(!teleported(1.01, 1.0));
        // A reconcile snap of a few dozen milliseconds: fine.
        assert!(!teleported(0.09, 0.016));
        // The count-in teleport: −2 → 185.6 in a 16 ms frame.
        assert!(teleported(187.6, 0.016));
        // Backward is never a teleport (loop wrap, MC handover).
        assert!(!teleported(-250.0, 0.016));
    }

    #[test]
    fn no_selector_picks_the_first_entry() {
        let entries = vec![entry("Circuit Breaker"), entry("Solder Groove")];
        assert_eq!(
            select_song(&entries, None).unwrap().title,
            "Circuit Breaker"
        );
    }

    #[test]
    fn numeric_selector_is_an_index() {
        let entries = vec![entry("Circuit Breaker"), entry("Solder Groove")];
        assert_eq!(
            select_song(&entries, Some("1")).unwrap().title,
            "Solder Groove"
        );
        assert!(select_song(&entries, Some("7")).is_err());
    }

    #[test]
    fn text_selector_matches_title_substring_case_insensitively() {
        let entries = vec![entry("Circuit Breaker"), entry("Solder Groove")];
        assert_eq!(
            select_song(&entries, Some("GROOVE")).unwrap().title,
            "Solder Groove"
        );
        assert!(select_song(&entries, Some("free bird")).is_err());
    }

    #[test]
    fn empty_library_is_an_error_even_without_selector() {
        assert!(select_song(&[], None).is_err());
    }

    #[test]
    fn difficulty_switch_resolves_or_fails_loudly() {
        use super::resolve_difficulty;
        use beatbyte_core::Difficulty;
        let offered = [Difficulty::Medium, Difficulty::Hard];
        assert_eq!(resolve_difficulty(None, &offered), Ok(None));
        assert_eq!(
            resolve_difficulty(Some("HARD"), &offered),
            Ok(Some(Difficulty::Hard))
        );
        // A difficulty the song does not offer, and a name that is
        // no difficulty at all: both loud errors, never a silent
        // fallback to the default.
        assert!(resolve_difficulty(Some("expert"), &offered).is_err());
        assert!(resolve_difficulty(Some("banana"), &offered).is_err());
    }
}

#[cfg(test)]
mod end_tests {
    use super::song_ended_sanely;

    #[test]
    fn a_song_that_never_played_is_not_a_clean_run() {
        // The healthy shape: the end check fires a little past the
        // content.
        assert!(song_ended_sanely(64.8, 63.3));
        assert!(song_ended_sanely(63.3, 63.3));
        // The v0.14.2 defect, exactly as measured: the clock sat
        // three minutes inside another track.
        assert!(!song_ended_sanely(185.6, 63.3));
        assert!(!song_ended_sanely(600.0, 120.0));
    }
}
