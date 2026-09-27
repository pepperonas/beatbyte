//! The editor's two questions, asked in a dialog rather than a status
//! line: what to do with unsaved changes on the way out, and where a
//! save goes.
//!
//! The way out used to be a hint in the status line ("ESC again to
//! leave") that armed for three seconds — easy to miss, and the second
//! ESC threw the work away. Saving never asked at all: the first save
//! made a new revision and every later one silently rewrote it. Both
//! questions now stop the editor until they are answered, by key or by
//! click, and the answer is the only thing that happens.
//!
//! Everything that decides is pure here and tested; the editor applies
//! the outcome.

use bevy::prelude::*;

use super::EditorScreen;
use crate::palette;
use crate::ui::UiFont;
use crate::ui_kit;

/// One answer a dialog offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Save (then leave).
    Save,
    /// Leave without saving.
    Discard,
    /// Close the dialog, change nothing.
    Cancel,
    /// Save as the next revision.
    NewRevision,
    /// Save over the revision being edited.
    Overwrite,
}

impl Choice {
    /// The label on its chip.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Choice::Save => "SAVE",
            Choice::Discard => "DISCARD",
            Choice::Cancel => "CANCEL",
            Choice::NewRevision => "NEW REVISION",
            Choice::Overwrite => "OVERWRITE",
        }
    }

    /// The letter that picks it directly.
    #[must_use]
    pub fn key(self) -> KeyCode {
        match self {
            Choice::Save => KeyCode::KeyS,
            Choice::Discard => KeyCode::KeyD,
            Choice::Cancel => KeyCode::KeyC,
            Choice::NewRevision => KeyCode::KeyN,
            Choice::Overwrite => KeyCode::KeyO,
        }
    }
}

/// Which question is being asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Leaving with unsaved changes; `quit` when the whole game is
    /// closing rather than only the editor.
    Leave {
        /// The game quits afterwards.
        quit: bool,
    },
    /// Where a save goes; `then_leave` when it was asked on the way
    /// out, so a successful save also leaves.
    Save {
        /// Leave after a successful save.
        then_leave: bool,
        /// Quit the game after leaving.
        quit: bool,
    },
}

/// An open dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    /// The question.
    pub kind: Kind,
    /// The answers, in their order on screen.
    pub choices: Vec<Choice>,
    /// The answer Enter takes.
    pub cursor: usize,
    /// The title line.
    pub title: String,
    /// What the answers do, in words.
    pub note: String,
}

/// What the save side knows, for [`Dialog::save`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveFacts {
    /// A new revision may be written (the file is in the scheme).
    pub can_start: bool,
    /// The revision being edited may be overwritten.
    pub can_overwrite: bool,
    /// Its number, if it has one.
    pub current: Option<u32>,
    /// The number a new revision would get.
    pub next: Option<u32>,
    /// Whether this session has saved already.
    pub saved_this_session: bool,
}

impl Dialog {
    /// Leaving with unsaved changes. SAVE is preselected: of the three
    /// answers it is the one that loses nothing.
    #[must_use]
    pub fn leave(quit: bool) -> Dialog {
        Dialog {
            kind: Kind::Leave { quit },
            choices: vec![Choice::Save, Choice::Discard, Choice::Cancel],
            cursor: 0,
            title: "UNSAVED CHANGES".to_owned(),
            note: if quit {
                "BeatByte is closing. Save the chart first?".to_owned()
            } else {
                "You are leaving the editor. Save the chart first?".to_owned()
            },
        }
    }

