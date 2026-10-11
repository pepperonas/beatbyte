//! Free-play input tester — no song, no stakes.
//!
//! Built because testing keyboard and guitar (with and without tap
//! mode) inside a running song means fighting the chart. Here you
//! just press things: five fret lamps, a strum flash, a Hype lamp, a
//! WOULD-HIT flash that applies the ACTIVE mode's rule (tap: fret
//! edge alone; strum mode: strum while a fret is held), and a `T`
//! toggle that flips tap mode on the spot (persisted like the
//! settings row). Leave with Escape or the pad's Start button —
//! deliberately NOT with the green fret, which is busy being tested.

use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::config::Settings;
use crate::controls::{GameAction, InputMap, InputSources};
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// Marker for the screen's entities.
#[derive(Component)]
struct TestScreen;

/// One fret lamp (0..4).
#[derive(Component)]
struct Lamp(u8);

/// The strum indicator.
#[derive(Component)]
struct StrumLamp;

/// The Hype indicator.
#[derive(Component)]
struct HypeLamp;

/// The "a note would have been HIT now" flash.
#[derive(Component)]
struct HitFlash;

/// The mode line (tap/strum + toggle hint).
#[derive(Component)]
struct ModeLine;

/// The connected-devices line.
#[derive(Component)]
struct DeviceLine;

/// Decay timers for the momentary flashes.
#[derive(Resource, Default)]
struct FlashTimers {
    strum: f32,
    hit: f32,
}

/// The tester plugin.
pub struct InputTestPlugin;

impl Plugin for InputTestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlashTimers>()
            .init_resource::<WhammyPreview>()
            .init_resource::<ActionBarClicks>()
            .add_systems(OnEnter(AppState::InputTest), spawn_screen)
            .add_systems(
                Update,
                (paint_action_bar, run_tester, run_whammy_test)
                    .chain()
                    .run_if(in_state(AppState::InputTest)),
            )
            .add_systems(OnExit(AppState::InputTest), despawn_screen);
    }
}

mod chip {
    pub const TAP_MODE: u8 = 0;
    pub const WHAMMY_PREVIEW: u8 = 1;
}

#[derive(Resource, Default)]
struct ActionBarClicks(Vec<u8>);

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

