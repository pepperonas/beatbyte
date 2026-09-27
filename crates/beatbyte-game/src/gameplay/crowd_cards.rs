//! The crowd behind the crowd: a dense mass of flat silhouettes.
//!
//! The fifty-six dancers of [`super::crowd`] are full figures, and
//! they are all the room had — behind them, between their rows and the
//! PA, lay empty deck, so the venue read as a rehearsal with a few
//! friends in it. A few hundred more full figures would cost a few
//! thousand more joint entities; this is the cheap way a room fills
//! up: cards. Each card is one quad showing a person from behind,
//! drawn once into a shared atlas (eight bodies × four poses), turned
//! toward the camera once at spawn — the camera never moves — and
//! afterwards only bobbed on the beat and, under Hype, swapped to an
//! arms-up cell. One material for all of them, eight × four shared
//! meshes, no per-frame material writes, no light, no shadow.
//!
//! A few raised hands hold a phone: a tiny unlit screen that shows
//! only under Hype, when the arms are up. Never more than
//! [`MAX_PHONES`] — a sea of phones is a different kind of concert.
//!
//! Like the dancers, every card stands off the neck, off the riser
//! and clear of the PA, and under STAGE MOTION off not a single
//! transform or mesh is written.

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use core::f32::consts::TAU;

use super::crowd::{CrowdMood, crowd_moves, song_seed};
use super::figure::shirt_palette;
use super::fx::hash01;
use super::stage3d::{self, CAMERA_POS, STAGE_DECK_TOP, STAGE_LAYER, Stage3d};
use super::{GameplayScreen, PlayerSession};
use crate::audio_sys::GameClock;
use crate::config::Settings;
use crate::states::AppState;

/// Card rows per side, as distance from the neck's centre line. The
/// dancers' back row stands at 5.25 ± 0.07.
pub const CARD_ROWS_X: [f32; 3] = [5.95, 6.85, 7.75];
/// The front of the card mass, along the neck (the PA stacks stand
/// at z −7; this keeps the cards behind them).
pub const CARD_NEAR_Z: f32 = -12.4;
/// The back of the card mass (the band riser begins behind it).
pub const CARD_FAR_Z: f32 = -27.6;
/// Spacing along a row, metres — shoulder to shoulder.
pub const CARD_PITCH: f32 = 0.62;

/// The card quad: wide enough for spread arms, tall enough for hands
/// above the tallest head.
pub const CARD_W: f32 = 1.15;
/// See [`CARD_W`].
pub const CARD_H: f32 = 2.3;

/// Bodies (atlas columns) and poses (atlas rows).
pub const BODIES: usize = 8;
/// Arms down, right up, both up, left up.
pub const POSES: usize = 4;
/// Atlas cell size in texels (wide, tall): the cell's aspect is the
/// card's, so a head is round.
const CELL: (usize, usize) = (128, 256);
/// The atlas is square for the mip chain: 8 × 128 across, 4 × 256 down.
const ATLAS: usize = 1024;

/// The most phones held up at once.
pub const MAX_PHONES: usize = 12;

/// How far a card bobs on the beat at full energy, metres.
const BOB: f32 = 0.06;

/// One card: who, where, and how it moves.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct CrowdCard {
    /// Atlas column.
    pub body: u8,
    /// The pose it takes under Hype (1..[`POSES`]).
    pub hype_pose: u8,
    /// Its bob's offset from the beat, radians.
    pub phase: f32,
    /// Its feet, before the bob.
    pub base: Vec3,
    /// Whether it currently shows the Hype pose.
    pub up: bool,
}

/// A phone held in a raised right hand.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct CardPhone;

/// The shared card assets: one material, a mesh per atlas cell.
#[derive(Resource, Clone)]
pub struct CardAssets {
    material: Handle<StandardMaterial>,
    phone_material: Handle<StandardMaterial>,
    phone: Handle<Mesh>,
    cells: Vec<Handle<Mesh>>,
}

