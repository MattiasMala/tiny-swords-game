// OWNER: C2 (sprites) — world billboards: spec 01 §3.2 (buildings: construction rising, fires, palisade auto-tiling),
// §8.3 (trees, gold stages + shine, stones, big deposits, sheep, pickups, projectiles, props), 07 §8.3 (draw rules),
// §3.2 (which entities cast shadows: `FLAT_PROPS`, stumps), §4 (light sources), 09 §2-§7 (anchors, sheets).
//
// * `Sheets`: every sheet the renderer needs, resolved once from the atlas (no string lookups per frame).
// * `Scenery`: static world index rebuilt on every new match (`Mirror::generation`): trees / gold / stone / big
//   deposits by `NodeId`, decor, capture points, bucketed in 16×16-cell chunks for culling.
// * `draw_static` rebuilds the static layer only when its key changes (visible chunks, 8/10 Hz animation ticks, fog,
//   node amounts, point owners); hits and light sources of scenery are refreshed every frame.
// * `draw_dynamic`: units (interpolated, local animT, hurt flash), buildings (+ ghosts of remembered enemy buildings,
//   construction, fire, palisades), sheep, projectiles, pickups, ducks, event props.

use crate::assets::{SheetId, SpriteAtlas};
use crate::link::GameEventMsg;
use crate::mirror::Mirror;
use crate::render::anim::{AnimKey, UnitAnims, UnitSheetCache, WorldClock, anim_frame, hash01};
use crate::render::consts::PX;
use crate::render::light::{LightCandidates, LightSrc};
use crate::render::sprites::{
    Basis, DrawStage, Flavor, Ghost, HitTarget, Layer, Painter, SpriteBatches, SpriteHit,
    SpriteHits, SpriteOpts, drawable, foot_world, ground_at, prio,
};
use crate::render::terrain::FogTexture;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use std::hash::{Hash, Hasher};
use ts_sim::TILE;
use ts_sim::defs::{
    BuildingKind, FxKind, KingdomColor, NodeKind, PointKind, PropKind, Res as ResKind, TREE_DEF,
    TREE_FOOT_DY, UnitKind,
};
use ts_sim::events::GameEvent;
use ts_sim::map::{DecorKind, GeneratedWorld, Map};
use ts_sim::slab::{BuildingId, NodeId, PointId, SheepId};
use ts_sim::snapshot_types::{BuildingView, UF_FLIP, UF_HURT, UF_INSIDE};
use ts_sim::world::SheepState;

/// Chunk size of the scenery index (cells).
const CHUNK: i32 = 16;

/// Props drawn without a shadow (`FLAT_PROPS`, spec 07 §3.2).
pub const FLAT_PROPS: [PropKind; 18] = [
    PropKind::Bones,
    PropKind::Flowers,
    PropKind::Grass,
    PropKind::Lilypads,
    PropKind::Mushrooms,
    PropKind::Reeds,
    PropKind::Boat,
    PropKind::FlowersRed,
    PropKind::FlowersBlue,
    PropKind::FlowersWhite,
    PropKind::Clover,
    PropKind::Pebbles,
    PropKind::Puddle,
    PropKind::Sticks,
    PropKind::Dock,
    PropKind::Shipwreck,
    PropKind::TallGrass,
    PropKind::Ferns,
];

/// Hurt flash colour (sim `hurtT > 0`).
const HURT_TINT: [f32; 4] = [2.2, 1.5, 1.5, 1.0];

// ---------------------------------------------------------------------------------------------------------------
// Sheets
// ---------------------------------------------------------------------------------------------------------------

/// Every sheet the world renderer uses (resolved once the atlas is ready). `u32::MAX` = missing.
#[derive(Resource)]
pub struct Sheets {
    pub ready: bool,
    pub trees: [SheetId; 4],
    pub stumps: [SheetId; 4],
    pub gold_stone: [SheetId; 6],
    pub gold_hl: [SheetId; 6],
    pub gold_res: SheetId,
    pub gold_res_hl: SheetId,
    pub wood_res: SheetId,
    pub meat_res: SheetId,
    pub stone_res: SheetId,
    pub stones: [SheetId; 3],
    pub gold_big: SheetId,
    pub stone_big: SheetId,
    pub sheep: [SheetId; 3],
    pub bush: [SheetId; 4],
    pub rock: [SheetId; 4],
    pub water_rock: [SheetId; 4],
    pub duck: SheetId,
    pub cloud: [SheetId; 8],
    pub props: Vec<SheetId>,
    pub points: [SheetId; 5],
    /// `[color][kind][variant]`
    pub buildings: Vec<SheetId>,
    pub arrow: [SheetId; 5],
    pub fx: [SheetId; 9],
    pub heal: [SheetId; 5],
    pub smallbar: [SheetId; 3],
    pub bigbar: [SheetId; 3],
    pub bigbar_fill: SheetId,
    pub bracket: [SheetId; 4],
    pub pennant: [SheetId; 5],
    pub ribbon: [[SheetId; 3]; 5],
    pub white: SheetId,
    pub ring: SheetId,
    pub ring_big: SheetId,
    pub glow: SheetId,
}

pub const NONE: SheetId = u32::MAX;

impl Default for Sheets {
    fn default() -> Self {
        Sheets {
            ready: false,
            trees: [NONE; 4],
            stumps: [NONE; 4],
            gold_stone: [NONE; 6],
            gold_hl: [NONE; 6],
            gold_res: NONE,
            gold_res_hl: NONE,
            wood_res: NONE,
            meat_res: NONE,
            stone_res: NONE,
            stones: [NONE; 3],
            gold_big: NONE,
            stone_big: NONE,
            sheep: [NONE; 3],
            bush: [NONE; 4],
            rock: [NONE; 4],
            water_rock: [NONE; 4],
            duck: NONE,
            cloud: [NONE; 8],
            props: Vec::new(),
            points: [NONE; 5],
            buildings: Vec::new(),
            arrow: [NONE; 5],
            fx: [NONE; 9],
            heal: [NONE; 5],
            smallbar: [NONE; 3],
            bigbar: [NONE; 3],
            bigbar_fill: NONE,
            bracket: [NONE; 4],
            pennant: [NONE; 5],
            ribbon: [[NONE; 3]; 5],
            white: NONE,
            ring: NONE,
            ring_big: NONE,
            glow: NONE,
        }
    }
}

/// Asset path of a building sprite (`buildingImg(color, kind, variant)`, spec 01 §3).
pub fn building_path(color: KingdomColor, kind: BuildingKind, variant: u8) -> String {
    let d = kind.def();
    let c = color.key();
    if d.neutral {
        format!("Extra/Buildings/{}.png", d.file)
    } else if d.extra {
        format!("Extra/Buildings/{c}/{}.png", d.file)
    } else if kind == BuildingKind::House {
        format!("Buildings/{c} Buildings/House{}.png", variant % 3 + 1)
    } else {
        format!("Buildings/{c} Buildings/{}.png", d.file)
    }
}

