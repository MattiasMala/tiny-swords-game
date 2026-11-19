// OWNER: S6 (net) — see docs/spec/08-net.md §A.7-A.8 (old snapshot content / client replication), §R6 (per-recipient
// fog filtering, deltas, ghosts, backpressure), addenda N2 (train speed on queue bars); 05 addenda C3 (ghosts);
// 04a §16 + addenda E7 (economy fields the UI needs); types in snapshot_types.rs.
//
// `build_snapshot(world, viewer, cache, events)` builds one recipient's snapshot for the current tick against what
// that recipient already received (`ViewerCache`, kept per session by the room so that a reload keeps the memory
// of enemy building ghosts):
//   * fog: a Player sees own things always, enemy units / sheep / projectiles / pickups / public positional events
//     only on currently visible cells (`fog.visible`), enemy buildings when any footprint cell is visible;
//     spectators (eliminated players), test-mode fog off and a finished match see everything;
//   * units: row deltas (anim time ignored unless the animation restarts) + `units_gone`;
//   * buildings: new/changed rows (timer-only changes at most every `TIMER_RESEND` ticks), `buildings_hidden`
//     when they leave vision (client keeps a ghost), `buildings_gone` once the remembered spot is seen without it;
//   * nodes and capture points: changes the recipient can see (or that concern it); explored cells: new bits;
//   * private events only to their recipient.
// `ClientMirror::apply` is the client half (same file).

use crate::World;
use crate::cmd::EntityRef;
use crate::defs::*;
use crate::events::GameEvent;
use crate::math::BitGrid;
use crate::slab::{BuildingId, NodeId, PickupId, PointId, SheepId, UnitId};
use crate::snapshot_types::wire::quant;
use crate::snapshot_types::*;
use crate::world::{Building, GatherTarget, ProjKind, RallyTarget, Unit};
use std::collections::BTreeMap;

/// Ticks between re-sends of a building whose only change is a timer (construction progress, queue time).
pub const TIMER_RESEND: u64 = 10;
/// Above this many newly explored cells in one tick the whole grid is sent instead of the cell list.
pub const EXPLORED_FULL_ABOVE: usize = 2048;

/// Who a snapshot is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewer {
    /// a kingdom's player (fog applies unless test fog-off / match over)
    Player(FactionIdx),
    /// eliminated player / observer: sees everything
    Spectator,
}

/// A unit as last sent to a recipient.
#[derive(Clone, Debug)]
pub struct SentUnit {
    pub view: UnitView,
    /// latest observed anim time (restart detection)
    pub anim_t: f32,
    /// build pass that last saw it
    pub pass: u64,
}

/// A building the recipient knows (visible now or remembered as a ghost).
#[derive(Clone, Debug)]
pub struct KnownBuilding {
    pub view: BuildingView,
    /// visible at the last build
    pub visible: bool,
    /// tick of the last send
    pub sent_tick: u64,
}

/// Per-recipient memory of what was already sent (delta encoding).
#[derive(Clone, Debug, Default)]
pub struct ViewerCache {
    /// at least one snapshot sent since (re)join
    pub started: bool,
    /// build counter (unit liveness marks)
    pub pass: u64,
    pub explored_sent: BitGrid,
    /// last amount the recipient knows per node (fogged changes are held back)
    pub nodes_sent: Vec<f32>,
    pub points_sent: Vec<Option<PointView>>,
    pub units_sent: BTreeMap<UnitId, SentUnit>,
    /// sheep rows last sent + build pass that saw them
    pub sheep_sent: BTreeMap<SheepId, (SheepView, u64)>,
    /// pickup rows last sent + build pass that saw them
    pub pickups_sent: BTreeMap<PickupId, (PickupView, u64)>,
    pub buildings_sent: BTreeMap<BuildingId, KnownBuilding>,
    pub factions_sent: Vec<PublicFaction>,
    pub economy_sent: Option<EconomyView>,
}

impl ViewerCache {
    /// Forget everything (next snapshot is full) — new match.
    pub fn reset(&mut self) {
        *self = ViewerCache::default();
    }

    /// (Re)join of the same recipient: the client mirror restarts from scratch, so the next snapshot is full, but
    /// the recipient's knowledge of the world (enemy building ghosts, last-seen node amounts and points) is kept
    /// and re-sent.
    pub fn resync(&mut self) {
        self.started = false;
        self.units_sent.clear();
        self.sheep_sent.clear();
        self.pickups_sent.clear();
        self.factions_sent.clear();
        self.economy_sent = None;
        self.explored_sent = BitGrid::default();
    }
}

#[inline]
fn q4(v: f32) -> f32 {
    quant(v, 4.0)
}
#[inline]
fn q10(v: f32) -> f32 {
    quant(v, 10.0)
}
#[inline]
fn q100(v: f32) -> f32 {
    quant(v, 100.0)
}

