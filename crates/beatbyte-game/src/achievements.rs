//! Achievements on disk, and the moment one is earned.
//!
//! Persistence only — the catalogue and every rule live in
//! [`beatbyte_core::achievements`], the screen is
//! [`crate::achievements_ui`]. Same shape as [`crate::players`]: a
//! small JSON file beside `players.json`, loaded once at startup,
//! written when it changes, and a failure to write warns rather than
//! taking gameplay with it.
//!
//! **What the file holds is only `{player: {id: when}}`.** No
//! progress, no counters, no totals: everything else is re-derived
//! from the play log on every pass, which is what lets an
//! achievement added next year unlock from runs played last year,
//! and what means a changed catalogue needs no migration.
//!
//! # Three moments, two of them silent
//!
//! - **At startup** the store is swept against the whole history and
//!   merged WITHOUT announcing anything. A player who has played for
//!   a year before this existed would otherwise be met by fifty
//!   toasts in a row; they find their history already credited, each
//!   with the date it actually happened.
//! - **On the way out of a song** the sweep runs again and the new
//!   ones ARE announced, one at a time.
//! - **On the way into the overview** it sweeps silently, so a
//!   history edited by the CLI is credited when the screen opens.

use std::collections::{BTreeMap, VecDeque};

use beatbyte_core::achievements::{Achievement, CATALOGUE, Unlocks, earned_at};
use beatbyte_core::player::PlayerId;
use bevy::prelude::*;
use bevy::ui::Val::{Percent as percent, Px as px};

use crate::states::AppState;

/// How long one unlock banner stays up.
const TOAST_S: f32 = 3.4;

/// Who has earned what, by player.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct Unlocked(pub BTreeMap<PlayerId, Unlocks>);

impl Unlocked {
    /// One player's unlocks, empty when they have none yet.
    #[must_use]
    pub fn of(&self, player: PlayerId) -> Unlocks {
        self.0.get(&player).cloned().unwrap_or_default()
    }
}

/// Achievements earned but not yet shown, oldest first.
#[derive(Resource, Debug, Clone, Default)]
pub struct UnlockQueue {
    /// Ids waiting for their banner.
    pub pending: VecDeque<&'static str>,
    /// Seconds left on the banner currently up.
    pub showing: f32,
}

/// Where the store lives — beside `players.json`.
#[must_use]
pub fn store_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("achievements.json"))
}

/// Load the store (missing or corrupt → empty, with a warning).
///
/// A corrupt store is NOT repaired in place: the file is left as it
/// is so it can be inspected, and the session runs with nothing
/// earned. The next sweep re-derives every unlock from the play log
/// anyway, which is the whole point of storing only the dates — the
/// worst a bad file costs is the dates, not the achievements.
#[must_use]
pub fn load_store() -> Unlocked {
    let Some(path) = store_path() else {
        return Unlocked::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<BTreeMap<PlayerId, Unlocks>>(&text) {
            Ok(store) => Unlocked(store),
            Err(error) => {
                warn!("achievements: {} is not readable: {error}", path.display());
                Unlocked::default()
            }
        },
        Err(_) => Unlocked::default(),
    }
}

/// Write the store out.
pub fn save_store(store: &Unlocked) {
    let Some(path) = store_path() else {
        return;
    };
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&store.0)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        std::fs::write(&path, text)
    };
    if let Err(error) = write() {
        warn!("cannot save achievements to {}: {error}", path.display());
    }
}

/// Look an achievement up by id. `None` for an id the catalogue no
/// longer carries — a store outlives a catalogue edit, and a
/// forgotten id must not panic a screen.
#[must_use]
pub fn find(id: &str) -> Option<&'static Achievement> {
    CATALOGUE.iter().find(|entry| entry.id == id)
}

/// Bring one player's store up to date with the whole play log.
///
/// Returns the ids earned since the last sweep. Pure apart from the
/// store it edits — the log is only read.
pub fn sweep(
    store: &mut Unlocked,
    log: &[beatbyte_core::history::PlayEntry],
    player: PlayerId,
) -> Vec<String> {
    let earned = earned_at(log, player);
    store.0.entry(player).or_default().merge(&earned)
}

/// The store, the queue, the banner.
pub struct AchievementsPlugin;

