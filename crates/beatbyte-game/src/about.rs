//! The About screen: who made this, under what license, where it
//! lives — and a changelog that keeps itself current.
//!
//! The changelog is **not** a hand-maintained list in this file: the
//! repository's `CHANGELOG.md` is compiled in via [`include_str!`]
//! and parsed at startup. The house rules already force every
//! user-visible change into that file in the same commit (and
//! `docs_stay_true` fails the build if the manifest version has no
//! section there), so the next release appears here without anyone
//! touching this screen.

use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::controls::MenuNav;
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// The changelog, exactly as the repository maintains it.
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// The version this binary was built as.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the project lives. Single sources for every link on the
/// screen, so a moved repository is one edit.
const REPO_URL: &str = "https://github.com/pepperonas/beatbyte";
/// The MIT license text inside the repository.
const LICENSE_URL: &str = "https://github.com/pepperonas/beatbyte/blob/main/LICENSE";
/// The author's site.
const WEBSITE_URL: &str = "https://celox.io";
/// The Google-Maps review page for celox.io.
const REVIEW_URL: &str = "https://g.page/r/CXgdRV3QysvxEBM/review";
/// PayPal donation link (same target the README uses).
const DONATE_URL: &str = "https://www.paypal.com/donate/?business=martin.pfeffer%40celox.io";
/// Contact, handed to the system mail client.
const CONTACT_URL: &str = "mailto:martin.pfeffer@celox.io";

/// One version section of the changelog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogEntry {
    /// The version, without the brackets (`0.13.1`).
    pub version: String,
    /// The release date as written (`2026-09-01`).
    pub date: String,
    /// The section's body, flattened to plain prose for the detail
    /// line (markdown list markers and sub-headings stripped).
    pub summary: String,
}

/// Parse a Keep-a-Changelog document into entries, newest first
/// (the file's own order). Pure — tested.
#[must_use]
pub fn parse_changelog(text: &str) -> Vec<ChangelogEntry> {
    let mut entries: Vec<ChangelogEntry> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## [") {
            let Some((version, tail)) = rest.split_once(']') else {
                continue;
            };
            let date = tail.trim_start_matches([' ', '-']).trim().to_owned();
            entries.push(ChangelogEntry {
                version: version.trim().to_owned(),
                date,
                summary: String::new(),
            });
        } else if let Some(current) = entries.last_mut() {
            // Body lines: drop the `### Added` style sub-headings
            // (the prose carries the information), flatten bullets.
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("###") {
                continue;
            }
            let content = trimmed.trim_start_matches("- ").trim();
            if !current.summary.is_empty() {
                current.summary.push(' ');
            }
            current.summary.push_str(content);
        }
    }
    entries
}

/// Open a target in the system's default handler (browser for
/// `https:`, mail client for `mailto:`). Fire-and-forget: a machine
/// without a handler logs a warning and the game moves on.
fn open_external(url: &str) {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    if let Err(error) = result {
        warn!("cannot open {url}: {error}");
    }
}

/// The fixed rows above the changelog, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfoRow {
    Author,
    License,
    Repo,
    Website,
    Review,
    Donate,
    Contact,
    Changelog,
}

impl InfoRow {
    const ALL: [InfoRow; 8] = [
        InfoRow::Author,
        InfoRow::License,
        InfoRow::Repo,
        InfoRow::Website,
        InfoRow::Review,
        InfoRow::Donate,
        InfoRow::Contact,
        InfoRow::Changelog,
    ];

    const fn label(self) -> &'static str {
        match self {
            InfoRow::Author => "MADE BY",
            InfoRow::License => "LICENSE",
            InfoRow::Repo => "SOURCE CODE",
            InfoRow::Website => "WEBSITE",
            InfoRow::Review => "RATE CELOX.IO",
            InfoRow::Donate => "DONATE",
            InfoRow::Contact => "CONTACT",
            InfoRow::Changelog => "CHANGELOG",
        }
    }

    /// The external target a confirm opens, where the row has one.
    const fn target(self) -> Option<&'static str> {
        match self {
            InfoRow::Author => None,
            InfoRow::License => Some(LICENSE_URL),
            InfoRow::Repo => Some(REPO_URL),
            InfoRow::Website => Some(WEBSITE_URL),
            InfoRow::Review => Some(REVIEW_URL),
            InfoRow::Donate => Some(DONATE_URL),
            InfoRow::Contact => Some(CONTACT_URL),
            InfoRow::Changelog => None,
        }
    }
}

/// Screen state: cursor, the parsed changelog, and whether the
/// changelog section is open (default: collapsed).
#[derive(Resource, Default)]
struct AboutState {
    cursor: usize,
    expanded: bool,
    entries: Vec<ChangelogEntry>,
}

