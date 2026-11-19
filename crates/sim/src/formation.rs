// OWNER: S2 (units) — see docs/spec/03a-units-combat.md §5.2 (orderUnits incl. embolden for humans), §6 (formation /
// group move: lineOf, rows, LINE_GAP, pace, escort), §6.1 (march with the `fresh` budget), addenda U3, U4;
// 03b §3 (gather/build/drop/repair resume rules); 04a §12 `order` / `recall`.
//
// Implemented: order_units (every order kind of §5.2), formation slots (§6, `formation_slots` is pure and tested),
// march (§6.1).

use crate::World;
use crate::cmd::{EntityRef, OrderKind};
use crate::defs::*;
use crate::math::{V2, v2};
use crate::slab::UnitId;
use crate::world::{GatherTarget, MoveOrder, Order, Target};

/// One unit as seen by the formation planner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormationUnit {
    pub pos: V2,
    pub kind: UnitKind,
    pub speed: f32,
}

/// Planned order of one unit of a group move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormationSlot {
    /// index into the input slice
    pub unit: usize,
    /// amove for combatants of an attack-move, else move
    pub amove: bool,
    pub order: MoveOrder,
}

/// Formation of §6 (entities.js:363-385) around the already-snapped goal `p`. `snap` = `nearestPassable`.
/// Lines: 0 melee + rams (front, on the click point), 1 archers, 2 catapults, 3 non-combatants (rear); inside a
/// line units keep their lateral order; rows of `per = max(3, ceil(sqrt(n)*1.5))` spaced `LINE_GAP`.
pub fn formation_slots(
    units: &[FormationUnit],
    p: V2,
    amove: bool,
    mut snap: impl FnMut(V2) -> V2,
) -> Vec<FormationSlot> {
    let n = units.len();
    if n == 0 {
        return Vec::new();
    }
    let army = units.iter().any(|u| u.kind.def().has_dmg());
    let mut m = V2::ZERO;
    let mut pace = f32::INFINITY;
    for u in units {
        m.x += u.pos.x / n as f32;
        m.y += u.pos.y / n as f32;
        if u.kind.def().has_dmg() {
            pace = pace.min(u.speed);
        }
    }
    let far = p.dist(m);
    let (dx, dy) = if far > 1.0 {
        ((p.x - m.x) / far, (p.y - m.y) / far)
    } else {
        (0.0, 1.0)
    };
    let per = 3usize.max(((n as f32).sqrt() * 1.5).ceil() as usize);
    let mut lines: [Vec<usize>; 4] = Default::default();
    for (i, u) in units.iter().enumerate() {
        lines[u.kind.def().line() as usize].push(i);
    }
    let mut out = Vec::with_capacity(n);
    let mut back = 0.0f32;
    for line in lines.iter_mut() {
        if line.is_empty() {
            continue;
        }
        // lateral coordinate: who is on the left stays on the left (stable sort)
        line.sort_by(|&a, &b| {
            let ka = units[a].pos.x * -dy + units[a].pos.y * dx;
            let kb = units[b].pos.x * -dy + units[b].pos.y * dx;
            // JS comparator `ka - kb`: equal keys (incl. -0 vs +0) keep their order (stable sort)
            ka.partial_cmp(&kb).unwrap_or(core::cmp::Ordering::Equal)
        });
        let len = line.len();
        for (i, &ui) in line.iter().enumerate() {
            let row = i / per;
            let in_row = per.min(len - row * per);
            let side = ((i % per) as f32 - (in_row as f32 - 1.0) / 2.0) * LINE_GAP;
            let deep = back + row as f32 * LINE_GAP;
            let q = snap(v2(p.x - dy * side - dx * deep, p.y + dx * side - dy * deep));
            let d = units[ui].kind.def();
            out.push(FormationSlot {
                unit: ui,
                amove: amove && d.has_dmg(),
                order: MoveOrder {
                    to: q,
                    group: if n > 1 { Some(p) } else { None },
                    pace: if amove && army { pace } else { 0.0 },
                    escort: amove && army && d.is_healer(),
                    own: false,
                    auto: false,
                    fought: false,
                },
            });
        }
        back += len.div_ceil(per) as f32 * LINE_GAP + LINE_EXTRA;
    }
    out
}

