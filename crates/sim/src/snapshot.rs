// OWNER: S6 (net) — see docs/spec/08-net.md §A.7-A.8 (old snapshot content / client replication), §R6 (per-recipient
// fog filtering, deltas, ghosts, backpressure), addenda N2 (train speed on queue bars); 05 addenda C3 (ghosts);
// 04a §16 + addenda E7 (economy fields the UI needs); types in snapshot_types.rs (foundation).
//
// Stub fns S6 must fill:
//   * `build_snapshot` — MINIMAL now: own + visible units/sheep/projectiles/pickups, ALL known buildings every tick
//     (no change detection, no ghost/gone logic), node deltas vs. the cache, explored grid only in full snapshots.
//     Do: building diffs + `buildings_gone` when the recipient sees the spot empty, explored deltas (diff of
//     `fog.explored` vs `cache.explored_sent`), event fog culling for public positional events, spectators.
//   * `ClientMirror::apply` — MINIMAL: replace/upsert. Do: ghosts (buildings that left vision), interpolation
//     hooks for the client, explored deltas.
#![allow(dead_code, unused)]

use crate::World;
use crate::cmd::EntityRef;
use crate::defs::*;
use crate::events::GameEvent;
use crate::math::BitGrid;
use crate::slab::BuildingId;
use crate::snapshot_types::*;
use crate::world::{Building, RallyTarget, Unit};
use std::collections::BTreeMap;

/// Who a snapshot is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewer {
    /// a kingdom's player (fog applies unless test fog-off / match over)
    Player(FactionIdx),
    /// eliminated player / observer: sees everything
    Spectator,
}

/// Per-connection memory of what was already sent (delta encoding).
#[derive(Clone, Debug, Default)]
pub struct ViewerCache {
    /// at least one snapshot sent since (re)join
    pub started: bool,
    pub explored_sent: BitGrid,
    pub nodes_sent: Vec<f32>,
    pub buildings_sent: BTreeMap<BuildingId, BuildingView>,
}

impl ViewerCache {
    /// Forget everything (next snapshot is full) — on (re)join.
    pub fn reset(&mut self) {
        *self = ViewerCache::default();
    }
}

fn unit_view(id: crate::slab::UnitId, u: &Unit, own: bool) -> UnitView {
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
        x: u.pos.x,
        y: u.pos.y,
        hp: u.hp.max(0.0).ceil() as u16,
        max_hp: u.max_hp as u16,
        anim: u.anim,
        dir: u.anim_dir,
        anim_t: u.anim_t,
        flags,
        carry: u.carry.map(|c| c.res),
        tool: u.tool,
        act: if own { u.act } else { Act::Idle },
        job: if own { u.job } else { None },
        faith: u.faith.max(0.0).floor().min(255.0) as u8,
    }
}

