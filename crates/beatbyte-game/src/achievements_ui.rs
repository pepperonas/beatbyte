//! The achievements screen: what you have earned, and what is close.
//!
//! One scrolling list in the house idiom ([`crate::ui_kit`]), with
//! four controls over it — a category to group by, a tier to narrow
//! by, a filter, and a sort. The arithmetic is
//! [`beatbyte_core::achievements`]; this module is the arrangement,
//! and holds no rule of its own except the one that makes a secret a
//! secret:
//!
//! **A hidden achievement that is not earned reveals nothing** — not
//! its name, not its description, and not its progress. A bar at
//! three tenths says "this is a count of ten", which is most of the
//! condition; [`row_lines`] is where that is decided, and it is
//! pinned.

use beatbyte_core::achievements::{
    Achievement, CATALOGUE, Category, Progress, Tier, Unlocks, evaluate,
};
use beatbyte_core::player::PlayerId;
use bevy::prelude::*;
use bevy::ui::Val::{Percent as percent, Px as px};

use crate::achievements::Unlocked;
use crate::controls::{InputMap, MenuNav};
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// Width of a row's progress bar.
const BAR_W: f32 = 120.0;
/// Height of a row's progress bar.
const BAR_H: f32 = 5.0;

/// Whose achievements the screen shows. Set before the state change;
/// falls back to whoever is playing.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct AchievementsFor(pub Option<PlayerId>);

/// Which achievements are listed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Show {
    /// Everything in the chosen category.
    #[default]
    All,
    /// Only what has been earned.
    Earned,
    /// Only what has not.
    Locked,
}

impl Show {
    /// All three, in cycle order.
    pub const ALL: [Show; 3] = [Show::All, Show::Earned, Show::Locked];

    /// The word the footer prints.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Show::All => "ALL",
            Show::Earned => "EARNED",
            Show::Locked => "LOCKED",
        }
    }

    /// The next filter along, wrapping — this is a cycle on one key,
    /// not a list cursor, so it may wrap where a list may not.
    #[must_use]
    pub fn next(self) -> Show {
        let at = Show::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Show::ALL[(at + 1) % Show::ALL.len()]
    }
}

/// The order the list is in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sort {
    /// Catalogue order: category, then roughly by difficulty.
    #[default]
    Catalogue,
    /// Closest to done first — the one that answers "what next".
    Closest,
    /// Most recently earned first.
    Newest,
}

impl Sort {
    /// All three, in cycle order.
    pub const ALL: [Sort; 3] = [Sort::Catalogue, Sort::Closest, Sort::Newest];

    /// The word the footer prints.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Sort::Catalogue => "CATALOGUE",
            Sort::Closest => "CLOSEST",
            Sort::Newest => "NEWEST",
        }
    }

    /// The next sort along, wrapping.
    #[must_use]
    pub fn next(self) -> Sort {
        let at = Sort::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Sort::ALL[(at + 1) % Sort::ALL.len()]
    }
}

/// What the screen is showing right now.
#[derive(Resource, Debug, Clone, Default)]
pub struct AchievementsView {
    /// Selected row, as an index into the visible list.
    pub row: usize,
    /// The category being shown; `None` is every category.
    pub group: Option<Category>,
    /// The tier being shown; `None` is every tier.
    pub tier: Option<Tier>,
    /// Which achievements are listed.
    pub show: Show,
    /// The order they are in.
    pub sort: Sort,
}

impl AchievementsView {
    /// Step the category left or right, `None` at the far left.
    ///
    /// Stops at the ends rather than wrapping, because every list
    /// cursor in this game does ([`ui_kit::step_cursor`]). Pure —
    /// tested.
    pub fn step_group(&mut self, delta: i32) {
        // `None` is position 0, the ten categories follow.
        let at = self
            .group
            .and_then(|group| Category::ALL.iter().position(|c| *c == group))
            .map_or(0, |index| index + 1);
        let next = ui_kit::step_cursor(at, Category::ALL.len() + 1, delta);
        if next == at {
            // At either end the cursor clamps and the category does
            // not move. Resetting the row anyway would throw the
            // player back to the top of three hundred rows for a
            // press that changed nothing they can see.
            return;
        }
        self.group = if next == 0 {
            None
        } else {
            Some(Category::ALL[next - 1])
        };
        self.row = 0;
    }

    /// Cycle the tier filter: every tier, then Easy → Rare, wrapping
    /// back to every. Pure — tested.
    pub fn next_tier(&mut self) {
        self.tier = match self.tier {
            None => Some(Tier::Easy),
            Some(Tier::Easy) => Some(Tier::Medium),
            Some(Tier::Medium) => Some(Tier::Hard),
            Some(Tier::Hard) => Some(Tier::Rare),
            Some(Tier::Rare) => None,
        };
        self.row = 0;
    }

    /// The heading for the current category.
    #[must_use]
    pub fn group_label(&self) -> &'static str {
        self.group.map_or("EVERYTHING", Category::label)
    }

    /// The word the footer prints for the tier filter.
    #[must_use]
    pub fn tier_label(&self) -> &'static str {
        self.tier.map_or("ALL", Tier::label)
    }
}

/// One row's place in the catalogue and where it stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    /// Index into [`CATALOGUE`].
    pub at: usize,
    /// How far along the player is.
    pub progress: Progress,
    /// When it was earned, if it was.
    pub when_ms: Option<u64>,
}

impl Row {
    /// Whether this one is done.
    #[must_use]
    pub fn earned(&self) -> bool {
        self.when_ms.is_some() || self.progress.earned()
    }

    /// The catalogue entry behind the row.
    #[must_use]
    pub fn entry(&self) -> &'static Achievement {
        &CATALOGUE[self.at]
    }

    /// Whether the row must keep its secret: hidden and not yet
    /// earned. Pure — tested.
    #[must_use]
    pub fn secret(&self) -> bool {
        self.entry().hidden && !self.earned()
    }
}