impl Sheets {
    fn resolve(a: &SpriteAtlas) -> Sheets {
        let g = |p: &str| a.sheet_id(p).unwrap_or(NONE);
        let mut s = Sheets {
            ready: true,
            ..default()
        };
        for v in 0..4 {
            s.trees[v] = g(&format!("Terrain/Resources/Wood/Trees/Tree{}.png", v + 1));
            s.stumps[v] = g(&format!("Terrain/Resources/Wood/Trees/Stump {}.png", v + 1));
            s.bush[v] = g(&format!("Terrain/Decorations/Bushes/Bushe{}.png", v + 1));
            s.rock[v] = g(&format!("Terrain/Decorations/Rocks/Rock{}.png", v + 1));
            s.water_rock[v] = g(&format!(
                "Terrain/Decorations/Rocks in the Water/Water Rocks_0{}.png",
                v + 1
            ));
        }
        for i in 0..6 {
            s.gold_stone[i] = g(&format!(
                "Terrain/Resources/Gold/Gold Stones/Gold Stone {}.png",
                i + 1
            ));
            s.gold_hl[i] = g(&format!(
                "Terrain/Resources/Gold/Gold Stones/Gold Stone {}_Highlight.png",
                i + 1
            ));
        }
        s.gold_res = g("Terrain/Resources/Gold/Gold Resource/Gold_Resource.png");
        s.gold_res_hl = g("Terrain/Resources/Gold/Gold Resource/Gold_Resource_Highlight.png");
        s.wood_res = g("Terrain/Resources/Wood/Wood Resource/Wood Resource.png");
        s.meat_res = g("Terrain/Resources/Meat/Meat Resource/Meat Resource.png");
        s.stone_res = g("Extra/Resources/Stone_Resource.png");
        for i in 0..3 {
            s.stones[i] = g(&format!("Extra/Resources/Stone_{}.png", i + 1));
        }
        s.gold_big = g("Extra/Resources/Gold_Big.png");
        s.stone_big = g("Extra/Resources/Stone_Big.png");
        s.sheep = [
            g("Terrain/Resources/Meat/Sheep/Sheep_Idle.png"),
            g("Terrain/Resources/Meat/Sheep/Sheep_Move.png"),
            g("Terrain/Resources/Meat/Sheep/Sheep_Grass.png"),
        ];
        s.duck = g("Terrain/Decorations/Rubber Duck/Rubber duck.png");
        for i in 0..8 {
            s.cloud[i] = g(&format!("Terrain/Decorations/Clouds/Clouds_0{}.png", i + 1));
        }
        s.props = PropKind::ALL
            .iter()
            .map(|p| g(&format!("Extra/Props/{}.png", p.name())))
            .collect();
        for (i, p) in PointKind::ALL.iter().enumerate() {
            s.points[i] = g(&format!("Extra/Buildings/{}.png", p.def().sprite));
        }
        for &c in KingdomColor::ALL {
            for &k in BuildingKind::ALL {
                for v in 0..3u8 {
                    s.buildings.push(g(&building_path(c, k, v)));
                }
            }
            s.arrow[c.index()] = g(&format!("Units/{} Units/Archer/Arrow.png", c.key()));
            s.heal[c.index()] = g(&format!("Units/{} Units/Monk/Heal_Effect.png", c.key()));
            s.pennant[c.index()] = g(&format!("region:pennant:{}", c.index()));
            for i in 0..3 {
                s.ribbon[c.index()][i] = g(&format!("region:ribbon:{}:{i}", c.index()));
            }
        }
        for (i, f) in ts_sim::defs::FX_INFO.iter().enumerate() {
            s.fx[i] = g(f.0);
        }
        for i in 0..3 {
            s.smallbar[i] = g(&format!("region:smallbar:{i}"));
            s.bigbar[i] = g(&format!("region:bigbar:{i}"));
        }
        s.bigbar_fill = g("region:bigbar_fill");
        for i in 0..4 {
            s.bracket[i] = g(&format!("region:bracket:{i}"));
        }
        s.white = g("gen:white");
        s.ring = g("gen:ring");
        s.ring_big = g("gen:ring_big");
        s.glow = g("gen:glow");
        s
    }