fn unit_view(id: UnitId, u: &Unit, own: bool) -> UnitView {
    let mut flags = 0;
    if u.flip {
        flags |= UF_FLIP;
    }
    if u.hurt_t > 0.0 {
        flags |= UF_HURT;
    }
    if u.inside.is_some() {
        flags |= UF_INSIDE;
    }
    if u.moving {
        flags |= UF_MOVING;
    }
    UnitView {
        id,
        kind: u.kind,
        faction: u.faction,
        x: q4(u.pos.x),
        y: q4(u.pos.y),
        hp: u.hp.max(0.0).ceil().min(u16::MAX as f32) as u16,
        max_hp: u.max_hp.max(0.0).min(u16::MAX as f32) as u16,
        anim: u.anim,
        dir: u.anim_dir,
        anim_t: q100(u.anim_t),
        flags,
        carry: u.carry.map(|c| c.res),
        tool: u.tool,
        act: if own { u.act } else { Act::Idle },
        job: if own { u.job } else { None },
        faith: u.faith.max(0.0).floor().min(255.0) as u8,
    }
}

/// Same unit row apart from the position and the anim time?
fn same_unit_but_pos(a: &UnitView, b: &UnitView) -> bool {
    a.kind == b.kind
        && a.faction == b.faction
        && a.hp == b.hp
        && a.max_hp == b.max_hp
        && a.anim == b.anim
        && a.dir == b.dir
        && a.flags == b.flags
        && a.carry == b.carry
        && a.tool == b.tool
        && a.act == b.act
        && a.job == b.job
        && a.faith == b.faith
}

fn building_view(w: &World, id: BuildingId, b: &Building, own: bool) -> BuildingView {
    BuildingView {
        id,
        kind: b.kind,
        faction: b.faction,
        cx: b.cx,
        cy: b.cy,
        variant: b.variant,
        hp: b.hp.max(0.0).round().min(u16::MAX as f32) as u16,
        max_hp: b.max_hp.max(0.0).min(u16::MAX as f32) as u16,
        done: b.done,
        progress: q100(b.progress),
        queue: if own {
            b.queue
                .iter()
                .map(|q| QueueView {
                    what: q.what,
                    t: q100(q.t),
                    total: q100(q.total),
                })
                .collect()
        } else {
            Vec::new()
        },
        train_speed: q100(w.train_speed_of(b.faction, b.kind)),
        rally: if own {
            Some(RallyView {
                x: q4(b.rally.pos.x),
                y: q4(b.rally.pos.y),
                target: b.rally.target.map(|t| match t {
                    RallyTarget::Gather(GatherTarget::Node(n)) => EntityRef::Node(n),
                    RallyTarget::Gather(GatherTarget::Sheep(s)) => EntityRef::Sheep(s),
                    RallyTarget::Building(b) => EntityRef::Building(b),
                }),
            })
        } else {
            None
        },
        sheltered: if own {
            b.inside.len().min(255) as u8
        } else {
            0
        },
    }
}

/// Differences between two rows of the same building: (structural, timers only).
fn building_diff(a: &BuildingView, b: &BuildingView) -> (bool, bool) {
    let structural = a.kind != b.kind
        || a.faction != b.faction
        || a.cx != b.cx
        || a.cy != b.cy
        || a.variant != b.variant
        || a.hp != b.hp
        || a.max_hp != b.max_hp
        || a.done != b.done
        || a.train_speed != b.train_speed
        || a.rally != b.rally
        || a.sheltered != b.sheltered
        || a.queue.len() != b.queue.len()
        || a.queue.iter().zip(&b.queue).any(|(x, y)| x.what != y.what || x.total != y.total);
    let timers =
        a.progress != b.progress || a.queue.iter().zip(&b.queue).any(|(x, y)| x.t != y.t);
    (structural, timers)
}

/// Any cell of the `w × h` rectangle at `(cx, cy)` visible to `f` (raw visible bits).
fn rect_visible(w: &World, f: FactionIdx, cx: i32, cy: i32, rw: i32, rh: i32) -> bool {
    let vis = &w.factions[f as usize].fog.visible;
    for y in cy..cy + rh {
        for x in cx..cx + rw {
            if w.map.inside(x, y) && vis.get(w.map.idx(x, y) as usize) {
                return true;
            }
        }
    }
    false
}

/// Public kingdom list.
pub fn public_factions(w: &World) -> Vec<PublicFaction> {
    w.factions
        .iter()
        .map(|f| PublicFaction {
            idx: f.idx,
            color: f.color,
            alive: f.alive,
            control: if f.human && f.connected {
                Control::Human
            } else if f.owned {
                Control::Disconnected
            } else {
                Control::Ai
            },
            player_name: f.player_name.clone(),
            kills: f.kills,
            razed: f.razed,
            age: f.age,
        })
        .collect()
}

/// Own economy view.
pub fn economy_view(w: &World, f: FactionIdx) -> EconomyView {
    let fac = w.faction(f);
    EconomyView {
        res: fac.res.floored(),
        pawn_cap: fac.pawn_cap,
        troop_cap: fac.troop_cap,
        pawn_n: fac.pawn_n,
        troop_n: fac.troop_n,
        age: fac.age,
        techs: fac.techs.0,
        busy: fac.busy.0,
        mods: fac.mods,
        trades: fac.trades.map(|t| q100(t as f32)),
        rates: fac.rate,
        has: fac.has,
        sites: fac.sites,
        kills: fac.kills,
        razed: fac.razed,
        gathered: q10(fac.gathered as f32),
        level: w.setup.level,
    }
}