    /// Where a save goes. Offers what the file allows; preselects a
    /// new revision, and after a first save in this session the
    /// revision that save made (the next S is usually more of the
    /// same work). Pure — tested.
    #[must_use]
    pub fn save(facts: SaveFacts, then_leave: bool, quit: bool) -> Dialog {
        let mut choices = Vec::new();
        if facts.can_start {
            choices.push(Choice::NewRevision);
        }
        if facts.can_overwrite {
            choices.push(Choice::Overwrite);
        }
        choices.push(Choice::Cancel);
        let cursor = if facts.saved_this_session && facts.can_overwrite {
            choices
                .iter()
                .position(|c| *c == Choice::Overwrite)
                .unwrap_or(0)
        } else {
            0
        };
        let mut lines = Vec::new();
        if facts.can_start {
            lines.push(match facts.next {
                Some(n) => format!("NEW REVISION saves revision {n} and plays it from now on."),
                None => "NEW REVISION saves the next revision and plays it from now on.".to_owned(),
            });
        }
        match (facts.can_overwrite, facts.current) {
            (true, Some(n)) => lines.push(format!("OVERWRITE saves over revision {n}.")),
            (true, None) => lines.push("This file has no revisions: it is saved over.".to_owned()),
            (false, Some(n)) => lines.push(format!(
                "Revision {n} was generated, so it stays as it is - only hand-made revisions can be overwritten."
            )),
            (false, None) => {}
        }
        Dialog {
            kind: Kind::Save { then_leave, quit },
            choices,
            cursor,
            title: "SAVE CHART".to_owned(),
            note: lines.join("\n"),
        }
    }
}

/// A key or click, as the dialog reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// Move the cursor to the previous answer.
    Previous,
    /// Move the cursor to the next answer.
    Next,
    /// Take the answer under the cursor.
    Confirm,
    /// Close the dialog (the same as CANCEL).
    Cancel,
    /// Take this answer (a click, or its letter).
    Pick(Choice),
}

/// Apply one input: the answer taken, if one was. Moving stops at the
/// ends like every list in the game. Pure — tested.
pub fn step(dialog: &mut Dialog, input: Input) -> Option<Choice> {
    let last = dialog.choices.len().saturating_sub(1);
    match input {
        Input::Previous => {
            dialog.cursor = dialog.cursor.saturating_sub(1);
            None
        }
        Input::Next => {
            dialog.cursor = (dialog.cursor + 1).min(last);
            None
        }
        Input::Confirm => dialog.choices.get(dialog.cursor).copied(),
        Input::Cancel => Some(Choice::Cancel),
        Input::Pick(choice) => dialog.choices.contains(&choice).then_some(choice),
    }
}

/// Read this frame's keys into dialog inputs.
#[must_use]
pub fn inputs(keys: &ButtonInput<KeyCode>, dialog: &Dialog) -> Vec<Input> {
    let pressed = |key| keys.just_pressed(key);
    let mut out = Vec::new();
    if pressed(KeyCode::ArrowLeft) || pressed(KeyCode::ArrowUp) {
        out.push(Input::Previous);
    }
    if pressed(KeyCode::ArrowRight) || pressed(KeyCode::ArrowDown) || pressed(KeyCode::Tab) {
        out.push(Input::Next);
    }
    if pressed(KeyCode::Enter) || pressed(KeyCode::NumpadEnter) || pressed(KeyCode::Space) {
        out.push(Input::Confirm);
    }
    if pressed(KeyCode::Escape) {
        out.push(Input::Cancel);
    }
    for choice in &dialog.choices {
        if pressed(choice.key()) {
            out.push(Input::Pick(*choice));
        }
    }
    out
}

// ---- drawing ---------------------------------------------------------------

/// The open dialog's UI.
#[derive(Component)]
pub(crate) struct DialogNode;

/// One answer chip: its place in the dialog's list.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct DialogChip(pub usize);

/// The answer clicked this frame, read by `editor_input`.
#[derive(Resource, Default)]
pub(crate) struct DialogClick(pub(crate) Option<Choice>);