/// The rows to draw, filtered and sorted. Pure — tested.
#[must_use]
pub fn visible_rows(progress: &[Progress], unlocks: &Unlocks, view: &AchievementsView) -> Vec<Row> {
    let mut rows: Vec<Row> = CATALOGUE
        .iter()
        .enumerate()
        .map(|(at, entry)| Row {
            at,
            // The store is the authority on "earned": a threshold
            // raised after the fact must not take an unlock back,
            // and only the store remembers that it happened.
            progress: progress.get(at).copied().unwrap_or(Progress {
                fraction: 0.0,
                have: 0.0,
                need: 1.0,
            }),
            when_ms: unlocks.when(entry.id),
        })
        .filter(|row| view.group.is_none_or(|group| row.entry().category == group))
        .filter(|row| view.tier.is_none_or(|tier| row.entry().tier == tier))
        .filter(|row| match view.show {
            Show::All => true,
            Show::Earned => row.earned(),
            Show::Locked => !row.earned(),
        })
        .collect();
    match view.sort {
        Sort::Catalogue => {}
        Sort::Closest => rows.sort_by(|a, b| {
            // Done ones to the bottom: the question this order
            // answers is "what next", and a finished row is not an
            // answer to it.
            (a.earned(), b.progress.fraction)
                .partial_cmp(&(b.earned(), a.progress.fraction))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.at.cmp(&b.at))
        }),
        // Unearned ones keep catalogue order at the bottom, because
        // they have no date to sort by and inventing one would put
        // them in an order that means nothing.
        Sort::Newest => rows.sort_by(|a, b| b.when_ms.cmp(&a.when_ms).then(a.at.cmp(&b.at))),
    }
    rows
}

/// What a row says: its name, its description, and its right-hand
/// column.
///
/// The secrecy rule lives here and nowhere else. A hidden row that
/// is not earned gives up its name, its description **and its
/// progress** — a bar at three tenths says "this is a count of ten",
/// which is most of the condition. Pure — tested.
#[must_use]
pub fn row_lines(row: &Row) -> (String, String, String) {
    let entry = row.entry();
    if row.secret() {
        return (
            "? ? ?".to_owned(),
            "A SECRET - KEEP PLAYING".to_owned(),
            "HIDDEN".to_owned(),
        );
    }
    let title = entry.title.to_uppercase();
    let blurb = entry.blurb.to_uppercase();
    if let Some(ms) = row.when_ms {
        let day = beatbyte_core::history::iso_utc(ms);
        return (title, blurb, day[..10.min(day.len())].to_owned());
    }
    // A count says how far along; a one-shot condition has no count
    // to report, and "0 / 1" would be noise.
    let right = if row.progress.need > 1.0 {
        format!("{:.0} / {:.0}", row.progress.have, row.progress.need)
    } else {
        entry.tier.label().to_owned()
    };
    (title, blurb, right)
}

/// How much of a row's bar is filled, 0.0–1.0.
///
/// A secret shows an empty bar rather than its real fraction, for
/// the same reason it shows no count. Pure — tested.
#[must_use]
pub fn bar_fraction(row: &Row) -> f32 {
    if row.secret() {
        return 0.0;
    }
    if row.earned() {
        return 1.0;
    }
    row.progress.fraction as f32
}

/// The subtitle: how the player stands overall. Pure — tested.
#[must_use]
pub fn standing(earned: usize, hidden_found: usize) -> String {
    let total = CATALOGUE.len();
    let share = (earned as f64 / total as f64 * 100.0).round();
    format!("{earned} OF {total}   {share:.0}%   {hidden_found} SECRETS FOUND")
}

/// The footer, which names the four controls and their state.
/// Pure — tested.
#[must_use]
pub fn footer_hint(show: Show, sort: Sort, tier: Option<Tier>) -> String {
    format!(
        "LEFT/RIGHT CATEGORY   chips: FILTER {} · ORDER {} · TIER {}   ESC BACK",
        show.label(),
        sort.label(),
        tier.map_or("ALL", Tier::label),
    )
}

/// ActionBar chip ids for Filter / Order / Tier.
mod chip {
    pub const FILTER: u8 = 0;
    pub const ORDER: u8 = 1;
    pub const TIER: u8 = 2;
}

#[derive(Resource, Default)]
struct ActionBarClicks(Vec<u8>);

fn achievements_chips() -> [ui_kit::ChipSpec; 3] {
    [
        ui_kit::ChipSpec {
            id: chip::FILTER,
            label: "Filter",
            enabled: true,
        },
        ui_kit::ChipSpec {
            id: chip::ORDER,
            label: "Order",
            enabled: true,
        },
        ui_kit::ChipSpec {
            id: chip::TIER,
            label: "Tier",
            enabled: true,
        },
    ]
}

fn paint_action_bar(
    mut chips: Query<(
        &ui_kit::ActionChip,
        &ui_kit::ChipEnabled,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
        &Children,
    )>,
    mut labels: Query<&mut TextColor>,
    mut clicks: ResMut<ActionBarClicks>,
) {
    clicks.0 = ui_kit::read_chips(&mut chips, &mut labels);
}

/// How many of a category's achievements are earned. Pure — tested.
#[must_use]
pub fn category_progress(category: Category, unlocks: &Unlocks) -> (usize, usize) {
    let mut earned = 0;
    let mut total = 0;
    for entry in CATALOGUE {
        if entry.category != category {
            continue;
        }
        total += 1;
        if unlocks.has(entry.id) {
            earned += 1;
        }
    }
    (earned, total)
}

/// Tab label with the standing inside that category. Pure — tested.
#[must_use]
pub fn category_tab_label(category: Category, unlocks: &Unlocks) -> String {
    let (earned, total) = category_progress(category, unlocks);
    format!("{} {earned}/{total}", category.label())
}

/// The colour a tier's bar wears. Reuses the judgment palette so the
/// four tiers read as a ramp rather than four unrelated hues.
#[must_use]
const fn tier_colour(tier: Tier) -> Color {
    match tier {
        Tier::Easy => palette::PERFECT,
        Tier::Medium => palette::GREAT,
        Tier::Hard => palette::GOOD,
        Tier::Rare => palette::HYPE,
    }
}

/// Everything this screen spawns.
#[derive(Component)]
struct AchievementsScreen;

/// What the list currently on screen was built from.
///
/// The category, the tier, the filter and the sort each change which
/// rows exist, and a resource that changes without a rebuild is a
/// control that does nothing. The cursor is deliberately NOT in here:
/// a three-hundred-row list must not be thrown away and re-scrolled
/// on every arrow key.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct DrawnFrom {
    /// The category the rows were filtered to.
    group: Option<Category>,
    /// The tier the rows were filtered to.
    tier: Option<Tier>,
    /// The filter they were drawn under.
    show: Show,
    /// The order they were drawn in.
    sort: Sort,
}