/// Builds this tick's snapshot for one recipient. `events` = the tick's drained `World.events` (plus any held
/// back while the recipient's connection was congested).
pub fn build_snapshot(
    w: &World,
    viewer: Viewer,
    cache: &mut ViewerCache,
    events: &[GameEvent],
) -> Snapshot {
    let me = match viewer {
        Viewer::Player(f) if (f as usize) < w.factions.len() => Some(f),
        _ => None,
    };
    let fog_off = me.is_none_or(|f| w.faction(f).fog_off) || w.outcome.is_some();
    let visible_cell = |c: i32| -> bool {
        fog_off
            || (c >= 0
                && me.is_some_and(|f| w.factions[f as usize].fog.visible.get(c as usize)))
    };
    let vis = |p: crate::math::V2| visible_cell(w.map.cell_of(p));
    let full = !cache.started;
    cache.started = true;
    cache.pass = cache.pass.wrapping_add(1);
    let pass = cache.pass;

    // ---- units: row deltas ----
    let mut units = Vec::new();
    let mut unit_moves = Vec::new();
    for (id, u) in w.units.iter() {
        if u.dead {
            continue;
        }
        let own = Some(u.faction) == me;
        if !own && (u.inside.is_some() || !vis(u.pos)) {
            continue;
        }
        let view = unit_view(id, u, own);
        match cache.units_sent.get_mut(&id) {
            Some(s) => {
                let restarted = view.anim_t + 0.001 < s.anim_t;
                if full || restarted || !same_unit_but_pos(&s.view, &view) {
                    units.push(view.clone());
                    s.view = view;
                } else if s.view.x != view.x || s.view.y != view.y {
                    unit_moves.push(UnitMove {
                        id,
                        dx: ((view.x - s.view.x) * 4.0).round() as i32,
                        dy: ((view.y - s.view.y) * 4.0).round() as i32,
                    });
                    s.view.x = view.x;
                    s.view.y = view.y;
                }
                s.anim_t = q100(u.anim_t);
                s.pass = pass;
            }
            None => {
                cache.units_sent.insert(
                    id,
                    SentUnit {
                        anim_t: view.anim_t,
                        view: view.clone(),
                        pass,
                    },
                );
                units.push(view);
            }
        }
    }
    let mut units_gone = Vec::new();
    cache.units_sent.retain(|id, s| {
        let keep = s.pass == pass;
        if !keep {
            units_gone.push(*id);
        }
        keep
    });

    // ---- buildings: deltas, ghosts, gone ----
    let mut buildings = Vec::new();
    let mut buildings_hidden = Vec::new();
    let mut buildings_gone = Vec::new();
    let tick = w.tick;
    for (id, b) in w.buildings.iter() {
        if b.dead {
            continue;
        }
        let own = Some(b.faction) == me;
        let d = b.def();
        let seen = own || fog_off || me.is_some_and(|f| rect_visible(w, f, b.cx, b.cy, d.w, d.h));
        if !seen {
            continue;
        }
        let view = building_view(w, id, b, own);
        match cache.buildings_sent.get_mut(&id) {
            Some(k) => {
                let (structural, timers) = building_diff(&k.view, &view);
                let send = full
                    || !k.visible
                    || structural
                    || (timers && tick.saturating_sub(k.sent_tick) >= TIMER_RESEND);
                k.visible = true;
                if send {
                    buildings.push(view.clone());
                    k.view = view;
                    k.sent_tick = tick;
                }
            }
            None => {
                buildings.push(view.clone());
                cache.buildings_sent.insert(
                    id,
                    KnownBuilding {
                        view,
                        visible: true,
                        sent_tick: tick,
                    },
                );
            }
        }
    }
    // known buildings not seen this tick: hidden (ghost), or gone once their remembered spot is visible
    let mut drop_ids = Vec::new();
    for (id, k) in cache.buildings_sent.iter_mut() {
        let current = w.buildings.get(*id).filter(|b| !b.dead);
        let seen_now = current.is_some_and(|b| {
            Some(b.faction) == me
                || fog_off
                || me.is_some_and(|f| {
                    let d = b.def();
                    rect_visible(w, f, b.cx, b.cy, d.w, d.h)
                })
        });
        if seen_now {
            continue;
        }
        let d = k.view.kind.def();
        let spot_seen = match (current, me) {
            // the owner (or a fog-off viewer) knows at once
            (None, _) if fog_off => true,
            (_, Some(f)) => {
                k.view.faction == f || rect_visible(w, f, k.view.cx, k.view.cy, d.w, d.h)
            }
            (_, None) => true,
        };
        if spot_seen {
            if !full {
                buildings_gone.push(*id);
            }
            drop_ids.push(*id);
        } else if full {
            // the client forgot everything: re-send the ghost
            buildings.push(k.view.clone());
            buildings_hidden.push(*id);
            k.visible = false;
        } else if k.visible {
            buildings_hidden.push(*id);
            k.visible = false;
        }
    }
    for id in drop_ids {
        cache.buildings_sent.remove(&id);
    }

    // ---- resource nodes: visible changes (full: everything that differs from the generated amount) ----
    if cache.nodes_sent.len() != w.nodes.len() {
        cache.nodes_sent = w.nodes.iter().map(|n| q10(n.max)).collect();
    }
    let mut nodes = Vec::new();
    for (i, n) in w.nodes.iter().enumerate() {
        let amount = q10(n.amount);
        let known = cache.nodes_sent[i];
        let changed = amount != known
            && (fog_off
                || if n.big {
                    w.map.quad(n.cell).iter().any(|c| visible_cell(*c))
                } else {
                    visible_cell(n.cell)
                });
        if changed {
            cache.nodes_sent[i] = amount;
        }
        if changed || (full && cache.nodes_sent[i] != q10(n.max)) {
            nodes.push(NodeDelta {
                id: NodeId(i as u32),
                amount: cache.nodes_sent[i],
            });
        }
    }

    // ---- capture points: changes the recipient can see or that concern it ----
    if cache.points_sent.len() != w.points.len() {
        cache.points_sent = vec![None; w.points.len()];
    }
    let mut points = Vec::new();
    for (i, p) in w.points.iter().enumerate() {
        let view = PointView {
            id: PointId(i as u32),
            kind: p.kind,
            owner: p.owner,
            by: p.by,
            t: q100(p.t),
        };
        let prev = cache.points_sent[i];
        let concerns = |v: Option<FactionIdx>| v.is_some() && v == me;
        let relevant = fog_off
            || prev.is_none()
            || concerns(p.owner)
            || concerns(p.by)
            || prev.is_some_and(|q| concerns(q.owner) || concerns(q.by))
            || me.is_some_and(|f| {
                let (cx, cy) = (p.cell % crate::map::W, p.cell / crate::map::W);
                rect_visible(w, f, cx, cy, 2, 2)
            });
        let changed = prev != Some(view) && relevant;
        if changed {
            cache.points_sent[i] = Some(view);
        }
        if changed || full {
            if let Some(v) = cache.points_sent[i] {
                points.push(v);
            }
        }
    }

    // ---- sheep: row deltas ----
    let mut sheep = Vec::new();
    let mut sheep_moves = Vec::new();
    for (id, s) in w.sheep.iter() {
        if s.dead || !vis(s.pos) {
            continue;
        }
        let view = SheepView {
            id,
            x: q4(s.pos.x),
            y: q4(s.pos.y),
            state: s.state,
            flip: s.flip,
        };
        match cache.sheep_sent.get_mut(&id) {
            Some(e) => {
                if full || e.0.state != view.state || e.0.flip != view.flip {
                    sheep.push(view);
                    e.0 = view;
                } else if e.0 != view {
                    sheep_moves.push(SheepMove {
                        id,
                        dx: ((view.x - e.0.x) * 4.0).round() as i32,
                        dy: ((view.y - e.0.y) * 4.0).round() as i32,
                    });
                    e.0 = view;
                }
                e.1 = pass;
            }
            None => {
                cache.sheep_sent.insert(id, (view, pass));
                sheep.push(view);
            }
        }
    }
    let mut sheep_gone = Vec::new();
    cache.sheep_sent.retain(|id, e| {
        let keep = e.1 == pass;
        if !keep {
            sheep_gone.push(*id);
        }
        keep
    });

    // ---- projectiles / pickups: complete visible lists ----
    let projectiles = w
        .projectiles
        .iter()
        .filter(|(_, p)| !p.dead && (Some(p.faction) == me || vis(p.pos)))
        .map(|(id, p)| {
            let rock = p.kind == ProjKind::Rock;
            let z = if rock && p.dur > 0.0 {
                let k = (p.t / p.dur).clamp(0.0, 1.0);
                (k * core::f32::consts::PI).sin() * 150.0
            } else {
                0.0
            };
            ProjView {
                id,
                x: q4(p.pos.x),
                y: q4(p.pos.y),
                ang: q100(p.ang),
                faction: p.faction,
                rock,
                z: q4(z),
            }
        })
        .collect();
    let mut pickups = Vec::new();
    for (id, p) in w.pickups.iter() {
        if !vis(p.pos) {
            continue;
        }
        let view = PickupView {
            id,
            res: p.res,
            x: q4(p.pos.x),
            y: q4(p.pos.y),
            hop: q4(p.hop),
            fading: p.life < 5.0,
            chest: p.chest,
        };
        match cache.pickups_sent.get_mut(&id) {
            Some(e) => {
                if full || e.0 != view {
                    pickups.push(view);
                    e.0 = view;
                }
                e.1 = pass;
            }
            None => {
                cache.pickups_sent.insert(id, (view, pass));
                pickups.push(view);
            }
        }
    }
    let mut pickups_gone = Vec::new();
    cache.pickups_sent.retain(|id, e| {
        let keep = e.1 == pass;
        if !keep {
            pickups_gone.push(*id);
        }
        keep
    });

    // ---- explored cells ----
    let explored = match me {
        Some(f) if !fog_off || full => {
            let grid = &w.faction(f).fog.explored;
            if full || cache.explored_sent.words.len() != grid.words.len() {
                cache.explored_sent = grid.clone();
                ExploredDelta::Full(grid.clone())
            } else {
                let mut cells = Vec::new();
                let mut many = false;
                for (wi, (a, b)) in grid.words.iter().zip(&cache.explored_sent.words).enumerate() {
                    let mut new = *a & !*b;
                    while new != 0 && !many {
                        let bit = new.trailing_zeros();
                        cells.push((wi * 64) as u32 + bit);
                        new &= new - 1;
                        many = cells.len() > EXPLORED_FULL_ABOVE;
                    }
                    if many {
                        break;
                    }
                }
                if many {
                    cache.explored_sent = grid.clone();
                    ExploredDelta::Full(grid.clone())
                } else if cells.is_empty() {
                    ExploredDelta::None
                } else {
                    for c in &cells {
                        cache.explored_sent.set(*c as usize);
                    }
                    ExploredDelta::Cells(cells)
                }
            }
        }
        _ => ExploredDelta::None,
    };

    // ---- kingdoms / economy (when changed) ----
    let pub_f = public_factions(w);
    let factions = if full || pub_f != cache.factions_sent {
        cache.factions_sent = pub_f.clone();
        pub_f
    } else {
        Vec::new()
    };
    let economy = me.and_then(|f| {
        let e = economy_view(w, f);
        if full || cache.economy_sent.as_ref() != Some(&e) {
            cache.economy_sent = Some(e.clone());
            Some(e)
        } else {
            None
        }
    });

    // ---- events: private ones only to their recipient, public positional ones only where visible ----
    let events = events
        .iter()
        .filter(|e| {
            e.is_for(me)
                && match e {
                    GameEvent::Fx { x, y, .. } | GameEvent::Sound { x, y, .. } => {
                        vis(crate::math::v2(*x, *y))
                    }
                    _ => true,
                }
        })
        .cloned()
        .collect();

    Snapshot {
        tick: w.tick,
        time: w.time,
        you: me,
        full,
        economy,
        factions,
        units,
        unit_moves,
        units_gone,
        buildings,
        buildings_hidden,
        buildings_gone,
        nodes,
        points,
        sheep,
        sheep_moves,
        sheep_gone,
        projectiles,
        pickups,
        pickups_gone,
        explored,
        events,
        frozen: w.frozen,
        fog_off,
        outcome: w.outcome,
    }
}