    #[inline]
    pub fn building(&self, color: KingdomColor, kind: BuildingKind, variant: u8) -> SheetId {
        let i = (color.index() * BuildingKind::COUNT + kind.index()) * 3 + (variant % 3) as usize;
        self.buildings.get(i).copied().unwrap_or(NONE)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Scenery index
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct NodeInfo {
    pub kind: NodeKind,
    pub big: bool,
    /// foot point used for drawing (px)
    pub x: f32,
    pub y: f32,
    /// entity position of the JS (px; pick boxes)
    pub ex: f32,
    pub ey: f32,
    /// tree sprite variant
    pub variant: u8,
    /// animation phase (whole frames)
    pub phase: f32,
}

#[derive(Clone, Copy, Debug)]
enum Item {
    Node(u32),
    Decor(u32),
    Point(u32),
}

#[derive(Resource, Default)]
pub struct Scenery {
    generation: Option<u32>,
    pub nodes: Vec<NodeInfo>,
    /// capture points: (foot px, kind), index = PointId
    pub points: Vec<(Vec2, PointKind)>,
    items: Vec<(Item, f32, f32)>,
    chunks: Vec<Vec<u32>>,
    cw: i32,
    ch: i32,
    /// pond centres (px) for duck radii
    ponds: Vec<Vec2>,
    /// static light sources (campfire / shrine props): world pos, radius px, power, seed, flicker
    lights: Vec<(Vec2, f32, f32, f32, f32)>,
}

impl Scenery {
    fn build(&mut self, g: &GeneratedWorld, generation: u32) {
        self.generation = Some(generation);
        self.nodes.clear();
        self.points.clear();
        self.items.clear();
        self.lights.clear();
        let map = &g.map;
        let center = |c: i32| {
            let p = map.center(c);
            (p.x, p.y)
        };
        for (i, t) in g.trees.iter().enumerate() {
            let (x, y) = center(t.cell);
            self.nodes.push(NodeInfo {
                kind: NodeKind::Tree,
                big: false,
                x,
                y: y + TREE_FOOT_DY,
                ex: x,
                ey: y + TREE_FOOT_DY,
                variant: t.variant % 4,
                phase: (hash01(i as u32, 11) * 8.0).floor(),
            });
        }
        let big_foot = |c: i32| {
            let (cx, cy) = map.xy(c);
            (
                (cx + 1) as f32 * TILE,
                (cy + 2) as f32 * TILE - 6.0,
                (cx + 1) as f32 * TILE,
                (cy + 1) as f32 * TILE,
            )
        };
        for (kind, small) in [(NodeKind::Gold, &g.golds), (NodeKind::Stone, &g.stones)] {
            for d in small.iter() {
                let (x, y) = center(d.cell);
                let (fy, ey) = if kind == NodeKind::Gold {
                    (y + 18.0, y)
                } else {
                    (y + 6.0 + 20.0, y + 6.0)
                };
                self.nodes.push(NodeInfo {
                    kind,
                    big: false,
                    x,
                    y: fy,
                    ex: x,
                    ey,
                    variant: 0,
                    phase: 0.0,
                });
            }
            for (_, d) in g.bigs.iter().filter(|(k, _)| *k == kind) {
                let (x, y, ex, ey) = big_foot(d.cell);
                self.nodes.push(NodeInfo {
                    kind,
                    big: true,
                    x,
                    y,
                    ex,
                    ey,
                    variant: 0,
                    phase: 0.0,
                });
            }
        }
        for p in &g.points {
            let (cx, cy) = map.xy(p.cell);
            self.points.push((
                Vec2::new((cx + 1) as f32 * TILE, (cy + 2) as f32 * TILE - 4.0),
                p.kind,
            ));
        }
        self.ponds = g
            .ponds
            .iter()
            .map(|&(x, y)| Vec2::new(x * TILE + TILE * 0.5, y * TILE + TILE * 0.5))
            .collect();
        for (i, n) in self.nodes.iter().enumerate() {
            self.items.push((Item::Node(i as u32), n.x, n.y));
        }
        for (i, d) in map.decor.iter().enumerate() {
            match d.kind {
                DecorKind::Duck | DecorKind::Cloud => continue,
                DecorKind::Prop(PropKind::Campfire) => {
                    self.lights.push((Vec2::new(d.x, d.y), 290.0, 1.25, d.x, 0.35))
                }
                DecorKind::Prop(PropKind::Shrine) => {
                    self.lights.push((Vec2::new(d.x, d.y), 150.0, 0.7, d.x, 0.35))
                }
                _ => {}
            }
            self.items.push((Item::Decor(i as u32), d.x, d.y));
        }
        for (i, (p, _)) in self.points.iter().enumerate() {
            self.items.push((Item::Point(i as u32), p.x, p.y));
        }
        self.cw = (map.w + CHUNK - 1) / CHUNK;
        self.ch = (map.h + CHUNK - 1) / CHUNK;
        self.chunks = vec![Vec::new(); (self.cw * self.ch) as usize];
        for (i, &(_, x, y)) in self.items.iter().enumerate() {
            let cx = ((x / TILE) as i32 / CHUNK).clamp(0, self.cw - 1);
            let cy = ((y / TILE) as i32 / CHUNK).clamp(0, self.ch - 1);
            self.chunks[(cy * self.cw + cx) as usize].push(i as u32);
        }
    }

    /// Chunk range overlapping a px rect.
    fn chunk_range(&self, r: [f32; 4]) -> (i32, i32, i32, i32) {
        let f = |v: f32| (v / TILE / CHUNK as f32).floor() as i32;
        (
            f(r[0]).clamp(0, self.cw - 1),
            f(r[1]).clamp(0, self.ch - 1),
            f(r[2]).clamp(0, self.cw - 1),
            f(r[3]).clamp(0, self.ch - 1),
        )
    }

    pub fn is_pond_duck(&self, x: f32, y: f32) -> bool {
        self.ponds
            .iter()
            .any(|p| p.distance_squared(Vec2::new(x, y)) < (3.0 * TILE) * (3.0 * TILE))
    }
}

/// Gold-vein twinkle frame (cosmetic, client-side, spec 03b `twinkle`): every `3..7 s` a 0.6-s shine. Quantised to
/// the 10 Hz grid so the static layer only changes on its animation ticks.
pub fn gold_shine(i: u32, t: f64) -> Option<i64> {
    let period = 36 + (hash01(i, 77) * 40.0) as i64;
    let off = (hash01(i, 78) * period as f32) as i64;
    let ph = ((t * 10.0).floor() as i64 + off).rem_euclid(period);
    (ph < 6).then_some(ph)
}

/// Local sheep animation clocks.
#[derive(Resource, Default)]
pub struct SheepAnims {
    map: HashMap<SheepId, (SheepState, f32)>,
}

/// Props added by `GameEvent::Prop` (e.g. `chest_open`), per match.
#[derive(Resource, Default)]
pub struct EventProps {
    generation: u32,
    pub list: Vec<(PropKind, f32, f32)>,
}

pub struct SpritesWorldPlugin;

impl Plugin for SpritesWorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sheets>()
            .init_resource::<Scenery>()
            .init_resource::<SheepAnims>()
            .init_resource::<EventProps>()
            .add_systems(
                Update,
                (
                    (resolve_sheets, index_scenery, collect_props)
                        .in_set(crate::render::sprites::SpriteSystems::Prepare),
                    (draw_static, draw_units, draw_buildings, draw_misc)
                        .chain()
                        .in_set(DrawStage::Fill),
                )
                    .run_if(in_state(crate::AppState::InGame)),
            );
    }
}

fn resolve_sheets(
    atlas: Res<SpriteAtlas>,
    mut sheets: ResMut<Sheets>,
    mut cache: ResMut<UnitSheetCache>,
) {
    if atlas.ready && !sheets.ready {
        *sheets = Sheets::resolve(&atlas);
        cache.clear();
    }
}

fn index_scenery(mirror: Res<Mirror>, mut scenery: ResMut<Scenery>, mut batches: ResMut<SpriteBatches>) {
    let Some(g) = mirror.world.as_ref() else {
        return;
    };
    if scenery.generation != Some(mirror.generation) {
        scenery.build(g, mirror.generation);
        batches.static_key = None;
    }
}

fn collect_props(
    mirror: Res<Mirror>,
    mut events: MessageReader<GameEventMsg>,
    mut props: ResMut<EventProps>,
) {
    if props.generation != mirror.generation {
        props.generation = mirror.generation;
        props.list.clear();
    }
    for GameEventMsg(e) in events.read() {
        if let GameEvent::Prop { prop, x, y } = e {
            props.list.push((*prop, *x, *y));
        }
    }
}

/// Fog helpers over the optional `FogTexture`.
pub struct Fog<'a>(pub Option<&'a FogTexture>);

