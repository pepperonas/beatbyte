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

// ── Layout ──────────────────────────────────────────────────────────

/// The About column is wider than the standard menu panel: the value
/// column carries full e-mail addresses and the detail block carries
/// changelog prose, and Press Start 2P advances a full em per glyph
/// (measured from the bundled TTF) — at the standard 620 px the
/// DONATE value wrapped mid-address.
const ABOUT_WIDTH: f32 = 760.0;
/// Gap between a bullet's dash marker and its text.
const BULLET_GAP: f32 = 8.0;
/// Text columns available to one bullet: the block width minus the
/// marker glyph and its gap, at [`ui_kit::ROW`]-size glyphs of 1 em
/// each. A test pins that this packs the width exactly.
const BULLET_COLUMNS: usize = 56;
/// How many bullets the detail block renders before the honest
/// "+ N more" note takes over.
const BULLETS_SHOWN: usize = 3;
/// Wrapped lines each bullet may take.
const BULLET_LINES: usize = 2;
/// The detail block's reserved height: heading, three two-line
/// bullets and the note. Fixed, so the footer does not jump while
/// the cursor moves between rows.
const DETAIL_MIN_H: f32 = 150.0;

/// One version section of the changelog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogEntry {
    /// The version, without the brackets (`0.13.1`).
    pub version: String,
    /// The release date as written (`2026-09-01`).
    pub date: String,
    /// The section's bullet points, in file order: one string per
    /// `- ` item with its continuation lines joined. Body prose
    /// without a marker becomes a bullet of its own; sub-headings
    /// (`### Added`) are dropped — the prose carries the
    /// information.
    pub bullets: Vec<String>,
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
                bullets: Vec::new(),
            });
        } else if let Some(current) = entries.last_mut() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("###") {
                continue;
            }
            if let Some(item) = trimmed.strip_prefix("- ") {
                current.bullets.push(item.trim().to_owned());
            } else if let Some(last) = current.bullets.last_mut() {
                // A continuation line of the bullet above it.
                last.push(' ');
                last.push_str(trimmed);
            } else {
                // Body prose without a marker: a bullet of its own.
                current.bullets.push(trimmed.to_owned());
            }
        }
    }
    for entry in &mut entries {
        for bullet in &mut entry.bullets {
            *bullet = plain_text(bullet);
        }
    }
    entries
}

/// Strip the Markdown a CHANGELOG bullet carries for GitHub — bold
/// and code markers, and link syntax — so the About screen shows the
/// words and not the markup. It rendered `**A band on the stage**`
/// with the asterisks for as long as the entries have used them; the
/// display face made it impossible to overlook. Pure — tested.
#[must_use]
pub fn plain_text(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("**") {
            rest = after;
        } else if let Some(after) = rest.strip_prefix('`') {
            rest = after;
        } else if rest.starts_with('[')
            && let Some(close) = rest.find("](")
            && let Some(end) = rest[close..].find(')')
        {
            // [text](url) → text
            out.push_str(&rest[1..close]);
            rest = &rest[close + end + 1..];
        } else {
            let mut chars = rest.chars();
            if let Some(c) = chars.next() {
                out.push(c);
            }
            rest = chars.as_str();
        }
    }
    out
}