/// Max events kept in a mirror nobody drains.
const MIRROR_MAX_EVENTS: usize = 1024;

impl ClientMirror {
    /// Applies one snapshot (deltas against the previous ones; `full` resets the mirror first).
    pub fn apply(&mut self, s: &Snapshot) {
        if s.full {
            let amounts = core::mem::take(&mut self.node_amounts);
            let events = core::mem::take(&mut self.events);
            *self = ClientMirror::new(s.you, amounts);
            self.events = events;
        }
        self.you = s.you;
        self.tick = s.tick;
        self.time = s.time;
        if s.you.is_none() {
            self.economy = None;
        } else if let Some(e) = &s.economy {
            let mut e = e.clone();
            e.mods = Mods::from_techs(TechSet(e.techs));
            self.economy = Some(e);
        }
        if !s.factions.is_empty() {
            self.factions = s.factions.clone();
        }
        for u in &s.units {
            self.units.insert(u.id, u.clone());
        }
        for m in &s.unit_moves {
            if let Some(u) = self.units.get_mut(&m.id) {
                u.x += m.dx as f32 / 4.0;
                u.y += m.dy as f32 / 4.0;
            }
        }
        for id in &s.units_gone {
            self.units.remove(id);
        }
        for b in &s.buildings {
            self.buildings.insert(b.id, b.clone());
            self.ghosts.remove(&b.id);
        }
        for id in &s.buildings_hidden {
            if self.buildings.contains_key(id) {
                self.ghosts.insert(*id);
            }
        }
        for id in &s.buildings_gone {
            self.buildings.remove(id);
            self.ghosts.remove(id);
        }
        for n in &s.nodes {
            if let Some(a) = self.node_amounts.get_mut(n.id.0 as usize) {
                *a = n.amount;
            }
        }
        for p in &s.points {
            match self.points.binary_search_by_key(&p.id, |q| q.id) {
                Ok(i) => self.points[i] = *p,
                Err(i) => self.points.insert(i, *p),
            }
        }
        for v in &s.sheep {
            self.sheep.insert(v.id, *v);
        }
        for m in &s.sheep_moves {
            if let Some(v) = self.sheep.get_mut(&m.id) {
                v.x += m.dx as f32 / 4.0;
                v.y += m.dy as f32 / 4.0;
            }
        }
        for id in &s.sheep_gone {
            self.sheep.remove(id);
        }
        self.projectiles.clone_from(&s.projectiles);
        for p in &s.pickups {
            match self.pickups.binary_search_by_key(&p.id, |q| q.id) {
                Ok(i) => self.pickups[i] = *p,
                Err(i) => self.pickups.insert(i, *p),
            }
        }
        if !s.pickups_gone.is_empty() {
            self.pickups.retain(|p| !s.pickups_gone.contains(&p.id));
        }
        match &s.explored {
            ExploredDelta::None => {}
            ExploredDelta::Cells(c) => {
                if self.explored.len == 0 {
                    self.explored = BitGrid::for_map();
                }
                for i in c {
                    self.explored.set(*i as usize);
                }
            }
            ExploredDelta::Full(g) => self.explored = g.clone(),
        }
        self.events.extend(s.events.iter().cloned());
        if self.events.len() > MIRROR_MAX_EVENTS {
            let extra = self.events.len() - MIRROR_MAX_EVENTS;
            self.events.drain(..extra);
        }
        self.frozen = s.frozen;
        self.fog_off = s.fog_off;
        self.outcome = s.outcome;
    }