impl World {
    /// `orderUnits(f, ids, kind, x, y, targetId)` (§5.2): filters own live units; humans → `embolden`;
    /// stop / explore / move+amove (formation §6) / attack / gather / build / drop / heal.
    pub fn order_units(
        &mut self,
        f: FactionIdx,
        ids: &[UnitId],
        kind: OrderKind,
        x: f32,
        y: f32,
        target: Option<EntityRef>,
    ) {
        let mut units: Vec<UnitId> = Vec::with_capacity(ids.len());
        for id in ids {
            if self
                .units
                .get(*id)
                .is_some_and(|u| !u.dead && u.faction == f)
                && !units.contains(id)
            {
                units.push(*id);
            }
        }
        if units.is_empty() {
            return;
        }
        if self.factions.get(f as usize).is_some_and(|fac| fac.human) {
            for &u in &units {
                self.embolden(u);
            }
        }
        match kind {
            OrderKind::Stop => {
                for u in units {
                    self.idle(u);
                }
                return;
            }
            OrderKind::Explore => {
                for u in units {
                    if self.units.get(u).is_some_and(|u| u.def().scout) {
                        self.set_order(
                            u,
                            Order::Explore {
                                goal: None,
                                bad: Vec::new(),
                            },
                        );
                    }
                }
                return;
            }
            OrderKind::Move | OrderKind::AMove => {
                if !(x.is_finite() && y.is_finite()) {
                    return;
                }
                let p = self.map.nearest_passable(v2(x, y));
                let fu: Vec<FormationUnit> = units
                    .iter()
                    .filter_map(|id| self.units.get(*id))
                    .map(|u| FormationUnit {
                        pos: u.pos,
                        kind: u.kind,
                        speed: u.speed,
                    })
                    .collect();
                let map = &self.map;
                let slots =
                    formation_slots(&fu, p, kind == OrderKind::AMove, |q| map.nearest_passable(q));
                for s in slots {
                    let o = if s.amove {
                        Order::AMove(s.order)
                    } else {
                        Order::Move(s.order)
                    };
                    self.set_order(units[s.unit], o);
                }
                return;
            }
            _ => {}
        }
        let Some(t) = target else { return };
        // `!t || t.dead`
        let alive = match t {
            EntityRef::Unit(u) => self.unit_alive(u),
            EntityRef::Building(b) => self.building_alive(b),
            EntityRef::Sheep(s) => self.sheep.get(s).is_some_and(|s| !s.dead),
            EntityRef::Node(n) => (n.0 as usize) < self.nodes.len(),
            EntityRef::Point(p) => (p.0 as usize) < self.points.len(),
        };
        if !alive {
            return;
        }
        for u in units {
            let Some(un) = self.units.get(u) else {
                continue;
            };
            let d = un.def();
            let pawn = un.kind == UnitKind::Pawn;
            let upos = un.pos;
            match (kind, t) {
                (OrderKind::Attack, EntityRef::Unit(_) | EntityRef::Building(_)) => {
                    let (tt, tf) = match t {
                        EntityRef::Unit(x) => (Target::Unit(x), self.units.get(x).map(|e| e.faction)),
                        EntityRef::Building(x) => (
                            Target::Building(x),
                            self.buildings.get(x).map(|b| b.faction),
                        ),
                        _ => continue,
                    };
                    if tf == Some(f) || tf.is_none() {
                        continue;
                    }
                    let is_b = matches!(tt, Target::Building(_));
                    if d.has_dmg() && (is_b || !d.only_buildings) {
                        self.set_order(u, Order::Attack { target: tt });
                    } else if d.is_healer() {
                        // monks follow the attackers
                        self.set_order(
                            u,
                            Order::Idle {
                                post: upos,
                                escort: true,
                            },
                        );
                    }
                }
                (OrderKind::Gather, EntityRef::Node(_) | EntityRef::Sheep(_)) if pawn => {
                    let g = match t {
                        EntityRef::Node(n) => GatherTarget::Node(n),
                        EntityRef::Sheep(s) => GatherTarget::Sheep(s),
                        _ => continue,
                    };
                    if self.node_valid(g)
                        && let Some(o) = self.gather_order(g)
                    {
                        self.set_order(u, o);
                    }
                }
                (OrderKind::Build, EntityRef::Building(b)) if pawn => {
                    let Some(bl) = self.buildings.get(b) else {
                        continue;
                    };
                    if bl.faction != f {
                        continue;
                    }
                    let done = bl.done;
                    // whoever leaves a gathering job for a site resumes it afterwards
                    let resume = match un.order.job_order() {
                        Order::Gather(g) => Some(*g),
                        Order::Build { resume, .. } | Order::Repair { resume, .. } => *resume,
                        _ => None,
                    };
                    let o = if done {
                        Order::Repair {
                            building: b,
                            resume,
                        }
                    } else {
                        Order::Build {
                            building: b,
                            resume,
                        }
                    };
                    self.set_order(u, o);
                }
                (OrderKind::Drop, EntityRef::Building(b)) if pawn => {
                    if self
                        .buildings
                        .get(b)
                        .is_some_and(|bl| bl.faction == f && bl.done)
                    {
                        self.set_order(u, Order::Drop { building: b });
                    }
                }
                (OrderKind::Heal, EntityRef::Unit(tu))
                    if un.kind == UnitKind::Monk
                        && tu != u
                        && self.units.get(tu).is_some_and(|e| e.faction == f) =>
                {
                    self.set_order(u, Order::Heal { target: tu });
                }
                _ => {}
            }
        }
    }

