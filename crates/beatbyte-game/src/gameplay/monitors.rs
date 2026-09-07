//! Two monitors on the PA stacks: the left one shows the tempo in
//! BPM, the right one the level in dBFS — both **measured at the
//! laptop**, from its audio input, by [`beatbyte_audio::listen`].
//! Nothing here reads the chart.
//!
//! The one rule: **no measurement, no monitor.** A machine without
//! an input device, a device that will not open, a permission that
//! was refused (an open device that never delivers a non-zero
//! sample), a stream that dies — in every one of those cases the
//! monitors are not dimmed, not blank, not showing zeros: they do
//! not exist. They are spawned the frame the listener first reports
//! a measurement and despawned the frame it stops.
//!
//! Drawn in the house's own way — no text in 3D exists, and none is
//! needed: a dark screen plate on each amp head's face, three
//! seven-segment cells of thin emissive bars, driven by visibility
//! alone. A cell's bars change only when its digit changes, so a
//! steady readout costs nothing per frame. The tempo cell is blank
//! while no tempo is heard (a quiet room has no BPM); the level
//! always has a value, so it always shows one.

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use beatbyte_audio::listen::Listener;

use super::GameplayScreen;
use super::pa::{self, CabinetKind};
use super::stage3d::{self, STAGE_LAYER, Stage3d};
use crate::config::Settings;
use crate::states::AppState;

/// The listener the stage holds while the venue is up. `None` when
/// the stage is not shown (nothing to mount a monitor on).
#[derive(Resource, Default)]
pub struct Ears(pub Option<Listener>);

/// Which number a monitor shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readout {
    /// Tempo, on the left stack.
    Bpm,
    /// Level in dBFS, on the right stack.
    Db,
}

impl Readout {
    /// The stack a readout is mounted on: −1 left, +1 right.
    #[must_use]
    pub fn side(self) -> f32 {
        match self {
            Readout::Bpm => -1.0,
            Readout::Db => 1.0,
        }
    }
}

/// What one cell shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// Nothing lit.
    Blank,
    /// A digit 0–9.
    Digit(u8),
    /// The minus sign (the middle bar).
    Minus,
}

/// Cells per monitor.
pub const CELLS: usize = 3;
/// Bars per cell (the classic seven: a top, b upper right, c lower
/// right, d bottom, e lower left, f upper left, g middle).
pub const BARS: usize = 7;

/// The screen plate's size: wide enough for three cells, no taller
/// than the amp head's free band above the knobs.
pub const SCREEN: Vec3 = Vec3::new(0.60, 0.17, 0.012);
/// Where the screen sits on the head's face, above its centre.
pub const SCREEN_RISE: f32 = 0.115;
/// How far the screen stands off the head's face.
pub const SCREEN_OFF: f32 = 0.020;
/// A cell's size.
pub const CELL: Vec2 = Vec2::new(0.10, 0.125);
/// The gap between cells.
pub const CELL_GAP: f32 = 0.03;
/// A bar's thickness.
pub const BAR: f32 = 0.014;
/// How far the bars stand off the screen.
pub const BAR_OFF: f32 = 0.008;

/// The seven-segment mask of a digit, bit k = bar k lit. Pure —
/// tested.
#[must_use]
pub fn digit_mask(digit: u8) -> u8 {
    const A: u8 = 1;
    const B: u8 = 2;
    const C: u8 = 4;
    const D: u8 = 8;
    const E: u8 = 16;
    const F: u8 = 32;
    const G: u8 = 64;
    match digit {
        0 => A | B | C | D | E | F,
        1 => B | C,
        2 => A | B | D | E | G,
        3 => A | B | C | D | G,
        4 => B | C | F | G,
        5 => A | C | D | F | G,
        6 => A | C | D | E | F | G,
        7 => A | B | C,
        8 => A | B | C | D | E | F | G,
        9 => A | B | C | D | F | G,
        _ => 0,
    }
}

/// The mask of a cell. Pure — tested.
#[must_use]
pub fn cell_mask(cell: Cell) -> u8 {
    match cell {
        Cell::Blank => 0,
        Cell::Digit(d) => digit_mask(d),
        Cell::Minus => 64,
    }
}