impl DrawnFrom {
    /// What a view asks the list to look like.
    const fn of(view: &AchievementsView) -> DrawnFrom {
        DrawnFrom {
            group: view.group,
            tier: view.tier,
            show: view.show,
            sort: view.sort,
        }
    }
}

/// A row, by its position in the visible list.
#[derive(Component)]
struct AchievementRow(usize);

/// A category tab the mouse can pick — `None` is EVERYTHING.
#[derive(Component)]
struct CategoryTab(Option<Category>);

/// The scrolling list itself.
#[derive(Component)]
struct AchievementList;

/// Screens and systems of the achievements overview.
pub struct AchievementsUiPlugin;

impl Plugin for AchievementsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AchievementsFor>()
            .init_resource::<AchievementsView>()
            .init_resource::<ActionBarClicks>()
            .add_systems(
                OnEnter(AppState::Achievements),
                // Behind the reload: this reads `PlayHistory`, and
                // the same state entry rewrites it.
                spawn_screen.after(crate::history::HistoryReloaded),
            )
            .add_systems(
                Update,
                (
                    paint_action_bar.before(screen_keys),
                    (
                        screen_keys,
                        screen_nav,
                        redraw_when_the_list_changes,
                        follow_selection,
                    )
                        .chain(),
                )
                    .run_if(in_state(AppState::Achievements)),
            )
            .add_systems(OnExit(AppState::Achievements), despawn_screen);
    }
}

/// Where Escape leads from this screen.
///
/// The roster opens it for one named player and the main menu opens
/// it for whoever is playing; going back to the main menu from a list
/// reached through the roster would lose your place in the roster.
/// Pure — tested.
#[must_use]
pub const fn back_to(opened_for_somebody: bool) -> AppState {
    if opened_for_somebody {
        AppState::Players
    } else {
        AppState::MainMenu
    }
}

/// Whose list this is, and what they have earned.
fn subject(
    chosen: &AchievementsFor,
    players: &crate::players::Players,
) -> Option<(PlayerId, String)> {
    let id = chosen.0.or(players.0.selected)?;
    let name = players
        .0
        .name_of(id)
        .map_or_else(|| "PLAYER".to_owned(), str::to_owned);
    Some((id, name))
}

#[allow(clippy::needless_pass_by_value)] // Bevy system params
fn spawn_screen(
    mut commands: Commands,
    font: Res<UiFont>,
    players: Res<crate::players::Players>,
    history: Res<crate::history::PlayHistory>,
    store: Res<Unlocked>,
    chosen: Res<AchievementsFor>,
    mut view: ResMut<AchievementsView>,
) {
    let Some((id, name)) = subject(&chosen, &players) else {
        commands
            .spawn((
                ui_kit::screen_root(),
                AchievementsScreen,
                DrawnFrom::of(&view),
            ))
            .with_children(|root| {
                ui_kit::header(root, &font, "ACHIEVEMENTS", "NO PLAYER CHOSEN");
                root.spawn(ui_kit::panel()).with_children(|panel| {
                    crate::plot::empty_note(
                        panel,
                        &font,
                        "CREATE A PLAYER ON THE PLAYERS SCREEN FIRST",
                    );
                });
                ui_kit::back_button(root, &font, "MAIN MENU");
                crate::prompts::device_footer(root, &font, "ESC BACK", "EAST back");
            });
        return;
    };
    let unlocks = store.of(id);
    let progress = evaluate(&history.0, id);
    let rows = visible_rows(&progress, &unlocks, &view);
    // A cursor left over from a previous visit can sit past the end
    // of a shorter list, and then no row is drawn as selected at all.
    view.row = view.row.min(rows.len().saturating_sub(1));
    let hidden_found = CATALOGUE
        .iter()
        .filter(|entry| entry.hidden && unlocks.has(entry.id))
        .count();

    commands
        .spawn((
            ui_kit::screen_root(),
            AchievementsScreen,
            DrawnFrom::of(&view),
        ))
        .with_children(|root| {
            ui_kit::header(
                root,
                &font,
                &format!("{} - ACHIEVEMENTS", name.to_uppercase()),
                &standing(unlocks.count(), hidden_found),
            );
            ui_kit::action_bar(root, &font, &achievements_chips());
            spawn_tabs(root, &font, view.group, &unlocks);
            root.spawn((ui_kit::scroll_panel(ui_kit::PANEL_WIDE), AchievementList))
                .with_children(|panel| {
                    if rows.is_empty() {
                        crate::plot::empty_note(panel, &font, "NOTHING HERE UNDER THIS FILTER");
                    }
                    for (index, row) in rows.iter().enumerate() {
                        spawn_row(panel, &font, index, row);
                    }
                });
            let back_label = if chosen.0.is_some() {
                "PLAYERS"
            } else {
                "MAIN MENU"
            };
            ui_kit::back_button(root, &font, back_label);
            crate::prompts::device_footer(
                root,
                &font,
                &footer_hint(view.show, view.sort, view.tier),
                "D-PAD category and rows  EAST back",
            );
        });
}

