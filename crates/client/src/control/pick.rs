//! `Ctl.pick(wx, wy)` — what is under the mouse (05 §7.2), HD-2D version (05 §17.5): the 2-D sprite-space hitboxes
//! of the JS game are rebuilt as billboard rectangles standing on the foot point (sprite px up = world +Y, 1 px =
//! 1/64 world unit), projected to the screen and tested against the cursor. Priority classes: units → buildings →
//! capture points → sheep → gold/stone nodes → trees; within units, buildings and trees the front-most (largest
//! sim `y`) wins, within the other classes the first in list order. Fog gating per 05 §12.2 (the mirror only
//! contains enemy units / sheep that are visible now; buildings in it are known; points/nodes/trees need the cell
//! explored).

use super::scene::SceneIndex;
use super::view::ViewInfo;
use crate::mirror::Mirror;
use bevy::prelude::*;
use ts_sim::TILE;
use ts_sim::cmd::EntityRef;
use ts_sim::defs::{NodeKind, UnitKind};
use ts_sim::map::Map;
use ts_sim::slab::NodeId;
use ts_sim::snapshot_types::UF_INSIDE;

/// `Fog.knows(x, y)`: explored by the local player (everything when the fog is off / spectating / match over).
pub fn knows(mirror: &Mirror, x: f32, y: f32) -> bool {
    if fog_off(mirror) {
        return true;
    }
    let Some(m) = mirror.map() else {
        return false;
    };
    let c = m.cell_at(x, y);
    m.valid(c) && mirror.view.explored.get(c as usize)
}

/// Fog disabled for the local view.
pub fn fog_off(mirror: &Mirror) -> bool {
    mirror.view.fog_off || mirror.you.is_none() || mirror.view.outcome.is_some()
}

/// Screen rectangle of a billboard hitbox: horizontal `±half` sprite px around the foot, vertical from `below` px
/// under the foot to `above` px over it.
fn billboard_rect(
    view: &ViewInfo,
    map: Option<&Map>,
    x: f32,
    y: f32,
    half: f32,
    above: f32,
    below: f32,
) -> Option<Rect> {
    let foot = view.project_px(map, x, y, 0.0)?;
    let top = view.project_px(map, x, y, above)?;
    let bottom = view.project_px(map, x, y, -below)?;
    let scale = view.scale_at(map, x, y) / TILE;
    let hw = half * scale;
    Some(Rect::new(
        foot.x - hw,
        top.y.min(bottom.y),
        foot.x + hw,
        top.y.max(bottom.y),
    ))
}

/// Screen rectangle of a footprint (ground rect `x0..x1 × y0..y1`) plus a billboard rising `lift` px from the
/// south edge (buildings, capture points).
fn footprint_rect(
    view: &ViewInfo,
    map: Option<&Map>,
    r: Rect,
    foot_y: f32,
    lift: f32,
) -> Option<Rect> {
    let mut out: Option<Rect> = None;
    let mut add = |p: Option<Vec2>| {
        if let Some(p) = p {
            out = Some(match out {
                Some(o) => o.union_point(p),
                None => Rect::from_center_size(p, Vec2::ZERO),
            });
        }
    };
    let hy = ground_level_px(map, (r.min.x + r.max.x) * 0.5, foot_y);
    for (x, y) in [
        (r.min.x, r.min.y),
        (r.max.x, r.min.y),
        (r.min.x, r.max.y),
        (r.max.x, r.max.y),
    ] {
        add(project_at_h(view, x, y, hy, 0.0));
    }
    add(project_at_h(view, r.min.x, foot_y, hy, lift));
    add(project_at_h(view, r.max.x, foot_y, hy, lift));
    out
}

fn ground_level_px(map: Option<&Map>, x: f32, y: f32) -> f32 {
    super::view::ground_h(map, x, y)
}

fn project_at_h(view: &ViewInfo, x: f32, y: f32, h: f32, lift: f32) -> Option<Vec2> {
    view.project(Vec3::new(x / TILE, h + lift / TILE, y / TILE))
}