    /// `march(u, o, dt, near)` (§6.1): pace, shared group goal until `fields.take_fresh()` or arrival, then own
    /// slot. Returns done (arrived at / stalled near the own slot).
    pub fn march(&mut self, id: UnitId, near: f32) -> bool {
        let Some(u) = self.units.get(id) else {
            return true;
        };
        let m = match &u.order {
            Order::Move(m) | Order::AMove(m) => *m,
            _ => return true,
        };
        let own_speed = u.speed;
        if m.pace > 0.0
            && let Some(u) = self.units.get_mut(id)
        {
            u.speed = own_speed.min(m.pace);
        }
        let mut done = false;
        match m.group {
            Some(g) if !m.own => {
                // walk toward the common goal until this unit's turn to compute its own slot path
                if (self.fields.take_fresh() || self.travel(id, g, TRAVEL_NEAR))
                    && let Some(u) = self.units.get_mut(id)
                {
                    if let Order::Move(mm) | Order::AMove(mm) = &mut u.order {
                        mm.own = true;
                    }
                    u.best_d = f32::INFINITY;
                    u.stall_t = 0.0;
                }
            }
            _ => done = self.travel(id, m.to, near),
        }
        if let Some(u) = self.units.get_mut(id) {
            u.speed = own_speed;
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fu(x: f32, y: f32, kind: UnitKind) -> FormationUnit {
        FormationUnit {
            pos: v2(x, y),
            kind,
            speed: kind.def().speed,
        }
    }

    #[test]
    fn lines_and_rows() {
        // group south of the goal walking north (dy = -1): front line on the click point, archers behind (south)
        let units = vec![
            fu(1000.0, 2000.0, UnitKind::Archer),
            fu(900.0, 2000.0, UnitKind::Warrior),
            fu(1100.0, 2000.0, UnitKind::Warrior),
            fu(1000.0, 2050.0, UnitKind::Monk),
            fu(1000.0, 2100.0, UnitKind::Catapult),
        ];
        let p = v2(1000.0, 1000.0);
        let slots = formation_slots(&units, p, true, |q| q);
        assert_eq!(slots.len(), 5);
        let get = |i: usize| slots.iter().find(|s| s.unit == i).unwrap();
        // centroid (1000, 2040) → direction (0, -1); per = max(3, ceil(sqrt5*1.5)) = 4
        // line 0: two warriors, sorted by lateral key x*1 + y*0: 900 left, 1100 right; side = ∓22
        let w_left = get(1).order.to;
        let w_right = get(2).order.to;
        assert!((w_left.y - 1000.0).abs() < 1e-3 && (w_right.y - 1000.0).abs() < 1e-3);
        assert!((w_left.x - 978.0).abs() < 1e-3, "{w_left:?}");
        assert!((w_right.x - 1022.0).abs() < 1e-3, "{w_right:?}");
        // line 1 (archer) behind: back = 1*44 + 36 = 80 → y = 1080
        assert!((get(0).order.to.y - 1080.0).abs() < 1e-3);
        // line 2 catapult: back = 80 + 80 = 160; line 3 monk: 240
        assert!((get(4).order.to.y - 1160.0).abs() < 1e-3);
        assert!((get(3).order.to.y - 1240.0).abs() < 1e-3);
        // amove: combatants amove, monk plain move with escort; pace = slowest combatant (catapult 58)
        assert!(get(1).amove && get(0).amove && get(4).amove);
        assert!(!get(3).amove && get(3).order.escort);
        assert!(slots.iter().all(|s| (s.order.pace - 58.0).abs() < 1e-3));
        assert!(slots.iter().all(|s| s.order.group == Some(p)));
    }

    #[test]
    fn plain_move_no_pace_single_unit_no_group() {
        let units = vec![fu(0.0, 0.0, UnitKind::Knight)];
        let slots = formation_slots(&units, v2(500.0, 0.0), false, |q| q);
        assert_eq!(slots[0].order.group, None);
        assert_eq!(slots[0].order.pace, 0.0);
        assert!(!slots[0].amove);
        assert_eq!(slots[0].order.to, v2(500.0, 0.0));
    }

    #[test]
    fn rows_wrap_after_per() {
        // 10 warriors walking east: per = max(3, ceil(sqrt(10)*1.5)) = 5 → 2 rows of 5, second row 44 px behind
        let units: Vec<_> = (0..10)
            .map(|i| fu(0.0, i as f32 * 10.0, UnitKind::Warrior))
            .collect();
        let p = v2(2000.0, 45.0);
        let slots = formation_slots(&units, p, false, |q| q);
        let xs: Vec<f32> = slots.iter().map(|s| s.order.to.x).collect();
        assert_eq!(xs.iter().filter(|x| (**x - 2000.0).abs() < 1.0).count(), 5);
        assert_eq!(xs.iter().filter(|x| (**x - 1956.0).abs() < 1.0).count(), 5);
        // lateral order preserved: walking east (dx=1, dy~0) lateral key = y → smaller y gets smaller side
        let first = slots.iter().find(|s| s.unit == 0).unwrap().order.to;
        let fifth = slots.iter().find(|s| s.unit == 4).unwrap().order.to;
        assert!(first.y < fifth.y);
    }
}