impl AboutState {
    fn row_count(&self) -> usize {
        InfoRow::ALL.len() + if self.expanded { self.entries.len() } else { 0 }
    }
}

/// Plugin for the About screen.
pub struct AboutPlugin;

impl Plugin for AboutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AboutState>()
            .add_systems(OnEnter(AppState::About), spawn_about)
            .add_systems(
                Update,
                (about_input, refresh_about, follow_about_cursor).run_if(in_state(AppState::About)),
            )
            .add_systems(OnExit(AppState::About), despawn_about);
    }
}

#[derive(Component)]
struct AboutScreen;

/// The scrolling list of rows.
#[derive(Component)]
struct AboutList;

/// A row by flat index.
#[derive(Component)]
struct AboutRow(usize);

/// A row's label.
#[derive(Component)]
struct AboutLabel(usize);

/// A row's value.
#[derive(Component)]
struct AboutValue(usize);

/// The wrapped detail line under the panel (the highlighted
/// changelog entry's summary).
#[derive(Component)]
struct AboutDetail;

/// The detail line's query, aliased for the lint's sake: it must
/// exclude the row texts to satisfy Bevy's aliasing rules.
type DetailText<'w, 's> =
    Query<'w, 's, &'static mut Text, (With<AboutDetail>, Without<AboutLabel>, Without<AboutValue>)>;

fn spawn_about(mut commands: Commands, font: Res<UiFont>, mut state: ResMut<AboutState>) {
    state.cursor = 0;
    // Default: collapsed. The harness may pre-expand for the
    // screenshot that proves the expanded state renders
    // (`BEATBYTE_ABOUT_EXPANDED=1`).
    state.expanded = std::env::var_os("BEATBYTE_ABOUT_EXPANDED").is_some();
    state.entries = parse_changelog(CHANGELOG);

    commands
        .spawn((AboutScreen, ui_kit::screen_root()))
        .with_children(|parent| {
            ui_kit::header(
                parent,
                &font,
                "ABOUT",
                &format!("BeatByte v{VERSION} - MIT license, (c) 2026 Martin Pfeffer"),
            );
            parent
                .spawn((AboutList, ui_kit::scroll_panel(ui_kit::PANEL_WIDTH)))
                .with_children(|panel| {
                    // The maximum row count (all info rows + every
                    // changelog entry) is spawned once; refresh hides
                    // the changelog rows while the section is closed.
                    let total = InfoRow::ALL.len() + state.entries.len();
                    for index in 0..total {
                        panel
                            .spawn((AboutRow(index), Button, ui_kit::row()))
                            .with_children(|row| {
                                row.spawn((
                                    AboutLabel(index),
                                    Text::new(""),
                                    font.text(ui_kit::ROW),
                                    TextColor(palette::TEXT_DIM),
                                    ui_kit::label_node(),
                                ));
                                row.spawn((
                                    AboutValue(index),
                                    Text::new(""),
                                    font.text(ui_kit::ROW),
                                    TextColor(palette::TEXT_DIM),
                                    ui_kit::value_node(),
                                ));
                            });
                    }
                });
            // The highlighted changelog entry's summary, wrapped.
            parent.spawn((
                AboutDetail,
                Text::new(""),
                font.text(ui_kit::SMALL),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.85)),
                Node {
                    max_width: px(ui_kit::PANEL_WIDTH),
                    margin: UiRect::top(px(12)),
                    ..default()
                },
            ));
            crate::prompts::device_footer(
                parent,
                &font,
                "UP/DOWN choose  ENTER open  ESC back",
                "D-PAD choose  SOUTH open  EAST back",
            );
        });
}

