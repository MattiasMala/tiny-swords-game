// OWNER: S4 (faction) — see docs/spec/04a-factions-economy.md §12 (execCommand: every command, validation order,
// refusal strings), §5.4 (trainBlock / techBlock / ageLock / techLock), §9 (canPlace via `Map::can_place`,
// zoneBlock via `World::zone_block`, placeBlock, moveBlock), §8 (trade), §12.2 setWorkers, §12.3 job, §12.4
// recall; addenda E2 (fog check on `place` for humans), E6; 05 addenda C5 (test-mode commands, solo only).
//
// Every `cmd::Command` is executed here with the JS validation order and the verbatim Italian refusals.
// `cmd::Command::well_formed()` already rejects oversized id lists / non-finite coordinates.
#![allow(dead_code, unused)]

use crate::World;
use crate::cmd::{self, Command, EntityRef, OrderKind, QueueWhat};
use crate::defs::*;
use crate::events::GameEvent;
use crate::slab::{BuildingId, UnitId};
use crate::unit::job_of;
use crate::world::{GatherTarget, Order, QueueItem, Rally, RallyTarget};
use crate::{TILE, math::v2};

/// `ageLock(f, def)`: `Richiede: AGE_NAME[age]`.
fn age_lock(age: u8, need: u8) -> Option<String> {
    (age < need).then(|| cmd::richiede(AGE_NAME.get(need as usize).copied().unwrap_or("")))
}

impl World {
    /// `execCommand(f, cmd, ...)`: `Err(refusal)` is shown to the player as toast + deny; malformed or
    /// not-applicable commands return `Ok(())` silently. Dead factions: no-op.
    /// Test-mode commands are executed here unconditionally — the ROOM must reject them outside solo play.
    pub fn exec_command(&mut self, f: FactionIdx, c: &Command) -> Result<(), String> {
        if (f as usize) >= self.factions.len()
            || !self.factions[f as usize].alive
            || !c.well_formed()
        {
            return Ok(());
        }
        match c {
            Command::Order {
                units,
                kind,
                x,
                y,
                target,
            } => {
                self.order_units(f, units, *kind, *x, *y, *target);
                Ok(())
            }
            Command::Place {
                kind,
                cx,
                cy,
                builders,
            } => self.cmd_place(f, *kind, *cx, *cy, builders),
            Command::Jobs { res, n } => self.set_workers(f, *res, *n),
            Command::Job { pawns, res } => self.cmd_job(f, pawns, *res),
            Command::Recall { units } => {
                self.cmd_recall(f, units);
                Ok(())
            }
            Command::Train { building, unit } => {
                let Some(b) = self.own_building(f, *building) else {
                    return Ok(());
                };
                if let Some(why) = self.train_block(f, b, *unit) {
                    return Err(why);
                }
                let d = unit.def();
                self.pay(f, &d.cost);
                if let Some(bl) = self.buildings.get_mut(b) {
                    bl.queue.push(QueueItem {
                        what: QueueWhat::Unit(*unit),
                        t: 0.0,
                        total: d.time,
                    });
                }
                Ok(())
            }
            Command::Research { building, tech } => {
                let Some(b) = self.own_building(f, *building) else {
                    return Ok(());
                };
                if let Some(why) = self.tech_block(f, b, *tech) {
                    return Err(why);
                }
                let d = tech.def();
                self.pay(f, &d.cost);
                self.factions[f as usize].busy.insert(*tech);
                if let Some(bl) = self.buildings.get_mut(b) {
                    bl.queue.push(QueueItem {
                        what: QueueWhat::Tech(*tech),
                        t: 0.0,
                        total: d.time,
                    });
                }
                Ok(())
            }
            Command::Cancel {
                building,
                index,
                expected,
            } => {
                let Some(b) = self.own_building(f, *building) else {
                    return Ok(());
                };
                let Some(bl) = self.buildings.get_mut(b) else {
                    return Ok(());
                };
                let i = *index as usize;
                // the queue may have advanced meanwhile (network): better nothing than the wrong item
                if bl.queue.get(i).is_none_or(|q| q.what != *expected) {
                    return Ok(());
                }
                let q = bl.queue.remove(i);
                match q.what {
                    QueueWhat::Unit(k) => self.refund(f, &k.def().cost),
                    QueueWhat::Tech(t) => {
                        self.refund(f, &t.def().cost);
                        self.factions[f as usize].busy.remove(t);
                    }
                }
                Ok(())
            }
            Command::Rally {
                building,
                x,
                y,
                target,
            } => {
                let Some(b) = self.own_building(f, *building) else {
                    return Ok(());
                };
                let t = target.and_then(|t| self.rally_target(f, t));
                if let Some(bl) = self.buildings.get_mut(b) {
                    bl.rally = Rally {
                        pos: v2(*x, *y),
                        target: t,
                    };
                }
                Ok(())
            }
            Command::Move { building, cx, cy } => {
                let Some(b) = self.own_building(f, *building) else {
                    return Ok(());
                };
                let Some(bl) = self.buildings.get(b) else {
                    return Ok(());
                };
                if (*cx, *cy) == (bl.cx, bl.cy) {
                    return Ok(());
                }
                if let Some(why) = self.move_block(f, b, *cx, *cy) {
                    return Err(why);
                }
                self.relocate_building(b, *cx, *cy);
                Ok(())
            }
            Command::Demolish { building } => {
                let Some(b) = self.own_building(f, *building) else {
                    return Ok(());
                };
                let Some(bl) = self.buildings.get(b) else {
                    return Ok(());
                };
                if bl.kind == BuildingKind::Castle {
                    return Err(cmd::CASTELLO_NON_SI_DEMOLISCE.to_string());
                }
                if !bl.done {
                    let cost = bl.def().cost;
                    self.refund(f, &cost);
                }
                self.destroy_building(b, crate::world::DamageSource::NONE);
                self.census(f);
                Ok(())
            }
            Command::Trade { market, op, res } => {
                let Some(b) = self.own_building(f, *market) else {
                    return Ok(());
                };
                let ok = self
                    .buildings
                    .get(b)
                    .is_some_and(|bl| bl.def().trade && bl.done);
                if !ok || !res.tradable() {
                    return Ok(());
                }
                self.trade(f, *op, *res)
            }
            Command::DebugGift { res } => {
                let fac = &mut self.factions[f as usize];
                match res {
                    Some(r) => fac.res[*r] += TEST_GIFT,
                    None => {
                        for r in Res::ALL {
                            fac.res[*r] += TEST_GIFT;
                        }
                    }
                }
                Ok(())
            }
            Command::DebugFreeze { on } => {
                self.frozen = *on;
                Ok(())
            }
            Command::DebugFogOff { on } => {
                self.factions[f as usize].fog_off = *on;
                Ok(())
            }
        }
    }