fn spawn_screen(mut commands: Commands, font: Res<UiFont>, mut timers: ResMut<FlashTimers>) {
    *timers = FlashTimers::default();
    commands
        .spawn((
            TestScreen,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(16),
                ..default()
            },
        ))
        .with_children(|parent| {
            ui_kit::header(
                parent,
                &font,
                "INPUT TEST",
                "press anything and watch it light up",
            );
            parent.spawn((
                DeviceLine,
                Text::new(""),
                font.text(ui_kit::SMALL),
                TextColor(palette::TEXT_DIM),
            ));
            parent.spawn((
                ModeLine,
                Text::new(""),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT),
                Node {
                    margin: UiRect::top(px(10)),
                    ..default()
                },
            ));
            parent
                .spawn(ui_kit::panel_centered())
                .with_children(|panel| {
                    // Fret lamps.
                    panel
                        .spawn(Node {
                            column_gap: px(18),
                            ..default()
                        })
                        .with_children(|lamps| {
                            for fret in 0..5u8 {
                                lamps.spawn((
                                    Lamp(fret),
                                    Node {
                                        width: px(40),
                                        height: px(40),
                                        border: UiRect::all(px(3)),
                                        border_radius: BorderRadius::all(px(20)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                    BorderColor::all(palette::dimmed(
                                        palette::LANES[fret as usize],
                                        0.5,
                                    )),
                                ));
                            }
                        });
                    panel.spawn((
                        WhammyText,
                        Text::new("WHAMMY — no whammy guitar"),
                        font.text(ui_kit::ROW),
                        TextColor(palette::TEXT_DIM),
                    ));
                    panel
                        .spawn((
                            Node {
                                width: px(260),
                                height: px(18),
                                padding: UiRect::all(px(2)),
                                ..default()
                            },
                            BackgroundColor(palette::SURFACE),
                        ))
                        .with_children(|bar| {
                            bar.spawn((
                                WhammyFill,
                                Node {
                                    width: percent(0),
                                    height: percent(100),
                                    ..default()
                                },
                                BackgroundColor(palette::BRAND),
                            ));
                        });
                    panel.spawn((
                        PreviewText,
                        Text::new("TONE OFF · P to listen · 440 → 330 Hz"),
                        font.text(ui_kit::SMALL),
                        TextColor(palette::TEXT_DIM),
                    ));
                    // Strum + Hype row.
                    panel
                        .spawn(Node {
                            column_gap: px(24),
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn((
                                StrumLamp,
                                Text::new("STRUM"),
                                font.text(ui_kit::ROW),
                                TextColor(palette::dimmed(palette::TEXT_DIM, 0.5)),
                            ));
                            row.spawn((
                                HypeLamp,
                                Text::new("HYPE"),
                                font.text(ui_kit::ROW),
                                TextColor(palette::dimmed(palette::TEXT_DIM, 0.5)),
                            ));
                        });
                });
            parent.spawn((
                HitFlash,
                Text::new("HIT!"),
                font.text(ui_kit::TITLE),
                TextColor(Color::NONE),
            ));
            ui_kit::action_bar(
                parent,
                &font,
                &[
                    ui_kit::ChipSpec {
                        id: chip::TAP_MODE,
                        label: "Tap mode",
                        enabled: true,
                    },
                    ui_kit::ChipSpec {
                        id: chip::WHAMMY_PREVIEW,
                        label: "Whammy tone (P)",
                        enabled: true,
                    },
                ],
            );
            crate::prompts::device_footer(
                parent,
                &font,
                "T tap mode  P whammy tone  ESC back",
                "START back",
            );
            ui_kit::back_button(parent, &font, "MAIN MENU");
        });
}