/// The row of category tabs, with the "everything" one first — each
/// named category carries how many of its achievements are earned.
fn spawn_tabs(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    active: Option<Category>,
    unlocks: &Unlocks,
) {
    parent
        .spawn(Node {
            column_gap: px(14.0),
            margin: UiRect::bottom(px(12.0)),
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(4.0),
            max_width: px(ui_kit::PANEL_WIDE),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|tabs| {
            let mut draw = |label: &str, group: Option<Category>, on: bool| {
                tabs.spawn((
                    CategoryTab(group),
                    Button,
                    Node {
                        padding: UiRect::axes(px(6.0), px(2.0)),
                        ..default()
                    },
                ))
                .with_children(|tab| {
                    tab.spawn((
                        Text::new(label.to_owned()),
                        font.text(ui_kit::SMALL),
                        TextColor(if on {
                            palette::BRAND
                        } else {
                            ui_kit::dimmed_subtitle()
                        }),
                    ));
                });
            };
            draw(
                &format!("ALL {}/{}", unlocks.count(), CATALOGUE.len()),
                None,
                active.is_none(),
            );
            for category in Category::ALL {
                draw(
                    &category_tab_label(category, unlocks),
                    Some(category),
                    active == Some(category),
                );
            }
        });
}

fn spawn_row(parent: &mut ChildSpawnerCommands, font: &UiFont, index: usize, row: &Row) {
    let (title, blurb, right) = row_lines(row);
    let earned = row.earned();
    let colour = tier_colour(row.entry().tier);
    parent
        .spawn((ui_kit::row(), AchievementRow(index), Button))
        .with_children(|node| {
            // No-wrap and clipped, both of them. `ui_kit::list_view`
            // measures ONE row and scrolls as though every row were
            // that tall; a blurb that wrapped to a second line would
            // make its own row taller and put the whole scroll
            // arithmetic out by a line. Same treatment the song
            // browser gives a long title.
            node.spawn((
                ui_kit::label_node(),
                Text::new(font.safe(&title)),
                font.text(ui_kit::ROW),
                TextColor(if earned {
                    palette::TEXT
                } else {
                    ui_kit::dimmed_subtitle()
                }),
                TextLayout::default().with_no_wrap(),
            ));
            node.spawn((
                Node {
                    flex_grow: 1.0,
                    min_width: px(0.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                Text::new(font.safe(&blurb)),
                font.text(ui_kit::SMALL),
                TextColor(ui_kit::dimmed_subtitle()),
                TextLayout::default().with_no_wrap(),
            ));
            // The bar: the one place the list says "how close" at a
            // glance, and the reason the secrecy rule has to cover it.
            node.spawn((
                Node {
                    width: px(BAR_W),
                    height: px(BAR_H),
                    margin: UiRect::horizontal(px(10.0)),
                    border_radius: BorderRadius::all(px(BAR_H / 2.0)),
                    ..default()
                },
                BackgroundColor(palette::dimmed(palette::TEXT_DIM, 0.25)),
            ))
            .with_children(|track| {
                track.spawn((
                    Node {
                        width: percent(bar_fraction(row) * 100.0),
                        height: percent(100.0),
                        border_radius: BorderRadius::all(px(BAR_H / 2.0)),
                        ..default()
                    },
                    BackgroundColor(if earned { colour } else { palette::BRAND }),
                ));
            });
            node.spawn((
                Node {
                    min_width: px(96.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                Text::new(font.safe(&right)),
                TextLayout::default().with_no_wrap(),
                font.text(ui_kit::SMALL),
                TextColor(if earned {
                    colour
                } else {
                    ui_kit::dimmed_subtitle()
                }),
            ));
        });
}

/// F, O and T — and the matching ActionBar chips.
#[allow(clippy::needless_pass_by_value)] // Bevy system params
fn screen_keys(
    keys: Res<ButtonInput<KeyCode>>,
    clicks: Res<ActionBarClicks>,
    mut view: ResMut<AchievementsView>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    let mut changed = false;
    if keys.just_pressed(KeyCode::KeyF) || ui_kit::chip_hit(&clicks.0, chip::FILTER) {
        view.show = view.show.next();
        changed = true;
    }
    // `O`, not `S`: the menu table binds W/A/S/D to the four
    // directions, so a sort on `S` would cycle the order AND walk the
    // cursor down on the same press. The roster's `A` is safe for the
    // mirror-image reason — its screen has nothing bound to left.
    if keys.just_pressed(KeyCode::KeyO) || ui_kit::chip_hit(&clicks.0, chip::ORDER) {
        view.sort = view.sort.next();
        changed = true;
    }
    if keys.just_pressed(KeyCode::KeyT) || ui_kit::chip_hit(&clicks.0, chip::TIER) {
        view.next_tier();
        changed = true;
    }
    if changed {
        view.row = 0;
        sounds.write(crate::sfx::UiSound::Toggle);
    }
}

/// Cursor, category, leaving.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system params
fn screen_nav(
    map: Res<InputMap>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&bevy::input::gamepad::Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut moved: MessageReader<bevy::window::CursorMoved>,
    all_rows: Query<&AchievementRow>,
    rows: Query<(&AchievementRow, &Interaction), Changed<Interaction>>,
    tabs: Query<(&CategoryTab, &Interaction), Changed<Interaction>>,
    mut back: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        With<ui_kit::BackButton>,
    >,
    mut view: ResMut<AchievementsView>,
    mut next: ResMut<NextState<AppState>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
    chosen: Res<AchievementsFor>,
) {
    let nav = MenuNav::read(&map, &keys, pads.iter());
    let count = all_rows.iter().count();
    if count > 0 {
        if nav.up {
            view.row = ui_kit::step_cursor(view.row, count, -1);
            sounds.write(crate::sfx::UiSound::Navigate);
        }
        if nav.down {
            view.row = ui_kit::step_cursor(view.row, count, 1);
            sounds.write(crate::sfx::UiSound::Navigate);
        }
        // Wheel moves the cursor; `follow_selection` then keeps the
        // row in view — the same path as up/down, so a long list
        // scrolls under the mouse without a separate free-scroll path.
        for event in wheel.read() {
            if event.y > 0.0 {
                view.row = ui_kit::step_cursor(view.row, count, -1);
            } else if event.y < 0.0 {
                view.row = ui_kit::step_cursor(view.row, count, 1);
            }
            if event.y != 0.0 {
                sounds.write(crate::sfx::UiSound::Navigate);
            }
        }
        let pointer = ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
        let mouse_moved = moved.read().next().is_some();
        if let Some(index) = ui_kit::hover_moves_cursor(&pointer, mouse_moved) {
            view.row = index;
        }
    }
    if nav.left {
        view.step_group(-1);
        sounds.write(crate::sfx::UiSound::Navigate);
    }
    if nav.right {
        view.step_group(1);
        sounds.write(crate::sfx::UiSound::Navigate);
    }
    // A click on a category tab picks that group, same as L/R.
    for (tab, interaction) in &tabs {
        if *interaction == Interaction::Pressed && view.group != tab.0 {
            view.group = tab.0;
            view.row = 0;
            sounds.write(crate::sfx::UiSound::Navigate);
        }
    }
    if ui_kit::wants_leave(
        nav.back,
        ui_kit::back_pressed(&mut back),
        mouse.just_pressed(MouseButton::Right),
    ) {
        sounds.write(crate::sfx::UiSound::Back);
        next.set(back_to(chosen.0.is_some()));
    }
}

/// Paint the cursor row and keep it in view.
#[allow(clippy::type_complexity, clippy::needless_pass_by_value)] // Bevy queries
fn follow_selection(
    view: Res<AchievementsView>,
    settings: Res<crate::config::Settings>,
    mut rows: Query<(
        &AchievementRow,
        &ComputedNode,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut lists: Query<(&mut ScrollPosition, &mut Node), With<AchievementList>>,
) {
    let mut count = 0;
    let mut row_height = None;
    for (row, computed, mut background, mut border) in &mut rows {
        count += 1;
        if computed.size().y > 0.0 && row_height.is_none() {
            row_height = Some((computed.size().y, computed.inverse_scale_factor()));
        }
        let state = ui_kit::state_for(row.0 == view.row, false);
        let style = ui_kit::styled_row(state, settings.high_contrast);
        *background = BackgroundColor(style.background);
        *border = BorderColor::all(style.accent);
    }
    let (Ok((mut scroll, mut node)), Some((height, inverse))) = (lists.single_mut(), row_height)
    else {
        return;
    };
    if let Some(window) = ui_kit::list_view(view.row, count, height, inverse, scroll.0.y) {
        let wanted = px(window.max_height);
        if node.max_height != wanted {
            node.max_height = wanted;
        }
        if (window.scroll - scroll.0.y).abs() > 0.5 {
            scroll.0.y = window.scroll;
        }
    }
}

/// Rebuild the list when the category, the filter or the sort has
/// moved since it was drawn.
///
/// The statistics screen does the same inline in its navigation
/// system; here it is its own system because three different keys can
/// change the shape, and a rebuild started by two of them in one
/// frame would spawn the screen twice.
#[allow(clippy::needless_pass_by_value)] // Bevy system params
fn redraw_when_the_list_changes(
    view: Res<AchievementsView>,
    mut commands: Commands,
    screens: Query<(Entity, &DrawnFrom)>,
) {
    let wanted = DrawnFrom::of(&view);
    let mut stale = false;
    for (entity, drawn) in &screens {
        if *drawn != wanted {
            commands.entity(entity).despawn();
            stale = true;
        }
    }
    if stale {
        commands.run_system_cached(spawn_screen);
    }
}

/// Leave nothing behind — including WHO the screen was opened for.
///
/// The roster sets that when it opens the list for a named player,
/// and nothing else ever cleared it: after one visit through the
/// roster, ACHIEVEMENTS on the main menu went on showing that player
/// instead of whoever is at the guitar.
fn despawn_screen(
    mut commands: Commands,
    entities: Query<Entity, With<AchievementsScreen>>,
    mut chosen: ResMut<AchievementsFor>,
) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    chosen.0 = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A view with everything shown in catalogue order.
    fn plain() -> AchievementsView {
        AchievementsView::default()
    }

    /// Progress where nothing is earned and one entry is part-way.
    fn nothing() -> Vec<Progress> {
        CATALOGUE
            .iter()
            .map(|_| Progress {
                fraction: 0.0,
                have: 0.0,
                need: 1.0,
            })
            .collect()
    }

    fn index_of(id: &str) -> usize {
        CATALOGUE
            .iter()
            .position(|entry| entry.id == id)
            .expect("a catalogue id")
    }

    /// An app that can run `spawn_screen` for real: the resources it
    /// reads, a font handle, and nothing else.
    fn wired(
        history: Vec<beatbyte_core::history::PlayEntry>,
        roster: beatbyte_core::player::Roster,
        store: Unlocked,
    ) -> App {
        let mut app = App::new();
        app.add_plugins((bevy::MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Font>()
            .insert_resource(crate::players::Players(roster))
            .insert_resource(crate::history::PlayHistory(history))
            .insert_resource(store)
            .init_resource::<crate::config::Settings>()
            .init_resource::<AchievementsFor>()
            .init_resource::<AchievementsView>();
        let font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .reserve_handle();
        app.insert_resource(UiFont::from_handle(font));
        app
    }

    /// One finished run, filed under a player.
    fn a_run(player: PlayerId) -> beatbyte_core::history::PlayEntry {
        beatbyte_core::history::PlayEntry {
            title: "Whole Lotta Love".to_owned(),
            artist: "Led Zeppelin".to_owned(),
            difficulty: "medium".to_owned(),
            started_ms: 1_756_000_000_000,
            played_s: 120.0,
            track_s: Some(120.0),
            completed: true,
            players: 1,
            practice: false,
            autopilot: false,
            score: 1000,
            accuracy: 0.85,
            source: "file".to_owned(),
            player: Some(player),
            detail: beatbyte_core::history::RunDetail::default(),
            co_players: Vec::new(),
            chart_hash: None,
            genre: None,
            tap_mode: Some(false),
            no_fail: Some(false),
            speed_percent: Some(100),
        }
    }

    /// Build the screen for real and read back every line of text on
    /// it.
    fn screen_text(
        history: Vec<beatbyte_core::history::PlayEntry>,
        store: Unlocked,
    ) -> Vec<String> {
        let mut roster = beatbyte_core::player::Roster::default();
        roster
            .add("Martin", 1)
            .expect("a fresh roster takes a name");
        let mut app = wired(history, roster, store);
        app.add_systems(Update, spawn_screen);
        app.update();
        let screens = app
            .world_mut()
            .query_filtered::<Entity, With<AchievementsScreen>>()
            .iter(app.world())
            .count();
        assert_eq!(screens, 1, "spawned {screens} screens");
        app.world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect()
    }

    #[test]
    fn the_screen_builds_and_keeps_its_secrets_on_screen() {
        // The screen-level half of the secrecy rule. `row_lines` is
        // pure and pinned, but what protects a secret is what ends up
        // in a `Text` node — and on a machine whose screen is locked,
        // reading the built UI back is the only way to see that.
        let earned_nothing = screen_text(Vec::new(), Unlocked::default());
        assert!(
            earned_nothing.iter().any(|line| line.contains("? ? ?")),
            "no secret is covered on the screen"
        );
        for leak in ["LOVE SONG", "VALENTINE", "REDEMPTION", "AIR GUITAR"] {
            assert!(
                !earned_nothing.iter().any(|line| line.contains(leak)),
                "the screen printed `{leak}` for an achievement nobody has earned"
            );
        }
        // A plain achievement is NOT covered: that is the contrast
        // the rule depends on, and a screen that hid everything would
        // pass the checks above.
        assert!(
            earned_nothing
                .iter()
                .any(|line| line.contains("PLUGGED IN")),
            "a visible achievement was covered too"
        );

        // Earned, the secret tells its whole story.
        let mut store = Unlocked::default();
        store
            .0
            .entry(1)
            .or_default()
            .merge(&[("cal_valentine".to_owned(), 1_771_070_400_000)]);
        let found = screen_text(vec![a_run(1)], store);
        assert!(
            found.iter().any(|line| line.contains("LOVE SONG")),
            "an earned secret is still covered"
        );
        assert!(
            found.iter().any(|line| line.contains("2026-02-14")),
            "the day it happened is not on the screen"
        );
    }

    #[test]
    fn every_line_of_every_row_refuses_to_wrap() {
        // `ui_kit::list_view` measures ONE row and scrolls as though
        // every row were that tall. A blurb that wrapped would make
        // its own row taller and put the scroll out by a line — and
        // the longest blurb in the catalogue is 61 characters beside
        // a 23-character title, a bar and a date, which is close
        // enough to the panel's width to matter.
        let mut roster = beatbyte_core::player::Roster::default();
        roster
            .add("Martin", 1)
            .expect("a fresh roster takes a name");
        let mut app = wired(vec![a_run(1)], roster, Unlocked::default());
        app.add_systems(Update, spawn_screen);
        app.update();
        let (mut texts, mut wrapped) = (0, 0);
        let mut query = app.world_mut().query::<(&Text, Option<&TextLayout>)>();
        for (_, layout) in query.iter(app.world()) {
            texts += 1;
            if layout.is_none_or(|layout| layout.linebreak != LineBreak::NoWrap) {
                wrapped += 1;
            }
        }
        assert!(
            texts > 100,
            "the screen drew {texts} lines, so this proves little"
        );
        // The header, the subtitle, the tabs and the footer are free
        // to wrap — they are not rows. Every ROW line must not.
        assert!(
            wrapped <= 16,
            "{wrapped} of {texts} lines can wrap; the rows must not"
        );
    }

    #[test]
    fn changing_the_filter_actually_changes_the_list_on_screen() {
        // The three controls the commission asks for are a category,
        // a filter and a sort. Each of them only edits a resource —
        // if nothing rebuilds the list, all three are inert and the
        // screen lies about what it is showing.
        let mut roster = beatbyte_core::player::Roster::default();
        roster
            .add("Martin", 1)
            .expect("a fresh roster takes a name");
        let mut store = Unlocked::default();
        store
            .0
            .entry(1)
            .or_default()
            .merge(&[("first_run".to_owned(), 1_000)]);
        // No history on purpose: a run would earn a handful of
        // achievements through `evaluate` as well as the one in the
        // store, and then "EARNED shows one row" would be wrong for a
        // reason that has nothing to do with the rebuild.
        let mut app = wired(Vec::new(), roster, store);
        // Spawn once, the way `OnEnter` does, and leave only the
        // redraw in `Update`. Running the spawner every frame would
        // stack screens and the count below would mean nothing.
        app.add_systems(Startup, spawn_screen)
            .add_systems(Update, redraw_when_the_list_changes);
        app.update();

        let rows = |app: &mut App| {
            app.world_mut()
                .query::<&AchievementRow>()
                .iter(app.world())
                .count()
        };
        let all = rows(&mut app);
        assert_eq!(all, CATALOGUE.len(), "the unfiltered list is the catalogue");

        // EARNED: one row, and the screen must actually show one row.
        app.world_mut().resource_mut::<AchievementsView>().show = Show::Earned;
        app.update();
        assert_eq!(
            rows(&mut app),
            1,
            "the filter changed but the list on screen did not"
        );

        // A category narrows it too, and going back widens it again —
        // a rebuild that only ever shrinks would pass the check above.
        app.world_mut().resource_mut::<AchievementsView>().show = Show::All;
        app.world_mut().resource_mut::<AchievementsView>().group = Some(Category::Calendar);
        app.update();
        let calendar = rows(&mut app);
        assert!(calendar > 0 && calendar < all, "{calendar} calendar rows");

        app.world_mut().resource_mut::<AchievementsView>().group = None;
        app.update();
        assert_eq!(rows(&mut app), all, "the list did not widen again");

        // Exactly one screen at a time: a rebuild that forgets to
        // despawn draws the old list underneath the new one.
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<AchievementsScreen>>()
                .iter(app.world())
                .count(),
            1
        );
    }

    #[test]
    fn moving_the_cursor_does_not_rebuild_the_list() {
        // The counterpart: rebuilding on every arrow key would throw
        // the scroll position away on a hundred-row list.
        let mut roster = beatbyte_core::player::Roster::default();
        roster
            .add("Martin", 1)
            .expect("a fresh roster takes a name");
        let mut app = wired(vec![a_run(1)], roster, Unlocked::default());
        // Spawn once, the way `OnEnter` does, and leave only the
        // redraw in `Update`. Running the spawner every frame would
        // stack screens and the count below would mean nothing.
        app.add_systems(Startup, spawn_screen)
            .add_systems(Update, redraw_when_the_list_changes);
        app.update();
        let first = app
            .world_mut()
            .query_filtered::<Entity, With<AchievementsScreen>>()
            .iter(app.world())
            .next()
            .expect("a screen");
        app.world_mut().resource_mut::<AchievementsView>().row = 7;
        app.update();
        let now = app
            .world_mut()
            .query_filtered::<Entity, With<AchievementsScreen>>()
            .iter(app.world())
            .next()
            .expect("a screen");
        assert_eq!(first, now, "the cursor move rebuilt the whole list");
    }

    #[test]
    fn a_visit_through_the_roster_does_not_stick_to_the_screen() {
        // The roster opens the list for a named player. If that
        // choice survives the visit, ACHIEVEMENTS on the main menu
        // goes on showing whoever you last inspected instead of
        // whoever is at the guitar — and there is nothing on the
        // screen to say so.
        let mut roster = beatbyte_core::player::Roster::default();
        let mine = roster
            .add("Martin", 1)
            .expect("a fresh roster takes a name");
        let theirs = roster.add("Kim", 2).expect("a second name is free");
        roster.select(mine);

        let mut app = wired(Vec::new(), roster, Unlocked::default());
        app.world_mut().resource_mut::<AchievementsFor>().0 = Some(theirs);
        app.add_systems(Startup, spawn_screen)
            .add_systems(Update, despawn_screen);
        app.update();
        assert_eq!(
            app.world().resource::<AchievementsFor>().0,
            None,
            "leaving the screen kept the roster's choice"
        );

        // And with it cleared, the screen is the playing player's.
        let (id, name) = subject(
            app.world().resource::<AchievementsFor>(),
            app.world().resource::<crate::players::Players>(),
        )
        .expect("somebody is playing");
        assert_eq!((id, name.as_str()), (mine, "Martin"));
    }

    #[test]
    fn escape_goes_back_where_the_screen_was_opened_from() {
        assert_eq!(back_to(true), AppState::Players);
        assert_eq!(back_to(false), AppState::MainMenu);
    }

    #[test]
    fn an_empty_roster_is_told_rather_than_drawn_around() {
        // The first-run path: ACHIEVEMENTS from the main menu before
        // anybody has been created. It must say what to do, not draw
        // an empty frame and not panic on the missing player.
        let mut app = wired(
            Vec::new(),
            beatbyte_core::player::Roster::default(),
            Unlocked::default(),
        );
        app.add_systems(Update, spawn_screen);
        app.update();
        let lines: Vec<String> = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect();
        assert!(
            lines.iter().any(|line| line.contains("NO PLAYER CHOSEN")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("PLAYERS SCREEN")),
            "the screen does not say where to go: {lines:?}"
        );
        // And no row: a list of a hundred locked achievements under
        // nobody's name would be a promise to a player who does not
        // exist.
        assert!(
            !lines.iter().any(|line| line.contains("? ? ?")),
            "{lines:?}"
        );
    }

    #[test]
    fn the_history_reaches_the_screen() {
        // A bare "some nodes exist" pin passes with every row
        // short-circuited, which is the blind spot the statistics
        // screen's drill was caught by. Play must change what is
        // drawn: the standing line counts, so it moves.
        let nothing = screen_text(Vec::new(), Unlocked::default());
        let mut store = Unlocked::default();
        store.0.entry(1).or_default().merge(&[
            ("first_run".to_owned(), 1_000),
            ("first_finish".to_owned(), 2_000),
        ]);
        let played = screen_text(vec![a_run(1)], store);
        let standing_of = |lines: &[String]| {
            lines
                .iter()
                .find(|line| line.contains("SECRETS FOUND"))
                .cloned()
                .expect("the standing line is on the screen")
        };
        assert!(standing_of(&nothing).starts_with("0 OF 300"));
        assert!(standing_of(&played).starts_with("2 OF 300"));
        assert!(
            played.iter().any(|line| line.contains("1970-01-01")),
            "an unlock date is not on the screen"
        );
    }

    #[test]
    fn a_secret_gives_up_nothing_until_it_is_earned() {
        // Not its name, not its description, and NOT its progress: a
        // bar at three tenths says "this is a count of ten", which is
        // most of the condition.
        let at = index_of("cal_valentine");
        let mut progress = nothing();
        progress[at] = Progress {
            fraction: 0.5,
            have: 1.0,
            need: 2.0,
        };
        let rows = visible_rows(&progress, &Unlocks::default(), &plain());
        let secret = rows.iter().find(|row| row.at == at).expect("listed");
        assert!(secret.secret());
        let (title, blurb, right) = row_lines(secret);
        assert_eq!(title, "? ? ?");
        assert!(
            !blurb.contains("VALENTINE") && !blurb.contains("LOVE"),
            "{blurb}"
        );
        assert_eq!(right, "HIDDEN");
        assert!(
            (bar_fraction(secret) - 0.0).abs() < f32::EPSILON,
            "the bar leaked the progress of a secret"
        );

        // Earned, it tells the whole story — that is the reward.
        let mut unlocks = Unlocks::default();
        unlocks.merge(&[("cal_valentine".to_owned(), 1_771_070_400_000)]);
        let rows = visible_rows(&progress, &unlocks, &plain());
        let found = rows.iter().find(|row| row.at == at).expect("listed");
        let (title, blurb, right) = row_lines(found);
        assert_eq!(title, "LOVE SONG");
        assert!(blurb.contains("LOVE"), "{blurb}");
        assert_eq!(right, "2026-02-14", "the day it happened");
        assert!((bar_fraction(found) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_visible_achievement_shows_how_far_along_it_is() {
        // The opposite case, and the reason the secrecy rule has to
        // be a rule: a plain achievement is a GOAL, and a goal with
        // no distance marked is a wall.
        let at = index_of("end_runs_10");
        let mut progress = nothing();
        progress[at] = Progress {
            fraction: 0.4,
            have: 4.0,
            need: 10.0,
        };
        let rows = visible_rows(&progress, &Unlocks::default(), &plain());
        let row = rows.iter().find(|row| row.at == at).expect("listed");
        let (_, _, right) = row_lines(row);
        assert_eq!(right, "4 / 10");
        assert!((bar_fraction(row) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn the_store_decides_what_is_earned_not_the_current_rule() {
        // A threshold raised after somebody earned it must not take
        // the unlock back — the store is the authority, and the
        // fraction is only the road to it.
        let at = index_of("end_runs_100");
        let mut unlocks = Unlocks::default();
        unlocks.merge(&[("end_runs_100".to_owned(), 5_000)]);
        let rows = visible_rows(&nothing(), &unlocks, &plain());
        let row = rows.iter().find(|row| row.at == at).expect("listed");
        assert!(row.earned(), "an earned achievement was taken back");
        assert!((bar_fraction(row) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_filter_and_the_category_narrow_the_list() {
        let mut unlocks = Unlocks::default();
        unlocks.merge(&[("first_run".to_owned(), 1_000)]);
        let mut view = plain();
        assert_eq!(
            visible_rows(&nothing(), &unlocks, &view).len(),
            CATALOGUE.len()
        );

        view.show = Show::Earned;
        let earned = visible_rows(&nothing(), &unlocks, &view);
        assert_eq!(earned.len(), 1);
        assert_eq!(earned[0].entry().id, "first_run");

        view.show = Show::Locked;
        assert_eq!(
            visible_rows(&nothing(), &unlocks, &view).len(),
            CATALOGUE.len() - 1
        );

        view.show = Show::All;
        view.group = Some(Category::Calendar);
        let calendar = visible_rows(&nothing(), &unlocks, &view);
        assert!(!calendar.is_empty());
        assert!(
            calendar
                .iter()
                .all(|row| row.entry().category == Category::Calendar),
            "a foreign category leaked into the list"
        );
    }

    #[test]
    fn the_closest_sort_answers_what_next() {
        // Nearly done first, finished ones out of the way: the whole
        // point of the order is the next thing to go for.
        let mut progress = nothing();
        progress[index_of("end_runs_100")] = Progress {
            fraction: 0.9,
            have: 90.0,
            need: 100.0,
        };
        progress[index_of("end_runs_250")] = Progress {
            fraction: 0.36,
            have: 90.0,
            need: 250.0,
        };
        let mut unlocks = Unlocks::default();
        unlocks.merge(&[("first_run".to_owned(), 1_000)]);
        let view = AchievementsView {
            sort: Sort::Closest,
            ..plain()
        };
        let rows = visible_rows(&progress, &unlocks, &view);
        assert_eq!(
            rows[0].entry().id,
            "end_runs_100",
            "the nearest is not first"
        );
        assert_eq!(rows[1].entry().id, "end_runs_250");
        assert!(
            rows.last().expect("rows").earned(),
            "a finished achievement is not out of the way"
        );
    }

    #[test]
    fn the_newest_sort_puts_the_last_unlock_on_top() {
        let mut unlocks = Unlocks::default();
        unlocks.merge(&[
            ("first_run".to_owned(), 1_000),
            ("first_finish".to_owned(), 9_000),
        ]);
        let view = AchievementsView {
            sort: Sort::Newest,
            ..plain()
        };
        let rows = visible_rows(&nothing(), &unlocks, &view);
        assert_eq!(rows[0].entry().id, "first_finish");
        assert_eq!(rows[1].entry().id, "first_run");
        // Unearned ones have no date, and must not be sorted as if
        // they had one: they keep catalogue order at the bottom.
        let tail: Vec<usize> = rows[2..].iter().map(|row| row.at).collect();
        let mut sorted = tail.clone();
        sorted.sort_unstable();
        assert_eq!(tail, sorted);
    }

    #[test]
    fn the_category_cursor_stops_at_the_ends_like_every_other_list() {
        let mut view = plain();
        assert_eq!(view.group_label(), "EVERYTHING");
        view.step_group(-1);
        assert_eq!(view.group, None, "the cursor wrapped off the left end");
        view.step_group(1);
        assert_eq!(view.group, Some(Category::ALL[0]));
        for _ in 0..40 {
            view.step_group(1);
        }
        assert_eq!(
            view.group,
            Some(Category::ALL[Category::ALL.len() - 1]),
            "the cursor wrapped off the right end"
        );
        // Changing category puts the cursor back at the top: row 40
        // of a ten-row category is off the end of the list.
        view.row = 40;
        view.step_group(-1);
        assert_eq!(view.row, 0);
        // But a press that changes nothing must change nothing. At
        // the far right the category clamps, and throwing the cursor
        // back to the top of three hundred rows for that would be a jump
        // the player cannot account for.
        for _ in 0..40 {
            view.step_group(1);
        }
        view.row = 37;
        view.step_group(1);
        assert_eq!(view.row, 37, "a clamped press moved the cursor");
        assert_eq!(view.group, Some(Category::ALL[Category::ALL.len() - 1]));
    }

    #[test]
    fn the_standing_counts_the_whole_catalogue() {
        let line = standing(75, 3);
        assert!(
            line.contains(&format!("75 OF {}", CATALOGUE.len())),
            "{line}"
        );
        assert!(line.contains("25%"), "{line}");
        assert!(line.contains("3 SECRETS FOUND"), "{line}");
        // Nothing earned reads as zero, not as an error.
        assert!(standing(0, 0).contains("0%"));
    }

    #[test]
    fn the_footer_names_the_state_of_every_switch() {
        // A cycling key whose current value is not on screen is a
        // key nobody presses twice — the chips carry the state.
        let hint = footer_hint(Show::Locked, Sort::Closest, Some(Tier::Rare));
        assert!(hint.contains("FILTER LOCKED"), "{hint}");
        assert!(hint.contains("ORDER CLOSEST"), "{hint}");
        assert!(hint.contains("TIER RARE"), "{hint}");
        // Neither switch may sit on a key the menu table already
        // steers with: W/A/S/D are the four directions, so a sort on
        // `S` would cycle the order and walk the cursor at once.
        let directions = [
            crate::controls::UiAction::NavUp,
            crate::controls::UiAction::NavDown,
            crate::controls::UiAction::NavLeft,
            crate::controls::UiAction::NavRight,
        ];
        let map = crate::controls::InputMap::default();
        for key in [KeyCode::KeyF, KeyCode::KeyO, KeyCode::KeyT] {
            for action in directions {
                assert!(
                    !map.ui_of(action)
                        .contains(&crate::controls::Binding::Key(key)),
                    "{key:?} is both a screen switch and a menu direction"
                );
            }
        }
        // Both cycles come back round.
        assert_eq!(Show::All.next().next().next(), Show::All);
        assert_eq!(Sort::Catalogue.next().next().next(), Sort::Catalogue);
    }

    #[test]
    fn the_tier_filter_narrows_the_list() {
        let mut view = plain();
        view.tier = Some(Tier::Rare);
        let rows = visible_rows(&nothing(), &Unlocks::default(), &view);
        assert!(!rows.is_empty());
        assert!(
            rows.iter().all(|row| row.entry().tier == Tier::Rare),
            "a foreign tier leaked into the list"
        );
        view.next_tier();
        assert_eq!(view.tier, None);
    }

    #[test]
    fn category_tabs_carry_how_many_are_earned() {
        let mut unlocks = Unlocks::default();
        unlocks.merge(&[("first_run".to_owned(), 1_000)]);
        let (earned, total) = category_progress(Category::FirstSteps, &unlocks);
        assert_eq!(earned, 1);
        assert!(total > 1);
        let label = category_tab_label(Category::FirstSteps, &unlocks);
        assert!(label.contains("FIRST STEPS"), "{label}");
        assert!(label.contains(&format!("1/{total}")), "{label}");
    }
}