/// Show the open dialog; rebuilt only when it opens, changes question
/// or closes. The cursor and hover are painted every frame.
pub(crate) fn sync_dialog(
    mut commands: Commands,
    state: Option<Res<super::EditorState>>,
    open: Query<Entity, With<DialogNode>>,
    font: Res<UiFont>,
    mut shown: Local<Option<(Kind, Vec<Choice>, String)>>,
) {
    let wanted = state
        .as_ref()
        .and_then(|s| s.dialog.as_ref())
        .map(|d| (d.kind, d.choices.clone(), d.note.clone()));
    if wanted == *shown {
        return;
    }
    *shown = wanted;
    for entity in &open {
        commands.entity(entity).despawn();
    }
    let Some(dialog) = state.as_ref().and_then(|s| s.dialog.clone()) else {
        return;
    };
    commands
        .spawn((
            EditorScreen,
            DialogNode,
            // The whole window, dimmed: the chart behind it is not
            // what the keys act on while the question is open.
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(palette::BACKGROUND.with_alpha(0.72)),
            GlobalZIndex(20),
        ))
        .with_children(|backdrop| {
            backdrop
                .spawn(ui_kit::panel_centered())
                // The house panel is translucent, made for a screen of
                // its own; in front of the chart the HUD's text read
                // straight through it. Same colour, solid.
                .insert(BackgroundColor(palette::SURFACE))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new(dialog.title.clone()),
                        font.text(ui_kit::ROW),
                        TextColor(palette::BRAND),
                    ));
                    panel.spawn((
                        Text::new(dialog.note.clone()),
                        font.text(ui_kit::SMALL),
                        TextColor(palette::TEXT),
                        TextLayout::justify(Justify::Center),
                    ));
                    panel
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            justify_content: JustifyContent::Center,
                            column_gap: px(8),
                            row_gap: px(8),
                            ..default()
                        })
                        .with_children(|row| {
                            for (index, choice) in dialog.choices.iter().enumerate() {
                                let (fg, line, fill) =
                                    ui_kit::selection_chip_colours(index == dialog.cursor, false);
                                row.spawn((
                                    DialogChip(index),
                                    Button,
                                    ui_kit::selection_chip_node(),
                                    BackgroundColor(fill),
                                    BorderColor::all(line),
                                ))
                                .with_children(|chip| {
                                    chip.spawn((
                                        Text::new(choice.label()),
                                        font.text(ui_kit::SMALL),
                                        TextColor(fg),
                                        TextLayout::default().with_no_wrap(),
                                    ));
                                });
                            }
                        });
                    panel.spawn((
                        Text::new("LEFT/RIGHT choose  ENTER take  ESC cancel"),
                        font.text(ui_kit::SMALL),
                        TextColor(ui_kit::dimmed_subtitle()),
                    ));
                });
        });
}