impl CardAssets {
    fn cell(&self, body: u8, pose: u8) -> Handle<Mesh> {
        self.cells[usize::from(pose) * BODIES + usize::from(body)].clone()
    }
}

// ---- where the cards stand -------------------------------------------------

/// Where one card stands, which body it shows, which pose it lifts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardSlot {
    /// Feet position.
    pub position: Vec3,
    /// Atlas column.
    pub body: u8,
    /// The Hype pose (1..[`POSES`]).
    pub hype_pose: u8,
    /// Bob phase, radians.
    pub phase: f32,
}

/// Every card of the song with `seed`, both sides. Pure — tested:
/// every card on the deck, outside the dancers and the barrier, clear
/// of the PA, and the same on every run.
#[must_use]
pub fn card_slots(seed: usize) -> Vec<CardSlot> {
    let per_row = ((CARD_NEAR_Z - CARD_FAR_Z) / CARD_PITCH) as usize;
    let mut slots = Vec::with_capacity(2 * CARD_ROWS_X.len() * per_row);
    for (side_index, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        for (row, &row_x) in CARD_ROWS_X.iter().enumerate() {
            let stagger = if row % 2 == 1 { 0.5 } else { 0.0 };
            for seat in 0..per_row {
                let id = seed
                    .wrapping_mul(6_151)
                    .wrapping_add(side_index * 4_099 + row * 1_021 + seat * 13);
                let h = |salt: usize| hash01(id.wrapping_add(salt));
                let z =
                    CARD_NEAR_Z - (seat as f32 + 0.5 + stagger) * CARD_PITCH + 0.18 * (h(1) - 0.5);
                if z < CARD_FAR_Z {
                    continue;
                }
                let x = side * (row_x + 0.28 * (h(2) - 0.5));
                slots.push(CardSlot {
                    position: Vec3::new(x, STAGE_DECK_TOP, z),
                    body: ((h(3) * BODIES as f32) as u8).min(BODIES as u8 - 1),
                    hype_pose: 1 + ((h(4) * 3.0) as u8).min(2),
                    // Loose clusters, like the dancers: a shared
                    // groove, not a field of random noise.
                    phase: row as f32 * 0.25 + (seat % 4) as f32 * 0.1 + 0.6 * (h(5) - 0.5),
                });
            }
        }
    }
    slots
}

/// Which cards hold a phone: the first [`MAX_PHONES`] whose Hype pose
/// raises the RIGHT hand, taken evenly from both sides. Pure — tested.
#[must_use]
pub fn phone_holders(slots: &[CardSlot]) -> Vec<usize> {
    let raises_right = |s: &CardSlot| s.hype_pose == 1 || s.hype_pose == 2;
    let (left, right): (Vec<usize>, Vec<usize>) = slots
        .iter()
        .enumerate()
        .filter(|(_, s)| raises_right(s))
        .map(|(i, _)| i)
        .partition(|&i| slots[i].position.x < 0.0);
    // Every third candidate, so they do not bunch at the front.
    left.into_iter()
        .step_by(3)
        .take(MAX_PHONES / 2)
        .chain(right.into_iter().step_by(3).take(MAX_PHONES / 2))
        .collect()
}

/// The yaw that turns a card's face (+Z) toward the camera. Pure.
#[must_use]
pub fn face_camera(position: Vec3) -> f32 {
    (CAMERA_POS.x - position.x).atan2(CAMERA_POS.z - position.z)
}

/// A card's lift on the beat at energy `e` (0..1): a quick rise on
/// the beat and back down. Zero at zero energy. Pure — tested.
#[must_use]
pub fn card_bob(beats: f32, phase: f32, e: f32) -> f32 {
    let pulse = (beats * TAU + phase).cos().max(0.0);
    BOB * e.clamp(0.0, 1.0) * pulse * pulse
}

// ---- the silhouettes -------------------------------------------------------

/// One body in the atlas: proportions, hair, clothes.
struct Body {
    height: f32,
    shoulder: f32,
    hair: HairCut,
    shirt: usize,
    hair_tone: [f32; 3],
    trousers: [f32; 3],
}

