//! The TV tube: the picture powers on when the game starts and
//! collapses to a dot when it quits.
//!
//! Ported from `inspector-rust`
//! (`core/frontend/src/lib/md3-motion.ts`, `playCrtOn` /
//! `playCrtOff`), including its two hard-won rules:
//!
//! - **The power-ON is front-loaded.** Its comment records why: at
//!   440 ms with the original curve the shell was a 1 %-height
//!   scanline for the first 150 ms and nothing was readable until
//!   ~360 ms. The offsets below keep the tube identity — dot →
//!   scanline → picture — but the width snap lands at 23 % and full
//!   height at 56 %; everything after that is the phosphor settling,
//!   which you can already read through.
//! - **The power-OFF must be shorter than the power-on** ("once you
//!   have decided to leave, any wait is pure cost"), which the ratio
//!   below guarantees by construction rather than by two numbers
//!   somebody has to keep in step. A test pins it.
//!
//! What differs from the source, and why: that app scales an HTML
//! shell with `transform` + `brightness`. A Bevy window has no such
//! shell, so the same shape is drawn as a MASK — black panels close
//! in from the edges to a bright scanline and pinch to a dot, over
//! the live picture. The geometry and timing are the port; the
//! technique is what this engine can actually do.

use bevy::prelude::*;

use crate::config::Settings;
use crate::palette;

/// Power-on duration.
///
/// The source ships 250 ms because it animates a POPUP that has to
/// be typed into immediately; it also documents a configurable
/// range of 80–900 ms. A game being launched is the other end of
/// that range: nobody is waiting to type, and the tube is supposed
/// to be seen (user report: "ich sehe sie noch nicht").
///
/// 900 ms is the TOP of the range the source documents, and the
/// real easings are why it needs the room: they are deliberately
/// front-loaded, so the dot, the scanline and the opening are all
/// over inside the first 56 % — at 700 ms that whole performance
/// took 390 ms and a capture taken 300 ms in already showed a
/// nearly-open picture. Stretching the total gives the drama the
/// time, without touching a single offset of the port.
pub const CRT_ON_S: f32 = 0.90;
/// Power-off duration, DERIVED — never configured beside the
/// power-on, so leaving can never become slower than arriving.
pub const CRT_OFF_RATIO: f32 = 190.0 / 250.0;
/// Residual width of the pinched dot, as a fraction of the screen.
const DOT_X: f32 = 0.02;
/// Residual height of the scanline.
const DOT_Y: f32 = 0.012;
/// How thick the glowing band grows at full brightness, in pixels.
const GLOW_PX: f32 = 26.0;

/// The power-off duration for the shipped power-on.
#[must_use]
pub fn crt_off_s(on_s: f32) -> f32 {
    if on_s <= 0.0 {
        return 0.0;
    }
    (on_s * CRT_OFF_RATIO).max(0.001)
}

/// What the mask looks like at one instant: the visible window's
/// size as a fraction of the screen, plus how hot the scanline
/// glows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrtFrame {
    /// Visible width, 0..1.
    pub width: f32,
    /// Visible height, 0..1.
    pub height: f32,
    /// Scanline glow, 0..1 — the port of the source's
    /// `brightness()` filter, which a mask cannot express directly.
    pub glow: f32,
}

impl CrtFrame {
    /// The settled picture: everything visible, nothing glowing.
    pub const PICTURE: CrtFrame = CrtFrame {
        width: 1.0,
        height: 1.0,
        glow: 0.0,
    };
}

/// Linear ramp between two keyframe values.
fn lerp(from: f32, to: f32, t: f32) -> f32 {
    to.mul_add(t, from * (1.0 - t))
}

/// One CSS `cubic-bezier(x1, y1, x2, y2)` curve.
///
/// The port used to interpolate its keyframes LINEARLY, which is
/// what made the tube read as a mechanical wipe rather than a
/// tube: the source assigns a different easing to every segment,
/// and those curves are most of the character. `x0`/`x3` are fixed
/// at 0 and 1 as CSS defines them.
#[derive(Debug, Clone, Copy)]
pub struct Bezier {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

impl Bezier {
    /// One coordinate of the curve at parameter `t`.
    fn axis(a: f32, b: f32, t: f32) -> f32 {
        let inv = 1.0 - t;
        // 3(1-t)²t·a + 3(1-t)t²·b + t³
        let term = 3.0 * inv * t;
        t.powi(3) + term.mul_add(inv * a + t * b, 0.0)
    }

