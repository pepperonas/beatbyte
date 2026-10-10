//! One bounded impulse per player, shared by the flat view, neck and ceiling.
//! Surface owners compose this impulse with their normal animation and restore
//! their base colours when it decays; no independent system overwrites them.
use beatbyte_core::SessionEvent;
use bevy::prelude::*;

use super::{GameplayScreen, HighwayLayout, SessionFeedback, fx, stage3d};
use crate::config::{MissEffect, Settings};
use crate::states::{AppState, GamePhase};

#[derive(Debug, Clone, Copy)]
struct Impulse {
    at: f32,
    strength: f32,
}

/// Bounded, independently decaying error signals for each player.
#[derive(Resource, Default)]
pub struct MissVisual {
    impulses: Vec<Option<Impulse>>,
}

impl MissVisual {
    pub(super) fn trigger(&mut self, player: usize, now: f32, strength: f32) {
        if player >= crate::multiplayer::MAX_PLAYERS || strength <= 0.0 {
            return;
        }
        self.impulses
            .resize_with(self.impulses.len().max(player + 1), || None);
        self.impulses[player] = Some(Impulse {
            at: now,
            strength: strength.clamp(0.0, 1.0),
        });
    }

    /// Current local signal strength, respecting disabled effects.
    pub fn strength(&self, player: usize, now: f32, settings: &Settings) -> f32 {
        if settings.miss_intensity <= 0.0 || settings.fx_intensity <= 0.0 {
            return 0.0;
        }
        self.impulses
            .get(player)
            .and_then(|impulse| *impulse)
            .map_or(0.0, |impulse| {
                impulse.strength * envelope(now - impulse.at, settings.reduced_flashing)
            })
    }

    /// Strongest active player signal for a shared surface.
    pub fn peak(&self, now: f32, settings: &Settings) -> f32 {
        (0..self.impulses.len())
            .map(|player| self.strength(player, now, settings))
            .fold(0.0, f32::max)
    }

    /// Signal for the owning player's highway only.
    pub fn neck(&self, player: usize, now: f32, settings: &Settings) -> f32 {
        if settings.miss_effect == MissEffect::HighwayFlash {
            self.strength(player, now, settings)
        } else {
            0.0
        }
    }

    /// Signal for the shared virtual ceiling only.
    pub fn ceiling(&self, now: f32, settings: &Settings) -> f32 {
        if settings.miss_effect == MissEffect::CeilingStrobe {
            self.peak(now, settings)
        } else {
            0.0
        }
    }
}

fn envelope(age: f32, reduced: bool) -> f32 {
    if reduced {
        fx::flash_curve(age, 0.12, 0.55) * 0.35
    } else {
        fx::flash_curve(age, 0.0, 0.25)
    }
}

fn target(effect: MissEffect, stage_3d: bool) -> MissEffect {
    if effect == MissEffect::CeilingStrobe && !stage_3d {
        MissEffect::BorderFlash
    } else {
        effect
    }
}

#[derive(Component)]
struct MissBorder;

#[derive(Component)]
struct MissHighway(usize);

/// Register the signal clock, flat targets and gameplay lifecycle.
pub fn register(app: &mut App) {
    app.init_resource::<MissVisual>()
        .add_systems(
            OnEnter(AppState::Gameplay),
            (clear, spawn.after(fx::spawn_fx_scenery)),
        )
        .add_systems(OnExit(AppState::Gameplay), clear)
        .add_systems(
            Update,
            trigger
                .after(super::drain_feedback)
                .before(stage3d::tint_stage_for_hype)
                .before(super::lightshow::drive_highlight)
                .before(fx::drive_screen_flash)
                .run_if(in_state(GamePhase::Playing)),
        )
        .add_systems(
            Update,
            draw.after(trigger).run_if(in_state(AppState::Gameplay)),
        );
}

fn clear(mut visual: ResMut<MissVisual>) {
    visual.impulses.clear();
}