    /// `ownBuilding(f, id)`: an alive building of `f`.
    pub fn own_building(&self, f: FactionIdx, id: BuildingId) -> Option<BuildingId> {
        self.buildings
            .get(id)
            .is_some_and(|b| b.faction == f && !b.dead)
            .then_some(id)
    }

    /// Rally target kept only if it is a node / live sheep or an alive own building (factions.js:398).
    fn rally_target(&self, f: FactionIdx, t: EntityRef) -> Option<RallyTarget> {
        match t {
            EntityRef::Node(n) => ((n.0 as usize) < self.nodes.len())
                .then_some(RallyTarget::Gather(GatherTarget::Node(n))),
            EntityRef::Sheep(s) => self
                .sheep
                .get(s)
                .is_some_and(|s| !s.dead)
                .then_some(RallyTarget::Gather(GatherTarget::Sheep(s))),
            EntityRef::Building(b) => self
                .buildings
                .get(b)
                .is_some_and(|bl| !bl.dead && bl.faction == f)
                .then_some(RallyTarget::Building(b)),
            EntityRef::Unit(_) | EntityRef::Point(_) => None,
        }
    }

    /// `place(kind, cx, cy, builders)` (04a §12.1): kind ∈ BUILD_MENU, `placeBlock`, new site, pay, `sites++`,
    /// dust2 under every column, first 200 builders get a `build` order.
    fn cmd_place(
        &mut self,
        f: FactionIdx,
        kind: BuildingKind,
        cx: i32,
        cy: i32,
        builders: &[UnitId],
    ) -> Result<(), String> {
        if !kind.placeable() {
            return Ok(());
        }
        if let Some(why) = self.place_block(f, kind, cx, cy) {
            return Err(why);
        }
        let d = kind.def();
        let site = self.add_building(f, kind, cx, cy, false);
        self.pay(f, &d.cost);
        let s = &mut self.factions[f as usize].sites[kind as usize];
        *s = s.saturating_add(1);
        for i in 0..d.w {
            self.emit(GameEvent::fx(
                FxKind::Dust2,
                (cx + i) as f32 * TILE + TILE * 0.5,
                (cy + d.h) as f32 * TILE,
            ));
        }
        let n = builders.len().min(cmd::MAX_IDS);
        if n > 0 {
            self.order_units(
                f,
                &builders[..n],
                OrderKind::Build,
                0.0,
                0.0,
                Some(EntityRef::Building(site)),
            );
        }
        Ok(())
    }