/// The cells for a tempo: the rounded BPM right-aligned, blank
/// while there is none. Pure — tested.
#[must_use]
pub fn bpm_cells(bpm: Option<f32>) -> [Cell; CELLS] {
    match bpm {
        Some(bpm) if bpm > 0.0 => number_cells(bpm.round().clamp(1.0, 999.0) as u32),
        _ => [Cell::Blank; CELLS],
    }
}

/// The cells for a level in dBFS: the rounded value with its sign,
/// −99 ..= 0. Pure — tested.
#[must_use]
pub fn db_cells(db: f32) -> [Cell; CELLS] {
    let value = db.round().clamp(-99.0, 0.0) as i32;
    if value == 0 {
        return [Cell::Blank, Cell::Blank, Cell::Digit(0)];
    }
    let magnitude = value.unsigned_abs();
    let tens = (magnitude / 10) as u8;
    let ones = (magnitude % 10) as u8;
    if tens == 0 {
        [Cell::Blank, Cell::Minus, Cell::Digit(ones)]
    } else {
        [Cell::Minus, Cell::Digit(tens), Cell::Digit(ones)]
    }
}

/// A positive number right-aligned in the cells, blanks in front.
fn number_cells(value: u32) -> [Cell; CELLS] {
    let mut cells = [Cell::Blank; CELLS];
    let mut rest = value.min(999);
    for slot in (0..CELLS).rev() {
        cells[slot] = Cell::Digit((rest % 10) as u8);
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    cells
}

/// A bar's pose inside a cell: `(centre offset from the cell's
/// centre, size)`. Horizontal bars run the cell's width less the
/// corners; vertical bars run half the height. Pure — tested.
#[must_use]
pub fn bar_pose(bar: usize) -> (Vec2, Vec2) {
    let half_w = CELL.x * 0.5 - BAR * 0.5;
    let half_h = CELL.y * 0.5 - BAR * 0.5;
    let horizontal = Vec2::new(CELL.x - 2.0 * BAR, BAR);
    let vertical = Vec2::new(BAR, CELL.y * 0.5 - 1.5 * BAR);
    match bar {
        0 => (Vec2::new(0.0, half_h), horizontal),
        1 => (Vec2::new(half_w, half_h * 0.5), vertical),
        2 => (Vec2::new(half_w, -half_h * 0.5), vertical),
        3 => (Vec2::new(0.0, -half_h), horizontal),
        4 => (Vec2::new(-half_w, -half_h * 0.5), vertical),
        5 => (Vec2::new(-half_w, half_h * 0.5), vertical),
        _ => (Vec2::ZERO, horizontal),
    }
}

/// A cell's centre x relative to the screen's centre. Pure — tested.
#[must_use]
pub fn cell_x(cell: usize) -> f32 {
    let pitch = CELL.x + CELL_GAP;
    (cell as f32 - (CELLS as f32 - 1.0) * 0.5) * pitch
}

/// Where a readout's screen sits: on the face of that stack's amp
/// head, above the knobs. Pure — tested against the PA's layout.
#[must_use]
pub fn screen_centre(readout: Readout) -> Vec3 {
    let head = pa::stack_layout(readout.side())
        .into_iter()
        .find(|cabinet| cabinet.kind == CabinetKind::Head)
        .expect("every stack has a head");
    Vec3::new(
        head.centre.x,
        head.centre.y + SCREEN_RISE,
        head.centre.z + head.size.z * 0.5 + SCREEN_OFF,
    )
}

/// A monitor's screen.
#[derive(Component, Debug, Clone, Copy)]
pub struct Monitor {
    /// What it shows.
    pub readout: Readout,
    /// The cells it last drew.
    pub shown: [Cell; CELLS],
}

/// One bar of one cell of one monitor.
#[derive(Component, Debug, Clone, Copy)]
pub struct MonitorBar {
    /// The monitor's readout.
    pub readout: Readout,
    /// The cell, 0 = leftmost.
    pub cell: usize,
    /// The bar, 0..7.
    pub bar: usize,
}

/// Open the room's input for the venue: the listener starts on its
/// own thread and the monitors follow what it hears. Nothing opens
/// when the stage is not shown.
pub fn open_ears(settings: Res<Settings>, mut ears: ResMut<Ears>) {
    ears.0 = stage3d::active(&settings).then(Listener::open);
}

/// Drop the listener with the venue: the thread stops, the device
/// closes.
pub fn close_ears(mut ears: ResMut<Ears>) {
    ears.0 = None;
}

/// Keep the monitors true to the listener: spawn them the moment a
/// measurement exists, despawn them the moment it does not, and
/// redraw a cell only when its digit changes.
#[allow(clippy::too_many_arguments)] // a Bevy system: every parameter is a world handle
pub fn tend_monitors(
    mut commands: Commands,
    settings: Res<Settings>,
    ears: Res<Ears>,
    theme: Res<crate::theme::ActiveTheme>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut monitors: Query<(Entity, &mut Monitor)>,
    mut bars: Query<(Entity, &MonitorBar, &mut Visibility)>,
    time: Res<Time>,
    mut reported_at: Local<f32>,
) {
    if !stage3d::active(&settings) {
        return;
    }
    let measuring = ears.0.as_ref().is_some_and(Listener::measuring);
    let present = !monitors.is_empty();
    match (measuring, present) {
        (false, false) => return,
        (false, true) => {
            info!("monitors: no measurement any more — despawned");
            for (entity, _) in &monitors {
                commands.entity(entity).despawn();
            }
            for (entity, _, _) in &bars {
                commands.entity(entity).despawn();
            }
            return;
        }
        (true, false) => {
            info!("monitors: the input is heard — spawned on both stacks");
            spawn_monitors(&mut commands, &mut meshes, &mut materials, theme.0.accent);
            return;
        }
        (true, true) => {}
    }
    let Some(listener) = ears.0.as_ref() else {
        return;
    };
    // A line every five seconds, so a run's log shows what the
    // monitors read (an ECS-level probe: a locked screen renders
    // black, a log line does not).
    if time.elapsed_secs() - *reported_at >= 5.0 {
        *reported_at = time.elapsed_secs();
        info!(
            "monitors: level {:.1} dBFS, tempo {}",
            listener.db(),
            listener
                .bpm()
                .map_or_else(|| "none".to_owned(), |bpm| format!("{bpm:.0} BPM"))
        );
    }
    let wanted = |readout: Readout| match readout {
        Readout::Bpm => bpm_cells(listener.bpm()),
        Readout::Db => db_cells(listener.db()),
    };
    for (_, mut monitor) in &mut monitors {
        let cells = wanted(monitor.readout);
        if cells == monitor.shown {
            continue;
        }
        for (_, bar, mut visibility) in &mut bars {
            if bar.readout != monitor.readout || cells[bar.cell] == monitor.shown[bar.cell] {
                continue;
            }
            let lit = cell_mask(cells[bar.cell]) & (1 << bar.bar) != 0;
            *visibility = if lit {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        monitor.shown = cells;
    }
}

/// Both monitors, every bar hidden: the first `tend` lights them.
fn spawn_monitors(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    accent: Color,
) {
    let layer = RenderLayers::layer(STAGE_LAYER);
    let screen = meshes.add(Cuboid::from_size(SCREEN));
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.022, 0.03),
        perceptual_roughness: 0.25,
        metallic: 0.0,
        reflectance: 0.35,
        ..default()
    });
    let lit = materials.add(StandardMaterial {
        base_color: accent,
        emissive: accent.to_linear() * 5.0,
        unlit: true,
        ..default()
    });
    let bar_meshes: Vec<Handle<Mesh>> = (0..BARS)
        .map(|bar| {
            let (_, size) = bar_pose(bar);
            meshes.add(Cuboid::new(size.x, size.y, BAR * 0.6))
        })
        .collect();
    for readout in [Readout::Bpm, Readout::Db] {
        let centre = screen_centre(readout);
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Monitor {
                readout,
                shown: [Cell::Blank; CELLS],
            },
            Mesh3d(screen.clone()),
            MeshMaterial3d(glass.clone()),
            Transform::from_translation(centre),
            layer.clone(),
        ));
        for cell in 0..CELLS {
            for (bar, mesh) in bar_meshes.iter().enumerate() {
                let (offset, _) = bar_pose(bar);
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    NotShadowCaster,
                    MonitorBar { readout, cell, bar },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(lit.clone()),
                    Transform::from_translation(
                        centre
                            + Vec3::new(
                                cell_x(cell) + offset.x,
                                offset.y,
                                SCREEN.z * 0.5 + BAR_OFF,
                            ),
                    ),
                    Visibility::Hidden,
                    layer.clone(),
                ));
            }
        }
    }
}