/// Paint the answer chips for the cursor and the pointer, and hand a
/// click to the editor.
#[allow(clippy::type_complexity)] // Bevy query tuple
pub(crate) fn paint_and_click(
    state: Option<Res<super::EditorState>>,
    mut chips: Query<(
        &DialogChip,
        &Interaction,
        Ref<Interaction>,
        &mut BackgroundColor,
        &mut BorderColor,
        &Children,
    )>,
    mut labels: Query<&mut TextColor>,
    mut click: ResMut<DialogClick>,
) {
    let Some(dialog) = state.as_ref().and_then(|s| s.dialog.as_ref()) else {
        return;
    };
    for (chip, interaction, changed, mut background, mut border, children) in &mut chips {
        let label = children
            .first()
            .and_then(|child| labels.get_mut(*child).ok());
        ui_kit::paint_selection_chip(
            *interaction,
            chip.0 == dialog.cursor,
            &mut background,
            &mut border,
            label.map(Mut::into_inner),
        );
        if changed.is_changed() && *interaction == Interaction::Pressed {
            click.0 = dialog.choices.get(chip.0).copied();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(can_start: bool, can_overwrite: bool, saved: bool) -> SaveFacts {
        SaveFacts {
            can_start,
            can_overwrite,
            current: Some(3),
            next: Some(4),
            saved_this_session: saved,
        }
    }

    #[test]
    fn leaving_offers_save_discard_cancel_with_save_first() {
        let dialog = Dialog::leave(false);
        assert_eq!(
            dialog.choices,
            vec![Choice::Save, Choice::Discard, Choice::Cancel]
        );
        assert_eq!(
            dialog.choices[dialog.cursor],
            Choice::Save,
            "the safe answer first"
        );
        assert!(Dialog::leave(true).note.contains("closing"));
    }

    #[test]
    fn a_generated_revision_offers_no_overwrite_and_says_why() {
        let dialog = Dialog::save(facts(true, false, false), false, false);
        assert_eq!(dialog.choices, vec![Choice::NewRevision, Choice::Cancel]);
        assert!(
            dialog.note.contains("Revision 3 was generated"),
            "{}",
            dialog.note
        );
        assert!(dialog.note.contains("revision 4"), "names the new number");
    }

    #[test]
    fn a_new_revision_is_preselected_until_this_session_saved() {
        let first = Dialog::save(facts(true, true, false), false, false);
        assert_eq!(first.choices[first.cursor], Choice::NewRevision);
        let later = Dialog::save(facts(true, true, true), false, false);
        assert_eq!(later.choices[later.cursor], Choice::Overwrite);
        assert!(later.note.contains("OVERWRITE saves over revision 3"));
    }

    #[test]
    fn a_file_outside_the_scheme_can_only_be_overwritten() {
        let dialog = Dialog::save(
            SaveFacts {
                can_start: false,
                can_overwrite: true,
                current: None,
                next: None,
                saved_this_session: false,
            },
            false,
            false,
        );
        assert_eq!(dialog.choices, vec![Choice::Overwrite, Choice::Cancel]);
        assert_eq!(dialog.choices[dialog.cursor], Choice::Overwrite);
    }

    #[test]
    fn the_cursor_stops_at_the_ends_and_enter_takes_it() {
        let mut dialog = Dialog::leave(false);
        assert_eq!(step(&mut dialog, Input::Previous), None);
        assert_eq!(dialog.cursor, 0);
        step(&mut dialog, Input::Next);
        step(&mut dialog, Input::Next);
        assert_eq!(dialog.cursor, 2);
        // One more from the last answer stays there: no wrap.
        step(&mut dialog, Input::Next);
        assert_eq!(dialog.cursor, 2, "no wrap");
        assert_eq!(step(&mut dialog, Input::Confirm), Some(Choice::Cancel));
        step(&mut dialog, Input::Previous);
        assert_eq!(step(&mut dialog, Input::Confirm), Some(Choice::Discard));
    }

    #[test]
    fn escape_cancels_and_a_pick_needs_an_offered_answer() {
        let mut dialog = Dialog::save(facts(true, false, false), false, false);
        assert_eq!(step(&mut dialog, Input::Cancel), Some(Choice::Cancel));
        // OVERWRITE is not offered for a generated revision: its
        // letter does nothing.
        assert_eq!(step(&mut dialog, Input::Pick(Choice::Overwrite)), None);
        assert_eq!(
            step(&mut dialog, Input::Pick(Choice::NewRevision)),
            Some(Choice::NewRevision)
        );
    }

    #[test]
    fn the_hud_names_the_revision_and_who_made_it() {
        use super::super::draw::revision_label;
        assert_eq!(
            revision_label(Some(3), true, "chart.v3.json"),
            "REV 3 - HAND-MADE"
        );
        assert_eq!(
            revision_label(Some(1), false, "chart.json"),
            "REV 1 - GENERATED"
        );
        assert_eq!(revision_label(None, true, "my.chart.json"), "my.chart.json");
    }

    #[test]
    fn every_answer_has_its_own_letter() {
        let all = [
            Choice::Save,
            Choice::Discard,
            Choice::Cancel,
            Choice::NewRevision,
            Choice::Overwrite,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.key(), b.key(), "{a:?} and {b:?}");
            }
        }
    }
}