/// Picks the entity under the window point `s`. `g` = ground point under it (sim px) for the cheap prefilter.
pub fn pick(
    mirror: &Mirror,
    scene: &SceneIndex,
    view: &ViewInfo,
    s: Vec2,
    g: Option<Vec2>,
) -> Option<EntityRef> {
    if !view.ok || !mirror.active {
        return None;
    }
    let map = mirror.map();
    let me = mirror.you;
    // objects south of the cursor stand up and can cover it; objects far away cannot
    let near = |x: f32, y: f32, reach: f32| -> bool {
        match g {
            Some(g) => (x - g.x).abs() < reach && y > g.y - reach * 0.8 && y < g.y + reach * 2.5,
            None => true,
        }
    };
    // 1. units: front-most
    let mut best: Option<(f32, EntityRef)> = None;
    for u in mirror.view.units.values() {
        if u.flags & UF_INSIDE != 0 {
            continue;
        }
        if !near(u.x, u.y, 320.0) {
            continue;
        }
        let d = u.kind.def();
        let wide = if d.siege || u.kind == UnitKind::Knight {
            40.0
        } else {
            24.0
        };
        let _ = me;
        if let Some(r) = billboard_rect(view, map, u.x, u.y, wide, d.bar_y - 8.0, 10.0)
            && r.contains(s)
            && best.is_none_or(|(by, _)| u.y > by)
        {
            best = Some((u.y, EntityRef::Unit(u.id)));
        }
    }
    if let Some((_, e)) = best {
        return Some(e);
    }
    // 2. buildings (known): front-most
    for b in mirror.view.buildings.values() {
        let d = b.kind.def();
        let x0 = b.cx as f32 * TILE;
        let y0 = b.cy as f32 * TILE;
        let x1 = (b.cx + d.w) as f32 * TILE;
        let y1 = (b.cy + d.h) as f32 * TILE;
        let bx = (x0 + x1) * 0.5;
        let by = y1 - 4.0;
        if !near(bx, by, 500.0) {
            continue;
        }
        let top = if d.w > 1 {
            by - d.ay as f32 * 0.85
        } else {
            y0 - 6.0
        };
        let lift = (by - top).max(0.0);
        if let Some(r) = footprint_rect(view, map, Rect::new(x0 - 4.0, y0, x1 + 4.0, y1), by, lift)
            && r.contains(s)
            && best.is_none_or(|(yy, _)| by > yy)
        {
            best = Some((by, EntityRef::Building(b.id)));
        }
    }
    if let Some((_, e)) = best {
        return Some(e);
    }
    // 3. capture points (explored at (x, y - TILE)): first in list order
    for (i, p) in scene.points.iter().enumerate() {
        if !near(p.pos.x, p.pos.y, 400.0) || !knows(mirror, p.pos.x, p.pos.y - TILE) {
            continue;
        }
        let lift = p.kind.def().ay as f32 * 0.85;
        let r = Rect::new(p.rect.min.x - 4.0, p.rect.min.y, p.rect.max.x + 4.0, p.rect.max.y);
        if let Some(sr) = footprint_rect(view, map, r, p.pos.y, lift)
            && sr.contains(s)
        {
            return Some(EntityRef::Point(ts_sim::slab::PointId(i as u32)));
        }
    }
    // 4. sheep (visible now)
    for sh in mirror.view.sheep.values() {
        if !near(sh.x, sh.y, 200.0) {
            continue;
        }
        if let Some(r) = billboard_rect(view, map, sh.x, sh.y, 26.0, 44.0, 8.0)
            && r.contains(s)
        {
            return Some(EntityRef::Sheep(sh.id));
        }
    }
    // 5. gold then stone nodes (explored; exhausted ones are dropped by the selection refresh)
    for kind in [NodeKind::Gold, NodeKind::Stone] {
        for (i, n) in scene.nodes.iter().enumerate() {
            if n.kind != kind || !near(n.pos.x, n.pos.y, 260.0) {
                continue;
            }
            let r = if n.big { 68.0 } else { 36.0 };
            if !knows(mirror, n.pos.x, n.pos.y) {
                continue;
            }
            if let Some(sr) = billboard_rect(view, map, n.pos.x, n.pos.y, r, r, r)
                && sr.contains(s)
            {
                return Some(EntityRef::Node(NodeId(i as u32)));
            }
        }
    }
    // 6. trees with wood: front-most
    let mut tree: Option<(f32, usize)> = None;
    for (i, n) in scene.nodes.iter().enumerate() {
        if n.kind != NodeKind::Tree || !near(n.pos.x, n.pos.y, 260.0) {
            continue;
        }
        if mirror.view.node_amounts.get(i).copied().unwrap_or(0.0) <= 0.0
            || !knows(mirror, n.pos.x, n.pos.y)
        {
            continue;
        }
        if let Some(r) = billboard_rect(view, map, n.pos.x, n.pos.y, 30.0, 100.0, 8.0)
            && r.contains(s)
            && tree.is_none_or(|(y, _)| n.pos.y > y)
        {
            tree = Some((n.pos.y, i));
        }
    }
    tree.map(|(_, i)| EntityRef::Node(NodeId(i as u32)))
}