/// Wire the monitors into the app: the ears open and close with the
/// venue, the monitors are tended while the song plays on.
pub fn register(app: &mut App) {
    app.init_resource::<Ears>()
        .add_systems(OnEnter(AppState::Gameplay), open_ears)
        .add_systems(OnExit(AppState::Gameplay), close_ears)
        .add_systems(
            Update,
            tend_monitors.run_if(
                in_state(crate::states::GamePhase::Playing)
                    .or_else(in_state(crate::states::GamePhase::Outro)),
            ),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_digit_has_its_bars() {
        let masks: Vec<u8> = (0..10).map(digit_mask).collect();
        // Ten different shapes, none empty, 8 lights everything.
        for (i, a) in masks.iter().enumerate() {
            assert_ne!(*a, 0);
            for b in masks.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
        assert_eq!(digit_mask(8), 127);
        assert_eq!(digit_mask(1), 2 | 4, "a one is the right-hand bars");
        assert_eq!(cell_mask(Cell::Minus), 64, "a minus is the middle bar");
        assert_eq!(cell_mask(Cell::Blank), 0);
    }

    #[test]
    fn a_tempo_is_right_aligned_and_blank_when_there_is_none() {
        assert_eq!(bpm_cells(None), [Cell::Blank; 3]);
        assert_eq!(bpm_cells(Some(0.0)), [Cell::Blank; 3]);
        assert_eq!(
            bpm_cells(Some(120.4)),
            [Cell::Digit(1), Cell::Digit(2), Cell::Digit(0)]
        );
        assert_eq!(
            bpm_cells(Some(96.6)),
            [Cell::Blank, Cell::Digit(9), Cell::Digit(7)]
        );
        assert_eq!(
            bpm_cells(Some(4000.0)),
            [Cell::Digit(9), Cell::Digit(9), Cell::Digit(9)]
        );
    }

    #[test]
    fn a_level_carries_its_sign() {
        assert_eq!(
            db_cells(-23.4),
            [Cell::Minus, Cell::Digit(2), Cell::Digit(3)]
        );
        assert_eq!(db_cells(-6.0), [Cell::Blank, Cell::Minus, Cell::Digit(6)]);
        assert_eq!(db_cells(0.0), [Cell::Blank, Cell::Blank, Cell::Digit(0)]);
        assert_eq!(
            db_cells(-100.0),
            [Cell::Minus, Cell::Digit(9), Cell::Digit(9)],
            "the floor is clamped to what three cells can say"
        );
        assert_eq!(db_cells(3.0), db_cells(0.0), "nothing above full scale");
    }

    #[test]
    fn the_bars_stay_inside_their_cell_and_the_cells_inside_the_screen() {
        for bar in 0..BARS {
            let (offset, size) = bar_pose(bar);
            assert!(
                offset.x.abs() + size.x * 0.5 <= CELL.x * 0.5 + 1e-6,
                "bar {bar}"
            );
            assert!(
                offset.y.abs() + size.y * 0.5 <= CELL.y * 0.5 + 1e-6,
                "bar {bar}"
            );
        }
        for cell in 0..CELLS {
            assert!(cell_x(cell).abs() + CELL.x * 0.5 < SCREEN.x * 0.5);
        }
        // The tallest bar reach stays inside the screen's height.
        let (top, size) = bar_pose(0);
        assert!(top.y + size.y * 0.5 < SCREEN.y * 0.5);
        // Vertical bars on one side do not overlap each other.
        let (upper, size) = bar_pose(1);
        let (lower, _) = bar_pose(2);
        assert!(upper.y - size.y * 0.5 >= lower.y + size.y * 0.5 - 1e-6);
    }

    #[test]
    fn the_screen_sits_on_the_heads_face_above_the_knobs() {
        for readout in [Readout::Bpm, Readout::Db] {
            let centre = screen_centre(readout);
            let head = pa::stack_layout(readout.side())[3];
            assert_eq!(head.kind, CabinetKind::Head);
            assert_eq!(centre.x, head.centre.x);
            // Inside the head's face, in front of it.
            let top = centre.y + SCREEN.y * 0.5;
            let bottom = centre.y - SCREEN.y * 0.5;
            assert!(
                top <= head.top() + 1e-6,
                "{top} over the head's top {}",
                head.top()
            );
            // The knobs sit at centre − 0.02 with radius 0.035: the
            // screen's bottom edge clears them.
            assert!(
                bottom >= head.centre.y - 0.02 + 0.035,
                "{bottom} into the knobs"
            );
            assert!(centre.z > head.centre.z + head.size.z * 0.5);
            // And under the camera, like the head itself.
            assert!(top < pa::CAMERA_Y - 0.2);
        }
        assert!(
            screen_centre(Readout::Bpm).x < 0.0,
            "tempo on the left stack"
        );
        assert!(
            screen_centre(Readout::Db).x > 0.0,
            "level on the right stack"
        );
    }

    /// The contract the commission is about, wired: no measurement,
    /// no monitor — not blank, not zero, absent — and the monitors
    /// appear the frame a measurement exists and vanish the frame it
    /// stops.
    #[test]
    fn the_monitors_exist_exactly_while_something_is_measured() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.insert_resource(Settings {
            stage_3d: true,
            ..Settings::default()
        });
        app.insert_resource(crate::theme::ActiveTheme::default());
        let count = |world: &mut World| {
            (
                world.query::<&Monitor>().iter(world).count(),
                world.query::<&MonitorBar>().iter(world).count(),
            )
        };
        // Open but never heard: a refused microphone. Nothing.
        app.insert_resource(Ears(Some(Listener::stub(false, -30.0, Some(120.0)))));
        app.world_mut()
            .run_system_once(tend_monitors)
            .expect("the system runs");
        assert_eq!(
            count(app.world_mut()),
            (0, 0),
            "a silent device shows no monitor"
        );
        // No listener at all (the stage is not shown): nothing.
        app.insert_resource(Ears(None));
        app.world_mut()
            .run_system_once(tend_monitors)
            .expect("the system runs");
        assert_eq!(count(app.world_mut()), (0, 0));
        // Measuring: both monitors, every bar of every cell.
        app.insert_resource(Ears(Some(Listener::stub(true, -23.4, Some(120.0)))));
        app.world_mut()
            .run_system_once(tend_monitors)
            .expect("the system runs");
        assert_eq!(count(app.world_mut()), (2, 2 * CELLS * BARS));
        // The second tend lights the digits: −23 on the right, 120
        // on the left, as visibility.
        app.world_mut()
            .run_system_once(tend_monitors)
            .expect("the system runs");
        let lit = |world: &mut World, readout: Readout| -> Vec<(usize, usize)> {
            let mut bars: Vec<(usize, usize)> = world
                .query::<(&MonitorBar, &Visibility)>()
                .iter(world)
                .filter(|(bar, visibility)| {
                    bar.readout == readout && **visibility == Visibility::Inherited
                })
                .map(|(bar, _)| (bar.cell, bar.bar))
                .collect();
            bars.sort_unstable();
            bars
        };
        let expect = |cells: [Cell; CELLS]| -> Vec<(usize, usize)> {
            let mut out = Vec::new();
            for (cell, value) in cells.iter().enumerate() {
                for bar in 0..BARS {
                    if cell_mask(*value) & (1 << bar) != 0 {
                        out.push((cell, bar));
                    }
                }
            }
            out
        };
        assert_eq!(lit(app.world_mut(), Readout::Db), expect(db_cells(-23.4)));
        assert_eq!(
            lit(app.world_mut(), Readout::Bpm),
            expect(bpm_cells(Some(120.0)))
        );
        // The stream dies: the monitors are gone, bars and all.
        app.insert_resource(Ears(Some(Listener::stub(false, -23.4, Some(120.0)))));
        app.world_mut()
            .run_system_once(tend_monitors)
            .expect("the system runs");
        assert_eq!(
            count(app.world_mut()),
            (0, 0),
            "a dead stream takes the monitors with it"
        );
    }
}