    /// `job(ids, res)` (04a §12.3): every listed own pawn (creation order) goes to its nearest free explored node
    /// of `res` (emboldened). Refusal only if pawns were asked and none could be sent.
    fn cmd_job(&mut self, f: FactionIdx, ids: &[UnitId], res: Res) -> Result<(), String> {
        let pawns = self.own_pawns(f);
        let (mut asked, mut sent) = (0u32, 0u32);
        for u in pawns {
            if !ids.contains(&u) {
                continue;
            }
            asked += 1;
            let Some(pos) = self.units.get(u).map(|u| u.pos) else {
                continue;
            };
            if let Some((node, _)) = self.node_near(f, res, pos)
                && let Some(o) = self.gather_order(node)
            {
                self.embolden(u);
                self.set_order(u, o);
                sent += 1;
            }
        }
        if asked > 0 && sent == 0 {
            Err(cmd::nessun_posto_libero(res))
        } else {
            Ok(())
        }
    }

    /// `recall(ids)` (04a §12.4): listed own non-pawns move (plain move) to `(castle.x, castle.y1 + 130)`.
    fn cmd_recall(&mut self, f: FactionIdx, ids: &[UnitId]) {
        let Some((x, y1)) = self.factions[f as usize]
            .castle
            .and_then(|c| self.buildings.get(c))
            .map(|c| (c.pos.x, c.rect.y1))
        else {
            return;
        };
        let list: Vec<UnitId> = self
            .units
            .iter()
            .filter(|(id, u)| {
                u.faction == f && !u.dead && u.kind != UnitKind::Pawn && ids.contains(id)
            })
            .map(|(id, _)| id)
            .collect();
        self.order_units(f, &list, OrderKind::Move, x, y1 + RECALL_DY, None);
    }

    /// `ownPawns(f)`: own live pawns in creation order.
    pub fn own_pawns(&self, f: FactionIdx) -> Vec<UnitId> {
        self.units
            .iter()
            .filter(|(_, u)| u.faction == f && !u.dead && u.kind == UnitKind::Pawn)
            .map(|(id, _)| id)
            .collect()
    }

    /// `trainBlock(f, b, type)` (04a §5.4): `Non disponibile` | `Richiede: AGE` | `Coda piena` |
    /// `Risorse insufficienti`.
    pub fn train_block(&self, f: FactionIdx, b: BuildingId, k: UnitKind) -> Option<String> {
        let d = k.def();
        let Some(bl) = self.buildings.get(b).filter(|bl| !bl.dead) else {
            return Some(cmd::NON_DISPONIBILE.to_string());
        };
        if d.from != bl.kind || !bl.done {
            return Some(cmd::NON_DISPONIBILE.to_string());
        }
        let Some(fac) = self.factions.get(f as usize) else {
            return Some(cmd::NON_DISPONIBILE.to_string());
        };
        age_lock(fac.age, d.age).or_else(|| {
            if bl.queue.len() >= QUEUE_MAX {
                Some(cmd::CODA_PIENA.to_string())
            } else if fac.res.can_afford(&d.cost) {
                None
            } else {
                Some(cmd::RISORSE_INSUFFICIENTI.to_string())
            }
        })
    }

    /// `techLock(f, def)`: age lock, then the prerequisite tech (`Richiede: label`).
    pub fn tech_lock(&self, f: FactionIdx, t: TechId) -> Option<String> {
        let d = t.def();
        let Some(fac) = self.factions.get(f as usize) else {
            return Some(cmd::NON_DISPONIBILE.to_string());
        };
        age_lock(fac.age, d.age).or_else(|| {
            d.req
                .filter(|r| !fac.techs.has(*r))
                .map(|r| cmd::richiede(r.def().label))
        })
    }