    /// `y` for a given `x` — the value CSS actually asks for.
    ///
    /// The parameter is NOT the x axis, so it is solved for:
    /// bisection, twenty steps. Newton converges faster but needs a
    /// derivative that degenerates on the flat curves used here
    /// (`cubic-bezier(0.5, 0, 0.85, 0.35)` starts horizontal), and
    /// twenty bisections land inside 1e-6 on a 0..1 domain — far
    /// finer than a pixel. Pure — tested.
    #[must_use]
    pub fn ease(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.0 || x >= 1.0 {
            return x;
        }
        let (mut low, mut high) = (0.0_f32, 1.0_f32);
        let mut t = x;
        for _ in 0..20 {
            if Self::axis(self.x1, self.x2, t) < x {
                low = t;
            } else {
                high = t;
            }
            t = f32::midpoint(low, high);
        }
        Self::axis(self.y1, self.y2, t)
    }
}

/// The source's per-segment easings, kept as it wrote them.
/// Power-on: the dot's widening, the scanline opening, the settle.
const ON_WIDEN: Bezier = Bezier {
    x1: 0.2,
    y1: 0.85,
    x2: 0.25,
    y2: 1.0,
};
const ON_OPEN: Bezier = Bezier {
    x1: 0.12,
    y1: 0.7,
    x2: 0.3,
    y2: 1.0,
};
/// `ease-out` — CSS's own definition.
const EASE_OUT: Bezier = Bezier {
    x1: 0.0,
    y1: 0.0,
    x2: 0.58,
    y2: 1.0,
};
/// `ease-in`.
const EASE_IN: Bezier = Bezier {
    x1: 0.42,
    y1: 0.0,
    x2: 1.0,
    y2: 1.0,
};
/// Power-off: the collapse, and the final pinch.
const OFF_COLLAPSE: Bezier = Bezier {
    x1: 0.5,
    y1: 0.0,
    x2: 0.85,
    y2: 0.35,
};
const OFF_PINCH: Bezier = Bezier {
    x1: 0.55,
    y1: 0.0,
    x2: 0.85,
    y2: 0.35,
};

/// Where `progress` sits between two offsets, 0 outside them.
fn span(progress: f32, from: f32, to: f32) -> f32 {
    ((progress - from) / (to - from)).clamp(0.0, 1.0)
}

/// The power-ON frame at `progress` (0..1): dot → scanline →
/// picture, with the source's offsets (0.23, 0.56). Pure — tested.
#[must_use]
pub fn power_on(progress: f32) -> CrtFrame {
    let p = progress.clamp(0.0, 1.0);
    if p >= 1.0 {
        return CrtFrame::PICTURE;
    }
    if p < 0.23 {
        // The dot widens into a scanline: width runs, height waits.
        let t = ON_WIDEN.ease(span(p, 0.0, 0.23));
        return CrtFrame {
            width: lerp(DOT_X, 1.0, t),
            height: DOT_Y,
            glow: lerp(1.0, 0.85, t),
        };
    }
    if p < 0.56 {
        // The scanline opens into the picture.
        let t = ON_OPEN.ease(span(p, 0.23, 0.56));
        return CrtFrame {
            width: 1.0,
            height: lerp(DOT_Y, 1.0, t),
            glow: lerp(0.85, 0.2, t),
        };
    }
    // The phosphor settles; the picture is already readable.
    let t = EASE_OUT.ease(span(p, 0.56, 1.0));
    CrtFrame {
        width: 1.0,
        height: 1.0,
        glow: lerp(0.2, 0.0, t),
    }
}

/// The power-OFF frame at `progress` (0..1): picture → scanline →
/// dot, with the source's offsets (0.55, 0.72). Pure — tested.
#[must_use]
pub fn power_off(progress: f32) -> CrtFrame {
    let p = progress.clamp(0.0, 1.0);
    if p < 0.55 {
        // The picture collapses vertically into a scanline.
        let t = OFF_COLLAPSE.ease(span(p, 0.0, 0.55));
        return CrtFrame {
            width: 1.0,
            height: lerp(1.0, DOT_Y, t),
            glow: lerp(0.0, 0.9, t),
        };
    }
    if p < 0.72 {
        // It flares wide for a moment — the source widens to 1.04;
        // a mask cannot exceed the screen, so the flare is carried
        // by the glow alone.
        let t = EASE_IN.ease(span(p, 0.55, 0.72));
        return CrtFrame {
            width: 1.0,
            height: DOT_Y,
            glow: lerp(0.9, 1.0, t),
        };
    }
    // …then pinches to a dot and burns out.
    let t = OFF_PINCH.ease(span(p, 0.72, 1.0));
    CrtFrame {
        width: lerp(1.0, DOT_X, t),
        height: DOT_Y,
        glow: lerp(1.0, 0.0, t),
    }
}

/// The burnout flare: how white the screen goes as the dot dies.
///
/// A real tube's last moment is a bright point, not a fade to
/// black — the source expresses it as `brightness(3)` while the
/// shell's opacity reaches zero. Only the final tenth of the
/// power-off carries it, and it falls back to nothing so the app
/// never exits on a lit screen. Pure — tested.
#[must_use]
pub fn burnout(crt: Crt) -> f32 {
    let Crt::Off(elapsed) = crt else {
        return 0.0;
    };
    let progress = (elapsed / crt_off_s(CRT_ON_S)).clamp(0.0, 1.0);
    if progress < 0.86 {
        return 0.0;
    }
    // Up over the pinch, then out: peak at 0.93, dark by the end.
    let t = span(progress, 0.86, 1.0);
    let arc = 1.0 - (t * 2.0 - 1.0).abs();
    (arc * 0.55).clamp(0.0, 1.0)
}

/// What the tube is doing.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub enum Crt {
    /// Powering on, seconds elapsed.
    On(f32),
    /// Settled — the mask is gone.
    Idle,
    /// Powering off, seconds elapsed. The app exits when it ends.
    Off(f32),
}