impl Fog<'_> {
    #[inline]
    pub fn knows(&self, x: f32, y: f32) -> bool {
        self.0.is_none_or(|f| f.knows_px(x, y))
    }
    #[inline]
    pub fn sees(&self, x: f32, y: f32) -> bool {
        self.0.is_none_or(|f| f.sees_px(x, y))
    }
    /// Smooth fog colour multiplier from the fog opacity (towards rgb(10,14,26)).
    #[inline]
    pub fn tint(&self, x: f32, y: f32) -> [f32; 4] {
        let s = match self.0 {
            Some(f) if f.on => f.shade_px(x, y).clamp(0.0, 1.0),
            _ => 0.0,
        };
        shade_tint(s)
    }
    /// Binary tint (static layer: only changes with the 5 Hz visibility).
    #[inline]
    pub fn tint_step(&self, x: f32, y: f32) -> [f32; 4] {
        if self.sees(x, y) {
            [1.0; 4]
        } else {
            shade_tint(crate::render::terrain::EXPLORED_SHADE)
        }
    }
    fn version(&self) -> u64 {
        let Some(f) = self.0 else { return 0 };
        let mut acc = f.on as u64;
        for w in f.visible.words.iter().chain(f.explored.words.iter()) {
            acc = acc.rotate_left(5) ^ w;
            acc = acc.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
        acc
    }
}

#[inline]
pub fn shade_tint(s: f32) -> [f32; 4] {
    [1.0 - s * 0.92, 1.0 - s * 0.89, 1.0 - s * 0.8, 1.0]
}

#[inline]
fn color_of(mirror: &Mirror, f: u8) -> KingdomColor {
    mirror
        .colors
        .get(f as usize)
        .copied()
        .unwrap_or(KingdomColor::Blue)
}

