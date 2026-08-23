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
        if enabled {
            app.add_systems(
                Update,
                (
                    autopilot_menu.run_if(in_state(AppState::MainMenu)),
                    autopilot_song_select.run_if(in_state(AppState::SongSelect)),
                    // Judgment is input-stamp-driven, so playing *before*
                    // the session advances makes the autopilot exact and
                    // frame-rate independent (hitches cannot cause
                    // misses — the stamps carry the truth).
                    autopilot_play
                        .before(crate::gameplay::advance_sessions)
                        .run_if(in_state(crate::states::GamePhase::Playing)),
                    autopilot_results.run_if(in_state(AppState::Results)),
                ),
            )
            .add_systems(OnEnter(AppState::Gameplay), autopilot_reset);

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

/// Where autopilot screenshots go.
#[derive(Resource)]
struct ShotDir(std::path::PathBuf);

/// Take one screenshot per named moment of the run.
fn autopilot_screenshots(
    mut commands: Commands,
    dir: Res<ShotDir>,
    state: Res<State<AppState>>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut taken: Local<std::collections::HashSet<&'static str>>,
) {
    let moment = match state.get() {
        AppState::MainMenu => Some("menu"),
        AppState::SongSelect => Some("songselect"),
        AppState::MultiplayerSetup => Some("join"),
        AppState::Gameplay => {
            let now = game_clock.song_time(&time).unwrap_or(0.0);
            if (24.0..26.0).contains(&now) {
                Some("gameplay")
            } else if (44.0..46.0).contains(&now) {
                Some("gameplay-late")
            } else {
                None
            }
        }
        AppState::Results => Some("results"),
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
        // Unattended runs should not blast anyone's speakers.
        music.0.set_volume(0.05);
        info!("autopilot: opening song select (volume lowered)");
        next_state.set(AppState::SongSelect);
    }
}

/// Pick the demo song and start it (optionally with simulated
/// multiplayer via `BEATBYTE_AUTOPILOT_PLAYERS=N`).
fn autopilot_song_select(
    mut commands: Commands,
    time: Res<Time>,
    mut delay: Local<f32>,
    library: Res<crate::library::SongLibrary>,
    demo: Res<crate::boot::DemoSong>,
    mut roster: ResMut<crate::multiplayer::PlayerRoster>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    *delay += time.delta_secs();
    if *delay > 0.6 {
        *delay = 0.0;
        let Some(entry) = library.entries.first() else {
            error!("autopilot: empty song library");
            return;
        };
        let players: usize = std::env::var("BEATBYTE_AUTOPILOT_PLAYERS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1)
            .clamp(1, crate::multiplayer::MAX_PLAYERS);
        roster.devices = vec![crate::multiplayer::DeviceId::Keyboard; players];
        match crate::song_select::prepare_song(entry, &demo) {
            Ok(song) => {
                info!(
                    "autopilot: starting \"{}\" with {players} player(s)",
                    entry.title
                );
                commands.insert_resource(song);
                next_state.set(AppState::Gameplay);
            }
            Err(reason) => error!("autopilot: cannot start song: {reason}"),
        }
    }
}

fn autopilot_reset(mut hands: ResMut<AutopilotHands>) {
    *hands = AutopilotHands::default();
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
            player.session.handle(
                GameInput {
                    time_s: stamp,
                    kind: InputKind::Strum,
                },
                &mut player.frame_events,
            );
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
        app_exit.write(AppExit::error());
        return;
    };
    if results.players.is_empty() {
        error!("autopilot: results carry no players");
        app_exit.write(AppExit::error());
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
        app_exit.write(AppExit::Success);
    } else {
        error!("autopilot: run FAILED — misses or overstrums in a perfect run");
        app_exit.write(AppExit::error());
    }
}
