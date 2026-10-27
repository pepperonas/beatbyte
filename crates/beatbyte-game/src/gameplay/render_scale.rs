//! The stage's render scale: on a GPU that cannot hold 60 frames a
//! second, the 3D stage renders into a smaller image that is drawn
//! across the window, while the HUD and every text stay at the
//! window's full sharpness.
//!
//! The scale is chosen by a governor that watches frame times, not by
//! a guess about the hardware. It steps down when frames are missed
//! and only probes upward after a long calm stretch; a probe that
//! fails locks that step out for the rest of the session, so the
//! scale settles instead of pumping. Only in PERFORMANCE MODE — with
//! it off the stage renders straight into the window as before.
//!
//! The 2015 MacBook Pro this exists for: `docs/perf/baseline.md`.

use bevy::asset::RenderAssetUsages;
use bevy::camera::{RenderTarget, visibility::RenderLayers};
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::window::PrimaryWindow;

use crate::audio_sys::GameClock;
use crate::config::Settings;
use crate::states::{AppState, GamePhase};

/// The scales the stage can render at, sharpest first.
pub const STEPS: [f32; 4] = [1.0, 0.85, 0.7, 0.55];

/// A frame slower than this misses a 60 Hz refresh — the goal.
pub const MISS_MS: f32 = 18.5;

/// The smoothing window of the frame-time average, in seconds.
const SMOOTH_S: f32 = 0.75;

/// How long a step must hold before the governor may step down
/// again: the new scale has to show its effect first.
const SETTLE_S: f32 = 1.5;

/// How long frames must stay in budget before a sharper step is
/// tried.
const CALM_S: f32 = 20.0;

/// A probe that misses within this long after stepping up failed.
const PROBE_S: f32 = 4.0;

/// The frame-time governor. Pure: it is fed frame times and answers
/// with a step; the plugin turns steps into pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Governor {
    step: usize,
    /// The sharpest step this session may use; a failed probe lowers
    /// it.
    ceiling: usize,
    average_ms: Option<f32>,
    held_s: f32,
    calm_s: f32,
    /// Seconds since the last upward probe, while one is running.
    probing_s: Option<f32>,
}

impl Governor {
    /// A governor starting at `step` (clamped into range) — the step
    /// this device settled on last time.
    #[must_use]
    pub fn new(step: usize) -> Self {
        Self {
            step: step.min(STEPS.len() - 1),
            ceiling: 0,
            average_ms: None,
            held_s: 0.0,
            calm_s: 0.0,
            probing_s: None,
        }
    }

    /// The current step (index into [`STEPS`]).
    #[must_use]
    pub fn step(&self) -> usize {
        self.step
    }

    /// The current scale.
    #[must_use]
    pub fn scale(&self) -> f32 {
        STEPS[self.step]
    }

    /// Feed one frame. `frame_ms` is the wall time of the frame;
    /// `steady` is false while frame times say nothing about the
    /// stage (a song loading, the count-in's first seconds, a pause):
    /// those frames are not counted at all. Returns true when the
    /// step changed.
    pub fn advance(&mut self, frame_ms: f32, steady: bool) -> bool {
        if !steady || !frame_ms.is_finite() || frame_ms <= 0.0 {
            return false;
        }
        // A hitch (an asset arriving, the OS stealing a moment) says
        // nothing about the stage's price: one frame counts at most as
        // two missed refreshes, so only a run of misses moves the
        // average over the line.
        let counted = frame_ms.min(2.0 * MISS_MS);
        let dt = counted / 1000.0;
        self.held_s += dt;
        // A plain mean until the window has filled, then a moving
        // average: right after a step change the first frames would
        // otherwise speak for the whole window.
        let weight = (dt / SMOOTH_S.min(self.held_s)).min(1.0);
        let average = match self.average_ms {
            Some(previous) => previous + (counted - previous) * weight,
            None => counted,
        };
        self.average_ms = Some(average);
        let missing = average > MISS_MS;

        if let Some(probing) = self.probing_s.as_mut() {
            *probing += dt;
            if missing && self.held_s >= SETTLE_S.min(PROBE_S) && *probing <= PROBE_S {
                // The sharper step cannot be held: back down, and do
                // not try it again this session.
                self.ceiling = self.step + 1;
                return self.go(self.step + 1);
            }
            if *probing > PROBE_S {
                self.probing_s = None;
            }
        }

        if missing {
            self.calm_s = 0.0;
            if self.held_s >= SETTLE_S && self.step + 1 < STEPS.len() {
                return self.go(self.step + 1);
            }
            return false;
        }
        self.calm_s += dt;
        if self.calm_s >= CALM_S && self.step > self.ceiling {
            let up = self.step - 1;
            let changed = self.go(up);
            self.probing_s = Some(0.0);
            return changed;
        }
        false
    }

