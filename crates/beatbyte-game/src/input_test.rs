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
            .add_systems(OnEnter(AppState::InputTest), spawn_screen)
            .add_systems(Update, run_tester.run_if(in_state(AppState::InputTest)))
            .add_systems(OnExit(AppState::InputTest), despawn_screen);
    }
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
            ui_kit::footer(parent, &font, "T toggle tap  ESC / pad START back");
        });
}

/// The whole tester in one system: read inputs through the REAL map,
/// light lamps, apply the active mode's hit rule.
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy system: params are DI
fn run_tester(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<(&Name, &Gamepad)>,
    map: Res<InputMap>,
    mut settings: ResMut<Settings>,
    time: Res<Time>,
    mut timers: ResMut<FlashTimers>,
    mut next_state: ResMut<NextState<AppState>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut lamps: Query<(&Lamp, &mut BackgroundColor)>,
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
    if keys.just_pressed(KeyCode::KeyT) {
        settings.tap_mode = !settings.tap_mode;
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

    // Leave with Escape or pad Start — NOT the green fret.
    let pad_start = pads
        .iter()
        .any(|(_, pad)| pad.just_pressed(GamepadButton::Start));
    if keys.just_pressed(KeyCode::Escape) || pad_start || mouse.just_pressed(MouseButton::Right) {
        crate::config::save_settings(&settings);
        next_state.set(AppState::MainMenu);
    }
}

fn despawn_screen(mut commands: Commands, entities: Query<Entity, With<TestScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}