    /// Is this building a last-seen ghost (not currently visible)?
    pub fn is_ghost(&self, id: BuildingId) -> bool {
        self.ghosts.contains(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Msg;
    use crate::math::v2;
    use crate::proto::{self, ServerMsg};
    use crate::world::{MatchSetup, SetupPlayer};

    fn world(seed: u32) -> World {
        let setup = MatchSetup {
            players: vec![SetupPlayer {
                color: KingdomColor::Blue,
                name: "Ada".into(),
            }],
            level: AiLevel::Normal,
            solo: false,
        };
        World::new(seed, &setup)
    }

    fn me(w: &World) -> FactionIdx {
        w.factions.iter().find(|f| f.human).unwrap().idx
    }

    fn show_cell(w: &mut World, f: FactionIdx, p: crate::math::V2, on: bool) {
        let c = w.map.cell_of(p) as usize;
        if on {
            w.factions[f as usize].fog.visible.set(c);
        } else {
            w.factions[f as usize].fog.visible.unset(c);
        }
    }

    /// Wire round trip (quantization included).
    fn wire(s: Snapshot) -> Snapshot {
        let b = proto::encode(&ServerMsg::Snapshot(Box::new(s)));
        match proto::decode::<ServerMsg>(&b).unwrap() {
            ServerMsg::Snapshot(s) => *s,
            _ => unreachable!(),
        }
    }

    #[test]
    fn fog_filters_enemy_units_and_events() {
        let mut w = world(11);
        let f = me(&w);
        for fac in w.factions.iter_mut() {
            fac.fog.visible.clear();
        }
        let mut cache = ViewerCache::default();
        let s = build_snapshot(&w, Viewer::Player(f), &mut cache, &[]);
        assert!(s.full);
        assert!(s.units.iter().all(|u| u.faction == f), "only own units");
        assert_eq!(s.units.len(), 5);
        assert!(s.buildings.iter().all(|b| b.faction == f));
        assert!(s.economy.is_some() && s.factions.len() == 5);
        // an enemy unit becomes visible → sent; hidden again → gone
        let (eid, epos) = w
            .units
            .iter()
            .find(|(_, u)| u.faction != f)
            .map(|(id, u)| (id, u.pos))
            .unwrap();
        show_cell(&mut w, f, epos, true);
        let cell = w.map.cell_of(epos);
        let mut there: Vec<UnitId> = w
            .units
            .iter()
            .filter(|(_, u)| u.faction != f && w.map.cell_of(u.pos) == cell)
            .map(|(id, _)| id)
            .collect();
        there.sort();
        let s = build_snapshot(&w, Viewer::Player(f), &mut cache, &[]);
        assert!(!s.full);
        assert_eq!(s.units.iter().map(|u| u.id).collect::<Vec<_>>(), there);
        assert!(there.contains(&eid));
        assert!(s.economy.is_none() && s.factions.is_empty(), "unchanged");
        assert!(s.units.iter().all(|u| u.act == Act::Idle && u.job.is_none()));
        let s = build_snapshot(&w, Viewer::Player(f), &mut cache, &[]);
        assert!(s.units.is_empty() && s.units_gone.is_empty(), "nothing changed");
        show_cell(&mut w, f, epos, false);
        let s = build_snapshot(&w, Viewer::Player(f), &mut cache, &[]);
        assert_eq!(s.units_gone, there);
        // events: private to someone else and invisible public fx are filtered
        let other = (f + 1) % 5;
        let evs = vec![
            GameEvent::Toast {
                to: other,
                msg: Msg::PawnsMax,
            },
            GameEvent::Toast {
                to: f,
                msg: Msg::NeedHouses,
            },
            GameEvent::fx(FxKind::Dust1, epos.x, epos.y),
            GameEvent::Say {
                to: None,
                msg: Msg::DestroyEnemyCastles,
                color: None,
                dur: 3.0,
            },
        ];
        let s = build_snapshot(&w, Viewer::Player(f), &mut cache, &evs);
        assert_eq!(s.events.len(), 2);
        assert!(s.events.iter().all(|e| !matches!(e, GameEvent::Fx { .. })));
        assert!(s.events.iter().all(|e| e.is_for(Some(f))));
        // spectator: everything public, nothing private
        let mut sc = ViewerCache::default();
        let s = build_snapshot(&w, Viewer::Spectator, &mut sc, &evs);
        assert_eq!(s.units.len(), w.units.len());
        assert!(s.fog_off && s.you.is_none() && s.economy.is_none());
        assert_eq!(s.events.len(), 2);
    }

    #[test]
    fn building_ghosts_and_gone() {
        let mut w = world(12);
        let f = me(&w);
        for fac in w.factions.iter_mut() {
            fac.fog.visible.clear();
        }
        let (bid, cell) = w
            .buildings
            .iter()
            .find(|(_, b)| b.faction != f)
            .map(|(id, b)| (id, (b.cx, b.cy)))
            .unwrap();
        let spot = v2(cell.0 as f32 * 64.0 + 32.0, cell.1 as f32 * 64.0 + 32.0);
        let mut cache = ViewerCache::default();
        let mut mirror = ClientMirror::new(Some(f), vec![0.0; w.nodes.len()]);
        mirror.apply(&wire(build_snapshot(&w, Viewer::Player(f), &mut cache, &[])));
        assert!(!mirror.buildings.contains_key(&bid));
        show_cell(&mut w, f, spot, true);
        mirror.apply(&wire(build_snapshot(&w, Viewer::Player(f), &mut cache, &[])));
        assert!(mirror.buildings.contains_key(&bid) && !mirror.is_ghost(bid));
        show_cell(&mut w, f, spot, false);
        let s = build_snapshot(&w, Viewer::Player(f), &mut cache, &[]);
        assert_eq!(s.buildings_hidden, vec![bid]);
        mirror.apply(&wire(s));
        assert!(mirror.is_ghost(bid), "remembered as a ghost");
        // a reload (resync) re-sends the ghost
        cache.resync();
        let mut m2 = ClientMirror::new(Some(f), vec![0.0; w.nodes.len()]);
        let s = build_snapshot(&w, Viewer::Player(f), &mut cache, &[]);
        assert!(s.full);
        m2.apply(&wire(s));
        assert!(m2.is_ghost(bid));
        // destroyed while unseen: still a ghost until the spot is seen
        w.buildings.get_mut(bid).unwrap().dead = true;
        let s = build_snapshot(&w, Viewer::Player(f), &mut cache, &[]);
        assert!(s.buildings_gone.is_empty());
        show_cell(&mut w, f, spot, true);
        let s = build_snapshot(&w, Viewer::Player(f), &mut cache, &[]);
        assert_eq!(s.buildings_gone, vec![bid]);
        mirror.apply(&wire(s));
        assert!(!mirror.buildings.contains_key(&bid) && !mirror.is_ghost(bid));
    }

    #[test]
    fn mirror_tracks_world_and_sizes() {
        let mut w = world(5);
        let f = me(&w);
        // all-AI driving keeps the world busy
        w.factions[f as usize].human = false;
        let mut sc = ViewerCache::default();
        let mut pc = ViewerCache::default();
        let mut sm = ClientMirror::new(None, w.nodes.iter().map(|n| n.max).collect());
        let mut pm = ClientMirror::new(Some(f), w.nodes.iter().map(|n| n.max).collect());
        let mut delta_bytes = (0usize, 0usize, 0usize);
        let ticks = 1200;
        for t in 0..ticks {
            w.step(&[]);
            let ev = w.drain_events();
            let s = build_snapshot(&w, Viewer::Spectator, &mut sc, &ev);
            let b = proto::encode(&ServerMsg::Snapshot(Box::new(s.clone())));
            if t > 0 {
                delta_bytes.0 += b.len();
                delta_bytes.1 = delta_bytes.1.max(b.len());
            }
            if t == ticks - 1 {
                println!(
                    "last spectator delta: units {} ({} B), unit moves {} ({} B), sheep {} + {} moves ({} B), \
                     buildings {} ({} B), pickups {}, events {} ({} B)",
                    s.units.len(),
                    proto::encode(&s.units).len(),
                    s.unit_moves.len(),
                    proto::encode(&s.unit_moves).len(),
                    s.sheep.len(),
                    s.sheep_moves.len(),
                    proto::encode(&(&s.sheep, &s.sheep_moves)).len(),
                    s.buildings.len(),
                    proto::encode(&s.buildings).len(),
                    s.pickups.len(),
                    s.events.len(),
                    proto::encode(&s.events).len(),
                );
            }
            sm.apply(&wire(s));
            let s = build_snapshot(&w, Viewer::Player(f), &mut pc, &ev);
            if t > 0 {
                delta_bytes.2 += proto::encode(&ServerMsg::Snapshot(Box::new(s.clone()))).len();
            }
            pm.apply(&wire(s));
        }
        // the spectator mirror matches the world exactly (positions to the wire precision)
        let alive: Vec<UnitId> = w
            .units
            .iter()
            .filter(|(_, u)| !u.dead)
            .map(|(id, _)| id)
            .collect();
        assert_eq!(sm.units.keys().copied().collect::<Vec<_>>(), alive);
        for (id, u) in w.units.iter() {
            let m = &sm.units[&id];
            assert!((m.x - u.pos.x).abs() <= 0.125 && (m.y - u.pos.y).abs() <= 0.125);
            assert_eq!(m.hp, u.hp.max(0.0).ceil() as u16);
        }
        let blds: Vec<BuildingId> = w
            .buildings
            .iter()
            .filter(|(_, b)| !b.dead)
            .map(|(id, _)| id)
            .collect();
        assert_eq!(sm.buildings.keys().copied().collect::<Vec<_>>(), blds);
        for (i, n) in w.nodes.iter().enumerate() {
            assert!((sm.node_amounts[i] - n.amount).abs() <= 0.05);
        }
        assert_eq!(sm.points.len(), w.points.len());
        let sheep: Vec<SheepId> = w
            .sheep
            .iter()
            .filter(|(_, s)| !s.dead)
            .map(|(id, _)| id)
            .collect();
        assert_eq!(sm.sheep.keys().copied().collect::<Vec<_>>(), sheep);
        for (id, s) in w.sheep.iter().filter(|(_, s)| !s.dead) {
            assert!((sm.sheep[&id].x - s.pos.x).abs() <= 0.125);
        }
        let picks: Vec<PickupId> = w.pickups.iter().map(|(id, _)| id).collect();
        assert_eq!(sm.pickups.iter().map(|p| p.id).collect::<Vec<_>>(), picks);
        let own = w.units.iter().filter(|(_, u)| u.faction == f && !u.dead).count();
        assert_eq!(pm.units.values().filter(|u| u.faction == f).count(), own);
        assert_eq!(pm.economy.as_ref().unwrap().mods, w.faction(f).mods);
        // sizes
        let mut fresh = ViewerCache::default();
        let full_spec = proto::encode(&ServerMsg::Snapshot(Box::new(build_snapshot(
            &w,
            Viewer::Spectator,
            &mut fresh,
            &[],
        ))))
        .len();
        let mut fresh = ViewerCache::default();
        let full_player = proto::encode(&ServerMsg::Snapshot(Box::new(build_snapshot(
            &w,
            Viewer::Player(f),
            &mut fresh,
            &[],
        ))))
        .len();
        let n = (ticks - 1) as usize;
        println!(
            "snapshot sizes after {ticks} ticks ({} units, {} buildings): full spectator {full_spec} B, \
             full player {full_player} B, delta spectator avg {} B (max {} B), delta player avg {} B",
            alive.len(),
            blds.len(),
            delta_bytes.0 / n,
            delta_bytes.1,
            delta_bytes.2 / n
        );
        assert!(delta_bytes.0 / n < 16 * 1024, "spectator deltas stay small");
    }

    /// A crowded battle: 5 armies of 60 marching to the centre. Logs bytes per snapshot (spectator = worst case,
    /// everything visible; player = own army + whatever its fog shows).
    #[test]
    fn crowded_snapshot_sizes() {
        let mut w = world(9);
        let f = me(&w);
        w.factions[f as usize].human = false;
        let centre = w.map.nearest_passable(v2(
            crate::MAP_W as f32 * 32.0,
            crate::MAP_H as f32 * 32.0,
        ));
        let kinds = [
            UnitKind::Warrior,
            UnitKind::Archer,
            UnitKind::Lancer,
            UnitKind::Knight,
            UnitKind::Pawn,
        ];
        let mut cmds = Vec::new();
        for fi in 0..w.factions.len() as FactionIdx {
            let castle = w.factions[fi as usize].castle.unwrap();
            let base = w.buildings.get(castle).unwrap().pos;
            let mut ids = Vec::new();
            for i in 0..60 {
                let p = w.map.nearest_passable(base + v2((i % 10) as f32 * 20.0 - 100.0, 80.0 + (i / 10) as f32 * 20.0));
                ids.push(w.spawn_unit(fi, kinds[i % kinds.len()], p));
            }
            cmds.push((
                fi,
                crate::cmd::Command::Order {
                    units: ids,
                    kind: crate::cmd::OrderKind::AMove,
                    x: centre.x,
                    y: centre.y,
                    target: None,
                },
            ));
        }
        w.step(&cmds);
        let mut sc = ViewerCache::default();
        let mut pc = ViewerCache::default();
        let (mut spec, mut spec_max, mut player, mut n) = (0usize, 0usize, 0usize, 0usize);
        let mut first = (0, 0);
        for t in 0..200 {
            w.step(&[]);
            let ev = w.drain_events();
            let a = proto::encode(&ServerMsg::Snapshot(Box::new(build_snapshot(
                &w,
                Viewer::Spectator,
                &mut sc,
                &ev,
            ))))
            .len();
            let b = proto::encode(&ServerMsg::Snapshot(Box::new(build_snapshot(
                &w,
                Viewer::Player(f),
                &mut pc,
                &ev,
            ))))
            .len();
            if t == 0 {
                first = (a, b);
            } else {
                spec += a;
                spec_max = spec_max.max(a);
                player += b;
                n += 1;
            }
        }
        let units = w.units.iter().filter(|(_, u)| !u.dead).count();
        println!(
            "crowded ({units} units): full spectator {} B, full player {} B; per tick: spectator avg {} B \
             (max {spec_max} B), player avg {} B",
            first.0,
            first.1,
            spec / n,
            player / n
        );
        assert!(spec / n < 24 * 1024);
    }
}