/// Ask for the tube to power off and the app to quit when it has.
/// The menu's quit paths write this instead of `AppExit`.
#[derive(Message)]
pub struct QuitRequested;

/// The mask's parts.
#[derive(Component)]
struct CrtRoot;
#[derive(Component)]
struct CrtTop;
#[derive(Component)]
struct CrtBottom;
#[derive(Component)]
struct CrtLeft;
#[derive(Component)]
struct CrtRight;
#[derive(Component)]
struct CrtGlow;
#[derive(Component)]
struct CrtFlash;

/// Plugin: the tube.
pub struct CrtPlugin;

impl Plugin for CrtPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Crt::Idle)
            .add_message::<QuitRequested>()
            .add_systems(Startup, spawn_mask)
            // ⚠️ NOT at startup. The boot screen is empty while the
            // songs are still being built, so a power-on there
            // revealed nothing and was over before the first menu
            // existed — which is exactly how it came to be invisible.
            // It plays when the first real picture arrives.
            .add_systems(OnEnter(crate::states::AppState::MainMenu), power_on_once)
            .add_systems(Update, (begin_power_off, run_tube).chain());
    }
}

/// Full-screen black, in four panels that close in on the picture.
/// Above everything (a global z-index), because a power-off has to
/// cover whatever screen the player was on.
fn spawn_mask(mut commands: Commands) {
    let black = || BackgroundColor(Color::BLACK);
    commands
        .spawn((
            CrtRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            GlobalZIndex(i32::MAX),
            Pickable::IGNORE,
        ))
        .with_children(|mask| {
            let bar = |top: bool| Node {
                position_type: PositionType::Absolute,
                left: percent(0),
                right: percent(0),
                top: if top { percent(0) } else { Val::Auto },
                bottom: if top { Val::Auto } else { percent(0) },
                width: percent(100),
                height: percent(0),
                ..default()
            };
            let side = |left: bool| Node {
                position_type: PositionType::Absolute,
                top: percent(0),
                bottom: percent(0),
                left: if left { percent(0) } else { Val::Auto },
                right: if left { Val::Auto } else { percent(0) },
                height: percent(100),
                width: percent(0),
                ..default()
            };
            mask.spawn((CrtTop, bar(true), black(), Pickable::IGNORE));
            mask.spawn((CrtBottom, bar(false), black(), Pickable::IGNORE));
            mask.spawn((CrtLeft, side(true), black(), Pickable::IGNORE));
            mask.spawn((CrtRight, side(false), black(), Pickable::IGNORE));
            // The scanline: a bright band across the middle, as wide
            // as the visible window.
            // The scanline BLOOMS: a hard-edged bar reads as a UI
            // rectangle, a real tube's line bleeds into the dark
            // around it. This is the port of the source's
            // `brightness()` filter, which a mask cannot apply to
            // the picture itself — the gradient puts the light where
            // the filter would have spilled it.
            mask.spawn((
                CrtGlow,
                Node {
                    position_type: PositionType::Absolute,
                    top: percent(50),
                    left: percent(0),
                    width: percent(100),
                    height: px(2),
                    ..default()
                },
                BackgroundGradient::from(LinearGradient::to_bottom(vec![
                    ColorStop::new(Color::NONE, percent(0)),
                    ColorStop::new(Color::WHITE, percent(50)),
                    ColorStop::new(Color::NONE, percent(100)),
                ])),
                Pickable::IGNORE,
            ));
            // The burnout: the last white flare as the dot dies.
            mask.spawn((
                CrtFlash,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                Pickable::IGNORE,
            ));
        });
}