impl Plugin for AchievementsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load_store())
            .init_resource::<UnlockQueue>()
            .add_systems(Startup, (spawn_banner, credit_the_past))
            .add_systems(
                OnExit(AppState::Gameplay),
                announce_new.after(crate::history::RunLogged),
            )
            .add_systems(
                OnEnter(AppState::Achievements),
                credit_quietly.after(crate::history::HistoryReloaded),
            )
            .add_systems(Update, run_banner);
    }
}

/// Credit everything the play log already earned, without a word.
///
/// A player who has played for a year before this existed would
/// otherwise meet fifty banners in a row on their next launch. The
/// dates are the real ones — the run that earned each — so the
/// overview reads as a history rather than as "all on the day the
/// feature shipped".
#[allow(clippy::needless_pass_by_value)] // Bevy system params
fn credit_the_past(mut store: ResMut<Unlocked>, players: Res<crate::players::Players>) {
    let log = crate::history::load();
    let mut credited = 0;
    // Every player on the machine, not just whoever is selected: the
    // screen can be opened for any of them.
    for player in &players.0.players {
        credited += sweep(&mut store, &log, player.id).len();
    }
    if credited > 0 {
        info!("achievements: {credited} already earned by earlier play");
        save_store(&store);
    }
}

/// Same sweep, on the way into the overview: a history edited by the
/// CLI between sessions is credited when the screen opens.
#[allow(clippy::needless_pass_by_value)] // Bevy system params
pub(crate) fn credit_quietly(
    mut store: ResMut<Unlocked>,
    history: Res<crate::history::PlayHistory>,
    players: Res<crate::players::Players>,
) {
    let mut changed = false;
    for player in &players.0.players {
        changed |= !sweep(&mut store, &history.0, player.id).is_empty();
    }
    if changed {
        save_store(&store);
    }
}

/// After a run: sweep, and queue whatever is new for a banner.
#[allow(clippy::needless_pass_by_value)] // Bevy system params
fn announce_new(
    mut store: ResMut<Unlocked>,
    mut queue: ResMut<UnlockQueue>,
    players: Res<crate::players::Players>,
) {
    let Some(player) = players.0.selected else {
        // Nobody is filed as playing, so nothing can be earned. A run
        // logged unattributed is credited later, by the adoption.
        return;
    };
    // Read from DISK, not from `PlayHistory`: the line for the run
    // that just ended was appended a moment ago and the in-memory
    // copy is one run stale. Crediting it next session would work,
    // but the banner is supposed to arrive now.
    let log = crate::history::load();
    let fresh = sweep(&mut store, &log, player);
    if fresh.is_empty() {
        return;
    }
    save_store(&store);
    for id in fresh {
        if let Some(entry) = find(&id) {
            queue.pending.push_back(entry.id);
        }
    }
}

/// The banner's root, spawned once and kept hidden.
#[derive(Component)]
struct BannerRoot;

/// The banner's title line.
#[derive(Component)]
struct BannerTitle;

/// The banner's blurb line.
#[derive(Component)]
struct BannerBlurb;

#[allow(clippy::needless_pass_by_value)] // Bevy system params
fn spawn_banner(mut commands: Commands, font: Res<crate::ui::UiFont>) {
    commands
        .spawn((
            BannerRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                align_items: AlignItems::FlexStart,
                justify_content: JustifyContent::Center,
                padding: UiRect::top(px(26.0)),
                ..default()
            },
            Pickable::IGNORE,
            GlobalZIndex(60),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6.0),
                    padding: UiRect::all(px(14.0)),
                    width: px(460.0),
                    border: UiRect::all(px(2.0)),
                    border_radius: BorderRadius::all(px(10.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.05, 0.11, 0.94)),
                BorderColor::all(crate::palette::HYPE.with_alpha(0.75)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("ACHIEVEMENT UNLOCKED".to_owned()),
                    font.text(crate::ui_kit::SMALL),
                    TextColor(crate::palette::HYPE),
                ));
                panel.spawn((
                    BannerTitle,
                    Text::new(String::new()),
                    font.text(crate::ui_kit::ROW),
                    TextColor(crate::palette::TEXT),
                ));
                panel.spawn((
                    BannerBlurb,
                    Text::new(String::new()),
                    font.text(crate::ui_kit::SMALL),
                    TextColor(crate::ui_kit::dimmed_subtitle()),
                ));
            });
        });
}

