//! Static scenery index for input (OWNER: C4): resource nodes and capture points of the regenerated world (ids in the
//! canonical order of spec 02 §8, same as `World::new`), plus the client-side placement map (terrain + known
//! building footprints) used to validate the ghost before a `place` / `move` command (05 §9.2).

use crate::mirror::Mirror;
use bevy::prelude::*;
use ts_sim::TILE;
use ts_sim::defs::{BuildingKind, NodeKind, PointKind, TREE_FOOT_DY};
use ts_sim::map::Map;
use ts_sim::slab::BuildingId;

/// A resource node (tree / gold / stone).
#[derive(Clone, Copy, Debug)]
pub struct NodeInfo {
    pub kind: NodeKind,
    pub big: bool,
    /// foot point (sim px)
    pub pos: Vec2,
}

/// A capture point.
#[derive(Clone, Copy, Debug)]
pub struct PointInfo {
    pub kind: PointKind,
    /// foot point `(cx+1, cy+2)*TILE - (0, 4)`
    pub pos: Vec2,
    /// footprint rect (sim px) `x0, y0, x1, y1`
    pub rect: Rect,
    /// footprint centre in cells (outpost zone)
    pub g: Vec2,
}

#[derive(Resource, Default)]
pub struct SceneIndex {
    pub generation: Option<u32>,
    pub nodes: Vec<NodeInfo>,
    pub points: Vec<PointInfo>,
    /// terrain + footprints of every known building (block counter) — `None` outside a match
    pub place_map: Option<Map>,
    /// signature of the building set the place map was built from
    sig: u64,
}

impl SceneIndex {
    /// `canPlace(kind, cx, cy, own)` against the known buildings (own = cells of the building being moved).
    pub fn can_place(&self, kind: BuildingKind, cx: i32, cy: i32, own: &[i32]) -> bool {
        self.place_map
            .as_ref()
            .is_some_and(|m| m.can_place(kind, cx, cy, own))
    }
}

/// Rebuilds the node / point tables on every new match and the place map when the building set changes.
pub fn refresh_scene(mirror: Res<Mirror>, mut scene: ResMut<SceneIndex>) {
    let Some(g) = mirror.world.as_ref() else {
        if scene.generation.is_some() {
            *scene = SceneIndex::default();
        }
        return;
    };
    if scene.generation != Some(mirror.generation) {
        scene.generation = Some(mirror.generation);
        let m = &g.map;
        let center = |c: i32| {
            let v = m.center(c);
            Vec2::new(v.x, v.y)
        };
        let mut nodes: Vec<NodeInfo> = g
            .trees
            .iter()
            .map(|t| NodeInfo {
                kind: NodeKind::Tree,
                big: false,
                pos: center(t.cell) + Vec2::new(0.0, TREE_FOOT_DY),
            })
            .collect();
        for kind in [NodeKind::Gold, NodeKind::Stone] {
            let small = if kind == NodeKind::Gold {
                &g.golds
            } else {
                &g.stones
            };
            let bigs = g.bigs.iter().filter(|(k, _)| *k == kind).map(|(_, d)| d);
            for d in small.iter().chain(bigs) {
                let c = center(d.cell);
                let pos = if d.big {
                    c + Vec2::new(32.0, 32.0)
                } else if kind == NodeKind::Stone {
                    c + Vec2::new(0.0, 6.0)
                } else {
                    c
                };
                nodes.push(NodeInfo {
                    kind,
                    big: d.big,
                    pos,
                });
            }
        }
        scene.nodes = nodes;
        scene.points = g
            .points
            .iter()
            .map(|p| {
                let (cx, cy) = (p.cell % m.w, p.cell / m.w);
                PointInfo {
                    kind: p.kind,
                    pos: Vec2::new((cx + 1) as f32 * TILE, (cy + 2) as f32 * TILE - 4.0),
                    rect: Rect::new(
                        cx as f32 * TILE,
                        cy as f32 * TILE,
                        (cx + 2) as f32 * TILE,
                        (cy + 2) as f32 * TILE,
                    ),
                    g: Vec2::new((cx + 1) as f32, (cy + 1) as f32),
                }
            })
            .collect();
        let mut pm = g.map.clone();
        pm.block.clone_from(&pm.static_block);
        scene.place_map = Some(pm);
        scene.sig = u64::MAX;
    }
    // building footprints
    let sig = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for b in mirror.view.buildings.values() {
            (b.id, b.kind, b.cx, b.cy).hash(&mut h);
        }
        h.finish()
    };
    if sig == scene.sig {
        return;
    }
    scene.sig = sig;
    let Some(pm) = scene.place_map.as_mut() else {
        return;
    };
    pm.block.clone_from(&pm.static_block);
    for b in mirror.view.buildings.values() {
        let d = b.kind.def();
        if let Some(cells) = pm.footprint(b.cx, b.cy, d.w, d.h) {
            for c in cells {
                let i = c as usize;
                pm.block[i] = pm.block[i].saturating_add(1);
            }
        }
    }
}

/// Footprint cells of a known building (move validation).
pub fn building_cells(mirror: &Mirror, id: BuildingId) -> Vec<i32> {
    let (Some(b), Some(m)) = (mirror.view.buildings.get(&id), mirror.map()) else {
        return Vec::new();
    };
    let d = b.kind.def();
    m.footprint(b.cx, b.cy, d.w, d.h).unwrap_or_default()
}