/// Play the tube once, when the game first shows a menu. Later
/// visits to the main menu are navigation, not a power-on.
fn power_on_once(mut crt: ResMut<Crt>, mut played: Local<bool>) {
    if *played {
        return;
    }
    *played = true;
    *crt = Crt::On(0.0);
}

/// A quit request starts the power-off — unless motion is off, in
/// which case the app leaves at once.
fn begin_power_off(
    mut requests: MessageReader<QuitRequested>,
    settings: Res<Settings>,
    mut crt: ResMut<Crt>,
    mut exit: MessageWriter<AppExit>,
) {
    if requests.read().count() == 0 {
        return;
    }
    if !settings.backdrop_motion || matches!(*crt, Crt::Off(_)) {
        exit.write(AppExit::Success);
        return;
    }
    *crt = Crt::Off(0.0);
}

/// Advance the tube and paint the mask.
#[allow(clippy::type_complexity)] // four disjoint panel queries
fn run_tube(
    time: Res<Time>,
    settings: Res<Settings>,
    mut crt: ResMut<Crt>,
    mut exit: MessageWriter<AppExit>,
    mut panels: ParamSet<(
        Query<&mut Node, With<CrtTop>>,
        Query<&mut Node, With<CrtBottom>>,
        Query<&mut Node, With<CrtLeft>>,
        Query<&mut Node, With<CrtRight>>,
        Query<(&mut Node, &mut BackgroundGradient), With<CrtGlow>>,
        Query<&mut BackgroundColor, With<CrtFlash>>,
    )>,
) {
    let frame = match *crt {
        Crt::Idle => return,
        Crt::On(elapsed) => {
            // Reduced motion skips the show but still settles the
            // mask, so a primed dot can never get stuck on screen.
            let elapsed = if settings.backdrop_motion {
                elapsed + time.delta_secs()
            } else {
                CRT_ON_S
            };
            if elapsed >= CRT_ON_S {
                *crt = Crt::Idle;
                CrtFrame::PICTURE
            } else {
                *crt = Crt::On(elapsed);
                power_on(elapsed / CRT_ON_S)
            }
        }
        Crt::Off(elapsed) => {
            let elapsed = elapsed + time.delta_secs();
            let total = crt_off_s(CRT_ON_S);
            if elapsed >= total {
                exit.write(AppExit::Success);
                power_off(1.0)
            } else {
                *crt = Crt::Off(elapsed);
                power_off(elapsed / total)
            }
        }
    };
    // Each panel covers half of what the window does not show.
    let bar_h = percent((1.0 - frame.height) * 50.0);
    let bar_w = percent((1.0 - frame.width) * 50.0);
    for mut node in panels.p0().iter_mut() {
        node.height = bar_h;
    }
    for mut node in panels.p1().iter_mut() {
        node.height = bar_h;
    }
    for mut node in panels.p2().iter_mut() {
        node.width = bar_w;
    }
    for mut node in panels.p3().iter_mut() {
        node.width = bar_w;
    }
    for (mut node, mut gradient) in panels.p4().iter_mut() {
        node.width = percent(frame.width * 100.0);
        node.left = percent((1.0 - frame.width) * 50.0);
        // The band grows with the glow: a fixed hairline is what a
        // 2 px line looks like on a 1440 px screen - nothing.
        let thickness = GLOW_PX.mul_add(frame.glow, 2.0);
        node.height = px(thickness);
        node.top = percent(50.0);
        node.margin = UiRect::top(px(-thickness / 2.0));
        *gradient = BackgroundGradient::from(LinearGradient::to_bottom(vec![
            ColorStop::new(Color::NONE, percent(0)),
            ColorStop::new(palette::TEXT.with_alpha(frame.glow), percent(50)),
            ColorStop::new(Color::NONE, percent(100)),
        ]));
    }
    // The burnout flare, over everything, for the last instant of
    // the pinch only. Gated on reduced flashing: a white full-screen
    // flash is exactly what that setting exists to suppress.
    for mut color in panels.p5().iter_mut() {
        let flash = if settings.reduced_flashing {
            0.0
        } else {
            burnout(*crt)
        };
        color.0 = Color::WHITE.with_alpha(flash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaving_is_never_slower_than_arriving() {
        // The source's rule, and the reason the power-off is derived
        // rather than configured beside the power-on: "once you have
        // decided to leave, any wait is pure cost".
        for on in [0.08, 0.25, 0.5, 0.9] {
            assert!(crt_off_s(on) < on, "power-off {on} is not shorter");
        }
        assert!((crt_off_s(0.0) - 0.0).abs() < f32::EPSILON, "off means off");
    }

    #[test]
    fn the_easing_solver_matches_css() {
        // The curves are the character of the animation, so the
        // solver gets checked against values CSS itself produces.
        // `ease-in` is `ease-out` mirrored, which is a free second
        // opinion on the solve.
        assert!((EASE_OUT.ease(0.5) - 0.6846).abs() < 1e-3);
        assert!((EASE_IN.ease(0.5) - 0.3154).abs() < 1e-3);
        assert!((EASE_OUT.ease(0.25) - 0.3781).abs() < 1e-3);
        // A curve whose control points lie on the diagonal IS
        // linear - if the solver were wrong this would drift.
        let straight = Bezier {
            x1: 0.25,
            y1: 0.25,
            x2: 0.75,
            y2: 0.75,
        };
        assert!((straight.ease(0.4) - 0.4).abs() < 1e-3);
        // The ends are exact, never approximated.
        assert!((EASE_OUT.ease(0.0) - 0.0).abs() < f32::EPSILON);
        assert!((EASE_OUT.ease(1.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_power_on_is_legible_before_it_finishes() {
        // The port's whole point: the tube identity survives, but
        // the picture must be readable early. Full height lands at
        // the source's 56 %, not at the end.
        assert!(power_on(0.56).height > 0.99, "full height by 56 %");
        // The opening is FRONT-LOADED - the source's
        // cubic-bezier(0.12, 0.7, 0.3, 1) throws most of the height
        // into the first third of its segment, which is what makes
        // a tube read as a tube instead of a wipe. (The first
        // version of this test asserted the opposite; it was
        // pinning the linear approximation this replaced.)
        assert!(power_on(0.30).height > 0.5, "well open a third in");
        assert!(power_on(0.24).height < 0.3, "but not instantly");
        // …and the width snaps first, so it reads as a scanline
        // rather than a growing box.
        assert!(power_on(0.23).width > 0.99);
        assert!(power_on(0.23).height < 0.05);
    }

    #[test]
    fn the_tube_starts_at_a_dot_and_ends_at_the_picture() {
        let start = power_on(0.0);
        assert!(start.width < 0.05 && start.height < 0.05, "a dot");
        assert!(start.glow > 0.9, "and a bright one");
        assert_eq!(power_on(1.0), CrtFrame::PICTURE);
        // Past the end stays settled - a frame that overshot would
        // leave a mask edge on screen forever.
        assert_eq!(power_on(1.5), CrtFrame::PICTURE);
    }

    #[test]
    fn the_power_off_collapses_the_other_way_round() {
        assert_eq!(power_off(0.0), CrtFrame::PICTURE);
        // Height goes first (the vertical collapse), width last (the
        // pinch) - the reverse of the power-on, which widens first.
        let scanline = power_off(0.55);
        assert!(scanline.height < 0.05 && scanline.width > 0.99);
        let dot = power_off(1.0);
        assert!(dot.width < 0.05 && dot.height < 0.05);
        assert!(dot.glow < 0.05, "the tube burns out dark");
    }

    #[test]
    fn the_burnout_flares_late_and_always_dies() {
        // A tube's last moment is a bright point. It must land at
        // the very end (or it reads as a flash mid-collapse) and it
        // must be gone by the last frame - the app exits there, and
        // exiting on a lit screen would leave the flash as the
        // player's final impression.
        let total = crt_off_s(CRT_ON_S);
        assert!((burnout(Crt::Off(0.0)) - 0.0).abs() < f32::EPSILON);
        assert!((burnout(Crt::Off(total * 0.5)) - 0.0).abs() < f32::EPSILON);
        assert!(burnout(Crt::Off(total * 0.93)) > 0.3, "it flares");
        assert!(burnout(Crt::Off(total)) < 0.05, "and it is gone");
        // Never during a power-on, and never while idle.
        assert!((burnout(Crt::On(0.1)) - 0.0).abs() < f32::EPSILON);
        assert!((burnout(Crt::Idle) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn every_frame_stays_inside_the_screen() {
        // The mask is drawn from these numbers; one above 1 would
        // put a black bar with negative size on screen.
        for step in 0..=100 {
            let p = step as f32 / 100.0;
            for frame in [power_on(p), power_off(p)] {
                assert!((0.0..=1.0).contains(&frame.width), "width at {p}");
                assert!((0.0..=1.0).contains(&frame.height), "height at {p}");
                assert!((0.0..=1.0).contains(&frame.glow), "glow at {p}");
            }
        }
    }
}