    fn go(&mut self, step: usize) -> bool {
        let step = step.min(STEPS.len() - 1);
        let changed = step != self.step;
        self.step = step;
        self.held_s = 0.0;
        self.calm_s = 0.0;
        // The average belonged to the old scale.
        self.average_ms = None;
        changed
    }
}

/// Whether any stage camera draws straight into the window. Only then
/// must the 2D camera on top LOAD the frame instead of clearing it,
/// and only then must the two agree on HDR: a stage scaled into an
/// image reaches the window as a sprite, like any other 2D content.
pub fn stage_on_window<'a>(targets: impl IntoIterator<Item = &'a RenderTarget>) -> bool {
    targets
        .into_iter()
        .any(|target| matches!(target, RenderTarget::Window(_)))
}

/// The size in physical pixels the stage renders at: the window's
/// size times the scale, never below one pixel.
#[must_use]
pub fn target_size(window_w: u32, window_h: u32, scale: f32) -> (u32, u32) {
    let side = |v: u32| ((v as f32 * scale).round() as u32).max(1);
    (side(window_w), side(window_h))
}

/// Song seconds before frame times count: the first ones carry the
/// stage's first uploads and the count-in, not the stage's price.
const WARMUP_S: f64 = 2.0;

/// The live governor and the image the stage renders into while it
/// is scaled down.
#[derive(Resource, Default)]
pub struct StageScale {
    governor: Option<Governor>,
    image: Option<Handle<Image>>,
    /// `BEATBYTE_STAGE_SCALE` pinned the step: measure one scale.
    pinned: bool,
}

/// The step `BEATBYTE_STAGE_SCALE` pins, if any (measurement only).
fn pinned_step() -> Option<usize> {
    std::env::var("BEATBYTE_STAGE_SCALE")
        .ok()
        .and_then(|value| value.trim().parse().ok())
}

/// The sprite that shows the scaled stage on the 2D camera.
#[derive(Component)]
pub struct StageScaleSprite;

/// Wire the render scale into the app.
pub struct RenderScalePlugin;

impl Plugin for RenderScalePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StageScale>()
            .add_systems(OnEnter(AppState::Gameplay), begin)
            .add_systems(OnExit(AppState::Gameplay), finish)
            .add_systems(
                PostUpdate,
                (govern, apply).chain().run_if(in_state(AppState::Gameplay)),
            );
    }
}

/// A song starts from the step this device settled on last time.
fn begin(mut scale: ResMut<StageScale>, settings: Res<Settings>) {
    let pinned = pinned_step();
    scale.pinned = pinned.is_some();
    let start = pinned.unwrap_or(usize::from(settings.stage_scale_step));
    scale.governor = Some(Governor::new(start));
}

/// Remember where the governor settled, and let the image go.
fn finish(mut scale: ResMut<StageScale>, mut settings: ResMut<Settings>) {
    let pinned = scale.pinned;
    if let Some(governor) = scale.governor.take()
        && !pinned
    {
        let step = u8::try_from(governor.step()).unwrap_or(0);
        if settings.performance_mode && settings.stage_scale_step != step {
            settings.stage_scale_step = step;
            crate::config::save_settings(&settings);
        }
    }
    scale.image = None;
}

/// Feed the governor this frame's wall time.
fn govern(
    mut scale: ResMut<StageScale>,
    settings: Res<Settings>,
    real: Res<Time<Real>>,
    time: Res<Time>,
    phase: Option<Res<State<GamePhase>>>,
    game_clock: Res<GameClock>,
) {
    if scale.pinned {
        return;
    }
    let Some(governor) = scale.governor.as_mut() else {
        return;
    };
    let playing = phase.is_some_and(|phase| *phase.get() == GamePhase::Playing);
    let warm = game_clock.song_time(&time).is_some_and(|t| t >= WARMUP_S);
    let steady = settings.performance_mode && playing && warm;
    let frame_ms = real.delta_secs() * 1000.0;
    if governor.advance(frame_ms, steady) {
        info!(
            "render scale: stage at {:.0} % (step {})",
            governor.scale() * 100.0,
            governor.step()
        );
    }
}

/// The image the scaled stage renders into.
fn stage_image(w: u32, h: u32) -> Image {
    let mut image = Image::new_target_texture(w, h, TextureFormat::Rgba8UnormSrgb, None);
    // The app samples nearest by default (pixel art); a scaled-up
    // stage wants a smooth filter.
    image.sampler = ImageSampler::linear();
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    image
}

