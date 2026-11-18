// OWNER: S2 (units) — see docs/spec/03a-units-combat.md §5.2 (orderUnits incl. embolden for humans), §6 (formation /
// group move: lineOf, rows, LINE_GAP, pace, escort), §6.1 (march with the `fresh` budget), addenda U3, U4;
// 03b §3 (gather/build/drop/repair resume rules); 04a §12 `order` / `recall`.
//
// Stub fns S2 must fill: order_units (MINIMAL: stop + single-point move/amove, no formation, no target kinds),
// march (MINIMAL: travel to own slot).
#![allow(dead_code, unused)]

use crate::World;
use crate::cmd::{EntityRef, OrderKind};
use crate::defs::*;
use crate::math::V2;
use crate::slab::UnitId;
use crate::world::{MoveOrder, Order};

impl World {
    /// `orderUnits(f, ids, kind, x, y, targetId)` (§5.2): filters own live units; humans → `embolden`;
    /// stop / explore / move+amove (formation §6) / attack / gather / build / drop / heal.
    /// MINIMAL: Stop and Move/AMove to the nearest passable point (no formation).
    pub fn order_units(
        &mut self,
        f: FactionIdx,
        ids: &[UnitId],
        kind: OrderKind,
        x: f32,
        y: f32,
        target: Option<EntityRef>,
    ) {
        let units: Vec<UnitId> = ids
            .iter()
            .copied()
            .filter(|id| {
                self.units
                    .get(*id)
                    .is_some_and(|u| !u.dead && u.faction == f)
            })
            .collect();
        if units.is_empty() {
            return;
        }
        match kind {
            OrderKind::Stop => {
                for u in units {
                    self.idle(u);
                }
            }
            OrderKind::Move | OrderKind::AMove => {
                if !(x.is_finite() && y.is_finite()) {
                    return;
                }
                let p = self.map.nearest_passable(V2::new(x, y));
                for u in units {
                    let amove = kind == OrderKind::AMove
                        && self.units.get(u).is_some_and(|u| u.def().has_dmg());
                    let m = MoveOrder::to(p);
                    self.set_order(
                        u,
                        if amove {
                            Order::AMove(m)
                        } else {
                            Order::Move(m)
                        },
                    );
                }
            }
            _ => {}
        }
    }

    /// `march(u, o, dt, near)` (§6.1): pace, shared group goal until `fields.take_fresh()` or arrival, then own
    /// slot. Returns done. MINIMAL: travel to the own slot.
    pub fn march(&mut self, id: UnitId, near: f32) -> bool {
        let Some(u) = self.units.get(id) else {
            return true;
        };
        match &u.order {
            Order::Move(m) | Order::AMove(m) => {
                let to = m.to;
                self.travel(id, to, near)
            }
            _ => true,
        }
    }
}