/// Greedy word wrap for the game's face. Press Start 2P is a true
/// monospace advancing exactly 1 em per glyph, so at a known column
/// count this IS the layout, not an estimate. A word longer than a
/// whole line breaks hard rather than overflowing. Pure — tested.
#[must_use]
pub fn wrap_line(text: &str, columns: usize) -> Vec<String> {
    let columns = columns.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let mut word = word;
        while word.chars().count() > columns {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let head: String = word.chars().take(columns).collect();
            word = &word[head.len()..];
            lines.push(head);
        }
        let space = usize::from(!current.is_empty());
        if current.chars().count() + space + word.chars().count() > columns {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// The first `lines` wrapped lines of a text, joined for one text
/// node. When something was cut the last line ends in an honest
/// ellipsis that still fits the column limit — the file remains the
/// full record. Pure — tested.
#[must_use]
pub fn wrapped_clip(text: &str, columns: usize, lines: usize) -> String {
    let wrapped = wrap_line(text, columns);
    if wrapped.len() <= lines {
        return wrapped.join("\n");
    }
    let mut shown: Vec<String> = wrapped.into_iter().take(lines.max(1)).collect();
    if let Some(last) = shown.last_mut() {
        let keep = columns.saturating_sub(4);
        if last.chars().count() > keep {
            *last = last
                .chars()
                .take(keep)
                .collect::<String>()
                .trim_end()
                .to_owned();
        }
        last.push_str(" ...");
    }
    shown.join("\n")
}

/// Which entry the detail block shows: the highlighted one while the
/// cursor sits on an entry row, otherwise the newest — the screen
/// answers "what's new in this build" the moment it opens. Pure.
#[must_use]
pub fn detail_entry(cursor: usize, info_rows: usize, entries: usize) -> usize {
    cursor
        .checked_sub(info_rows)
        .filter(|index| *index < entries)
        .unwrap_or(0)
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
            // The maker's row opens the maker's site.
            InfoRow::Author | InfoRow::Website => Some(WEBSITE_URL),
            InfoRow::License => Some(LICENSE_URL),
            InfoRow::Repo => Some(REPO_URL),
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

/// A text of the detail block under the panel: the version heading,
/// one bullet's text, or the "+ N more" note. One marker for all
/// three keeps the refresh to a single query.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum DetailPart {
    Heading,
    Bullet(usize),
    Note,
}

/// A bullet's row node (marker + text), hidden when the highlighted
/// entry has fewer bullets.
#[derive(Component)]
struct DetailBulletRow(usize);

/// The detail texts' query, aliased for the lint's sake: it must
/// exclude the row texts to satisfy Bevy's aliasing rules.
type DetailTexts<'w, 's> = Query<
    'w,
    's,
    (&'static DetailPart, &'static mut Text),
    (Without<AboutLabel>, Without<AboutValue>),
>;

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
                .spawn((AboutList, ui_kit::scroll_panel(ABOUT_WIDTH)))
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
            // The detail block: the highlighted changelog entry (or,
            // anywhere else, this build's) as a version heading and
            // real bullet points at row size. Fixed minimum height so
            // the footer does not jump while the cursor moves.
            parent
                .spawn(Node {
                    width: px(ABOUT_WIDTH),
                    min_height: px(DETAIL_MIN_H),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    margin: UiRect::top(px(14)),
                    ..default()
                })
                .with_children(|detail| {
                    detail.spawn((
                        DetailPart::Heading,
                        Text::new(""),
                        font.text(ui_kit::ROW),
                        TextColor(palette::BRAND),
                    ));
                    for index in 0..BULLETS_SHOWN {
                        detail
                            .spawn((
                                DetailBulletRow(index),
                                Node {
                                    column_gap: px(BULLET_GAP),
                                    ..default()
                                },
                            ))
                            .with_children(|row| {
                                row.spawn((
                                    Text::new("-"),
                                    font.text(ui_kit::ROW),
                                    TextColor(palette::BRAND),
                                    Node {
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                ));
                                row.spawn((
                                    DetailPart::Bullet(index),
                                    Text::new(""),
                                    font.text(ui_kit::ROW),
                                    TextColor(palette::TEXT_DIM),
                                ));
                            });
                    }
                    detail.spawn((
                        DetailPart::Note,
                        Text::new(""),
                        font.text(ui_kit::SMALL),
                        TextColor(palette::dimmed(palette::TEXT_DIM, 0.75)),
                    ));
                });
            crate::prompts::device_footer(
                parent,
                &font,
                "UP/DOWN choose  ENTER open  ESC back",
                "D-PAD choose  SOUTH open  EAST back",
            );
            ui_kit::back_button(parent, &font, "MAIN MENU");
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
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    rows: Query<(&AboutRow, &Interaction), Changed<Interaction>>,
    mut state: ResMut<AboutState>,
    mut next_state: ResMut<NextState<AppState>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
    mut back: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        With<ui_kit::BackButton>,
    >,
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
    // The wheel scrolls the rows, like the song list.
    for event in wheel.read() {
        if event.y > 0.0 {
            state.cursor = (state.cursor + count - 1) % count;
        } else if event.y < 0.0 {
            state.cursor = (state.cursor + 1) % count;
        }
        if event.y != 0.0 {
            sounds.write(crate::sfx::UiSound::Navigate);
        }
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
    if nav.back || ui_kit::back_pressed(&mut back) || mouse.just_pressed(MouseButton::Right) {
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
    mut rows: Query<
        (&AboutRow, &mut Node, &mut BackgroundColor, &mut BorderColor),
        Without<DetailBulletRow>,
    >,
    mut labels: Query<(&AboutLabel, &mut Text, &mut TextColor), Without<AboutValue>>,
    mut values: Query<(&AboutValue, &mut Text, &mut TextColor), Without<AboutLabel>>,
    mut details: DetailTexts,
    mut bullet_rows: Query<(&DetailBulletRow, &mut Node), Without<AboutRow>>,
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
    // The detail block: highlighted entry, or the newest one.
    let shown_index = detail_entry(state.cursor, info, state.entries.len());
    let entry = state.entries.get(shown_index);
    for (part, mut text) in &mut details {
        let wanted = match (entry, *part) {
            (None, _) => String::new(),
            (Some(entry), DetailPart::Heading) => {
                let tag = if shown_index == 0 {
                    " - THIS BUILD"
                } else {
                    ""
                };
                format!("v{} - {}{tag}", entry.version, entry.date)
            }
            (Some(entry), DetailPart::Bullet(index)) => entry
                .bullets
                .get(index)
                .map(|bullet| wrapped_clip(bullet, BULLET_COLUMNS, BULLET_LINES))
                .unwrap_or_default(),
            (Some(entry), DetailPart::Note) => {
                let hidden = entry.bullets.len().saturating_sub(BULLETS_SHOWN);
                if hidden > 0 {
                    format!("+ {hidden} more in CHANGELOG.md")
                } else {
                    String::new()
                }
            }
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    let bullet_count = entry.map_or(0, |entry| entry.bullets.len().min(BULLETS_SHOWN));
    for (row, mut node) in &mut bullet_rows {
        let wanted = if row.0 < bullet_count {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }
}

/// Keep the cursor row in view — the measured whole-row window every
/// scrolling screen uses.
fn follow_about_cursor(
    state: Res<AboutState>,
    rows: Query<(&AboutRow, &ComputedNode)>,
    mut lists: Query<(&mut ScrollPosition, &mut Node), With<AboutList>>,
) {
    let Ok((mut scroll, mut node)) = lists.single_mut() else {
        return;
    };
    let Some(row) = rows
        .iter()
        .map(|(_, node)| node)
        .find(|node| node.size().y > 0.0)
    else {
        return;
    };
    ui_kit::follow_list(state.cursor, state.row_count(), row, &mut scroll, &mut node);
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
            !first.bullets.is_empty(),
            "the newest entry has bullet points to show"
        );
    }

    #[test]
    fn entries_arrive_newest_first_with_clean_bullets() {
        let entries = parse_changelog(CHANGELOG);
        // Keep-a-Changelog order IS newest-first; trust but verify
        // on the two ends.
        let newest = &entries[0].version;
        let oldest = &entries[entries.len() - 1].version;
        assert!(newest > oldest, "{newest} should sort above {oldest}");
        for entry in entries.iter().take(5) {
            for bullet in &entry.bullets {
                assert!(
                    !bullet.contains("###"),
                    "sub-headings must be stripped from v{}",
                    entry.version
                );
                assert!(
                    !bullet.starts_with("- "),
                    "list markers must be stripped from v{}",
                    entry.version
                );
            }
        }
    }

    #[test]
    fn bullets_lose_their_markdown_but_keep_their_words() {
        use super::plain_text;
        assert_eq!(
            plain_text("**A band on the stage** (round style)."),
            "A band on the stage (round style)."
        );
        assert_eq!(
            plain_text("see `docs/x.md` and [the plan](docs/p.md) now"),
            "see docs/x.md and the plan now"
        );
        assert_eq!(plain_text("plain words"), "plain words");
        // A lone asterisk is not markup and survives.
        assert_eq!(plain_text("2 * 3"), "2 * 3");
    }

    #[test]
    fn a_synthetic_document_parses_exactly() {
        let doc = "# Changelog\n\nintro prose\n\n## [1.2.3] - 2026-01-02\n\n### Added\n\n- one thing\n  spread over two lines\n- another\n\n## [1.2.2] - 2026-01-01\n\ntext body\n";
        let entries = parse_changelog(doc);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, "1.2.3");
        assert_eq!(entries[0].date, "2026-01-02");
        assert_eq!(
            entries[0].bullets,
            vec!["one thing spread over two lines", "another"],
            "a bullet's continuation lines join it; bullets stay separate"
        );
        assert_eq!(entries[1].bullets, vec!["text body"]);
    }

    #[test]
    fn wrapping_is_exact_for_the_monospace_face() {
        // Press Start 2P advances exactly 1 em per glyph, so wrapping
        // by column count IS the rendered layout. Words move whole.
        assert_eq!(wrap_line("aa bb cc", 5), vec!["aa bb", "cc"]);
        assert_eq!(wrap_line("one two", 3), vec!["one", "two"]);
        // A word longer than the line breaks hard instead of
        // overflowing into the footer.
        assert_eq!(wrap_line("abcdefgh xy", 4), vec!["abcd", "efgh", "xy"]);
        assert!(wrap_line("", 10).is_empty());
        for line in wrap_line(&"word ".repeat(50), 20) {
            assert!(line.chars().count() <= 20, "overflow: {line:?}");
        }
    }

    #[test]
    fn a_clipped_bullet_stays_inside_its_lines_and_says_so() {
        let long = "word ".repeat(100);
        let clipped = wrapped_clip(&long, BULLET_COLUMNS, BULLET_LINES);
        assert!(clipped.ends_with(" ..."), "an honest ellipsis: {clipped}");
        let lines: Vec<&str> = clipped.lines().collect();
        assert_eq!(lines.len(), BULLET_LINES, "the line budget holds");
        for line in &lines {
            assert!(
                line.chars().count() <= BULLET_COLUMNS,
                "a line overflows its columns: {line:?}"
            );
        }
        // A short bullet passes through whole, without an ellipsis.
        assert_eq!(wrapped_clip("short", BULLET_COLUMNS, BULLET_LINES), "short");
    }

    #[test]
    fn the_bullet_columns_pack_the_block_exactly() {
        // The columns constant is hand-derived from the block width;
        // this keeps it honest against both drifts: too wide and a
        // line overflows the block, too narrow and the block wastes
        // a glyph column it could use.
        let available = ABOUT_WIDTH - ui_kit::ROW - BULLET_GAP;
        assert!(
            BULLET_COLUMNS as f32 * ui_kit::ROW <= available,
            "a full line would overflow the detail block"
        );
        assert!(
            (BULLET_COLUMNS + 1) as f32 * ui_kit::ROW > available,
            "the block has room for another column"
        );
    }

    #[test]
    fn the_detail_block_always_has_an_entry_to_show() {
        let info = InfoRow::ALL.len();
        // On the info rows (changelog collapsed or not): this build.
        assert_eq!(detail_entry(0, info, 40), 0);
        assert_eq!(detail_entry(info - 1, info, 40), 0);
        // On an entry row: that entry.
        assert_eq!(detail_entry(info, info, 40), 0);
        assert_eq!(detail_entry(info + 7, info, 40), 7);
        // A stale cursor past the end falls back to this build
        // rather than indexing thin air.
        assert_eq!(detail_entry(info + 40, info, 40), 0);
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
            InfoRow::Author.target(),
            Some("https://celox.io"),
            "MADE BY opens the maker's site too (user request 2026-09-01)"
        );
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
        // The changelog row toggles; it opens nothing.
        assert_eq!(InfoRow::Changelog.target(), None);
    }
}