/// Point the stage camera at the window (full scale) or at an image
/// of the scaled size drawn across the 2D camera's view.
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy system: params are DI, not an API
fn apply(
    mut commands: Commands,
    mut scale: ResMut<StageScale>,
    settings: Res<Settings>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut stage: Query<&mut RenderTarget, (With<Camera3d>, With<super::stage3d::Stage3d>)>,
    view: Query<&Projection, With<Camera2d>>,
    mut sprites: Query<(Entity, &mut Sprite, &mut Transform), With<StageScaleSprite>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok(mut target) = stage.single_mut() else {
        return;
    };
    let factor = match (&scale.governor, settings.performance_mode) {
        (Some(governor), true) => governor.scale(),
        _ => 1.0,
    };
    if factor >= 1.0 {
        // Full scale: straight into the window, exactly as before.
        if !matches!(*target, RenderTarget::Window(_)) {
            *target = RenderTarget::Window(bevy::window::WindowRef::Primary);
        }
        for (entity, ..) in &sprites {
            commands.entity(entity).despawn();
        }
        scale.image = None;
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let (w, h) = target_size(window.physical_width(), window.physical_height(), factor);
    let size = Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    // A new size gets a NEW image. Resizing one in place makes Bevy
    // copy the old texture into the new one, which needs COPY_SRC on a
    // render target — without it wgpu raises a validation error and
    // the app quits the moment the governor steps (seen on the 2015).
    // The stage redraws every frame, so there is nothing to keep.
    let current = scale.image.clone().filter(|handle| {
        images
            .get(handle)
            .is_some_and(|image| image.texture_descriptor.size == size)
    });
    let handle = match current {
        Some(handle) => handle,
        None => {
            if let Some(old) = scale.image.take() {
                images.remove(&old);
            }
            let handle = images.add(stage_image(w, h));
            scale.image = Some(handle.clone());
            handle
        }
    };
    if target.as_image() != Some(&handle) {
        *target = RenderTarget::Image(handle.clone().into());
        info!("render scale: stage renders into a {w}x{h} image");
    }
    // Cover exactly what the 2D camera shows: the stage filled the
    // window before, and it fills the same rectangle now.
    let area = view
        .iter()
        .find_map(|projection| match projection {
            Projection::Orthographic(ortho) => Some(ortho.area),
            _ => None,
        })
        .unwrap_or(Rect::new(-640.0, -360.0, 640.0, 360.0));
    let place = Transform::from_translation(area.center().extend(-999.0));
    if let Some((_, mut sprite, mut transform)) = sprites.iter_mut().next() {
        if sprite.custom_size != Some(area.size()) {
            sprite.custom_size = Some(area.size());
        }
        if *transform != place {
            *transform = place;
        }
        if sprite.image != handle {
            sprite.image = handle;
        }
    } else {
        commands.spawn((
            super::GameplayScreen,
            StageScaleSprite,
            Sprite {
                image: handle,
                custom_size: Some(area.size()),
                ..default()
            },
            place,
            RenderLayers::layer(0),
        ));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Feed `seconds` of frames at `ms` each; return how often the
    /// step changed.
    fn run(governor: &mut Governor, ms: f32, seconds: f32) -> usize {
        let frames = (seconds * 1000.0 / ms).ceil() as usize;
        (0..frames).filter(|_| governor.advance(ms, true)).count()
    }

    /// A step change must hand the stage a NEW image of the new size
    /// and drop the old one: resizing in place made Bevy copy the old
    /// texture, which a render target cannot do without COPY_SRC, and
    /// the app quit with a validation error on the 2015.
    #[test]
    fn a_step_change_swaps_in_a_fresh_image() {
        let mut app = App::new();
        app.insert_resource(Assets::<Image>::default())
            .insert_resource(Settings {
                performance_mode: true,
                ..default()
            })
            .insert_resource(StageScale {
                governor: Some(Governor::new(1)),
                image: None,
                pinned: false,
            })
            .add_systems(Update, apply);
        let mut window = Window::default();
        window.resolution.set_physical_resolution(2000, 1000);
        app.world_mut().spawn((window, PrimaryWindow));
        let camera = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                super::super::stage3d::Stage3d,
                RenderTarget::Window(bevy::window::WindowRef::Primary),
            ))
            .id();
        app.update();
        let first = app.world().resource::<StageScale>().image.clone().unwrap();
        let first_size = app
            .world()
            .resource::<Assets<Image>>()
            .get(&first)
            .unwrap()
            .size();
        assert_eq!(first_size, UVec2::from(target_size(2000, 1000, STEPS[1])));

        app.world_mut().resource_mut::<StageScale>().governor = Some(Governor::new(2));
        app.update();
        let second = app.world().resource::<StageScale>().image.clone().unwrap();
        assert_ne!(
            first, second,
            "a new size must be a new image, not a resize"
        );
        let images = app.world().resource::<Assets<Image>>();
        assert!(images.get(&first).is_none(), "the old image is dropped");
        assert_eq!(
            images.get(&second).unwrap().size(),
            UVec2::from(target_size(2000, 1000, STEPS[2]))
        );
        assert_eq!(
            app.world().get::<RenderTarget>(camera).unwrap().as_image(),
            Some(&second)
        );

        // The same size again keeps the image it has.
        app.update();
        assert_eq!(app.world().resource::<StageScale>().image, Some(second));
    }

    #[test]
    fn a_fast_machine_never_leaves_full_scale() {
        for ms in [4.0, 8.33, 10.0, 16.67] {
            let mut g = Governor::new(0);
            assert_eq!(run(&mut g, ms, 120.0), 0, "{ms} ms");
            assert_eq!(g.step(), 0);
        }
    }

    #[test]
    fn missed_frames_step_down_one_settled_step_at_a_time() {
        let mut g = Governor::new(0);
        // A single second of misses is still inside the settle time.
        run(&mut g, 33.3, 1.0);
        assert_eq!(g.step(), 0);
        run(&mut g, 33.3, 1.0);
        assert_eq!(g.step(), 1);
        // Never further than the last step, however long it misses.
        run(&mut g, 33.3, 60.0);
        assert_eq!(g.step(), STEPS.len() - 1);
    }

    #[test]
    fn unsteady_frames_are_not_counted() {
        let mut g = Governor::new(0);
        for _ in 0..1000 {
            assert!(!g.advance(250.0, false));
        }
        assert_eq!(g, Governor::new(0));
        assert!(!g.advance(f32::NAN, true));
        assert!(!g.advance(0.0, true));
        assert!(!g.advance(-3.0, true));
    }

    #[test]
    fn a_single_stall_does_not_step_down() {
        let mut g = Governor::new(0);
        run(&mut g, 16.67, 5.0);
        // A 200 ms hitch (an asset arriving) in a steady stream.
        g.advance(200.0, true);
        run(&mut g, 16.67, 5.0);
        assert_eq!(g.step(), 0);
    }

    #[test]
    fn a_long_calm_probes_one_step_sharper() {
        let mut g = Governor::new(2);
        run(&mut g, 16.67, CALM_S - 1.0);
        assert_eq!(g.step(), 2);
        run(&mut g, 16.67, 2.0);
        assert_eq!(g.step(), 1);
    }

    #[test]
    fn a_failed_probe_is_not_tried_again() {
        let mut g = Governor::new(2);
        run(&mut g, 16.67, CALM_S + 0.1);
        assert_eq!(g.step(), 1, "probed up");
        // The sharper step misses: straight back, where it holds.
        run(&mut g, 33.3, 1.6);
        assert_eq!(g.step(), 2, "probe failed");
        run(&mut g, 16.67, 1.0);
        assert_eq!(g.step(), 2);
        // However calm it gets, step 1 is out for this session.
        run(&mut g, 16.67, CALM_S * 5.0);
        assert_eq!(g.step(), 2);
    }

    #[test]
    fn a_held_probe_keeps_climbing() {
        let mut g = Governor::new(3);
        run(&mut g, 12.0, CALM_S * 4.0);
        assert_eq!(g.step(), 0);
    }

    #[test]
    fn the_start_step_is_clamped() {
        assert_eq!(Governor::new(99).step(), STEPS.len() - 1);
        assert_eq!(Governor::new(1).scale(), STEPS[1]);
    }

    #[test]
    fn steps_get_strictly_smaller_and_stay_visible() {
        assert_eq!(STEPS[0], 1.0);
        for pair in STEPS.windows(2) {
            assert!(pair[1] < pair[0]);
        }
        assert!(STEPS[STEPS.len() - 1] >= 0.5);
    }

    #[test]
    fn target_size_scales_and_never_vanishes() {
        assert_eq!(target_size(2560, 1440, 1.0), (2560, 1440));
        assert_eq!(target_size(2560, 1440, 0.55), (1408, 792));
        assert_eq!(target_size(1, 1, 0.55), (1, 1));
        assert_eq!(target_size(0, 0, 1.0), (1, 1));
    }

    #[test]
    fn only_a_stage_on_the_window_shares_the_frame() {
        use bevy::camera::ImageRenderTarget;
        let window = RenderTarget::Window(bevy::window::WindowRef::Primary);
        let image = RenderTarget::Image(ImageRenderTarget::from(Handle::<Image>::default()));
        assert!(!stage_on_window([]));
        assert!(stage_on_window([&window]));
        assert!(!stage_on_window([&image]));
        assert!(stage_on_window([&image, &window]));
    }
}