    /// `techBlock(f, b, id)` (04a §5.4): `Non disponibile` | `Già ricercata` | `Ricerca in corso` | age/req lock |
    /// `Coda piena` | `Risorse insufficienti`.
    pub fn tech_block(&self, f: FactionIdx, b: BuildingId, t: TechId) -> Option<String> {
        let d = t.def();
        let Some(bl) = self.buildings.get(b).filter(|bl| !bl.dead) else {
            return Some(cmd::NON_DISPONIBILE.to_string());
        };
        if d.at != bl.kind || !bl.done {
            return Some(cmd::NON_DISPONIBILE.to_string());
        }
        let Some(fac) = self.factions.get(f as usize) else {
            return Some(cmd::NON_DISPONIBILE.to_string());
        };
        if fac.techs.has(t) {
            return Some(cmd::GIA_RICERCATA.to_string());
        }
        if fac.busy.has(t) {
            return Some(cmd::RICERCA_IN_CORSO.to_string());
        }
        self.tech_lock(f, t).or_else(|| {
            if bl.queue.len() >= QUEUE_MAX {
                Some(cmd::CODA_PIENA.to_string())
            } else if fac.res.can_afford(&d.cost) {
                None
            } else {
                Some(cmd::RISORSE_INSUFFICIENTI.to_string())
            }
        })
    }

    /// `placeBlock(f, kind, cx, cy)` (04a §9.3 + E2): age lock | `Risorse insufficienti` | zone | terrain
    /// (`canPlace`) | footprint centre not explored (humans only) → `Qui non si può costruire`.
    pub fn place_block(
        &self,
        f: FactionIdx,
        kind: BuildingKind,
        cx: i32,
        cy: i32,
    ) -> Option<String> {
        if !kind.placeable() {
            return Some(cmd::QUI_NON_SI_PUO_COSTRUIRE.to_string());
        }
        let d = kind.def();
        let Some(fac) = self.factions.get(f as usize) else {
            return Some(cmd::QUI_NON_SI_PUO_COSTRUIRE.to_string());
        };
        if let Some(l) = age_lock(fac.age, d.age) {
            return Some(l);
        }
        if !fac.res.can_afford(&d.cost) {
            return Some(cmd::RISORSE_INSUFFICIENTI.to_string());
        }
        if let Some(z) = self.zone_block(f, kind, cx, cy) {
            return Some(z);
        }
        let centre = v2(
            (cx as f32 + d.w as f32 / 2.0) * TILE,
            (cy as f32 + d.h as f32 / 2.0) * TILE,
        );
        // `inside` first: an absurd cell (e.g. i32::MAX) must not reach the footprint arithmetic
        if !self.map.inside(cx, cy)
            || !self.map.can_place(kind, cx, cy, &[])
            || !self.explored(f, centre)
        {
            return Some(cmd::QUI_NON_SI_PUO_COSTRUIRE.to_string());
        }
        None
    }

    /// `moveBlock(f, b, cx, cy)` (04a §9.3): `Il castello non si sposta` | zone | `Qui non può stare` (the
    /// building's own cells do not count as occupied).
    pub fn move_block(&self, f: FactionIdx, b: BuildingId, cx: i32, cy: i32) -> Option<String> {
        let Some(bl) = self.buildings.get(b) else {
            return Some(cmd::QUI_NON_PUO_STARE.to_string());
        };
        if bl.kind == BuildingKind::Castle {
            return Some(cmd::CASTELLO_NON_SI_SPOSTA.to_string());
        }
        if (f as usize) >= self.factions.len() {
            return Some(cmd::QUI_NON_PUO_STARE.to_string());
        }
        if let Some(z) = self.zone_block(f, bl.kind, cx, cy) {
            return Some(z);
        }
        if self.map.inside(cx, cy) && self.map.can_place(bl.kind, cx, cy, &bl.cells()) {
            None
        } else {
            Some(cmd::QUI_NON_PUO_STARE.to_string())
        }
    }