fn building_view(w: &World, id: BuildingId, b: &Building, own: bool) -> BuildingView {
    BuildingView {
        id,
        kind: b.kind,
        faction: b.faction,
        cx: b.cx,
        cy: b.cy,
        variant: b.variant,
        hp: b.hp.max(0.0).round() as u16,
        max_hp: b.max_hp as u16,
        done: b.done,
        progress: b.progress,
        queue: if own {
            b.queue
                .iter()
                .map(|q| QueueView {
                    what: q.what,
                    t: q.t,
                    total: q.total,
                })
                .collect()
        } else {
            Vec::new()
        },
        train_speed: w.train_speed_of(b.faction, b.kind),
        rally: if own {
            Some(RallyView {
                x: b.rally.pos.x,
                y: b.rally.pos.y,
                target: b.rally.target.map(|t| match t {
                    RallyTarget::Gather(crate::world::GatherTarget::Node(n)) => EntityRef::Node(n),
                    RallyTarget::Gather(crate::world::GatherTarget::Sheep(s)) => {
                        EntityRef::Sheep(s)
                    }
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
        trades: fac.trades.map(|t| (t * 100.0).round() as f32 / 100.0),
        rates: fac.rate,
        has: fac.has,
        sites: fac.sites,
        kills: fac.kills,
        razed: fac.razed,
        gathered: fac.gathered as f32,
        level: w.setup.level,
    }
}

/// Builds this tick's snapshot for one recipient. `events` = the tick's drained `World.events`.
pub fn build_snapshot(
    w: &World,
    viewer: Viewer,
    cache: &mut ViewerCache,
    events: &[GameEvent],
) -> Snapshot {
    let me = match viewer {
        Viewer::Player(f) => Some(f),
        Viewer::Spectator => None,
    };
    let sees_all = me.is_none_or(|f| w.faction(f).fog_off) || w.outcome.is_some();
    let vis = |p: crate::math::V2| sees_all || me.is_some_and(|f| w.is_visible(f, p));
    let full = !cache.started;
    cache.started = true;

    let units = w
        .units
        .iter()
        .filter(|(_, u)| !u.dead && (Some(u.faction) == me || (u.inside.is_none() && vis(u.pos))))
        .map(|(id, u)| unit_view(id, u, Some(u.faction) == me))
        .collect();
    let buildings = w
        .buildings
        .iter()
        .filter(|(_, b)| !b.dead && (Some(b.faction) == me || vis(b.pos)))
        .map(|(id, b)| building_view(w, id, b, Some(b.faction) == me))
        .collect();
    if cache.nodes_sent.len() != w.nodes.len() {
        cache.nodes_sent = vec![f32::NAN; w.nodes.len()];
    }
    let mut nodes = Vec::new();
    for (i, n) in w.nodes.iter().enumerate() {
        if full || cache.nodes_sent[i] != n.amount {
            cache.nodes_sent[i] = n.amount;
            nodes.push(NodeDelta {
                id: crate::slab::NodeId(i as u32),
                amount: n.amount,
            });
        }
    }
    let points = w
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| PointView {
            id: crate::slab::PointId(i as u32),
            kind: p.kind,
            owner: p.owner,
            by: p.by,
            t: p.t,
        })
        .collect();
    let sheep = w
        .sheep
        .iter()
        .filter(|(_, s)| !s.dead && vis(s.pos))
        .map(|(id, s)| SheepView {
            id,
            x: s.pos.x,
            y: s.pos.y,
            state: s.state,
            flip: s.flip,
        })
        .collect();
    let projectiles = w
        .projectiles
        .iter()
        .filter(|(_, p)| vis(p.pos))
        .map(|(id, p)| ProjView {
            id,
            x: p.pos.x,
            y: p.pos.y,
            ang: p.ang,
            faction: p.faction,
            rock: p.kind == crate::world::ProjKind::Rock,
            z: 0.0,
        })
        .collect();
    let pickups = w
        .pickups
        .iter()
        .filter(|(_, p)| vis(p.pos))
        .map(|(id, p)| PickupView {
            id,
            res: p.res,
            x: p.pos.x,
            y: p.pos.y,
            hop: p.hop,
            fading: p.life < 5.0,
            chest: p.chest,
        })
        .collect();
    let explored = match me {
        Some(f) if full => {
            cache.explored_sent = w.faction(f).fog.explored.clone();
            ExploredDelta::Full(cache.explored_sent.clone())
        }
        _ => ExploredDelta::None,
    };
    Snapshot {
        tick: w.tick,
        time: w.time,
        you: me,
        full,
        economy: me.map(|f| economy_view(w, f)),
        factions: public_factions(w),
        units,
        buildings,
        buildings_gone: Vec::new(),
        nodes,
        points,
        sheep,
        projectiles,
        pickups,
        explored,
        events: events.iter().filter(|e| e.is_for(me)).cloned().collect(),
        frozen: w.frozen,
        fog_off: me.is_some_and(|f| w.faction(f).fog_off),
        outcome: w.outcome,
    }
}

impl ClientMirror {
    /// Applies one snapshot (S6). MINIMAL: full replace of per-tick lists, upsert of buildings.
    pub fn apply(&mut self, s: &Snapshot) {
        if s.full {
            let amounts = core::mem::take(&mut self.node_amounts);
            *self = ClientMirror::new(s.you, amounts);
        }
        self.you = s.you;
        self.tick = s.tick;
        self.time = s.time;
        self.economy = s.economy.clone();
        self.factions = s.factions.clone();
        self.units = s.units.iter().map(|u| (u.id, u.clone())).collect();
        for b in &s.buildings {
            self.buildings.insert(b.id, b.clone());
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
        self.points = s.points.clone();
        self.sheep = s.sheep.iter().map(|v| (v.id, *v)).collect();
        self.projectiles = s.projectiles.clone();
        self.pickups = s.pickups.clone();
        match &s.explored {
            ExploredDelta::None => {}
            ExploredDelta::Cells(c) => {
                for i in c {
                    self.explored.set(*i as usize);
                }
            }
            ExploredDelta::Full(g) => self.explored = g.clone(),
        }
        self.events.extend(s.events.iter().cloned());
        self.frozen = s.frozen;
        self.fog_off = s.fog_off;
        self.outcome = s.outcome;
    }
}