/// Show the queue one at a time.
#[allow(clippy::type_complexity, clippy::needless_pass_by_value)] // Bevy queries
fn run_banner(
    time: Res<Time>,
    mut queue: ResMut<UnlockQueue>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
    mut root: Query<&mut Visibility, With<BannerRoot>>,
    mut title: Query<&mut Text, (With<BannerTitle>, Without<BannerBlurb>)>,
    mut blurb: Query<&mut Text, (With<BannerBlurb>, Without<BannerTitle>)>,
) {
    if queue.showing > 0.0 {
        queue.showing -= time.delta_secs();
        if queue.showing > 0.0 {
            return;
        }
        queue.showing = 0.0;
        if let Ok(mut visibility) = root.single_mut() {
            *visibility = Visibility::Hidden;
        }
    }
    let Some(id) = queue.pending.pop_front() else {
        return;
    };
    let Some(entry) = find(id) else {
        return;
    };
    if let Ok(mut text) = title.single_mut() {
        **text = entry.title.to_uppercase();
    }
    if let Ok(mut text) = blurb.single_mut() {
        **text = entry.blurb.to_uppercase();
    }
    if let Ok(mut visibility) = root.single_mut() {
        *visibility = Visibility::Visible;
    }
    queue.showing = TOAST_S;
    sounds.write(crate::sfx::UiSound::Confirm);
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_core::history::{PlayEntry, RunDetail};

    fn run(started_ms: u64, accuracy: f64, player: PlayerId) -> PlayEntry {
        PlayEntry {
            title: "Africa".to_owned(),
            artist: "Toto".to_owned(),
            difficulty: "medium".to_owned(),
            started_ms,
            played_s: 120.0,
            track_s: Some(120.0),
            completed: true,
            players: 1,
            practice: false,
            autopilot: false,
            score: 1000,
            accuracy,
            source: "file".to_owned(),
            player: Some(player),
            detail: RunDetail::default(),
            co_players: Vec::new(),
            chart_hash: None,
            genre: None,
            tap_mode: Some(false),
            no_fail: Some(false),
            speed_percent: Some(100),
        }
    }

    #[test]
    fn a_sweep_announces_each_achievement_exactly_once() {
        // The banner fires on what the sweep RETURNS, so a second
        // sweep over the same log returning anything would show the
        // player the same unlock every time they finish a song.
        let log = [run(1_756_000_000_000, 0.85, 1)];
        let mut store = Unlocked::default();
        let first = sweep(&mut store, &log, 1);
        assert!(first.contains(&"first_run".to_owned()), "{first:?}");
        assert!(
            sweep(&mut store, &log, 1).is_empty(),
            "the same achievements were announced twice"
        );
    }

    #[test]
    fn two_players_keep_separate_stores() {
        let log = [run(1_000, 0.85, 1), run(2_000, 0.5, 2)];
        let mut store = Unlocked::default();
        sweep(&mut store, &log, 1);
        sweep(&mut store, &log, 2);
        // 80 % is player one's run, and must not reach player two.
        assert!(store.of(1).has("first_80"));
        assert!(!store.of(2).has("first_80"), "an unlock crossed players");
        assert!(store.of(2).has("first_run"), "player two played too");
        // A player nobody has swept has nothing, not a panic.
        assert_eq!(store.of(99).count(), 0);
    }

    #[test]
    fn a_store_survives_a_round_trip_and_an_unknown_id() {
        let log = [run(1_000, 0.85, 1)];
        let mut store = Unlocked::default();
        sweep(&mut store, &log, 1);
        let json = serde_json::to_string(&store.0).expect("serializes");
        let back: BTreeMap<PlayerId, Unlocks> = serde_json::from_str(&json).expect("parses");
        assert_eq!(Unlocked(back), store);
        // A file written by a LATER catalogue carries ids this build
        // does not know. It must read, and the unknown id must not be
        // findable rather than be a crash on the overview.
        let future: BTreeMap<PlayerId, Unlocks> =
            serde_json::from_str(r#"{"1":{"at":{"not_a_real_id":5}}}"#).expect("parses");
        assert_eq!(future[&1].when("not_a_real_id"), Some(5));
        assert!(find("not_a_real_id").is_none());
        assert!(find("first_run").is_some());
    }

    #[test]
    fn the_store_sits_beside_the_other_save_files() {
        let Some(path) = store_path() else {
            return; // headless CI without a data dir
        };
        assert!(path.ends_with("beatbyte/achievements.json"));
        assert_eq!(
            path.parent(),
            crate::players::roster_path()
                .as_deref()
                .and_then(std::path::Path::parent)
        );
    }
}