#[derive(Clone, Copy)]
enum HairCut {
    Short,
    Long,
    Cap,
    Bun,
}

const DARK_HAIR: [f32; 3] = [0.05, 0.04, 0.04];
const BROWN_HAIR: [f32; 3] = [0.20, 0.12, 0.07];
const FAIR_HAIR: [f32; 3] = [0.40, 0.30, 0.17];
const BLACK_JEANS: [f32; 3] = [0.05, 0.05, 0.06];
const BLUE_JEANS: [f32; 3] = [0.10, 0.13, 0.20];
const SKIN: [f32; 3] = [0.60, 0.44, 0.34];

/// The eight bodies of the atlas: mostly dark shirts, as the dancers.
fn bodies() -> [Body; BODIES] {
    let b = |height, shoulder, hair, shirt, hair_tone, trousers| Body {
        height,
        shoulder,
        hair,
        shirt,
        hair_tone,
        trousers,
    };
    [
        b(1.80, 0.23, HairCut::Short, 0, DARK_HAIR, BLUE_JEANS),
        b(1.68, 0.19, HairCut::Long, 2, BROWN_HAIR, BLACK_JEANS),
        b(1.86, 0.25, HairCut::Cap, 1, DARK_HAIR, BLACK_JEANS),
        b(1.62, 0.18, HairCut::Bun, 6, FAIR_HAIR, BLUE_JEANS),
        b(1.76, 0.22, HairCut::Short, 5, BROWN_HAIR, BLACK_JEANS),
        b(1.72, 0.20, HairCut::Long, 0, DARK_HAIR, BLUE_JEANS),
        b(1.83, 0.24, HairCut::Short, 3, FAIR_HAIR, BLACK_JEANS),
        b(1.66, 0.20, HairCut::Short, 7, DARK_HAIR, BLUE_JEANS),
    ]
}

/// Signed distance to the segment `a`–`b` thickened by `r`.
fn capsule(p: Vec2, a: Vec2, b: Vec2, r: f32) -> f32 {
    let (pa, ba) = (p - a, b - a);
    let t = (pa.dot(ba) / ba.length_squared()).clamp(0.0, 1.0);
    (pa - ba * t).length() - r
}

/// Signed distance to a box of half-sizes `h` with corner radius `r`,
/// centred on `c`.
fn rounded_box(p: Vec2, c: Vec2, h: Vec2, r: f32) -> f32 {
    let q = (p - c).abs() - h + Vec2::splat(r);
    q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - r
}

/// Where a pose puts one hand, relative to the feet (x right as the
/// viewer sees the card). `right` picks the hand. Pure.
fn hand(body: &Body, pose: usize, right: bool) -> Vec2 {
    let side = if right { 1.0 } else { -1.0 };
    let up = match pose {
        1 => right,
        2 => true,
        3 => !right,
        _ => false,
    };
    if up {
        // A raised arm leans out a little; both up makes a V.
        let out = if pose == 2 { 0.17 } else { 0.06 };
        Vec2::new(side * (body.shoulder + out), body.height + 0.27)
    } else {
        Vec2::new(side * (body.shoulder + 0.03), body.height * 0.47)
    }
}