/// The whole tester in one system: read inputs through the REAL map,
/// light lamps, apply the active mode's hit rule.
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy system: params are DI
fn run_tester(
    keys: Res<ButtonInput<KeyCode>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
    pads: Query<(&Name, &Gamepad)>,
    map: Res<InputMap>,
    mut settings: ResMut<Settings>,
    time: Res<Time>,
    mut timers: ResMut<FlashTimers>,
    mut next_state: ResMut<NextState<AppState>>,
    mouse: Res<ButtonInput<MouseButton>>,
    clicks: Res<ActionBarClicks>,
    mut lamps: Query<(&Lamp, &mut BackgroundColor), Without<ui_kit::BackButton>>,
    mut back: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (With<ui_kit::BackButton>, Without<Lamp>),
    >,
    mut texts: ParamSet<(
        Query<&mut Text, With<DeviceLine>>,
        Query<&mut Text, With<ModeLine>>,
        Query<(&mut TextColor, &mut Text), With<StrumLamp>>,
        Query<&mut TextColor, (With<HypeLamp>, Without<StrumLamp>)>,
        Query<&mut TextColor, (With<HitFlash>, Without<StrumLamp>, Without<HypeLamp>)>,
    )>,
) {
    let sources = InputSources {
        keys: &keys,
        pads: pads.iter().map(|(_, pad)| pad).collect(),
    };

    // Devices + mode lines.
    if let Ok(mut text) = texts.p0().single_mut() {
        let names: Vec<String> = pads.iter().map(|(name, _)| name.to_string()).collect();
        let wanted = if names.is_empty() {
            "no controller - keyboard only".to_owned()
        } else {
            format!("connected: {}", names.join(", "))
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    if keys.just_pressed(KeyCode::KeyT) || ui_kit::chip_hit(&clicks.0, chip::TAP_MODE) {
        settings.tap_mode = !settings.tap_mode;
        sounds.write(crate::sfx::UiSound::Toggle);
    }
    if let Ok(mut text) = texts.p1().single_mut() {
        let wanted = if settings.tap_mode {
            "< TAP >  fret press alone hits"
        } else {
            "< STRUM >  hold fret + strum to hit"
        };
        if text.0 != wanted {
            text.0 = wanted.to_owned();
        }
    }

    // Fret lamps + hit-rule inputs.
    let mut any_held = false;
    let mut any_edge = false;
    for fret in 0..5u8 {
        let held = sources.pressed(&map, GameAction::Fret(fret));
        let edge = sources.just_pressed(&map, GameAction::Fret(fret));
        any_held |= held;
        any_edge |= edge;
        for (lamp, mut color) in &mut lamps {
            if lamp.0 == fret {
                color.0 = if held {
                    palette::LANES[fret as usize]
                } else {
                    Color::NONE
                };
            }
        }
    }
    let strummed = sources.just_pressed(&map, GameAction::StrumUp)
        || sources.just_pressed(&map, GameAction::StrumDown);
    if strummed {
        timers.strum = 0.35;
    }
    // The active mode's rule, exactly as gameplay applies it.
    let would_hit = if settings.tap_mode {
        any_edge
    } else {
        strummed && any_held
    };
    if would_hit {
        timers.hit = 0.5;
    }
    timers.strum = (timers.strum - time.delta_secs()).max(0.0);
    timers.hit = (timers.hit - time.delta_secs()).max(0.0);

    if let Ok((mut color, _)) = texts.p2().single_mut() {
        color.0 = if timers.strum > 0.0 {
            palette::BRAND
        } else {
            palette::dimmed(palette::TEXT_DIM, 0.5)
        };
    }
    if let Ok(mut color) = texts.p3().single_mut() {
        color.0 = if sources.pressed(&map, GameAction::Hype) {
            palette::HYPE
        } else {
            palette::dimmed(palette::TEXT_DIM, 0.5)
        };
    }
    if let Ok(mut color) = texts.p4().single_mut() {
        color.0 = if timers.hit > 0.0 {
            Color::srgb(0.3, 1.0, 0.55).with_alpha((timers.hit * 2.0).min(1.0))
        } else {
            Color::NONE
        };
    }

    // Leave with Escape, pad Start/Select, the back button or
    // right-click — NOT the green fret (busy being tested).
    let pad_leave = pads.iter().any(|(_, pad)| {
        pad.just_pressed(GamepadButton::Start) || pad.just_pressed(GamepadButton::Select)
    });
    if keys.just_pressed(KeyCode::Escape)
        || pad_leave
        || ui_kit::back_pressed(&mut back)
        || mouse.just_pressed(MouseButton::Right)
    {
        sounds.write(crate::sfx::UiSound::Back);
        crate::config::save_settings(&settings);
        // Back to Settings: its INPUT TEST row is the only door here.
        next_state.set(AppState::Settings);
    }
}

fn despawn_screen(
    mut commands: Commands,
    entities: Query<Entity, With<TestScreen>>,
    mut preview: ResMut<WhammyPreview>,
    music: Option<Res<crate::audio_sys::Music>>,
) {
    if let Some(music) = music {
        music.0.set_whammy(0.0);
        if preview.0 {
            music.0.stop();
        }
    }
    preview.0 = false;
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}

#[derive(Resource, Default)]
struct WhammyPreview(bool);
#[derive(Component)]
struct WhammyFill;
#[derive(Component)]
struct WhammyText;
#[derive(Component)]
struct PreviewText;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_whammy_test(
    pads: Query<(Entity, &Gamepad)>,
    native: Option<Res<crate::xplorer::GuitarWhammy>>,
    keys: Res<ButtonInput<KeyCode>>,
    clicks: Res<ActionBarClicks>,
    music: Option<Res<crate::audio_sys::Music>>,
    mut preview: ResMut<WhammyPreview>,
    mut bars: Query<&mut Node, With<WhammyFill>>,
    mut texts: Query<(&mut Text, &mut TextColor), (With<WhammyText>, Without<PreviewText>)>,
    mut tone: Query<&mut Text, (With<PreviewText>, Without<WhammyText>)>,
) {
    let connected = native.as_ref().is_some_and(|n| {
        !n.0.is_empty()
            || pads
                .iter()
                .any(|(e, p)| crate::xplorer::whammy_connected(e, p, n))
    });
    let depth = native.as_ref().map_or(0.0, |n| {
        pads.iter()
            .map(|(e, p)| crate::xplorer::whammy_depth(e, p, n))
            .chain(n.0.values().map(|v| crate::xplorer::normalize_whammy(*v)))
            .fold(0.0_f32, f32::max)
    });
    if keys.just_pressed(KeyCode::KeyP) || ui_kit::chip_hit(&clicks.0, chip::WHAMMY_PREVIEW) {
        preview.0 = !preview.0;
        if let Some(music) = &music {
            if preview.0 {
                music.0.play_whammy_preview();
            } else {
                music.0.set_whammy(0.0);
                music.0.stop();
            }
        }
    }
    if preview.0
        && let Some(music) = &music
    {
        music.0.set_whammy(depth);
    }
    for mut node in &mut bars {
        node.width = percent(depth * 100.0);
    }
    for (mut text, mut color) in &mut texts {
        text.0 = if connected {
            format!("WHAMMY {:3.0}%", depth * 100.0)
        } else {
            "WHAMMY — no whammy guitar".into()
        };
        color.0 = if depth > 0.0 {
            palette::BRAND
        } else {
            palette::TEXT_DIM
        };
    }
    for mut text in &mut tone {
        text.0 = if preview.0 {
            if music.as_ref().is_some_and(|m| !m.0.is_healthy()) {
                "TONE ON · no audio output".into()
            } else {
                format!(
                    "TONE ON · {:.0} Hz · P to stop · music volume / M mute",
                    440.0 * 2.0_f32.powf(-5.0 * depth / 12.0)
                )
            }
        } else {
            "TONE OFF · P to listen · 440 → 330 Hz".into()
        };
    }
}

#[cfg(test)]
mod whammy_tests {
    use super::*;
    #[test]
    fn input_test_reads_native_bar_and_highlights_without_a_song() {
        let mut app = App::new();
        app.init_resource::<crate::xplorer::GuitarWhammy>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ActionBarClicks>()
            .init_resource::<WhammyPreview>()
            .add_systems(Update, run_whammy_test);
        let bar = app.world_mut().spawn((WhammyFill, Node::default())).id();
        let label = app
            .world_mut()
            .spawn((WhammyText, Text::default(), TextColor(palette::TEXT_DIM)))
            .id();
        app.update();
        assert!(
            app.world()
                .get::<Text>(label)
                .expect("valid test fixture")
                .0
                .contains("no whammy guitar")
        );
        let guitar = app.world_mut().spawn_empty().id();
        app.world_mut()
            .resource_mut::<crate::xplorer::GuitarWhammy>()
            .0
            .insert(guitar, 1.0);
        app.update();
        assert_eq!(
            app.world()
                .get::<Node>(bar)
                .expect("valid test fixture")
                .width,
            percent(100.0)
        );
        assert!(
            app.world()
                .get::<Text>(label)
                .expect("valid test fixture")
                .0
                .contains("100%")
        );
        assert_eq!(
            app.world()
                .get::<TextColor>(label)
                .expect("valid test fixture")
                .0,
            palette::BRAND
        );
        app.world_mut()
            .resource_mut::<crate::xplorer::GuitarWhammy>()
            .0
            .insert(guitar, 0.52);
        app.update();
        let Val::Percent(value) = app
            .world()
            .get::<Node>(bar)
            .expect("valid test fixture")
            .width
        else {
            panic!("percentage bar")
        };
        assert!((value - 50.0).abs() < 0.001);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyP);
        app.update();
        assert!(app.world().resource::<WhammyPreview>().0);
        app.world_mut()
            .resource_mut::<crate::xplorer::GuitarWhammy>()
            .0
            .clear();
        app.update();
        assert_eq!(
            app.world()
                .get::<Node>(bar)
                .expect("valid test fixture")
                .width,
            percent(0.0)
        );
    }
}