    /// `setWorkers(f, res, n)` (04a §12.2): bring the pawns working on `res` (sheltered ones included, `jobOf`)
    /// to `n`. Excess: empty-handed first; they deliver their load (drop) or go idle (sheltered: idle on exit).
    /// Missing: repeatedly the GLOBAL best (free pawn, nearest free explored node) pair. Partial success is kept.
    /// Free = idle or delivering (`drop`).
    pub fn set_workers(&mut self, f: FactionIdx, res: Res, n: u16) -> Result<(), String> {
        let pawns = self.own_pawns(f);
        let mut on: Vec<(UnitId, bool)> = pawns
            .iter()
            .filter_map(|id| {
                let u = self.units.get(*id)?;
                (job_of(&u.order) == Some(res)).then_some((*id, u.carry.is_some()))
            })
            .collect();
        on.sort_by_key(|(_, carry)| *carry); // stable: empty-handed first
        let excess = on.len().saturating_sub(n as usize);
        for (id, _) in on.drain(..excess).collect::<Vec<_>>() {
            let Some(u) = self.units.get(id) else {
                continue;
            };
            let (pos, carry) = (u.pos, u.carry);
            if matches!(u.order, Order::Shelter { .. }) {
                // sheltering: stays free when it comes out
                if let Some(u) = self.units.get_mut(id)
                    && let Order::Shelter { back, .. } = &mut u.order
                {
                    **back = Order::idle_at(pos);
                }
                continue;
            }
            match carry.and_then(|c| self.drop_site_at(f, pos, c.res)) {
                Some(site) => self.set_order(id, Order::Drop { building: site }),
                None => self.idle(id),
            }
        }
        let free = |w: &World, id: UnitId| {
            w.units
                .get(id)
                .is_some_and(|u| !u.dead && matches!(u.order, Order::Idle { .. } | Order::Drop { .. }))
        };
        let mut have = on.len();
        while have < n as usize {
            let mut best: Option<(UnitId, GatherTarget, f32)> = None;
            for &id in &pawns {
                if !free(self, id) {
                    continue;
                }
                let Some(pos) = self.units.get(id).map(|u| u.pos) else {
                    continue;
                };
                if let Some((node, d)) = self.node_near(f, res, pos)
                    && best.is_none_or(|(_, _, bd)| d < bd)
                {
                    best = Some((id, node, d));
                }
            }
            let Some((id, node, _)) = best else {
                return Err(if pawns.iter().any(|id| free(self, *id)) {
                    cmd::nessun_posto_libero(res)
                } else {
                    cmd::NESSUN_PEDONE_LIBERO.to_string()
                });
            };
            match self.gather_order(node) {
                Some(o) => self.set_order(id, o),
                None => return Err(cmd::nessun_posto_libero(res)),
            }
            // safety: the chosen pawn must have left the free pool, else this loop could spin up to `n` (u16)
            // times rescanning every pawn × node (JS: `gatherOrder` always makes the pawn non-free)
            if free(self, id) {
                return Err(cmd::nessun_posto_libero(res));
            }
            have += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::TradeOp;
    use crate::faction::tests::{base_world, human};

    fn castle_of(w: &World, f: FactionIdx) -> BuildingId {
        w.factions[f as usize].castle.unwrap()
    }

    /// A free spot for `kind` near the castle (scan outward), inside the zone and explored.
    fn spot(w: &World, f: FactionIdx, kind: BuildingKind) -> (i32, i32) {
        let (bx, by) = w.factions[f as usize].base;
        for r in 3..12 {
            for dy in -r..=r {
                for dx in -r..=r {
                    let (cx, cy) = (bx + dx, by + dy);
                    if w.place_block(f, kind, cx, cy).is_none() {
                        return (cx, cy);
                    }
                }
            }
        }
        panic!("no spot")
    }

    #[test]
    fn train_research_cancel() {
        let mut w = base_world();
        let f = human(&w);
        w.step(&[]);
        let c = castle_of(&w, f);
        w.factions[f as usize].res = Stock([1000.0, 1000.0, 1000.0, 1000.0]);
        // wrong building / age / queue / resources
        let t = |w: &mut World, u| w.exec_command(f, &Command::Train { building: c, unit: u });
        assert_eq!(t(&mut w, UnitKind::Warrior), Err("Non disponibile".into()));
        assert_eq!(t(&mut w, UnitKind::Pawn), Ok(()));
        assert_eq!(w.factions[f as usize].res[Res::Meat], 950.0);
        let r = |w: &mut World, tech| w.exec_command(f, &Command::Research { building: c, tech });
        assert_eq!(r(&mut w, TechId::Age3), Err("Richiede: Età Feudale".into()));
        assert_eq!(r(&mut w, TechId::Carts), Err("Richiede: Età Feudale".into()));
        assert_eq!(r(&mut w, TechId::Axes), Err("Non disponibile".into()));
        assert_eq!(r(&mut w, TechId::Age2), Ok(()));
        assert!(w.factions[f as usize].busy.has(TechId::Age2));
        assert_eq!(r(&mut w, TechId::Age2), Err("Ricerca in corso".into()));
        for _ in 0..3 {
            assert_eq!(t(&mut w, UnitKind::Scout), Ok(()));
        }
        assert_eq!(t(&mut w, UnitKind::Pawn), Err("Coda piena".into()));
        // cancel with a stale expectation: nothing happens
        let before = w.factions[f as usize].res;
        let cancel = Command::Cancel {
            building: c,
            index: 1,
            expected: QueueWhat::Unit(UnitKind::Pawn),
        };
        assert_eq!(w.exec_command(f, &cancel), Ok(()));
        assert_eq!(w.factions[f as usize].res, before);
        // cancel the tech: full refund, busy cleared
        let cancel = Command::Cancel {
            building: c,
            index: 1,
            expected: QueueWhat::Tech(TechId::Age2),
        };
        assert_eq!(w.exec_command(f, &cancel), Ok(()));
        assert_eq!(w.factions[f as usize].res[Res::Meat], before[Res::Meat] + 250.0);
        assert!(!w.factions[f as usize].busy.has(TechId::Age2));
        assert_eq!(w.buildings.get(c).unwrap().queue.len(), 4);
        // done tech
        w.factions[f as usize].techs.insert(TechId::Age2);
        w.factions[f as usize].age = 2;
        assert_eq!(r(&mut w, TechId::Age2), Err("Già ricercata".into()));
        assert_eq!(t(&mut w, UnitKind::Pawn), Ok(()));
        assert_eq!(r(&mut w, TechId::Age3), Err("Coda piena".into()));
        w.buildings.get_mut(c).unwrap().queue.clear();
        w.factions[f as usize].res = Stock([0.0; 4]);
        assert_eq!(r(&mut w, TechId::Age3), Err("Risorse insufficienti".into()));
        assert_eq!(t(&mut w, UnitKind::Pawn), Err("Risorse insufficienti".into()));
        assert_eq!(
            w.tech_lock(f, TechId::Picks),
            Some("Richiede: Asce affilate".into())
        );
    }

    #[test]
    fn place_move_demolish() {
        let mut w = base_world();
        let f = human(&w);
        w.step(&[]);
        // castle can never be placed / moved / demolished
        let c = castle_of(&w, f);
        let place = |kind, cx, cy| Command::Place {
            kind,
            cx,
            cy,
            builders: vec![],
        };
        assert_eq!(w.exec_command(f, &place(BuildingKind::Castle, 0, 0)), Ok(()));
        assert_eq!(
            w.exec_command(f, &Command::Demolish { building: c }),
            Err("Il castello non si demolisce".into())
        );
        let (bx, by) = w.factions[f as usize].base;
        assert_eq!(
            w.exec_command(f, &Command::Move { building: c, cx: bx, cy: by + 3 }),
            Err("Il castello non si sposta".into())
        );
        // age / zone / terrain refusals
        assert_eq!(
            w.place_block(f, BuildingKind::Tower, bx, by),
            Some("Richiede: Età Feudale".into())
        );
        assert_eq!(
            w.place_block(f, BuildingKind::House, bx - 2, by - 1),
            Some("Qui non si può costruire".into())
        );
        let far = if bx > 144 { bx - 60 } else { bx + 60 };
        assert_eq!(
            w.place_block(f, BuildingKind::House, far, by),
            Some("Troppo lontano: entro 40 caselle dal castello o 9 da un tuo avamposto".into())
        );
        assert_eq!(
            w.place_block(f, BuildingKind::Wall, if bx > 144 { bx - 14 } else { bx + 14 }, by),
            Some("Torri e palizzate: entro 12 caselle dal castello o 9 da un tuo avamposto".into())
        );
        // a real placement: pays, counts the site, orders builders (first 200)
        let (hx, hy) = spot(&w, f, BuildingKind::House);
        let wood = w.factions[f as usize].res[Res::Wood];
        let pawn = w.factions[f as usize].pawns[0];
        let mut builders = vec![pawn; 1];
        builders.extend(vec![UnitId::default(); 300]);
        let cmd = Command::Place {
            kind: BuildingKind::House,
            cx: hx,
            cy: hy,
            builders,
        };
        assert_eq!(w.exec_command(f, &cmd), Ok(()));
        assert_eq!(w.factions[f as usize].res[Res::Wood], wood - 30.0);
        assert_eq!(w.factions[f as usize].sites[BuildingKind::House as usize], 1);
        let site = w
            .buildings
            .iter()
            .find(|(_, b)| b.faction == f && b.kind == BuildingKind::House)
            .map(|(id, _)| id)
            .unwrap();
        // same spot again: occupied
        assert_eq!(w.exec_command(f, &cmd), Err("Qui non si può costruire".into()));
        // move onto itself shifted by one cell may or may not fit; moving to the same cell is a no-op
        assert_eq!(
            w.exec_command(f, &Command::Move { building: site, cx: hx, cy: hy }),
            Ok(())
        );
        // demolish the site: full refund
        assert_eq!(w.exec_command(f, &Command::Demolish { building: site }), Ok(()));
        assert_eq!(w.factions[f as usize].res[Res::Wood], wood);
        assert_eq!(w.factions[f as usize].sites[BuildingKind::House as usize], 0);
        // not enough wood
        w.factions[f as usize].res = Stock([0.0; 4]);
        assert_eq!(
            w.place_block(f, BuildingKind::House, hx, hy),
            Some("Risorse insufficienti".into())
        );
        // other kingdoms' buildings are ignored silently
        let other = castle_of(&w, (f + 1) % 5);
        assert_eq!(w.exec_command(f, &Command::Demolish { building: other }), Ok(()));
        assert!(w.building_alive(other));
    }

    #[test]
    fn fog_blocks_human_placement() {
        let mut w = base_world();
        let f = human(&w);
        w.step(&[]);
        w.factions[f as usize].res = Stock([5000.0; 4]);
        // forget everything explored: placement in the dark is refused, the AI is exempt
        let (hx, hy) = spot(&w, f, BuildingKind::House);
        w.factions[f as usize].fog.explored.clear();
        assert_eq!(
            w.place_block(f, BuildingKind::House, hx, hy),
            Some("Qui non si può costruire".into())
        );
        // AI-driven (takeover): no fog for its placements
        w.factions[f as usize].human = false;
        assert_eq!(w.place_block(f, BuildingKind::House, hx, hy), None);
    }

    #[test]
    fn absurd_input_never_panics() {
        let mut w = base_world();
        let f = human(&w);
        w.step(&[]);
        w.factions[f as usize].res = Stock([5000.0; 4]);
        let c = castle_of(&w, f);
        let site = {
            let (hx, hy) = spot(&w, f, BuildingKind::House);
            let cmd = Command::Place { kind: BuildingKind::House, cx: hx, cy: hy, builders: vec![] };
            w.exec_command(f, &cmd).unwrap();
            w.buildings
                .iter()
                .find(|(_, b)| b.faction == f && b.kind == BuildingKind::House)
                .map(|(id, _)| id)
                .unwrap()
        };
        for (cx, cy) in [(i32::MAX, i32::MAX), (i32::MIN, i32::MIN), (i32::MAX, 0), (0, i32::MIN)] {
            let p = Command::Place { kind: BuildingKind::Wall, cx, cy, builders: vec![] };
            assert!(w.exec_command(f, &p).is_err());
            let m = Command::Move { building: site, cx, cy };
            assert!(w.exec_command(f, &m).is_err());
        }
        let r = Command::Rally { building: c, x: f32::MAX, y: -f32::MAX, target: None };
        assert_eq!(w.exec_command(f, &r), Ok(()));
        let cancel = Command::Cancel { building: c, index: 255, expected: QueueWhat::Unit(UnitKind::Pawn) };
        assert_eq!(w.exec_command(f, &cancel), Ok(()));
        assert!(w.exec_command(f, &Command::Jobs { res: Res::Wood, n: u16::MAX }).is_err());
        // unknown faction index: ignored
        assert_eq!(w.exec_command(200, &Command::DebugGift { res: None }), Ok(()));
        assert!(!w.can_afford(200, &BuildingKind::House.def().cost));
        assert_eq!(w.train_block(200, c, UnitKind::Pawn), Some("Non disponibile".into()));
        // far-off rally point / units: fog stamping ignores off-map centres
        crate::fog::stamp_disc(&mut w.factions[f as usize].fog.explored, w.map.w, w.map.h, v2(3.0e38, -3.0e38), 9);
        w.step(&[]);
    }

    #[test]
    fn trade_rally_and_misc() {
        let mut w = base_world();
        let f = human(&w);
        let c = castle_of(&w, f);
        // trading needs an own finished market (else silently ignored)
        let tr = Command::Trade {
            market: c,
            op: TradeOp::Sell,
            res: Res::Wood,
        };
        let res = w.factions[f as usize].res;
        assert_eq!(w.exec_command(f, &tr), Ok(()));
        assert_eq!(w.factions[f as usize].res, res);
        let m = w.add_building(f, BuildingKind::Market, 1, 1, true);
        w.block_building(m, false);
        let tr = Command::Trade {
            market: m,
            op: TradeOp::Sell,
            res: Res::Wood,
        };
        assert_eq!(w.exec_command(f, &tr), Ok(()));
        assert_eq!(w.factions[f as usize].res[Res::Gold], res[Res::Gold] + 50.0);
        w.factions[f as usize].res[Res::Wood] = 10.0;
        assert_eq!(w.exec_command(f, &tr), Err("Risorse insufficienti".into()));
        // rally: own building kept, enemy building dropped
        let other = castle_of(&w, (f + 1) % 5);
        let rally = |t| Command::Rally {
            building: c,
            x: 100.0,
            y: 200.0,
            target: Some(t),
        };
        w.exec_command(f, &rally(EntityRef::Building(other))).unwrap();
        assert_eq!(w.buildings.get(c).unwrap().rally.target, None);
        assert_eq!(w.buildings.get(c).unwrap().rally.pos, v2(100.0, 200.0));
        w.exec_command(f, &rally(EntityRef::Building(m))).unwrap();
        assert_eq!(
            w.buildings.get(c).unwrap().rally.target,
            Some(RallyTarget::Building(m))
        );
        // non-finite rally is ignored
        let bad = Command::Rally {
            building: c,
            x: f32::NAN,
            y: 0.0,
            target: None,
        };
        w.exec_command(f, &bad).unwrap();
        assert_eq!(w.buildings.get(c).unwrap().rally.pos, v2(100.0, 200.0));
        // test mode
        w.exec_command(f, &Command::DebugGift { res: Some(Res::Stone) }).unwrap();
        assert_eq!(w.factions[f as usize].res[Res::Stone], res[Res::Stone] + 1000.0);
        w.exec_command(f, &Command::DebugFreeze { on: true }).unwrap();
        let tick = w.tick;
        w.step(&[]);
        assert_eq!(w.tick, tick);
        w.exec_command(f, &Command::DebugFreeze { on: false }).unwrap();
        // workers: nobody to remove, the slider at 0 is fine; recall without soldiers in the list is a no-op
        assert_eq!(w.set_workers(f, Res::Wood, 0), Ok(()));
        let warrior = w.factions[f as usize].soldiers[0];
        w.exec_command(f, &Command::Recall { units: vec![warrior] }).unwrap();
        assert!(matches!(w.units.get(warrior).unwrap().order, Order::Move(_)));
        // all pawns busy → 'Nessun pedone libero'
        let pawns = w.own_pawns(f);
        for p in &pawns {
            let pos = w.units.get(*p).unwrap().pos;
            w.set_order(*p, Order::Move(crate::world::MoveOrder::to(pos)));
        }
        assert_eq!(w.set_workers(f, Res::Gold, 1), Err("Nessun pedone libero".into()));
        // job with no listed own pawn: nothing asked, no refusal
        assert_eq!(
            w.exec_command(f, &Command::Job { pawns: vec![warrior], res: Res::Gold }),
            Ok(())
        );
    }

    #[test]
    fn set_workers_removes_empty_handed_first() {
        let mut w = base_world();
        let f = human(&w);
        let pawns = w.own_pawns(f);
        let (a, b) = (pawns[0], pawns[1]);
        let pos = w.units.get(a).unwrap().pos;
        let go = Order::Gather(crate::world::GatherOrder {
            res: Res::Wood,
            first: None,
            origin: pos,
        });
        w.set_order(a, go.clone());
        w.set_order(b, go);
        w.units.get_mut(a).unwrap().carry = Some(crate::world::Carry {
            res: Res::Wood,
            amount: 10.0,
        });
        assert_eq!(w.set_workers(f, Res::Wood, 1), Ok(()));
        // b (empty-handed) was removed and idles; a keeps gathering
        assert!(w.units.get(b).unwrap().order.is_idle());
        assert!(matches!(w.units.get(a).unwrap().order, Order::Gather(_)));
        // lowering to 0: the carrying pawn delivers first
        assert_eq!(w.set_workers(f, Res::Wood, 0), Ok(()));
        assert!(matches!(w.units.get(a).unwrap().order, Order::Drop { .. }));
    }
}
