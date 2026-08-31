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
        app.insert_resource(Autopilot { enabled })
            .init_resource::<AutopilotHands>();

        // Screen photography works without the autopilot: the screens
        // it cannot reach are exactly the ones that need it.
        if let Ok(raw) = std::env::var("BEATBYTE_SHOT_STATE") {
            match shot_state(&raw) {
                Some(target) => {
                    app.insert_resource(ShotState(target))
                        .add_systems(Update, (enter_shot_state, quit_after_shot));
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
        if enabled {
            app.add_systems(
                Update,
                (
                    autopilot_menu.run_if(in_state(AppState::MainMenu)),
                    autopilot_song_select.run_if(in_state(AppState::SongSelect)),
                    autopilot_edit.run_if(in_state(AppState::Editor)),
                    autopilot_results.run_if(in_state(AppState::Results)),
                    autopilot_drop.run_if(in_state(AppState::SongSelect)),
                    fail_if_window_vanishes,
                ),
            )
            .add_systems(OnEnter(AppState::Gameplay), autopilot_reset);
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
                );
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
            if std::env::var_os("BEATBYTE_AUTOPILOT_KEYS").is_some() {
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
fn enter_shot_state(
    target: Res<ShotState>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut cursor: ResMut<crate::song_select::BrowserCursor>,
    mut view: ResMut<crate::song_select::BrowserView>,
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
    next.set(target.0);
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
            if hype {
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
    muted: Res<crate::mute::Muted>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    *delay += time.delta_secs();
    if *delay > 0.8 {
        *delay = 0.0;
        // Respect the LIVE mute state — the watcher may have toggled
        // it mid-run (the env var only seeds it).
        music.0.set_volume(0.5 * muted.factor());
        info!("autopilot: opening song select");
        next_state.set(AppState::SongSelect);
    }
}

/// In editor-validation mode (`BEATBYTE_AUTOPILOT_EDIT=1`), drive the
/// real editor: add a note, undo, redo, save, verify, exit.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn autopilot_edit(
    time: Res<Time>,
    mut delay: Local<f32>,
    mut edits_done: Local<bool>,
    state: Option<ResMut<crate::editor_ui::EditorState>>,
    music: Res<crate::audio_sys::Music>,
    muted: Res<crate::mute::Muted>,
    mut game_clock: ResMut<crate::audio_sys::GameClock>,
    clicks: Res<crate::editor_ui::AuditionClicks>,
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
        music.0.stop();
        game_clock.clock.stop();
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
    ok &= state.session.is_valid();
    ok &= beatbyte_chart::save_chart_file(&state.chart_path, state.session.chart()).is_ok();
    state.session.mark_saved();
    // Verify on disk.
    ok &= beatbyte_chart::load_chart_file(&state.chart_path)
        .ok()
        .and_then(|chart| chart.chart_for(difficulty).map(|d| d.notes.len()))
        == after_redo;
    if ok {
        // Edits verified; start the audition (preview from cursor)
        // and let phase 2 assert the metronome overlay.
        music.0.set_volume(0.3 * muted.factor());
        music.0.play_file(state.audio_path.clone());
        music.0.seek_s(state.cursor_s);
        game_clock
            .clock
            .start(time.elapsed_secs_f64(), state.cursor_s);
        state.previewing = true;
        *edits_done = true;
    } else {
        error!("autopilot: editor validation FAILED");
        deliver(&mut app_exit, AppExit::error());
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
    mut next_state: ResMut<NextState<AppState>>,
) {
    *delay += time.delta_secs();
    if *delay > 0.6 {
        *delay = 0.0;
        // Editor mode: open the first file-based song instead.
        if std::env::var_os("BEATBYTE_AUTOPILOT_EDIT").is_some() {
            let file_entry = library.entries.iter().find_map(|entry| {
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
            match crate::editor_ui::open_editor(&mut commands, &chart_path, &audio_path, difficulty)
            {
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
        // Deletion mode owns the browser — never start a song.
        if std::env::var_os("BEATBYTE_AUTOPILOT_DELETE").is_some() {
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

/// `BEATBYTE_AUTOPILOT_RATE=<1-5>`: on the results screen, press the
/// real digit key (and ArrowRight when the played chart carries
/// provenance), then parse the session log back and verify the
/// feedback lines actually landed. Failing to find them fails the
/// run loudly.
fn autopilot_rate(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    logs: Option<Res<crate::telemetry::SessionLogFiles>>,
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
            let Some(logs) = logs.as_ref().filter(|l| !l.files.is_empty()) else {
                error!("autopilot: rate drill FAILED — no session log to rate into");
                deliver(&mut app_exit, AppExit::error());
                *frame += 1;
                return;
            };
            for path in &logs.files {
                let content = std::fs::read_to_string(path).unwrap_or_default();
                let Some((_, lines)) = beatbyte_core::telemetry::parse_session(&content) else {
                    error!(
                        "autopilot: rate drill FAILED — {} unparseable",
                        path.display()
                    );
                    deliver(&mut app_exit, AppExit::error());
                    *frame += 1;
                    return;
                };
                let fun_ok = lines.iter().any(|line| {
                    matches!(line, beatbyte_core::telemetry::NoteLine::Fun { fun } if *fun == rating)
                });
                let versus_ok = !has_parent
                    || lines.iter().any(|line| {
                        matches!(
                            line,
                            beatbyte_core::telemetry::NoteLine::Versus { versus, .. }
                                if versus == "better"
                        )
                    });
                if !fun_ok || !versus_ok {
                    error!(
                        "autopilot: rate drill FAILED — {} lacks fun={fun_ok} versus={versus_ok}",
                        path.display()
                    );
                    deliver(&mut app_exit, AppExit::error());
                    *frame += 1;
                    return;
                }
            }
            info!(
                "autopilot: rate drill PASSED — fun {rating} (versus: {}) landed in {} log(s)",
                if has_parent { "better" } else { "no parent" },
                logs.files.len()
            );
        }
        _ => {}
    }
    *frame += 1;
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
    // so state transitions and menu systems settle in between.
    const STRIDE: u32 = 8;
    let script = [
        KeyCode::Escape,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ArrowRight,
        KeyCode::Escape,
    ];
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
                3 if (settings.sfx_volume - after_down).abs() > 1e-4 => fail(format!(
                    "two LEFTs on the sfx row left {:.3}, expected {after_down:.3}",
                    settings.sfx_volume
                )),
                5 if (settings.sfx_volume - after_up).abs() > 1e-4 => fail(format!(
                    "two RIGHTs on the sfx row left {:.3}, expected {after_up:.3}",
                    settings.sfx_volume
                )),
                6 if *phase.get() != crate::states::GamePhase::Playing => {
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
    let needle = selector.to_lowercase();
    entries
        .iter()
        .find(|entry| entry.title.to_lowercase().contains(&needle))
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

/// `BEATBYTE_AUTOPILOT_DELETE=<title-substring>`: arrow down to the
/// matching entry with real key presses, hit Backspace twice through
/// the real confirm flow, and succeed once the song left the library.
fn autopilot_delete(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    library: Res<crate::library::SongLibrary>,
    time: Res<Time>,
    mut frame: Local<u32>,
    mut waited: Local<f32>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let Some(target) = std::env::var("BEATBYTE_AUTOPILOT_DELETE").ok() else {
        return;
    };
    let needle = target.to_lowercase();
    let index = library
        .entries
        .iter()
        .position(|e| e.title.to_lowercase().contains(&needle));
    *waited += time.delta_secs();
    if *waited > 30.0 {
        error!(
            "autopilot: delete timed out (target still present: {:?})",
            index
        );
        deliver(&mut app_exit, AppExit::error());
        return;
    }
    let Some(index) = index else {
        // Gone — deletion done (only counts after we actually acted).
        if *frame > 0 {
            info!("autopilot: delete validation PASSED");
            deliver(&mut app_exit, AppExit::Success);
        }
        return;
    };
    // Alternate press/release frames: downs to reach the entry, then
    // Backspace, a pause across the confirm window, Backspace again.
    let downs = index as u32;
    let step = *frame / 2;
    let pressing = (*frame).is_multiple_of(2);
    if step < downs {
        if pressing {
            keys.press(KeyCode::ArrowDown);
        } else {
            keys.release(KeyCode::ArrowDown);
        }
    } else if step == downs || step == downs + 8 {
        if pressing {
            keys.press(KeyCode::Backspace);
        } else {
            keys.release(KeyCode::Backspace);
        }
    }
    *frame += 1;
}

fn autopilot_reset(mut hands: ResMut<AutopilotHands>) {
    *hands = AutopilotHands::default();
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

/// Play every note event exactly on time through the real session
/// API — for every player.
fn autopilot_play(
    mut players: Query<(&crate::gameplay::PlayerIndex, &mut PlayerSession)>,
    mut hands: ResMut<AutopilotHands>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    hands.ensure(players.iter().count());

    for (index, mut player) in &mut players {
        let slot = index.0;
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
            // note — strumming on top would be an overstrum. Only
            // strum while the event is still pending (which also
            // stays correct for HOPOs in classic mode).
            if matches!(
                player.session.note_state(event_index),
                Some(NoteState::Pending)
            ) {
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

/// Log the outcome and exit.
fn autopilot_results(
    time: Res<Time>,
    mut hands: ResMut<AutopilotHands>,
    results: Option<Res<LastResults>>,
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
    let mut all_ok = true;
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
    use crate::states::AppState;

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
            title: title.to_owned(),
            artist: "Tests".to_owned(),
            bpm: 120.0,
            duration_s: None,
            difficulties: vec![],
            note_counts: vec![],
            genre: None,
            source: SongSource::Builtin(0),
        }
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