/// The colour and coverage of one point of a silhouette, `p` in
/// metres from the card's foot point (x right, y up): `(rgb, alpha)`.
/// Parts are painted back to front, each over the last.
fn silhouette(body: &Body, pose: usize, p: Vec2, texel: f32) -> ([f32; 3], f32) {
    let h = body.height;
    let cover = |d: f32| (0.5 - d / texel).clamp(0.0, 1.0);
    let shirt = {
        let c = shirt_palette()[body.shirt].to_srgba();
        [c.red, c.green, c.blue]
    };
    let mut rgb = [0.0f32; 3];
    let mut alpha = 0.0f32;
    let mut paint = |colour: [f32; 3], coverage: f32| {
        for k in 0..3 {
            rgb[k] = rgb[k] * (1.0 - coverage) + colour[k] * coverage;
        }
        alpha = alpha.max(coverage);
    };
    // Legs.
    for side in [-1.0f32, 1.0] {
        let d = capsule(
            p,
            Vec2::new(side * 0.085, h * 0.50),
            // Feet ending above the card's bottom edge: a shape that
            // touches the cell border bleeds into the mip chain.
            Vec2::new(side * 0.10, 0.09),
            0.068,
        );
        paint(body.trousers, cover(d));
    }
    // Torso, a little narrower at the waist.
    let waist = rounded_box(
        p,
        Vec2::new(0.0, h * 0.56),
        Vec2::new(body.shoulder * 0.82, h * 0.09),
        0.04,
    );
    let chest = rounded_box(
        p,
        Vec2::new(0.0, h * 0.71),
        Vec2::new(body.shoulder, h * 0.10),
        0.06,
    );
    paint(shirt, cover(waist.min(chest)));
    // Arms, sleeves to the elbow, skin below.
    for right in [false, true] {
        let side = if right { 1.0 } else { -1.0 };
        let shoulder = Vec2::new(side * (body.shoulder - 0.035), h * 0.79);
        let hand_at = hand(body, pose, right);
        let elbow = shoulder.lerp(hand_at, 0.45);
        paint(shirt, cover(capsule(p, shoulder, elbow, 0.052)));
        paint(SKIN, cover(capsule(p, elbow, hand_at, 0.042)));
    }
    // Neck and head (seen from behind: the back of the head is hair).
    let head = Vec2::new(0.0, h * 0.925);
    paint(
        SKIN,
        cover(capsule(p, Vec2::new(0.0, h * 0.82), head, 0.05)),
    );
    let skull = (p - head).length() - 0.105;
    let hair = match body.hair {
        // A little LARGER than the skull: the first cut subtracted the
        // other way and left a ring of skin around every short haircut.
        HairCut::Short => skull - 0.006,
        HairCut::Long => skull.min(rounded_box(
            p,
            Vec2::new(0.0, h * 0.86),
            Vec2::new(0.10, 0.09),
            0.05,
        )),
        HairCut::Cap => {
            skull.min(rounded_box(
                p,
                head + Vec2::new(0.0, 0.03),
                Vec2::new(0.118, 0.04),
                0.02,
            )) - 0.002
        }
        HairCut::Bun => skull.min((p - head - Vec2::new(0.0, 0.12)).length() - 0.045),
    };
    paint(SKIN, cover(skull));
    let hair_colour = if matches!(body.hair, HairCut::Cap) {
        [0.06, 0.06, 0.07]
    } else {
        body.hair_tone
    };
    paint(hair_colour, cover(hair));
    (rgb, alpha)
}

/// The atlas texel at column `x`, row `y`: straight RGBA, 0..1.
///
/// A rim of light runs along every upward-facing edge — head,
/// shoulders, raised arms — because the stage in front of these people
/// is the brightest thing in the room and outlines them from behind.
/// Pure — tested.
#[must_use]
pub fn atlas_texel(x: usize, y: usize) -> [f32; 4] {
    let (col, row) = (x / CELL.0, y / CELL.1);
    if col >= BODIES || row >= POSES {
        return [0.0; 4];
    }
    let bodies = bodies();
    let body = &bodies[col];
    let texel = CARD_H / CELL.1 as f32;
    // Metres from the card's foot point; image rows run downward.
    let local = |dx: f32, dy: f32| {
        Vec2::new(
            ((x % CELL.0) as f32 + 0.5 + dx) / CELL.0 as f32 * CARD_W - CARD_W / 2.0,
            CARD_H - ((y % CELL.1) as f32 + 0.5 + dy) / CELL.1 as f32 * CARD_H,
        )
    };
    let (rgb, alpha) = silhouette(body, row, local(0.0, 0.0), texel);
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    // Three texels up: is that still the person? If not, this is a
    // top edge and it catches the rim.
    let (_, above) = silhouette(body, row, local(0.0, -3.0), texel);
    let rim = 0.30 * (1.0 - above);
    let lit = rgb.map(|c| (c + rim * (0.9 - c)).clamp(0.0, 1.0));
    [lit[0], lit[1], lit[2], alpha]
}