// ---------------------------------------------------------------------------------------------------------------
// Static layer
// ---------------------------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn draw_static(
    mirror: Res<Mirror>,
    atlas: Res<SpriteAtlas>,
    sheets: Res<Sheets>,
    basis: Res<Basis>,
    fog: Option<Res<FogTexture>>,
    clock: Res<WorldClock>,
    scenery: Res<Scenery>,
    mut batches: ResMut<SpriteBatches>,
    mut hits: ResMut<SpriteHits>,
    mut lights: ResMut<LightCandidates>,
) {
    let Some(map) = drawable(&mirror, &atlas, &basis) else {
        return;
    };
    if !sheets.ready || scenery.generation != Some(mirror.generation) || batches.pages == 0 {
        return;
    }
    let fog = Fog(fog.as_deref());
    let t = clock.t;
    let (x0, y0, x1, y1) = scenery.chunk_range(basis.cull);
    // ---- key ----
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (x0, y0, x1, y1).hash(&mut h);
    ((t * 8.0).floor() as i64).hash(&mut h);
    ((t * 10.0).floor() as i64).hash(&mut h);
    fog.version().hash(&mut h);
    for a in &mirror.view.node_amounts {
        a.to_bits().hash(&mut h);
    }
    for p in &mirror.view.points {
        p.owner.hash(&mut h);
    }
    mirror.generation.hash(&mut h);
    basis.right.x.to_bits().hash(&mut h);
    basis.up.y.to_bits().hash(&mut h);
    batches.lit.hash(&mut h);
    let key = h.finish();
    let rebuild = batches.static_key != Some(key);
    if rebuild {
        batches.static_key = Some(key);
        batches.clear_layer(Layer::Static);
    }
    let amounts = &mirror.view.node_amounts;
    let mut p = Painter {
        batches: &mut batches,
        basis: &basis,
        atlas: &atlas,
    };
    let decor = &map.decor;
    for cy in y0..=y1 {
        for cx in x0..=x1 {
            for &ii in &scenery.chunks[(cy * scenery.cw + cx) as usize] {
                let (item, x, y) = scenery.items[ii as usize];
                if !basis.in_view(x, y, 64.0) {
                    continue;
                }
                match item {
                    Item::Node(i) => {
                        let n = scenery.nodes[i as usize];
                        if !fog.knows(n.ex, n.ey) {
                            continue;
                        }
                        let amount = amounts.get(i as usize).copied().unwrap_or(0.0);
                        let foot = foot_world(map, n.x, n.y);
                        if rebuild {
                            draw_node(&mut p, &sheets, i, &n, amount, foot, t, fog.tint_step(n.ex, n.ey));
                        }
                        // pick boxes (control.js pick)
                        let alive = amount > 0.0;
                        let hit = match n.kind {
                            NodeKind::Tree if alive => Some((30.0, n.ey - 100.0, n.ey + 8.0, prio::TREE)),
                            NodeKind::Gold | NodeKind::Stone => {
                                let r = if n.big { 68.0 } else { 36.0 };
                                Some((r, n.ey - r, n.ey + r, prio::DEPOSIT))
                            }
                            _ => None,
                        };
                        if let Some((hw, top, bot, pr)) = hit {
                            push_hit(
                                &mut hits,
                                &basis,
                                foot,
                                n.y,
                                HitTarget::Node(NodeId(i)),
                                hw,
                                n.y - bot,
                                n.y - top,
                                pr,
                                Vec2::new(n.ex, n.ey),
                            );
                        }
                        if n.kind == NodeKind::Gold && amount > 0.0 {
                            lights.push(LightSrc {
                                pos: foot + Vec3::Y * 0.2,
                                r: 120.0,
                                power: 0.4,
                                seed: i as f32,
                                flick: 0.2,
                            });
                        }
                    }
                    Item::Decor(i) => {
                        if !rebuild {
                            continue;
                        }
                        let d = decor[i as usize];
                        if !fog.knows(d.x, d.y) {
                            continue;
                        }
                        draw_decor(&mut p, &sheets, map, &d, t, fog.tint_step(d.x, d.y));
                    }
                    Item::Point(i) => {
                        let (pos, kind) = scenery.points[i as usize];
                        if !fog.knows(pos.x, pos.y - TILE) {
                            continue;
                        }
                        let pv = mirror.view.points.iter().find(|v| v.id == PointId(i));
                        let owner = pv.and_then(|v| v.owner);
                        let foot = foot_world(map, pos.x, pos.y);
                        let def = kind.def();
                        if rebuild {
                            let tint = fog.tint_step(pos.x, pos.y - TILE);
                            let sh = sheets.points[kind.index()];
                            if sh != super::sprites_world::NONE {
                                p.sprite(Layer::Static, Flavor::Lit, sh, 0, foot, SpriteOpts {
                                    color: tint,
                                    ..default()
                                });
                            }
                            // owner pennant (drawPennant at (x, top - 34), s 0.5)
                            if let Some(o) = owner {
                                let c = color_of(&mirror, o);
                                let pen = sheets.pennant[c.index()];
                                if pen != NONE {
                                    let f = *atlas.frame(pen, 0);
                                    let s = 0.5;
                                    let top = def.ay as f32 + 34.0;
                                    // anchor = bottom centre of the pennant
                                    p.frame(Layer::Static, Flavor::Lit, &f, 45.0, 108.0, foot, SpriteOpts {
                                        scale: s,
                                        lift: top - 108.0 * s,
                                        color: tint,
                                        fold: false,
                                        ..default()
                                    });
                                }
                            }
                        }
                        let (hx, top) = (64.0 + 4.0, def.ay as f32 * 0.85);
                        push_hit(
                            &mut hits,
                            &basis,
                            foot,
                            pos.y,
                            HitTarget::Point(PointId(i)),
                            hx,
                            -4.0,
                            top,
                            prio::POINT,
                            pos,
                        );
                        if owner.is_some() {
                            lights.push(LightSrc {
                                pos: foot + basis.up * (def.ay as f32 * 0.3),
                                r: 220.0,
                                power: 0.9,
                                seed: i as f32,
                                flick: 0.12,
                            });
                        }
                    }
                }
            }
        }
    }
    // static light props in view
    for &(pos, r, power, seed, flick) in &scenery.lights {
        if basis.in_view(pos.x, pos.y, 300.0) && fog.knows(pos.x, pos.y) {
            lights.push(LightSrc {
                pos: foot_world(map, pos.x, pos.y) + basis.up * 18.0,
                r,
                power,
                seed,
                flick,
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_node(
    p: &mut Painter,
    sheets: &Sheets,
    i: u32,
    n: &NodeInfo,
    amount: f32,
    foot: Vec3,
    t: f64,
    tint: [f32; 4],
) {
    let depleted = amount <= 0.0;
    let (flavor, color) = if depleted {
        (Flavor::Translucent, [tint[0], tint[1], tint[2], 0.45])
    } else {
        (Flavor::Lit, tint)
    };
    match n.kind {
        NodeKind::Tree => {
            let v = n.variant as usize;
            if !depleted {
                let sh = sheets.trees[v];
                if sh == NONE {
                    return;
                }
                let s = *p.atlas.sheet(sh);
                let f = *p.atlas.frame(sh, (t * 8.0 + n.phase as f64).floor() as i64);
                p.frame(Layer::Static, Flavor::Lit, &f, 96.0, TREE_DEF[v].0 as f32, foot, SpriteOpts {
                    color: tint,
                    ..default()
                });
                let _ = s;
            } else {
                let sh = sheets.stumps[v];
                if sh == NONE {
                    return;
                }
                let f = *p.atlas.frame(sh, 0);
                p.frame(Layer::Static, Flavor::LitFlat, &f, 96.0, TREE_DEF[v].1 as f32, foot, SpriteOpts {
                    color: tint,
                    ..default()
                });
            }
        }
        NodeKind::Gold => {
            if n.big {
                let sh = sheets.gold_big;
                if sh != NONE {
                    p.sprite(Layer::Static, flavor, sh, 0, foot, SpriteOpts {
                        color,
                        ..default()
                    });
                    bottom_centre(p, sh, foot, flavor, color);
                }
                return;
            }
            let stage = ((amount / 40.0).ceil() as i32).clamp(1, 6) as usize - 1;
            // entity y = cell centre; foot = y + 18 (draw order), anchor (64, 64 + 18)
            let shine = if depleted { None } else { gold_shine(i, t) };
            let (sh, fr) = match shine {
                Some(f) if sheets.gold_hl[stage] != NONE => (sheets.gold_hl[stage], f),
                _ => (sheets.gold_stone[stage], 0),
            };
            if sh == NONE {
                return;
            }
            let f = *p.atlas.frame(sh, fr);
            p.frame(Layer::Static, flavor, &f, 64.0, 64.0 + 18.0, foot, SpriteOpts {
                color,
                ..default()
            });
        }
        NodeKind::Stone => {
            if n.big {
                let sh = sheets.stone_big;
                if sh != NONE {
                    bottom_centre(p, sh, foot, flavor, color);
                }
                return;
            }
            let k = if amount > 190.0 {
                2
            } else if amount > 80.0 {
                1
            } else {
                0
            };
            let sh = sheets.stones[k];
            if sh == NONE {
                return;
            }
            // drawn at (x - w/2, y + 22 - h) with y = cy + 6; foot = y + 20 → anchor (w/2, h - 2)
            let s = *p.atlas.sheet(sh);
            let f = *p.atlas.frame(sh, 0);
            p.frame(Layer::Static, flavor, &f, (s.fw * 0.5).floor(), s.fh - 2.0, foot, SpriteOpts {
                color,
                ..default()
            });
        }
    }
}

/// Image anchored at its bottom centre.
fn bottom_centre(p: &mut Painter, sh: SheetId, foot: Vec3, flavor: Flavor, color: [f32; 4]) {
    let s = *p.atlas.sheet(sh);
    let f = *p.atlas.frame(sh, 0);
    p.frame(Layer::Static, flavor, &f, (s.fw * 0.5).floor(), s.fh, foot, SpriteOpts {
        color,
        ..default()
    });
}

fn draw_decor(
    p: &mut Painter,
    sheets: &Sheets,
    map: &Map,
    d: &ts_sim::map::Decor,
    t: f64,
    tint: [f32; 4],
) {
    let v = (d.variant % 4) as usize;
    let foot = foot_world(map, d.x, d.y);
    match d.kind {
        DecorKind::Bush => {
            let sh = sheets.bush[v];
            if sh != NONE {
                let fr = (t * 8.0 + d.phase.floor() as f64).floor() as i64;
                p.sprite(Layer::Static, Flavor::Lit, sh, fr, foot, SpriteOpts {
                    color: tint,
                    ..default()
                });
            }
        }
        DecorKind::Rock => {
            let sh = sheets.rock[v];
            if sh != NONE {
                let f = *p.atlas.frame(sh, 0);
                p.frame(Layer::Static, Flavor::Lit, &f, 32.0, 50.0, foot, SpriteOpts {
                    color: tint,
                    ..default()
                });
            }
        }
        DecorKind::WaterRock => {
            let sh = sheets.water_rock[v];
            if sh != NONE {
                let fr = (t * 10.0 + d.phase.floor() as f64).floor() as i64;
                let f = *p.atlas.frame(sh, fr);
                p.frame(Layer::Static, Flavor::LitFlat, &f, 32.0, 36.0, foot, SpriteOpts {
                    color: tint,
                    ..default()
                });
            }
        }
        DecorKind::Tree => {
            let sh = sheets.trees[v];
            if sh != NONE {
                let f = *p.atlas.frame(sh, (t * 8.0 + d.phase.floor() as f64).floor() as i64);
                p.frame(Layer::Static, Flavor::Lit, &f, 96.0, TREE_DEF[v].0 as f32, foot, SpriteOpts {
                    color: tint,
                    scale: if d.scale > 0.2 && d.scale < 3.0 { d.scale } else { 1.0 },
                    ..default()
                });
            }
        }
        DecorKind::Prop(k) => draw_prop(p, sheets, Layer::Static, k, foot, tint),
        DecorKind::Duck | DecorKind::Cloud => {}
    }
}

/// `drawProp`: `Extra/Props/{name}.png` bottom-centred on the foot.
fn draw_prop(p: &mut Painter, sheets: &Sheets, layer: Layer, k: PropKind, foot: Vec3, tint: [f32; 4]) {
    let sh = sheets.props.get(k.index()).copied().unwrap_or(NONE);
    if sh == NONE {
        return;
    }
    let (w, h) = k.size();
    let flavor = if FLAT_PROPS.contains(&k) {
        Flavor::LitFlat
    } else {
        Flavor::Lit
    };
    let f = *p.atlas.frame(sh, 0);
    p.frame(layer, flavor, &f, (w / 2) as f32, h as f32, foot, SpriteOpts {
        color: tint,
        ..default()
    });
}

/// Pushes a pick box (billboard px around `foot`: half width, vertical range `[bottom, top]` above the foot).
#[allow(clippy::too_many_arguments)]
fn push_hit(
    hits: &mut SpriteHits,
    basis: &Basis,
    foot: Vec3,
    _sy: f32,
    target: HitTarget,
    half_w: f32,
    bottom: f32,
    top: f32,
    priority: u8,
    pos: Vec2,
) {
    if let Some((rect, depth)) = basis.box_rect(foot, half_w, bottom, top) {
        let anchor = basis
            .project(foot + basis.up * 26.0)
            .map(|(s, _)| s)
            .unwrap_or(rect.center());
        hits.hits.push(SpriteHit {
            target,
            screen_rect: rect,
            depth,
            priority,
            anchor,
            foot: pos,
        });
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Dynamic layer
// ---------------------------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn draw_units(
    mirror: Res<Mirror>,
    atlas: Res<SpriteAtlas>,
    basis: Res<Basis>,
    time: Res<Time<Real>>,
    anims: Res<UnitAnims>,
    mut cache: ResMut<UnitSheetCache>,
    mut batches: ResMut<SpriteBatches>,
    mut hits: ResMut<SpriteHits>,
) {
    let Some(map) = drawable(&mirror, &atlas, &basis) else {
        return;
    };
    if batches.pages == 0 {
        return;
    }
    let now = time.elapsed_secs_f64();
    let mut p = Painter {
        batches: &mut batches,
        basis: &basis,
        atlas: &atlas,
    };
    for (id, u) in &mirror.view.units {
        if u.flags & UF_INSIDE != 0 {
            continue;
        }
        let pos = mirror
            .unit_pos_interp(*id, now)
            .unwrap_or(Vec2::new(u.x, u.y));
        if !basis.in_view(pos.x, pos.y, 120.0) {
            continue;
        }
        let color = color_of(&mirror, u.faction);
        let key = AnimKey::of(u, color);
        let Some((sh, fps)) = cache.get(&atlas, key) else {
            continue;
        };
        let at = anims.get(*id).map(|a| a.t).unwrap_or(u.anim_t);
        let frames = atlas.sheet(sh).frames;
        let fr = anim_frame(at, fps, frames, key.clamps());
        let foot = foot_world(map, pos.x, pos.y);
        let hurt = u.flags & UF_HURT != 0;
        p.sprite(Layer::Dynamic, Flavor::Lit, sh, fr, foot, SpriteOpts {
            flip: u.flags & UF_FLIP != 0,
            color: if hurt { HURT_TINT } else { [1.0; 4] },
            ..default()
        });
        let d = u.kind.def();
        let wide = if d.siege || u.kind == UnitKind::Knight {
            40.0
        } else {
            24.0
        };
        push_hit(
            &mut hits,
            &basis,
            foot,
            pos.y,
            HitTarget::Unit(*id),
            wide,
            -10.0,
            d.bar_y - 8.0,
            prio::UNIT,
            pos,
        );
    }
}

/// Foot point (px) of a building: `x = (cx + w/2)*64`, `y = (cy + h)*64 - 4`.
#[inline]
pub fn building_foot(b: &BuildingView) -> Vec2 {
    let d = b.kind.def();
    Vec2::new(
        (b.cx as f32 + d.w as f32 * 0.5) * TILE,
        (b.cy + d.h) as f32 * TILE - 4.0,
    )
}

/// Fire flames hash (`h1`, `h2` of drawBuilding) for a per-building seed.
#[inline]
fn fire_hash(seed: f32, i: usize) -> (f32, f32) {
    let h1 = ((seed + i as f32 * 12.9898).sin() * 43758.547).abs().fract();
    let h2 = ((seed + i as f32 * 78.233).sin() * 12543.123).abs().fract();
    (h1, h2)
}

#[allow(clippy::too_many_arguments)]
fn draw_buildings(
    mirror: Res<Mirror>,
    atlas: Res<SpriteAtlas>,
    sheets: Res<Sheets>,
    basis: Res<Basis>,
    fog: Option<Res<FogTexture>>,
    clock: Res<WorldClock>,
    ghost: Res<Ghost>,
    mut batches: ResMut<SpriteBatches>,
    mut hits: ResMut<SpriteHits>,
    mut lights: ResMut<LightCandidates>,
) {
    let Some(map) = drawable(&mirror, &atlas, &basis) else {
        return;
    };
    if !sheets.ready || batches.pages == 0 {
        return;
    }
    let fog = Fog(fog.as_deref());
    let t = clock.t;
    // palisade neighbourhood (World.wallOf over every known wall)
    let walls: HashSet<(i32, i32)> = mirror
        .view
        .buildings
        .values()
        .filter(|b| b.kind == BuildingKind::Wall)
        .map(|b| (b.cx, b.cy))
        .collect();
    let mut p = Painter {
        batches: &mut batches,
        basis: &basis,
        atlas: &atlas,
    };
    for (id, b) in &mirror.view.buildings {
        if ghost.moving == Some(*id) && ghost.kind.is_some() {
            continue;
        }
        let d = b.kind.def();
        let fp = building_foot(b);
        if !basis.in_view(fp.x, fp.y, d.w as f32 * 40.0 + 64.0) {
            continue;
        }
        let remembered = mirror.view.ghosts.contains(id);
        let own = Some(b.faction) == mirror.you;
        if !own && !remembered && !fog.knows(fp.x, fp.y - 20.0) {
            continue;
        }
        let tint = if remembered {
            shade_tint(crate::render::terrain::EXPLORED_SHADE)
        } else {
            fog.tint(fp.x, fp.y - 20.0)
        };
        let color = color_of(&mirror, b.faction);
        let sh = sheets.building(color, b.kind, b.variant);
        let foot = foot_world(map, fp.x, fp.y);
        let prog = if d.work > 0.0 {
            (b.progress / d.work).clamp(0.0, 1.0)
        } else {
            1.0
        };
        if sh != NONE {
            if b.kind == BuildingKind::Wall {
                draw_wall(&mut p, sh, b, &walls, map, fp, tint, prog);
            } else if !b.done {
                let s = *atlas.sheet(sh);
                p.sprite(Layer::Dynamic, Flavor::Translucent, sh, 0, foot, SpriteOpts {
                    color: [tint[0], tint[1], tint[2], 0.3],
                    ..default()
                });
                let hpx = (s.fh * prog).round();
                if hpx > 0.0 {
                    p.sprite(Layer::Dynamic, Flavor::Lit, sh, 0, foot, SpriteOpts {
                        color: tint,
                        rows: Some((s.fh - hpx, s.fh)),
                        ..default()
                    });
                }
            } else {
                p.sprite(Layer::Dynamic, Flavor::Lit, sh, 0, foot, SpriteOpts {
                    color: tint,
                    ..default()
                });
            }
        }
        // fire on damaged buildings
        let r = if b.max_hp > 0 {
            b.hp as f32 / b.max_hp as f32
        } else {
            1.0
        };
        let seed = hash01(id.0.index, 5) * 100.0;
        if b.done && r < 0.7 && b.kind != BuildingKind::Wall {
            let (fx, n) = if r < 0.25 {
                (FxKind::Fire3, 4)
            } else if r < 0.45 {
                (FxKind::Fire2, 3)
            } else {
                (FxKind::Fire1, 2)
            };
            let fsh = sheets.fx[fx.index()];
            if fsh != NONE {
                let fps = atlas.sheet(fsh).fps;
                for i in 0..n {
                    let (h1, h2) = fire_hash(seed, i);
                    let fx_x = fp.x + (h1 - 0.5) * d.w as f32 * TILE * 0.7;
                    let lift = 24.0 + h2 * d.ay as f32 * 0.45;
                    let ffoot = foot_world(map, fx_x, fp.y);
                    p.sprite(
                        Layer::Dynamic,
                        Flavor::Overlay,
                        fsh,
                        (t * fps as f64 + (i * 3) as f64).floor() as i64,
                        ffoot,
                        SpriteOpts {
                            scale: 1.4,
                            lift,
                            fold: false,
                            pull: 0.15,
                            color: [tint[0] * 1.3, tint[1] * 1.3, tint[2] * 1.3, 1.0],
                            ..default()
                        },
                    );
                }
            }
            lights.push(LightSrc {
                pos: foot + basis.up * (d.ay as f32 * 0.4),
                r: 240.0,
                power: 1.2,
                seed: seed * 3.0,
                flick: 0.5,
            });
        }
        if b.done && b.kind != BuildingKind::Wall {
            let rr = if b.kind == BuildingKind::Castle {
                340.0
            } else if d.w > 2 {
                260.0
            } else {
                200.0
            };
            lights.push(LightSrc {
                pos: foot + basis.up * (d.ay as f32 * 0.32),
                r: rr,
                power: 1.0,
                seed: id.0.index as f32,
                flick: 0.12,
            });
        }
        // pick box: x0-4..x1+4, top = wide ? y - ay*0.85 : y0 - 6, bottom y1
        let top = if d.w > 1 {
            d.ay as f32 * 0.85
        } else {
            d.h as f32 * TILE - 4.0 + 6.0
        };
        push_hit(
            &mut hits,
            &basis,
            foot,
            fp.y,
            HitTarget::Building(*id),
            d.w as f32 * 32.0 + 4.0,
            -4.0,
            top,
            prio::BUILDING,
            fp,
        );
    }
}

/// Palisade auto-tiling (`drawWall`, spec 01 §3.2).
#[allow(clippy::too_many_arguments)]
fn draw_wall(
    p: &mut Painter,
    sh: SheetId,
    b: &BuildingView,
    walls: &HashSet<(i32, i32)>,
    map: &Map,
    fp: Vec2,
    tint: [f32; 4],
    prog: f32,
) {
    let has = |dx: i32, dy: i32| walls.contains(&(b.cx + dx, b.cy + dy));
    let side = has(-1, 0) || has(1, 0);
    let up = has(0, -1);
    let down = has(0, 1);
    let front = side || !(up || down);
    let (flavor, color) = if b.done {
        (Flavor::Lit, tint)
    } else {
        let a = 0.35 + 0.65 * prog;
        (Flavor::Translucent, [tint[0], tint[1], tint[2], a])
    };
    let s = *p.atlas.sheet(sh);
    let f = *p.atlas.frame(sh, 0);
    let y0 = b.cy as f32 * TILE;
    if !front || up {
        let n = if front { STAKE_N - 1 } else { STAKE_N };
        let sx = ts_sim::defs::STAKE_SX as f32;
        let sw = ts_sim::defs::STAKE_SW as f32;
        for i in 0..n {
            // stake image top at y0 + (i+1)*64/5 - 72; its anchor row (ay) is the foot
            let top = y0 + (i + 1) as f32 * TILE / STAKE_N as f32 - 72.0;
            let fy = top + s.ay;
            let foot = foot_world(map, fp.x, fy);
            p.frame(Layer::Dynamic, flavor, &f, sx + sw * 0.5, s.ay, foot, SpriteOpts {
                color,
                cols: Some((sx, sx + sw)),
                ..default()
            });
        }
    }
    if front {
        let foot = foot_world(map, fp.x, fp.y);
        p.frame(Layer::Dynamic, flavor, &f, s.ax, s.ay, foot, SpriteOpts {
            color,
            ..default()
        });
    }
}

const STAKE_N: usize = ts_sim::defs::STAKE_N as usize;

#[allow(clippy::too_many_arguments)]
fn draw_misc(
    mirror: Res<Mirror>,
    atlas: Res<SpriteAtlas>,
    sheets: Res<Sheets>,
    basis: Res<Basis>,
    fog: Option<Res<FogTexture>>,
    clock: Res<WorldClock>,
    scenery: Res<Scenery>,
    props: Res<EventProps>,
    mut sheep_anims: ResMut<SheepAnims>,
    mut batches: ResMut<SpriteBatches>,
    mut hits: ResMut<SpriteHits>,
) {
    let Some(map) = drawable(&mirror, &atlas, &basis) else {
        return;
    };
    if !sheets.ready || batches.pages == 0 {
        return;
    }
    let fog = Fog(fog.as_deref());
    let t = clock.t;
    let dt = clock.dt;
    let mut p = Painter {
        batches: &mut batches,
        basis: &basis,
        atlas: &atlas,
    };
    // ---- sheep ----
    sheep_anims
        .map
        .retain(|id, _| mirror.view.sheep.contains_key(id));
    for (id, s) in &mirror.view.sheep {
        let e = sheep_anims.map.entry(*id).or_insert((s.state, hash01(id.0.index, 3) * 2.0));
        if e.0 != s.state {
            *e = (s.state, 0.0);
        } else {
            e.1 += dt;
        }
        if !basis.in_view(s.x, s.y, 80.0) {
            continue;
        }
        let (i, fps) = match s.state {
            SheepState::Idle => (0, 8.0),
            SheepState::Move => (1, 8.0),
            SheepState::Grass => (2, 10.0),
        };
        let sh = sheets.sheep[i];
        if sh == NONE {
            continue;
        }
        let foot = foot_world(map, s.x, s.y);
        p.sprite(Layer::Dynamic, Flavor::Lit, sh, (e.1 * fps).floor() as i64, foot, SpriteOpts {
            flip: s.flip,
            color: fog.tint(s.x, s.y),
            ..default()
        });
        push_hit(
            &mut hits,
            &basis,
            foot,
            s.y,
            HitTarget::Sheep(*id),
            26.0,
            -8.0,
            44.0,
            prio::SHEEP,
            Vec2::new(s.x, s.y),
        );
    }
    // ---- projectiles ----
    for pr in &mirror.view.projectiles {
        if !basis.in_view(pr.x, pr.y, 120.0) {
            continue;
        }
        if pr.rock {
            let sh = sheets.stone_res;
            if sh == NONE {
                continue;
            }
            let foot = foot_world(map, pr.x, pr.y);
            let f = *atlas.frame(sh, 0);
            p.frame(Layer::Dynamic, Flavor::Lit, &f, 13.0, 11.0, foot, SpriteOpts {
                scale: 1.3,
                lift: pr.z.max(0.0) + 11.0,
                rot: -pr.ang,
                fold: false,
                ..default()
            });
        } else {
            let c = color_of(&mirror, pr.faction);
            let sh = sheets.arrow[c.index()];
            if sh == NONE {
                continue;
            }
            // the 2D arrow point (x, y) is drawn 28 px above its ground point
            let foot = foot_world(map, pr.x, pr.y + 28.0);
            let f = *atlas.frame(sh, 0);
            p.frame(Layer::Dynamic, Flavor::Lit, &f, 32.0, 32.0, foot, SpriteOpts {
                lift: 28.0,
                rot: -pr.ang,
                fold: false,
                ..default()
            });
        }
    }
    // ---- pickups ----
    for pk in &mirror.view.pickups {
        if !basis.in_view(pk.x, pk.y, 80.0) || !fog.knows(pk.x, pk.y) {
            continue;
        }
        let tint = fog.tint(pk.x, pk.y);
        let foot = foot_world(map, pk.x, pk.y);
        if pk.chest {
            // prop 'chest' at y + 8
            let foot = foot_world(map, pk.x, pk.y + 8.0);
            draw_prop(&mut p, &sheets, Layer::Dynamic, PropKind::Chest, foot, tint);
        } else {
            if pk.fading && (t * 6.0).floor() as i64 % 2 == 1 {
                continue;
            }
            let lift = pk.hop + (t as f32 * 3.0).sin().abs() * 4.0;
            let opts = SpriteOpts {
                scale: 1.5,
                lift,
                color: tint,
                fold: false,
                ..default()
            };
            match pk.res {
                ResKind::Gold => {
                    let ph = t.rem_euclid(2.2);
                    let (sh, fr) = if ph < 0.6 && sheets.gold_res_hl != NONE {
                        (sheets.gold_res_hl, (ph * 10.0).floor() as i64)
                    } else {
                        (sheets.gold_res, 0)
                    };
                    if sh != NONE {
                        let f = *atlas.frame(sh, fr);
                        p.frame(Layer::Dynamic, Flavor::Lit, &f, 64.0, 70.0, foot, opts);
                    }
                }
                ResKind::Stone => {
                    if sheets.stone_res != NONE {
                        let f = *atlas.frame(sheets.stone_res, 0);
                        p.frame(Layer::Dynamic, Flavor::Lit, &f, 13.0, 20.0, foot, opts);
                    }
                }
                ResKind::Wood | ResKind::Meat => {
                    let sh = if pk.res == ResKind::Wood {
                        sheets.wood_res
                    } else {
                        sheets.meat_res
                    };
                    if sh != NONE {
                        let f = *atlas.frame(sh, 0);
                        p.frame(Layer::Dynamic, Flavor::Lit, &f, 32.0, 42.0, foot, opts);
                    }
                }
            }
        }
        push_hit(
            &mut hits,
            &basis,
            foot,
            pk.y,
            HitTarget::Pickup(pk.id),
            24.0,
            -8.0,
            40.0,
            prio::PICKUP,
            Vec2::new(pk.x, pk.y),
        );
    }
    // ---- ducks ----
    for d in map.decor.iter().filter(|d| d.kind == DecorKind::Duck) {
        if !basis.in_view(d.x, d.y, 100.0) || !fog.knows(d.x, d.y) {
            continue;
        }
        let sh = sheets.duck;
        if sh == NONE {
            break;
        }
        let r = if scenery.is_pond_duck(d.x, d.y) {
            18.0
        } else {
            46.0
        };
        let tf = t as f32;
        let a = tf * 0.3 + d.phase;
        let x = d.x + a.sin() * r;
        let y = d.y + (a * 0.8).cos() * r * 0.4 + (tf * 2.0 + d.phase).sin() * 1.5;
        let foot = Vec3::new(x * PX, crate::render::consts::WATER_Y, y * PX);
        p.sprite(
            Layer::Dynamic,
            Flavor::Lit,
            sh,
            (t * 4.0 + d.phase as f64).floor() as i64,
            foot,
            SpriteOpts {
                flip: a.cos() < 0.0,
                color: fog.tint(x, y),
                ..default()
            },
        );
    }
    // ---- props from events (opened chests) ----
    for &(k, x, y) in &props.list {
        if basis.in_view(x, y, 80.0) && fog.knows(x, y) {
            let foot = foot_world(map, x, y);
            draw_prop(&mut p, &sheets, Layer::Dynamic, k, foot, fog.tint(x, y));
        }
    }
    let _ = ground_at;
    let _: Option<BuildingId> = None;
}