/// What confirming the flat row at `index` does.
enum Activate {
    Open(&'static str),
    ToggleChangelog,
    Nothing,
}

fn activation(index: usize) -> Activate {
    match InfoRow::ALL.get(index) {
        Some(InfoRow::Changelog) => Activate::ToggleChangelog,
        Some(row) => row.target().map_or(Activate::Nothing, Activate::Open),
        // Changelog entry rows: informational.
        None => Activate::Nothing,
    }
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn about_input(
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<crate::controls::InputMap>,
    pads: Query<&Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    rows: Query<(&AboutRow, &Interaction), Changed<Interaction>>,
    mut state: ResMut<AboutState>,
    mut next_state: ResMut<NextState<AppState>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    let nav = MenuNav::read(&map, &keys, pads.iter());
    let count = state.row_count();
    if nav.up {
        state.cursor = (state.cursor + count - 1) % count;
    }
    if nav.down {
        state.cursor = (state.cursor + 1) % count;
    }
    if nav.up || nav.down {
        sounds.write(crate::sfx::UiSound::Navigate);
    }
    let pointer = ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
    if let Some(index) = pointer.hovered
        && index < count
    {
        state.cursor = index;
    }
    if nav.confirm || pointer.clicked {
        match activation(state.cursor) {
            Activate::Open(url) => {
                sounds.write(crate::sfx::UiSound::Confirm);
                open_external(url);
            }
            Activate::ToggleChangelog => {
                state.expanded = !state.expanded;
                sounds.write(crate::sfx::UiSound::Toggle);
            }
            Activate::Nothing => {}
        }
    }
    if nav.back || mouse.just_pressed(MouseButton::Right) {
        sounds.write(crate::sfx::UiSound::Back);
        next_state.set(AppState::MainMenu);
    }
}

/// Labels, values, visibility and highlight — driven from the state
/// every frame, like every other screen.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn refresh_about(
    state: Res<AboutState>,
    settings: Res<crate::config::Settings>,
    mut rows: Query<(&AboutRow, &mut Node, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<(&AboutLabel, &mut Text, &mut TextColor), Without<AboutValue>>,
    mut values: Query<(&AboutValue, &mut Text, &mut TextColor), Without<AboutLabel>>,
    mut detail: DetailText,
) {
    let count = state.row_count();
    let style_of = |index: usize| {
        ui_kit::styled_row(
            ui_kit::state_for(index == state.cursor, false),
            settings.high_contrast,
        )
    };
    let info = InfoRow::ALL.len();
    let text_for = |index: usize| -> (String, String) {
        if let Some(row) = InfoRow::ALL.get(index) {
            let value = match row {
                InfoRow::Author => "Martin Pfeffer - celox.io - 2026".to_owned(),
                InfoRow::License => "MIT".to_owned(),
                InfoRow::Repo => "github.com/pepperonas/beatbyte".to_owned(),
                InfoRow::Website => "celox.io".to_owned(),
                InfoRow::Review => "Google Maps".to_owned(),
                InfoRow::Donate => "PayPal - martin.pfeffer@celox.io".to_owned(),
                InfoRow::Contact => "martin.pfeffer@celox.io".to_owned(),
                InfoRow::Changelog => {
                    let arrow = if state.expanded { "close" } else { "open" };
                    format!("{} versions - ENTER to {arrow}", state.entries.len())
                }
            };
            (row.label().to_owned(), value)
        } else if let Some(entry) = state.entries.get(index - info) {
            (format!("  v{}", entry.version), entry.date.clone())
        } else {
            (String::new(), String::new())
        }
    };
    for (row, mut node, mut background, mut border) in &mut rows {
        let shown = row.0 < count;
        let wanted = if shown { Display::Flex } else { Display::None };
        if node.display != wanted {
            node.display = wanted;
        }
        if !shown {
            continue;
        }
        let style = style_of(row.0);
        background.0 = style.background;
        *border = BorderColor::all(style.accent);
    }
    for (label, mut text, mut color) in &mut labels {
        let (wanted, _) = text_for(label.0);
        if text.0 != wanted {
            text.0 = wanted;
        }
        color.0 = style_of(label.0).label;
    }
    for (value, mut text, mut color) in &mut values {
        let (_, wanted) = text_for(value.0);
        if text.0 != wanted {
            text.0 = wanted;
        }
        color.0 = style_of(value.0).value;
    }
    if let Ok(mut text) = detail.single_mut() {
        let wanted = state
            .entries
            .get(state.cursor.wrapping_sub(info))
            .filter(|_| state.cursor >= info)
            .map(|entry| clipped_summary(&entry.summary, 220))
            .unwrap_or_default();
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

/// The first `limit` characters of a summary, cut at a word edge
/// with an honest ellipsis — the file remains the full record. Pure.
#[must_use]
pub fn clipped_summary(summary: &str, limit: usize) -> String {
    if summary.chars().count() <= limit {
        return summary.to_owned();
    }
    let clipped: String = summary.chars().take(limit).collect();
    let cut = clipped.rfind(' ').unwrap_or(clipped.len());
    format!("{} ...", &clipped[..cut])
}

/// Keep the cursor row in view — the measured whole-row window every
/// scrolling screen uses.
fn follow_about_cursor(
    state: Res<AboutState>,
    rows: Query<(&AboutRow, &ComputedNode)>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition, &mut Node), With<AboutList>>,
) {
    let Ok((list, mut scroll, mut node)) = lists.single_mut() else {
        return;
    };
    let Some(row_h) = rows
        .iter()
        .map(|(_, node)| node.size().y)
        .find(|height| *height > 0.0)
    else {
        return;
    };
    let count = state.row_count();
    let pitch = row_h + ui_kit::ROW_GAP;
    if let Some(height) =
        ui_kit::whole_rows_height(row_h, ui_kit::ROW_GAP, count, ui_kit::PANEL_MAX_H)
    {
        let wanted = px(height);
        if node.max_height != wanted {
            node.max_height = wanted;
        }
    } else if node.max_height != px(ui_kit::PANEL_MAX_H) {
        // Collapsed again: release the snapped window so the short
        // list sits in its natural height.
        node.max_height = px(ui_kit::PANEL_MAX_H);
    }
    let total = count as f32;
    let content_h = total.mul_add(row_h, (total - 1.0).max(0.0) * ui_kit::ROW_GAP);
    let viewport_h = list.size().y - 2.0 * ui_kit::PANEL_PAD;
    let row_top = state.cursor as f32 * pitch;
    let wanted = ui_kit::scroll_to_show(row_top, row_h, viewport_h, content_h, scroll.0.y);
    if (wanted - scroll.0.y).abs() > 0.5 {
        scroll.0.y = wanted;
    }
}

fn despawn_about(mut commands: Commands, entities: Query<Entity, With<AboutScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_changelog_parses_and_the_build_version_leads_it() {
        // The whole point of the screen: the list comes from the
        // maintained CHANGELOG.md, so the entry for THIS build must
        // be the first one - docs_stay_true guarantees the section
        // exists, this pins that the parser actually finds it.
        let entries = parse_changelog(CHANGELOG);
        assert!(
            entries.len() >= 40,
            "the changelog has {} sections - parsing lost most of it",
            entries.len()
        );
        let first = &entries[0];
        assert_eq!(
            first.version, VERSION,
            "the newest changelog entry must be this build's version"
        );
        assert!(!first.date.is_empty(), "every release carries its date");
        assert!(
            !first.summary.is_empty(),
            "the newest entry has body text to show"
        );
    }

    #[test]
    fn entries_arrive_newest_first_with_clean_summaries() {
        let entries = parse_changelog(CHANGELOG);
        // Keep-a-Changelog order IS newest-first; trust but verify
        // on the two ends.
        let newest = &entries[0].version;
        let oldest = &entries[entries.len() - 1].version;
        assert!(newest > oldest, "{newest} should sort above {oldest}");
        for entry in entries.iter().take(5) {
            assert!(
                !entry.summary.contains("###"),
                "sub-headings must be stripped from v{}",
                entry.version
            );
        }
    }

    #[test]
    fn a_synthetic_document_parses_exactly() {
        let doc = "# Changelog\n\nintro prose\n\n## [1.2.3] - 2026-01-02\n\n### Added\n\n- one thing\n- another\n\n## [1.2.2] - 2026-01-01\n\ntext body\n";
        let entries = parse_changelog(doc);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, "1.2.3");
        assert_eq!(entries[0].date, "2026-01-02");
        assert_eq!(entries[0].summary, "one thing another");
        assert_eq!(entries[1].summary, "text body");
    }

    #[test]
    fn the_summary_clips_at_a_word_and_says_so() {
        assert_eq!(clipped_summary("short", 220), "short");
        let long = "word ".repeat(100);
        let clipped = clipped_summary(&long, 40);
        assert!(clipped.ends_with(" ..."), "an honest ellipsis: {clipped}");
        assert!(clipped.chars().count() <= 44);
    }

    #[test]
    fn expanding_appends_exactly_the_changelog_rows() {
        // The screenshot of the expanded state is display-dependent
        // (a locked screen photographs black); the expansion LOGIC is
        // not: closed shows the info rows alone, open appends one row
        // per parsed version.
        let mut state = AboutState {
            entries: parse_changelog(CHANGELOG),
            ..Default::default()
        };
        assert!(!state.expanded, "the changelog starts collapsed");
        assert_eq!(state.row_count(), InfoRow::ALL.len());
        state.expanded = true;
        assert_eq!(state.row_count(), InfoRow::ALL.len() + state.entries.len());
        assert!(state.entries.len() >= 40, "all versions listed");
    }

    #[test]
    fn every_info_row_targets_what_its_label_promises() {
        // The commission's link table, as a test.
        assert_eq!(InfoRow::Repo.target(), Some(REPO_URL));
        assert_eq!(InfoRow::Website.target(), Some("https://celox.io"));
        assert_eq!(
            InfoRow::Review.target(),
            Some("https://g.page/r/CXgdRV3QysvxEBM/review")
        );
        assert_eq!(
            InfoRow::Donate.target(),
            Some("https://www.paypal.com/donate/?business=martin.pfeffer%40celox.io")
        );
        assert_eq!(
            InfoRow::Contact.target(),
            Some("mailto:martin.pfeffer@celox.io"),
            "the contact address is celox.io with a DOT - the source \
             material carried a comma typo"
        );
        // The two informational rows open nothing.
        assert_eq!(InfoRow::Author.target(), None);
        assert_eq!(InfoRow::Changelog.target(), None);
    }
}