fn spawn(mut commands: Commands, layout: Res<HighwayLayout>, settings: Res<Settings>) {
    commands.spawn((
        GameplayScreen,
        MissBorder,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            border: UiRect::all(px(8)),
            ..default()
        },
        BorderColor::all(Color::NONE),
        GlobalZIndex(20),
        bevy::picking::Pickable::IGNORE,
    ));
    if !stage3d::active(&settings) {
        for player in 0..layout.players() {
            commands.spawn((
                GameplayScreen,
                MissHighway(player),
                Sprite::from_color(Color::NONE, Vec2::new(layout.bed_width(), 900.0)),
                Transform::from_xyz(layout.origin(player), 0.0, 0.8),
                Visibility::Hidden,
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn trigger(
    mut events: MessageReader<SessionFeedback>,
    time: Res<Time>,
    settings: Res<Settings>,
    mut visual: ResMut<MissVisual>,
    mut flash: ResMut<fx::ScreenFlash>,
    mut shake: ResMut<fx::Shake>,
) {
    for message in events.read() {
        if !matches!(
            message.event,
            SessionEvent::NoteMissed { .. } | SessionEvent::Overstrum
        ) {
            continue;
        }
        let strength = settings.miss_intensity * settings.fx_intensity;
        visual.trigger(message.player_index, time.elapsed_secs(), strength);
        if settings.screen_shake && strength > 0.0 {
            let trauma = if matches!(message.event, SessionEvent::Overstrum) {
                0.20
            } else {
                0.30
            };
            shake.add(trauma * strength);
        }
        if matches!(
            settings.miss_effect,
            MissEffect::RedOverlay | MissEffect::WhiteFlash
        ) {
            let mut profile =
                fx::FlashProfile::miss(settings.reduced_flashing, strength, settings.miss_effect);
            profile.color = settings.miss_color.color(settings.miss_effect);
            flash.request(profile);
        }
    }
}

fn draw(
    time: Res<Time>,
    settings: Res<Settings>,
    visual: Res<MissVisual>,
    mut border: Query<&mut BorderColor, With<MissBorder>>,
    mut highways: Query<(&MissHighway, &mut Sprite, &mut Visibility)>,
) {
    let now = time.elapsed_secs();
    let color = settings.miss_color.color(settings.miss_effect);
    let border_strength =
        if target(settings.miss_effect, stage3d::active(&settings)) == MissEffect::BorderFlash {
            visual.peak(now, &settings)
        } else {
            0.0
        };
    for mut paint in &mut border {
        *paint = BorderColor::all(color.with_alpha(border_strength * 0.65));
    }
    for (player, mut sprite, mut visibility) in &mut highways {
        let strength = visual.neck(player.0, now, &settings);
        sprite.color = color.with_alpha(strength * 0.18);
        *visibility = if strength > 0.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_error_events_use_selected_color_and_zero_disables_every_impulse() {
        use bevy::ecs::system::RunSystemOnce;
        for event in [
            SessionEvent::NoteMissed { event_index: 0 },
            SessionEvent::Overstrum,
        ] {
            for intensity in [0.0, 0.5, 1.0] {
                let mut app = App::new();
                app.init_resource::<Time>()
                    .init_resource::<MissVisual>()
                    .init_resource::<fx::ScreenFlash>()
                    .init_resource::<fx::Shake>()
                    .add_message::<SessionFeedback>()
                    .insert_resource(Settings {
                        miss_color: crate::config::MissColor::Cyan,
                        miss_intensity: intensity,
                        ..default()
                    });
                app.world_mut().write_message(SessionFeedback {
                    player: Entity::PLACEHOLDER,
                    player_index: 0,
                    event,
                });
                app.world_mut()
                    .run_system_once(trigger)
                    .expect("feedback system runs");
                let flash = app.world().resource::<fx::ScreenFlash>();
                assert!((flash.alpha() - 0.1 * intensity).abs() < 0.00001);
                if intensity > 0.0 {
                    assert_eq!(
                        flash.color(),
                        crate::config::MissColor::Cyan.color(MissEffect::RedOverlay)
                    );
                }
                let settings = app.world().resource::<Settings>();
                assert_eq!(
                    app.world().resource::<MissVisual>().peak(0.0, settings),
                    intensity
                );
            }
        }
    }

    #[test]
    fn flat_targets_render_only_the_selected_surface_and_restore_it() {
        use bevy::ecs::system::RunSystemOnce;
        for effect in [
            MissEffect::BorderFlash,
            MissEffect::HighwayFlash,
            MissEffect::CeilingStrobe,
        ] {
            let mut app = App::new();
            app.init_resource::<Time>()
                .insert_resource(Settings {
                    stage_3d: false,
                    miss_effect: effect,
                    miss_color: crate::config::MissColor::Violet,
                    ..default()
                })
                .init_resource::<MissVisual>();
            app.world_mut()
                .resource_mut::<MissVisual>()
                .trigger(1, 0.0, 0.5);
            let border = app
                .world_mut()
                .spawn((MissBorder, BorderColor::all(Color::NONE)))
                .id();
            let player0 = app
                .world_mut()
                .spawn((MissHighway(0), Sprite::default(), Visibility::Hidden))
                .id();
            let player1 = app
                .world_mut()
                .spawn((MissHighway(1), Sprite::default(), Visibility::Hidden))
                .id();
            app.world_mut()
                .run_system_once(draw)
                .expect("flat feedback renders");
            let border_alpha = app
                .world()
                .get::<BorderColor>(border)
                .expect("border")
                .top
                .alpha();
            assert_eq!(border_alpha > 0.0, effect != MissEffect::HighwayFlash);
            assert_eq!(
                *app.world()
                    .get::<Visibility>(player0)
                    .expect("other highway"),
                Visibility::Hidden
            );
            assert_eq!(
                *app.world().get::<Visibility>(player1).expect("own highway")
                    == Visibility::Visible,
                effect == MissEffect::HighwayFlash
            );
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs(1));
            app.world_mut()
                .run_system_once(draw)
                .expect("feedback decays");
            assert_eq!(
                app.world()
                    .get::<BorderColor>(border)
                    .expect("border")
                    .top
                    .alpha(),
                0.0
            );
            assert_eq!(
                *app.world().get::<Visibility>(player1).expect("highway"),
                Visibility::Hidden
            );
        }
    }

    #[test]
    fn impulses_are_bounded_local_and_decay_to_zero() {
        let settings = Settings::default();
        let mut visual = MissVisual::default();
        for _ in 0..100 {
            visual.trigger(1, 1.0, 1.0);
        }
        assert_eq!(visual.strength(1, 1.0, &settings), 1.0);
        assert_eq!(visual.strength(0, 1.0, &settings), 0.0);
        assert_eq!(visual.peak(1.3, &settings), 0.0);
        assert_eq!(
            visual.peak(
                1.0,
                &Settings {
                    miss_intensity: 0.0,
                    ..settings
                }
            ),
            0.0
        );
    }

    #[test]
    fn reduced_flashing_is_a_slow_swell_and_ceiling_has_a_flat_fallback() {
        assert_eq!(envelope(0.0, true), 0.0);
        assert!(envelope(0.12, true) > 0.0);
        assert!(envelope(0.12, true) <= 0.35);
        assert_eq!(envelope(0.56, true), 0.0);
        assert_eq!(
            target(MissEffect::CeilingStrobe, false),
            MissEffect::BorderFlash
        );
        assert_eq!(
            target(MissEffect::CeilingStrobe, true),
            MissEffect::CeilingStrobe
        );
    }
}