/// World position (sim px, foot point) of an entity, if it is in the view.
pub fn entity_pos(mirror: &Mirror, scene: &SceneIndex, e: EntityRef) -> Option<Vec2> {
    match e {
        EntityRef::Unit(id) => mirror.view.units.get(&id).map(|u| Vec2::new(u.x, u.y)),
        EntityRef::Building(id) => mirror.view.buildings.get(&id).map(|b| {
            let d = b.kind.def();
            Vec2::new(
                (b.cx as f32 + d.w as f32 / 2.0) * TILE,
                (b.cy + d.h) as f32 * TILE - 4.0,
            )
        }),
        EntityRef::Node(n) => scene.nodes.get(n.0 as usize).map(|n| n.pos),
        EntityRef::Sheep(id) => mirror.view.sheep.get(&id).map(|s| Vec2::new(s.x, s.y)),
        EntityRef::Point(p) => scene.points.get(p.0 as usize).map(|p| p.pos),
    }
}

/// `nodeValid` / still-present test used by the selection refresh (05 §7.1).
pub fn entity_valid(mirror: &Mirror, scene: &SceneIndex, e: EntityRef) -> bool {
    match e {
        EntityRef::Unit(id) => mirror.view.units.contains_key(&id),
        EntityRef::Building(id) => mirror.view.buildings.contains_key(&id),
        EntityRef::Sheep(id) => mirror.view.sheep.contains_key(&id),
        EntityRef::Point(p) => (p.0 as usize) < scene.points.len(),
        EntityRef::Node(n) => {
            (n.0 as usize) < scene.nodes.len()
                && mirror
                    .view
                    .node_amounts
                    .get(n.0 as usize)
                    .is_some_and(|a| *a > 0.0)
        }
    }
}

/// `pick()` over the sprite renderer's hit boxes (C2 `SpriteHits`, built with the exact billboard basis and the
/// spec hitboxes). `None` when the renderer published nothing this frame (caller falls back to [`pick`]).
/// Class order: lowest `priority` wins (pickups are not selectable); within units, buildings and trees the
/// front-most (largest sim `y`) wins, within the other classes the first in list order. Fog gating is applied
/// again here so a stale hit can never select something unknown.
pub fn pick_hits(
    hits: &crate::render::sprites::SpriteHits,
    mirror: &Mirror,
    scene: &SceneIndex,
    s: Vec2,
) -> Option<Option<EntityRef>> {
    use crate::render::sprites::prio;
    if hits.hits.is_empty() {
        return None;
    }
    let mut best: Option<(u8, f32, EntityRef)> = None;
    for h in &hits.hits {
        if h.priority > prio::TREE || !h.screen_rect.contains(s) {
            continue;
        }
        let Some(e) = h.target.entity_ref() else {
            continue;
        };
        let ok = match e {
            EntityRef::Unit(id) => mirror
                .view
                .units
                .get(&id)
                .is_some_and(|u| u.flags & UF_INSIDE == 0),
            EntityRef::Building(id) => mirror.view.buildings.contains_key(&id),
            EntityRef::Sheep(id) => mirror.view.sheep.contains_key(&id),
            EntityRef::Point(p) => scene
                .points
                .get(p.0 as usize)
                .is_some_and(|p| knows(mirror, p.pos.x, p.pos.y - TILE)),
            EntityRef::Node(n) => scene.nodes.get(n.0 as usize).is_some_and(|n| {
                knows(mirror, n.pos.x, n.pos.y)
                    && (n.kind != NodeKind::Tree
                        || mirror.view.node_amounts.get(n_index(e)).copied().unwrap_or(0.0) > 0.0)
            }),
        };
        if !ok {
            continue;
        }
        let front_most = matches!(h.priority, p if p == prio::UNIT || p == prio::BUILDING || p == prio::TREE);
        let better = match best {
            None => true,
            Some((bp, by, _)) => {
                h.priority < bp || (h.priority == bp && front_most && h.foot.y > by)
            }
        };
        if better {
            best = Some((h.priority, h.foot.y, e));
        }
    }
    Some(best.map(|(_, _, e)| e))
}

fn n_index(e: EntityRef) -> usize {
    match e {
        EntityRef::Node(n) => n.0 as usize,
        _ => usize::MAX,
    }
}
