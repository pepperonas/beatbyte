//! Two monitors on the PA stacks: the left one shows the tempo in
//! BPM, the right one the level — both **measured at the laptop**,
//! from its audio input, by [`beatbyte_audio::listen`]. Nothing here
//! reads the chart.
//!
//! The one rule: **no measurement, no monitor.** A machine without
//! an input device, a device that will not open, a permission that
//! was refused (an open device that never delivers a non-zero
//! sample), a stream that dies — in every one of those cases the
//! monitors are not dimmed, not blank, not showing zeros: they do
//! not exist. They are spawned the frame the listener first reports
//! a measurement and despawned the frame it stops.
//!
//! Drawn the way the reference rig draws its readouts — the R4's LED
//! matrix and the dB-Analyse page: a dot-matrix panel standing on
//! each amp head, three 5×7 digits of emissive dots, big enough to
//! read from the camera (the first cut, seven-segment cells on the
//! head's face, was reported too small). The level is shown the way
//! the dB-Analyse shows it, **dBFS + 100** — a positive, phone-
//! comparable figure, the reference's convention since its
//! recalibration — never as the negative dBFS the meter measures.
//! Dots change by visibility alone, a digit only when it changes.
//! The tempo panel is blank while no tempo is heard.

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
    /// Level, on the right stack.
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
}

/// Cells per monitor.
pub const CELLS: usize = 3;
/// Dot columns per cell.
pub const COLS: usize = 5;
/// Dot rows per cell.
pub const ROWS: usize = 7;
/// Blank columns between cells.
pub const CELL_GAP_COLS: usize = 2;
/// Dot columns across the panel.
pub const PANEL_COLS: usize = CELLS * COLS + (CELLS - 1) * CELL_GAP_COLS;

/// What the level monitor adds to the measured dBFS before showing
/// it: the dB-Analyse's convention (its display is dBFS + 100), so
/// the two readouts agree. A display offset, not a calibration —
/// the meter still measures dBFS.
pub const DB_SHOWN_OFFSET: f32 = 100.0;

/// The dot pitch, world units.
pub const DOT_PITCH: f32 = 0.058;
/// A dot's diameter.
pub const DOT: f32 = 0.042;
/// The panel's margin around the dots.
pub const PANEL_MARGIN: f32 = 0.05;
/// The panel's depth.
pub const PANEL_DEPTH: f32 = 0.06;
/// The gap between the head's top and the panel's bottom (the
/// handle sits there).
pub const PANEL_LIFT: f32 = 0.045;
/// How far the dots stand off the panel's face.
pub const DOT_OFF: f32 = 0.004;

/// The panel's size: the dot grid plus its margin.
#[must_use]
pub fn panel_size() -> Vec3 {
    Vec3::new(
        PANEL_COLS as f32 * DOT_PITCH + 2.0 * PANEL_MARGIN,
        ROWS as f32 * DOT_PITCH + 2.0 * PANEL_MARGIN,
        PANEL_DEPTH,
    )
}

/// The 5×7 dot font, one row per byte, bit 4 the leftmost column.
/// Pure — tested.
#[must_use]
pub fn glyph_rows(cell: Cell) -> [u8; ROWS] {
    match cell {
        Cell::Blank => [0; ROWS],
        Cell::Digit(d) => match d {
            0 => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
            1 => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
            2 => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
            3 => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
            4 => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
            5 => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
            6 => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
            7 => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
            8 => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
            9 => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
            _ => [0; ROWS],
        },
    }
}