fn atlas_image() -> Image {
    let mut data = Vec::with_capacity(ATLAS * ATLAS * 4);
    for y in 0..ATLAS {
        for x in 0..ATLAS {
            let t = atlas_texel(x, y);
            // sRGB bytes: the palette is written in sRGB.
            data.extend(t.map(|c| (c * 255.0).round() as u8));
        }
    }
    let mut image = Image::new(
        bevy::render::render_resource::Extent3d {
            width: ATLAS as u32,
            height: ATLAS as u32,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        data,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = bevy::image::ImageSampler::linear();
    crate::surfaces::with_mips(image, false)
}

/// The quad for one atlas cell: feet at the origin, facing +Z.
fn cell_mesh(body: usize, pose: usize) -> Mesh {
    let (u0, u1) = (
        body as f32 / BODIES as f32,
        (body + 1) as f32 / BODIES as f32,
    );
    let (v0, v1) = (pose as f32 / POSES as f32, (pose + 1) as f32 / POSES as f32);
    let hw = CARD_W / 2.0;
    Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-hw, 0.0, 0.0],
            [hw, 0.0, 0.0],
            [hw, CARD_H, 0.0],
            [-hw, CARD_H, 0.0],
        ],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
    )
    .with_inserted_indices(bevy::mesh::Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

// ---- systems ---------------------------------------------------------------

/// Spawn the card mass. The atlas is built once per run and kept.
pub fn spawn_cards(
    mut commands: Commands,
    settings: Res<Settings>,
    song: Res<crate::boot::LoadedSong>,
    existing: Option<Res<CardAssets>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !stage3d::active(&settings) {
        return;
    }
    let assets = if let Some(existing) = existing {
        existing.clone()
    } else {
        let assets = CardAssets {
            material: materials.add(StandardMaterial {
                base_color_texture: Some(images.add(atlas_image())),
                // Behind the dancers, and a flat quad turned square to
                // the front light catches more of it than a round body
                // does: at full albedo the first card rows stood out
                // brighter than the people in front of them.
                base_color: Color::srgb(0.7, 0.7, 0.7),
                alpha_mode: AlphaMode::Mask(0.5),
                perceptual_roughness: 1.0,
                ..default()
            }),
            phone_material: materials.add(StandardMaterial {
                base_color: Color::srgb(0.85, 0.92, 1.0),
                emissive: LinearRgba::rgb(2.5, 2.8, 3.2),
                unlit: true,
                ..default()
            }),
            phone: meshes.add(Rectangle::new(0.07, 0.12)),
            cells: (0..POSES)
                .flat_map(|pose| (0..BODIES).map(move |body| (body, pose)))
                .map(|(body, pose)| meshes.add(cell_mesh(body, pose)))
                .collect(),
        };
        commands.insert_resource(assets.clone());
        assets
    };
    let layer = RenderLayers::layer(STAGE_LAYER);
    let slots = card_slots(song_seed(&song.chart.song.title));
    let phones = phone_holders(&slots);
    let bodies = bodies();
    for (index, slot) in slots.iter().enumerate() {
        let card = commands
            .spawn((
                GameplayScreen,
                Stage3d,
                CrowdCard {
                    body: slot.body,
                    hype_pose: slot.hype_pose,
                    phase: slot.phase,
                    base: slot.position,
                    up: false,
                },
                Mesh3d(assets.cell(slot.body, 0)),
                MeshMaterial3d(assets.material.clone()),
                NotShadowCaster,
                Transform::from_translation(slot.position)
                    .with_rotation(Quat::from_rotation_y(face_camera(slot.position))),
                layer.clone(),
            ))
            .id();
        if phones.contains(&index) {
            let at = hand(
                &bodies[usize::from(slot.body)],
                usize::from(slot.hype_pose),
                true,
            );
            commands.entity(card).with_child((
                CardPhone,
                Mesh3d(assets.phone.clone()),
                MeshMaterial3d(assets.phone_material.clone()),
                NotShadowCaster,
                Visibility::Hidden,
                Transform::from_xyz(at.x, at.y + 0.08, 0.02),
                layer.clone(),
            ));
        }
    }
}

/// Bob the cards on the beat and raise their arms under Hype. Reads
/// the dancers' mood, so the whole room moves as one crowd.
#[allow(clippy::type_complexity, clippy::too_many_arguments)] // Bevy system: params are DI
pub fn animate_cards(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    players: Query<&PlayerSession>,
    mood: Option<Res<CrowdMood>>,
    assets: Option<Res<CardAssets>>,
    mut cards: Query<(
        &mut CrowdCard,
        &mut Transform,
        &mut Mesh3d,
        Option<&Children>,
    )>,
    mut phones: Query<&mut Visibility, With<CardPhone>>,
) {
    if !crowd_moves(&settings) {
        return;
    }
    let (Some(now), Some(player), Some(mood), Some(assets)) = (
        game_clock.song_time(&time),
        players.iter().next(),
        mood,
        assets,
    ) else {
        return;
    };
    let beats = player.session.track().tempo.beats_at(now) as f32;
    let up = mood.arms_up > 0.5;
    let (mut swapped, mut shown) = (0usize, 0usize);
    for (mut card, mut transform, mut mesh, children) in &mut cards {
        let y = card.base.y + card_bob(beats, card.phase, mood.energy);
        if (transform.translation.y - y).abs() > 1e-5 {
            transform.translation.y = y;
        }
        if card.up != up {
            card.up = up;
            let pose = if up { card.hype_pose } else { 0 };
            *mesh = Mesh3d(assets.cell(card.body, pose));
            swapped += 1;
            for child in children.into_iter().flatten() {
                if let Ok(mut visibility) = phones.get_mut(*child) {
                    shown += usize::from(up);
                    *visibility = if up {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                }
            }
        }
    }
    if swapped > 0 {
        // One line per change of state, never per frame: the evidence
        // that Hype reached the back of the room when no screenshot
        // can be taken.
        info!(
            "crowd cards: arms {} ({swapped} cards, {shown} phones shown)",
            if up { "raised" } else { "lowered" }
        );
    }
}

/// Add the card crowd to the app: spawned with the stage, moved with
/// the dancers (after them — it reads their mood).
pub fn register(app: &mut App) {
    app.add_systems(
        OnEnter(AppState::Gameplay),
        spawn_cards.after(super::setup_gameplay),
    )
    .add_systems(
        Update,
        animate_cards.after(super::crowd::animate_crowd).run_if(
            in_state(crate::states::GamePhase::Playing)
                .or_else(in_state(crate::states::GamePhase::Outro)),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::super::stage3d::DECK_WIDTH;
    use super::*;

    #[test]
    fn every_card_stands_on_the_deck_behind_the_dancers_and_clear_of_the_pa() {
        for seed in [0usize, 7, 99_999] {
            let slots = card_slots(seed);
            assert!(slots.len() > 100, "a crowd, not a handful: {}", slots.len());
            for s in &slots {
                let p = s.position;
                assert!(p.x.abs() > 5.25 + 0.07 + 0.4, "behind the dancers: {p}");
                assert!(
                    p.x.abs() + CARD_W / 2.0 < DECK_WIDTH / 2.0,
                    "on the deck: {p}"
                );
                assert!(p.z <= CARD_NEAR_Z + 0.1 && p.z >= CARD_FAR_Z, "{p}");
                // The PA stacks at x ±7, z −7: the mass starts well behind.
                assert!(p.z < -7.0 - 4.0, "clear of the PA: {p}");
                assert!((p.y - STAGE_DECK_TOP).abs() < 1e-6, "on the floor");
                assert!(usize::from(s.body) < BODIES);
                assert!((1..POSES as u8).contains(&s.hype_pose));
            }
            assert_eq!(slots, card_slots(seed), "the same crowd every run");
        }
    }

    #[test]
    fn the_hype_poses_are_mixed() {
        let slots = card_slots(3);
        for pose in 1..POSES as u8 {
            let n = slots.iter().filter(|s| s.hype_pose == pose).count();
            assert!(n > slots.len() / 8, "pose {pose} used {n} times");
        }
    }

    #[test]
    fn phones_are_few_and_only_in_a_raised_right_hand() {
        for seed in [1usize, 42, 1234] {
            let slots = card_slots(seed);
            let phones = phone_holders(&slots);
            assert!(
                !phones.is_empty() && phones.len() <= MAX_PHONES,
                "{}",
                phones.len()
            );
            for &i in &phones {
                assert!(matches!(slots[i].hype_pose, 1 | 2), "raised right hand");
            }
            let left = phones
                .iter()
                .filter(|&&i| slots[i].position.x < 0.0)
                .count();
            assert!(left > 0 && left < phones.len(), "both sides");
        }
    }

    #[test]
    fn a_card_faces_the_camera() {
        for s in card_slots(5) {
            let face = Quat::from_rotation_y(face_camera(s.position)) * Vec3::Z;
            let to_camera = (CAMERA_POS - s.position).with_y(0.0).normalize();
            assert!(face.dot(to_camera) > 0.999, "{}", s.position);
        }
    }

    #[test]
    fn a_card_is_still_at_zero_energy_and_bobs_up_only() {
        for i in 0..50 {
            let beats = i as f32 * 0.137;
            assert_eq!(card_bob(beats, 0.4, 0.0), 0.0);
            let b = card_bob(beats, 0.4, 1.0);
            assert!((0.0..=BOB).contains(&b));
        }
        assert!(
            (card_bob(0.0, 0.0, 1.0) - BOB).abs() < 1e-6,
            "full lift on the beat"
        );
    }

    fn cell_rows(body: usize, pose: usize) -> Vec<Vec<f32>> {
        (0..CELL.1)
            .map(|y| {
                (0..CELL.0)
                    .map(|x| atlas_texel(body * CELL.0 + x, pose * CELL.1 + y)[3])
                    .collect()
            })
            .collect()
    }

    #[test]
    fn every_cell_holds_one_person_inside_its_border() {
        for body in 0..BODIES {
            for pose in 0..POSES {
                let rows = cell_rows(body, pose);
                let filled: f32 = rows.iter().flatten().sum();
                let share = filled / (CELL.0 * CELL.1) as f32;
                assert!((0.06..0.45).contains(&share), "{body}/{pose}: {share}");
                // Nothing touches the cell's edge, or it would bleed
                // into its neighbour in the mip chain.
                let edge = rows[0].iter().chain(rows[CELL.1 - 1].iter()).sum::<f32>()
                    + rows.iter().map(|r| r[0] + r[CELL.0 - 1]).sum::<f32>();
                assert!(edge < 1e-6, "{body}/{pose} touches its border");
            }
        }
    }

    #[test]
    fn raised_arms_reach_above_the_head() {
        let top = |body, pose| {
            cell_rows(body, pose)
                .iter()
                .position(|r| r.iter().any(|&a| a > 0.5))
                .expect("someone is there")
        };
        for body in 0..BODIES {
            let resting = top(body, 0);
            for pose in 1..POSES {
                assert!(
                    top(body, pose) + 15 < resting,
                    "{body}/{pose}: arms {} vs head {resting}",
                    top(body, pose)
                );
            }
        }
    }

    #[test]
    fn a_top_edge_catches_the_rim_light() {
        // Straight down the middle of a resting body: the first texel
        // of the head is brighter than the hair a little lower.
        let x = CELL.0 / 2;
        let first = (0..CELL.1)
            .find(|&y| atlas_texel(x, y)[3] > 0.99)
            .expect("a head");
        let luma = |t: [f32; 4]| t[0] + t[1] + t[2];
        assert!(luma(atlas_texel(x, first)) > luma(atlas_texel(x, first + 8)) + 0.2);
    }
}