/// Whether the dot at `(col, row)` of a cell is lit. Pure — tested.
#[must_use]
pub fn dot_lit(cell: Cell, col: usize, row: usize) -> bool {
    if col >= COLS || row >= ROWS {
        return false;
    }
    glyph_rows(cell)[row] & (1 << (COLS - 1 - col)) != 0
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

/// The cells for a measured level in dBFS: shown as dBFS + 100, the
/// dB-Analyse's figure, right-aligned, 0 ..= 100. Pure — tested.
#[must_use]
pub fn db_cells(db: f32) -> [Cell; CELLS] {
    let shown = (db + DB_SHOWN_OFFSET).round().clamp(0.0, 100.0) as u32;
    number_cells(shown)
}

/// A number right-aligned in the cells, blanks in front (a zero is
/// still a zero).
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

/// A dot's offset from the panel's centre, in the panel's plane.
/// Pure — tested.
#[must_use]
pub fn dot_offset(cell: usize, col: usize, row: usize) -> Vec2 {
    let column = cell * (COLS + CELL_GAP_COLS) + col;
    let x = (column as f32 - (PANEL_COLS as f32 - 1.0) * 0.5) * DOT_PITCH;
    let y = ((ROWS as f32 - 1.0) * 0.5 - row as f32) * DOT_PITCH;
    Vec2::new(x, y)
}

/// Where a readout's panel stands: on that stack's amp head, its
/// face flush with the head's, lifted over the handle. Pure — tested
/// against the PA's layout.
#[must_use]
pub fn panel_centre(readout: Readout) -> Vec3 {
    let head = pa::stack_layout(readout.side())
        .into_iter()
        .find(|cabinet| cabinet.kind == CabinetKind::Head)
        .expect("every stack has a head");
    let size = panel_size();
    Vec3::new(
        head.centre.x,
        head.top() + PANEL_LIFT + size.y * 0.5,
        head.centre.z + head.size.z * 0.5 - size.z * 0.5,
    )
}

/// A monitor's panel.
#[derive(Component, Debug, Clone, Copy)]
pub struct Monitor {
    /// What it shows.
    pub readout: Readout,
    /// The cells it last drew.
    pub shown: [Cell; CELLS],
}

/// One dot of one cell of one monitor.
#[derive(Component, Debug, Clone, Copy)]
pub struct MonitorDot {
    /// The monitor's readout.
    pub readout: Readout,
    /// The cell, 0 = leftmost.
    pub cell: usize,
    /// The column inside the cell.
    pub col: usize,
    /// The row inside the cell, 0 = top.
    pub row: usize,
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
    mut dots: Query<(Entity, &MonitorDot, &mut Visibility)>,
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
            for (entity, _, _) in &dots {
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
            "monitors: level {:.1} dBFS (shown {}), tempo {}",
            listener.db(),
            (listener.db() + DB_SHOWN_OFFSET).round().clamp(0.0, 100.0),
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
        for (_, dot, mut visibility) in &mut dots {
            if dot.readout != monitor.readout || cells[dot.cell] == monitor.shown[dot.cell] {
                continue;
            }
            *visibility = if dot_lit(cells[dot.cell], dot.col, dot.row) {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        monitor.shown = cells;
    }
}

/// Both monitors, every dot hidden: the first `tend` lights them.
fn spawn_monitors(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    accent: Color,
) {
    let layer = RenderLayers::layer(STAGE_LAYER);
    let size = panel_size();
    let panel = meshes.add(Cuboid::from_size(size));
    let housing = materials.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.022, 0.03),
        perceptual_roughness: 0.6,
        metallic: 0.0,
        reflectance: 0.3,
        ..default()
    });
    let lit = materials.add(StandardMaterial {
        base_color: accent,
        emissive: accent.to_linear() * 5.0,
        unlit: true,
        ..default()
    });
    let dot = meshes.add(Cuboid::new(DOT, DOT, DOT * 0.4));
    for readout in [Readout::Bpm, Readout::Db] {
        let centre = panel_centre(readout);
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Monitor {
                readout,
                shown: [Cell::Blank; CELLS],
            },
            Mesh3d(panel.clone()),
            MeshMaterial3d(housing.clone()),
            Transform::from_translation(centre),
            layer.clone(),
        ));
        for cell in 0..CELLS {
            for col in 0..COLS {
                for row in 0..ROWS {
                    let offset = dot_offset(cell, col, row);
                    commands.spawn((
                        GameplayScreen,
                        Stage3d,
                        NotShadowCaster,
                        MonitorDot {
                            readout,
                            cell,
                            col,
                            row,
                        },
                        Mesh3d(dot.clone()),
                        MeshMaterial3d(lit.clone()),
                        Transform::from_translation(
                            centre + Vec3::new(offset.x, offset.y, size.z * 0.5 + DOT_OFF),
                        ),
                        Visibility::Hidden,
                        layer.clone(),
                    ));
                }
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
    fn every_digit_has_a_shape_of_its_own() {
        let shapes: Vec<[u8; ROWS]> = (0..10).map(|d| glyph_rows(Cell::Digit(d))).collect();
        for (i, a) in shapes.iter().enumerate() {
            assert!(a.iter().any(|row| *row != 0), "digit {i} is blank");
            assert!(
                a.iter().all(|row| *row < 32),
                "digit {i} spills past five columns"
            );
            for b in shapes.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
        assert_eq!(glyph_rows(Cell::Blank), [0; ROWS]);
        // A one is a stem with a foot; its top-left dot is dark.
        assert!(dot_lit(Cell::Digit(1), 2, 0));
        assert!(!dot_lit(Cell::Digit(1), 0, 0));
        assert!(dot_lit(Cell::Digit(1), 1, 6) && dot_lit(Cell::Digit(1), 3, 6));
        assert!(
            !dot_lit(Cell::Digit(1), 0, 6),
            "the foot stops short of the edge"
        );
        assert!(
            !dot_lit(Cell::Digit(8), COLS, 0),
            "outside the cell is dark"
        );
        // Left is left: a seven's stroke hangs from the RIGHT end of
        // its bar (a mirrored font would pass every symmetric check).
        assert!(dot_lit(Cell::Digit(7), 4, 1) && !dot_lit(Cell::Digit(7), 0, 1));
        assert!(dot_lit(Cell::Digit(4), 3, 0) && !dot_lit(Cell::Digit(4), 1, 0));
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
    fn the_level_is_shown_as_the_db_analyse_shows_it() {
        // dBFS + 100: a positive figure, never a minus sign.
        assert_eq!(
            db_cells(-38.4),
            [Cell::Blank, Cell::Digit(6), Cell::Digit(2)]
        );
        assert_eq!(
            db_cells(-6.0),
            [Cell::Blank, Cell::Digit(9), Cell::Digit(4)]
        );
        assert_eq!(
            db_cells(0.0),
            [Cell::Digit(1), Cell::Digit(0), Cell::Digit(0)],
            "full scale reads 100"
        );
        assert_eq!(db_cells(-100.0), [Cell::Blank, Cell::Blank, Cell::Digit(0)]);
        assert_eq!(db_cells(-140.0), db_cells(-100.0), "the floor is clamped");
        assert_eq!(db_cells(3.0), db_cells(0.0), "nothing above full scale");
        assert_eq!(DB_SHOWN_OFFSET, 100.0, "the reference's convention");
    }

    #[test]
    fn the_dots_fill_the_panel_without_leaving_it() {
        let size = panel_size();
        let mut min = Vec2::splat(f32::MAX);
        let mut max = Vec2::splat(f32::MIN);
        for cell in 0..CELLS {
            for col in 0..COLS {
                for row in 0..ROWS {
                    let at = dot_offset(cell, col, row);
                    min = min.min(at);
                    max = max.max(at);
                    assert!(
                        at.x.abs() + DOT * 0.5 < size.x * 0.5,
                        "dot outside the panel"
                    );
                    assert!(
                        at.y.abs() + DOT * 0.5 < size.y * 0.5,
                        "dot outside the panel"
                    );
                }
            }
        }
        // Symmetric about the centre, and the grid uses most of it.
        assert!((min.x + max.x).abs() < 1e-5 && (min.y + max.y).abs() < 1e-5);
        assert!(max.x - min.x > size.x * 0.8);
        // Two cells never share a column: the gap is real.
        let right_of_first = dot_offset(0, COLS - 1, 0).x;
        let left_of_second = dot_offset(1, 0, 0).x;
        assert!(left_of_second - right_of_first > DOT_PITCH * 1.5);
        // A digit is taller than the old seven-segment cell by a
        // clear margin — the reason for the redesign.
        assert!(ROWS as f32 * DOT_PITCH > 0.125 * 3.0);
    }

    #[test]
    fn the_panel_stands_on_the_head_with_its_face_flush() {
        for readout in [Readout::Bpm, Readout::Db] {
            let centre = panel_centre(readout);
            let size = panel_size();
            let head = pa::stack_layout(readout.side())[3];
            assert_eq!(head.kind, CabinetKind::Head);
            assert_eq!(centre.x, head.centre.x);
            let bottom = centre.y - size.y * 0.5;
            assert!(
                bottom > head.top(),
                "on top of the head, clear of the handle"
            );
            assert!(bottom - head.top() < 0.1, "not floating");
            let front = centre.z + size.z * 0.5;
            assert!(
                (front - (head.centre.z + head.size.z * 0.5)).abs() < 1e-5,
                "flush"
            );
            // Wide as the head, no wider.
            assert!(
                size.x <= head.size.x + 1e-6,
                "{} wider than the head {}",
                size.x,
                head.size.x
            );
            assert!(size.x > head.size.x * 0.8);
        }
        assert!(
            panel_centre(Readout::Bpm).x < 0.0,
            "tempo on the left stack"
        );
        assert!(
            panel_centre(Readout::Db).x > 0.0,
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
                world.query::<&MonitorDot>().iter(world).count(),
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
        // Measuring: both monitors, every dot of every cell.
        app.insert_resource(Ears(Some(Listener::stub(true, -38.4, Some(120.0)))));
        app.world_mut()
            .run_system_once(tend_monitors)
            .expect("the system runs");
        assert_eq!(count(app.world_mut()), (2, 2 * CELLS * COLS * ROWS));
        // The second tend lights the digits: 62 on the right, 120 on
        // the left, as visibility.
        app.world_mut()
            .run_system_once(tend_monitors)
            .expect("the system runs");
        let lit = |world: &mut World, readout: Readout| -> Vec<(usize, usize, usize)> {
            let mut dots: Vec<(usize, usize, usize)> = world
                .query::<(&MonitorDot, &Visibility)>()
                .iter(world)
                .filter(|(dot, visibility)| {
                    dot.readout == readout && **visibility == Visibility::Inherited
                })
                .map(|(dot, _)| (dot.cell, dot.col, dot.row))
                .collect();
            dots.sort_unstable();
            dots
        };
        let expect = |cells: [Cell; CELLS]| -> Vec<(usize, usize, usize)> {
            let mut out = Vec::new();
            for (cell, value) in cells.iter().enumerate() {
                for col in 0..COLS {
                    for row in 0..ROWS {
                        if dot_lit(*value, col, row) {
                            out.push((cell, col, row));
                        }
                    }
                }
            }
            out
        };
        assert_eq!(lit(app.world_mut(), Readout::Db), expect(db_cells(-38.4)));
        assert!(!lit(app.world_mut(), Readout::Db).is_empty());
        assert_eq!(
            lit(app.world_mut(), Readout::Bpm),
            expect(bpm_cells(Some(120.0)))
        );
        // The stream dies: the monitors are gone, dots and all.
        app.insert_resource(Ears(Some(Listener::stub(false, -38.4, Some(120.0)))));
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
