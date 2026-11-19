// OWNER: S4 (faction) — see docs/spec/04a-factions-economy.md §3 (faction data, setup), §4 (census, caps),
// §5.5 (canAfford/pay/refund), §6 (applyTech), §7 (updateFaction), §8 (market prices), §11.3 (estimateRates),
// §13 (elimination, end of match, reconnect), addenda E1-E7; 04b §12 (takeover / reclaim) + addenda A1, A6;
// 08-net addenda N1.
//
// Everything here is complete: update_faction, census, can_afford / pay / refund, room_for, train_speed_of,
// apply_tech (mods + live rescale of units and buildings + events), estimate_rates, sell/buy prices, market trade,
// eliminate_faction (+ eliminated player's fog lifted), check_end, takeover, reclaim, free_building, queued.
#![allow(dead_code, unused)]

use crate::TICK_DT;
use crate::World;
use crate::cmd::{self, QueueWhat, TradeOp};
use crate::defs::*;
use crate::events::{GameEvent, Msg, dur};
use crate::path::FieldTarget;
use crate::slab::{BuildingId, UnitId};
use crate::world::{DamageSource, GatherTarget, MatchOutcome, Order};

impl World {
    /// `updateFaction(f, dt)` (04a §7, 04b §11): AI trickle, exploration (`explore`, for any human-OWNED faction,
    /// addenda A6), 1-s tick (census, rates for humans, trades decay), AI think (`ai_tick`) every `level.think`.
    pub fn update_faction(&mut self, f: FactionIdx) {
        let dt = TICK_DT;
        let fi = f as usize;
        if fi >= self.factions.len() || !self.factions[fi].alive {
            return;
        }
        let (human, owned) = (self.factions[fi].human, self.factions[fi].owned);
        if !human {
            let trickle = self.factions[fi].ai.level.def().trickle;
            if trickle != 0.0 {
                for r in Res::ALL {
                    self.factions[fi].res[*r] += trickle * dt as f64;
                }
            }
        }
        // JS: `elif human: explore`; A6: keep exploring for a human-owned slot while the AI drives it.
        if human || owned {
            let fac = &mut self.factions[fi];
            fac.fog.see_t -= dt;
            if fac.fog.see_t <= 0.0 {
                fac.fog.see_t = EXPLORE_PERIOD;
                self.explore(f);
            }
        }
        let fac = &mut self.factions[fi];
        fac.tick_t -= dt;
        if fac.tick_t <= 0.0 {
            fac.tick_t = CENSUS_PERIOD;
            self.census(f);
            if human {
                self.estimate_rates(f);
            }
            for t in self.factions[fi].trades.iter_mut() {
                *t = (*t - TRADE_DECAY).max(0.0);
            }
        }
        if !human {
            let fac = &mut self.factions[fi];
            fac.ai.think_t -= dt;
            if fac.ai.think_t <= 0.0 {
                fac.ai.think_t = fac.ai.level.def().think;
                self.ai_tick(f);
            }
        }
    }

    /// `census(f)` (04a §4): pawns/soldiers lists and counts, `has`/`sites` per kind, caps
    /// `min(30, Σ pawns)` / `min(50, Σ troops)` over finished buildings.
    pub fn census(&mut self, f: FactionIdx) {
        let fi = f as usize;
        if fi >= self.factions.len() {
            return;
        }
        // reuse the faction's vectors (no per-second allocation)
        let mut pawns = core::mem::take(&mut self.factions[fi].pawns);
        let mut soldiers = core::mem::take(&mut self.factions[fi].soldiers);
        pawns.clear();
        soldiers.clear();
        for (id, u) in self.units.iter() {
            if u.faction != f || u.dead {
                continue;
            }
            if u.kind == UnitKind::Pawn {
                pawns.push(id);
            } else {
                soldiers.push(id);
            }
        }
        let mut has = [0u16; 14];
        let mut sites = [0u16; 14];
        let (mut pc, mut tc) = (0u16, 0u16);
        for b in self.buildings.values() {
            if b.faction != f || b.dead {
                continue;
            }
            if !b.done {
                sites[b.kind as usize] += 1;
                continue;
            }
            has[b.kind as usize] += 1;
            pc = pc.saturating_add(b.def().pawns);
            tc = tc.saturating_add(b.def().troops);
        }
        let fac = &mut self.factions[fi];
        fac.pawn_n = pawns.len() as u16;
        fac.troop_n = soldiers.len() as u16;
        fac.pawns = pawns;
        fac.soldiers = soldiers;
        fac.has = has;
        fac.sites = sites;
        fac.pawn_cap = pc.min(PAWN_MAX);
        fac.troop_cap = tc.min(TROOP_MAX);
    }

    /// `canAfford(f, cost)`.
    pub fn can_afford(&self, f: FactionIdx, c: &Cost) -> bool {
        self.factions.get(f as usize).is_some_and(|fac| fac.res.can_afford(c))
    }

    /// `pay(f, cost)` (no check).
    pub fn pay(&mut self, f: FactionIdx, c: &Cost) {
        if let Some(fac) = self.factions.get_mut(f as usize) {
            fac.res.pay(c);
        }
    }

    /// `refund(f, cost)`.
    pub fn refund(&mut self, f: FactionIdx, c: &Cost) {
        if let Some(fac) = self.factions.get_mut(f as usize) {
            fac.res.refund(c);
        }
    }

    /// `roomFor(f, type)`: pawns need `pawn_n < pawn_cap`; every other kind (scout included) is a troop.
    pub fn room_for(&self, f: FactionIdx, k: UnitKind) -> bool {
        let Some(fac) = self.factions.get(f as usize) else {
            return false;
        };
        if k == UnitKind::Pawn {
            fac.pawn_n < fac.pawn_cap
        } else {
            fac.troop_n < fac.troop_cap
        }
    }

    /// Training speed of a building kind for `f` (`trainSpeed(f.has[kind] || 1)`).
    pub fn train_speed_of(&self, f: FactionIdx, k: BuildingKind) -> f32 {
        train_speed(self.factions.get(f as usize).map_or(0, |fac| fac.has[k as usize]) as u32)
    }

    /// `applyTech(f, id)` (04a §6.2): mark researched (and no longer busy); age techs → `age`, banner
    /// `${name}: ${AGE}` 4 s to all, play `age` (humans); others → mods, rescale speed/maxHp of every unit and
    /// maxHp of every building (sites too) of `f` proportionally, humans: play `research` + private banner
    /// `Ricerca completata: label` (2.6 s).
    pub fn apply_tech(&mut self, f: FactionIdx, t: TechId) {
        let fi = f as usize;
        if fi >= self.factions.len() {
            return;
        }
        let human = self.factions[fi].human;
        {
            let fac = &mut self.factions[fi];
            fac.techs.insert(t);
            fac.busy.remove(t);
        }
        if let Some(a) = t.age_target() {
            let color = self.factions[fi].color;
            self.factions[fi].age = a;
            self.emit(GameEvent::Say {
                to: None,
                msg: Msg::AgeReached { color, age: a },
                color: Some(f),
                dur: dur::AGE,
            });
            if human {
                self.emit(GameEvent::Play {
                    to: f,
                    sound: Sound::Age,
                });
            }
            return;
        }
        self.factions[fi].mods.apply_tech(t);
        let mods = self.factions[fi].mods;
        // "chi è già in campo riceve subito i miglioramenti" (JS loops every unit of f, dead ones included)
        for (_, u) in self.units.iter_mut() {
            if u.faction != f {
                continue;
            }
            let hp = mods.unit_max_hp(u.kind);
            u.speed = mods.unit_speed(u.kind);
            if hp != u.max_hp {
                if u.max_hp > 0.0 {
                    u.hp *= hp / u.max_hp;
                }
                u.max_hp = hp;
            }
        }
        for (_, b) in self.buildings.iter_mut() {
            if b.faction != f {
                continue;
            }
            let hp = mods.building_max_hp(b.kind);
            if hp != b.max_hp {
                if b.max_hp > 0.0 {
                    b.hp *= hp / b.max_hp;
                }
                b.max_hp = hp;
            }
        }
        if human {
            self.emit(GameEvent::Play {
                to: f,
                sound: Sound::Research,
            });
            self.emit(GameEvent::Say {
                to: Some(f),
                msg: Msg::ResearchDone(t),
                color: Some(f),
                dur: dur::RESEARCH,
            });
        }
    }

    /// `estimateRates(f)` (04a §11.3): expected income per minute for the HUD (humans, every 1 s). Per gathering
    /// pawn: load per trip / (work time + walk both ways along the real distance field + 0.9 s of pauses); a pawn
    /// carrying a load without a node reuses its last trip; meat capped by what farms and flocks can supply
    /// (quirk kept: the flock is a count); + 30/min per owned income point. Caches `trip`/`haul` on the pawns.
    pub fn estimate_rates(&mut self, f: FactionIdx) {
        let fi = f as usize;
        if fi >= self.factions.len() {
            return;
        }
        let side = side_of(f);
        let mods = self.factions[fi].mods;
        let w = self.map.w;
        let mut rate = [0f64; 4];
        let pawns = self.factions[fi].pawns.clone();
        for id in pawns {
            let Some(u) = self.units.get(id) else {
                continue;
            };
            if u.dead {
                continue;
            }
            let Order::Gather(o) = &u.order else {
                continue;
            };
            let res = o.res;
            let (upos, carry, speed) = (u.pos, u.carry.is_some(), u.speed);
            let node = u.node.filter(|n| self.node_valid(*n));
            if let Some(n) = node {
                let Some(npos) = self.gather_pos(n) else {
                    continue;
                };
                let site = self.drop_site_at(f, npos, res);
                let here = self.map.cell_of(npos);
                let mut d = FIELD_INF;
                if let Some(site) = site {
                    let from = self.map.cell_of(upos);
                    let fr = self.field_for(FieldTarget::Building(site), side, from);
                    d = self.fields.dist(fr.id, here);
                    if d == FIELD_INF
                        && let GatherTarget::Node(nid) = n
                    {
                        let cells = self
                            .nodes
                            .get(nid.0 as usize)
                            .map(|nd| nd.cells(&self.map))
                            .unwrap_or_default();
                        for k in cells {
                            for (dx, dy) in DIRS8 {
                                d = d.min(self.fields.dist(fr.id, k + dx + dy * w));
                            }
                        }
                    }
                }
                let sheep = matches!(n, GatherTarget::Sheep(_));
                let trip = if d != FIELD_INF {
                    (d as f32 / 10.0 - 0.7).max(0.0) + if sheep { 1.3 } else { 0.0 }
                } else {
                    -1.0
                };
                let haul = match n {
                    GatherTarget::Sheep(_) => SHEEP_MEAT,
                    GatherTarget::Node(nid) => {
                        let left = self.nodes.get(nid.0 as usize).map_or(0.0, |nd| nd.amount);
                        mods.carry_cap().min(left)
                    }
                };
                if let Some(u) = self.units.get_mut(id) {
                    u.trip = trip;
                    u.haul = haul;
                }
            } else if !carry {
                continue; // searching: does not count
            }
            let Some(u) = self.units.get(id) else {
                continue;
            };
            if !(u.trip >= 0.0) || speed <= 0.0 {
                continue;
            }
            let cycle = res.job_time() as f64 / mods.gather[res as usize] as f64
                + 2.0 * u.trip as f64 * crate::TILE as f64 / speed as f64
                + 0.9;
            rate[res as usize] += u.haul as f64 / cycle * 60.0;
        }
        let mut flock = 0u32;
        if let Some(c) = self.factions[fi]
            .castle
            .and_then(|c| self.buildings.get(c))
            .map(|c| c.pos)
        {
            for s in self.sheep.values() {
                if !s.dead && s.pos.dist(c) < WORK_RADIUS && self.explored(f, s.pos) {
                    flock += 1;
                }
            }
        }
        let farms = self.factions[fi].has[BuildingKind::Farm as usize] as f64;
        let cap = (farms * 60.0 / FARM_BREED as f64 * mods.breed as f64 + flock as f64)
            * SHEEP_MEAT as f64;
        let meat = Res::Meat as usize;
        rate[meat] = rate[meat].min(cap);
        for p in &self.points {
            if p.owner == Some(f)
                && let Some(r) = p.kind.def().res
            {
                rate[r as usize] += POINT_INCOME as f64 * 60.0;
            }
        }
        // JS Math.round (half up; all rates are ≥ 0)
        self.factions[fi].rate = rate.map(|r| (r + 0.5).floor() as i32);
    }

    /// `sellPrice(f, res)`: gold received for 100 units (fractional).
    pub fn sell_price_of(&self, f: FactionIdx, r: Res) -> f64 {
        sell_price(self.factions.get(f as usize).map_or(0.0, |fac| fac.trades[r as usize]))
    }

    /// `buyPrice(f, res)`: gold paid for 100 units (fractional).
    pub fn buy_price_of(&self, f: FactionIdx, r: Res) -> f64 {
        buy_price(self.factions.get(f as usize).map_or(0.0, |fac| fac.trades[r as usize]))
    }

    /// Market trade (04a §8, factions.js:408-420): the caller checked the market (own, done) and the resource
    /// (wood/meat/stone). Sell: needs 100 (`Risorse insufficienti`); buy: needs the price in gold
    /// (`Oro insufficiente`). Every trade worsens both prices of that resource (`trades[res]++`); coin sound.
    pub fn trade(&mut self, f: FactionIdx, op: TradeOp, r: Res) -> Result<(), String> {
        if !r.tradable() {
            return Ok(());
        }
        let Some(fac) = self.factions.get_mut(f as usize) else {
            return Ok(());
        };
        let ri = r as usize;
        match op {
            TradeOp::Sell => {
                if fac.res[r] < TRADE_AMOUNT {
                    return Err(cmd::RISORSE_INSUFFICIENTI.to_string());
                }
                fac.res[r] -= TRADE_AMOUNT;
                fac.res[Res::Gold] += sell_price(fac.trades[ri]);
            }
            TradeOp::Buy => {
                let price = buy_price(fac.trades[ri]);
                if fac.res[Res::Gold] < price {
                    return Err(cmd::ORO_INSUFFICIENTE.to_string());
                }
                fac.res[Res::Gold] -= price;
                fac.res[r] += TRADE_AMOUNT;
            }
        }
        fac.trades[ri] += 1.0;
        if fac.human {
            self.emit(GameEvent::Play {
                to: f,
                sound: Sound::Coin,
            });
        }
        Ok(())
    }

    /// `freeBuilding(f, kind)` (factions.js:671-677): finished, alive own building of `kind` with the shortest
    /// queue (first in creation order on ties). Used by the AI and the Units/Research/Market screens.
    pub fn free_building(&self, f: FactionIdx, kind: BuildingKind) -> Option<BuildingId> {
        let mut best: Option<(BuildingId, usize)> = None;
        for (id, b) in self.buildings.iter() {
            if b.faction != f || b.dead || !b.done || b.kind != kind {
                continue;
            }
            if best.is_none_or(|(_, n)| b.queue.len() < n) {
                best = Some((id, b.queue.len()));
            }
        }
        best.map(|(id, _)| id)
    }

    /// `queued(f, test)` (factions.js:678-682): number of queue items of `f` (all own buildings) matching `test`.
    pub fn queued(&self, f: FactionIdx, test: impl Fn(QueueWhat) -> bool) -> usize {
        self.buildings
            .values()
            .filter(|b| b.faction == f && !b.dead)
            .map(|b| b.queue.iter().filter(|q| test(q.what)).count())
            .sum()
    }

    /// `eliminateFaction(f, by)` (04a §13.1): kill units and destroy buildings without credit, free points,
    /// banner `${name} è caduto!` 4 s, play `win` for a human eliminator, end check. The eliminated player becomes
    /// a spectator: its fog is lifted (JS `checkEnd` → result `lose` → fog off).
    pub fn eliminate_faction(&mut self, f: FactionIdx, by: Option<FactionIdx>) {
        let fi = f as usize;
        if fi >= self.factions.len() || !self.factions[fi].alive {
            return;
        }
        self.factions[fi].alive = false;
        let units: Vec<UnitId> = self
            .units
            .iter()
            .filter(|(_, u)| u.faction == f && !u.dead)
            .map(|(id, _)| id)
            .collect();
        for u in units {
            self.kill_unit(u, DamageSource::NONE);
        }
        let blds: Vec<_> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.faction == f && !b.dead)
            .map(|(id, _)| id)
            .collect();
        for b in blds {
            self.destroy_building(b, DamageSource::NONE);
        }
        for p in self.points.iter_mut() {
            if p.owner == Some(f) {
                p.owner = None;
            }
            if p.by == Some(f) {
                p.by = None;
                p.t = 0.0;
            }
        }
        // everything is dead now: census zeroes lists, counts, has/sites and caps (the HUD of the spectator and the
        // snapshot no longer report the fallen kingdom's buildings)
        self.census(f);
        let fac = &mut self.factions[fi];
        fac.fog_off = true;
        fac.busy = TechSet::default();
        fac.ai.wave = None;
        let color = fac.color;
        self.emit(GameEvent::Say {
            to: None,
            msg: Msg::Fallen { color },
            color: Some(f),
            dur: dur::FALLEN,
        });
        if let Some(b) = by.filter(|b| (*b as usize) < self.factions.len() && self.factions[*b as usize].human) {
            self.emit(GameEvent::Play {
                to: b,
                sound: Sound::Win,
            });
        }
        self.check_end();
    }

    /// Server-side end (04a §13.2): ≤ 1 kingdom alive → `outcome` (per-player win/lose is derived client-side:
    /// lose = own kingdom fallen, win = own kingdom is the only one left). Also set for demo battles: the client
    /// restarts those 8 s later.
    pub fn check_end(&mut self) {
        if self.outcome.is_some() || self.factions.is_empty() {
            return;
        }
        let mut alive = self.factions.iter().filter(|f| f.alive).map(|f| f.idx);
        let first = alive.next();
        if alive.next().is_none() {
            self.outcome = Some(MatchOutcome {
                winner: first,
                time: self.time,
            });
        }
    }

    /// AI takeover when the player's connection is gone (04b §12.3, addenda A1, 08 N1): human = false, soldiers
    /// idle, AI state fast-forward (`ai_reset_for_takeover`, census fresh first), banner
    /// `${name}: il giocatore è uscito, ora lo guida l'IA` (4 s, all). The slot stays `owned` (exploration keeps
    /// running, A6) and `AiState` is kept across takeovers.
    pub fn takeover(&mut self, f: FactionIdx) {
        let fi = f as usize;
        if fi >= self.factions.len() {
            return;
        }
        let fac = &mut self.factions[fi];
        fac.connected = false;
        if !fac.alive || !fac.human {
            return;
        }
        fac.human = false;
        self.census(f);
        let soldiers = self.factions[fi].soldiers.clone();
        for u in soldiers {
            if self.unit_alive(u) {
                self.idle(u);
            }
        }
        self.ai_reset_for_takeover(f);
        let color = self.factions[fi].color;
        self.emit(GameEvent::Say {
            to: None,
            msg: Msg::PlayerLeft { color },
            color: Some(f),
            dur: dur::PLAYER,
        });
    }

    /// Reclaim by the returning player (04b §12.3): human = true, wave forgotten, alarm cleared, explore,
    /// visible set and census/rates on the next tick; units keep their orders; `AiState` otherwise kept for a later
    /// takeover; banner `${name}: il giocatore è tornato` (4 s, all).
    pub fn reclaim(&mut self, f: FactionIdx) {
        let fi = f as usize;
        if fi >= self.factions.len() {
            return;
        }
        let fac = &mut self.factions[fi];
        fac.connected = true;
        if !fac.alive || fac.human {
            return;
        }
        fac.human = true;
        fac.owned = true;
        fac.ai.wave = None;
        fac.ai.due_t = None;
        fac.alarm = None;
        fac.fog.see_t = 0.0;
        fac.fog.vis_t = 0.0;
        fac.tick_t = 0.0;
        let color = fac.color;
        self.emit(GameEvent::Say {
            to: None,
            msg: Msg::PlayerBack { color },
            color: Some(f),
            dur: dur::PLAYER,
        });
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::cmd::Command;
    use crate::world::{MatchSetup, SetupPlayer};
    use std::sync::OnceLock;

    /// One generated world shared by every test of this module (map generation is the slow part).
    pub(crate) fn base_world() -> World {
        static W: OnceLock<World> = OnceLock::new();
        W.get_or_init(|| {
            let setup = MatchSetup {
                players: vec![SetupPlayer {
                    color: KingdomColor::Blue,
                    name: "Ada".into(),
                }],
                level: AiLevel::Normal,
                solo: true,
            };
            World::new(11, &setup)
        })
        .clone()
    }

    pub(crate) fn human(w: &World) -> FactionIdx {
        w.factions.iter().find(|f| f.human).unwrap().idx
    }

    #[test]
    fn market_prices_and_trades() {
        assert_eq!(sell_price(0.0), 50.0);
        assert_eq!(buy_price(0.0), 130.0);
        assert!((sell_price(0.96) - 44.24).abs() < 1e-9);
        assert_eq!(sell_price(10.0), 20.0);
        assert_eq!(buy_price(30.0), 320.0);
        let mut w = base_world();
        let f = human(&w);
        w.factions[f as usize].res = Stock([250.0, 100.0, 50.0, 0.0]);
        assert_eq!(w.trade(f, TradeOp::Sell, Res::Wood), Ok(()));
        assert_eq!(w.factions[f as usize].res[Res::Wood], 150.0);
        assert_eq!(w.factions[f as usize].res[Res::Gold], 150.0);
        assert_eq!(w.factions[f as usize].trades[Res::Wood as usize], 1.0);
        // second sale is cheaper (shared counter for buy and sell)
        assert_eq!(w.trade(f, TradeOp::Sell, Res::Wood), Ok(()));
        assert_eq!(w.factions[f as usize].res[Res::Gold], 194.0);
        assert_eq!(
            w.trade(f, TradeOp::Sell, Res::Wood),
            Err("Risorse insufficienti".to_string())
        );
        // buy price after two trades: 150
        assert_eq!(w.trade(f, TradeOp::Buy, Res::Wood), Ok(()));
        assert_eq!(w.factions[f as usize].res[Res::Gold], 44.0);
        assert_eq!(
            w.trade(f, TradeOp::Buy, Res::Meat),
            Err("Oro insufficiente".to_string())
        );
        assert!(w.events.iter().any(|e| matches!(
            e,
            GameEvent::Play {
                sound: Sound::Coin,
                ..
            }
        )));
        // recovery: -0.04 per 1-s faction tick
        w.factions[f as usize].trades = [1.0, 0.0, 0.0, 0.0];
        w.factions[f as usize].tick_t = 0.01;
        w.update_faction(f);
        assert!((w.factions[f as usize].trades[0] - 0.96).abs() < 1e-12);
        for _ in 0..30 {
            w.factions[f as usize].tick_t = 0.01;
            w.update_faction(f);
        }
        assert_eq!(w.factions[f as usize].trades[0], 0.0);
    }

    #[test]
    fn tech_rescales_units_and_buildings() {
        let mut w = base_world();
        let f = human(&w);
        // a hurt pawn and a damaged castle
        let pawn = w.factions[f as usize].pawns[0];
        w.units.get_mut(pawn).unwrap().hp = 25.0;
        let castle = w.factions[f as usize].castle.unwrap();
        w.buildings.get_mut(castle).unwrap().hp = 1300.0;
        w.events.clear();
        w.apply_tech(f, TechId::Carts);
        let u = w.units.get(pawn).unwrap();
        assert!((u.speed - 92.0 * 1.15).abs() < 1e-3);
        assert_eq!(u.max_hp, 50.0);
        assert_eq!(w.factions[f as usize].mods.carry, 5.0);
        assert!(w.events.iter().any(|e| matches!(
            e,
            GameEvent::Say {
                to: Some(_),
                msg: Msg::ResearchDone(TechId::Carts),
                ..
            }
        )));
        w.apply_tech(f, TechId::Masonry);
        let c = w.buildings.get(castle).unwrap();
        assert_eq!(c.max_hp, (2600.0f32 * 1.3).round());
        assert!((c.hp - 1300.0 * 1.3).abs() < 0.01);
        // other factions untouched
        let other = (f + 1) % 5;
        let oc = w.factions[other as usize].castle.unwrap();
        assert_eq!(w.buildings.get(oc).unwrap().max_hp, 2600.0);
        // monks: scriptures doubles max hp proportionally
        let monk = w.spawn_unit(f, UnitKind::Monk, w.units.get(pawn).unwrap().pos);
        w.units.get_mut(monk).unwrap().hp = 30.0;
        w.apply_tech(f, TechId::Scriptures);
        let m = w.units.get(monk).unwrap();
        assert_eq!(m.max_hp, 130.0);
        assert!((m.hp - 60.0).abs() < 1e-3);
        // age techs: age + public banner, no mods
        w.events.clear();
        w.factions[f as usize].busy.insert(TechId::Age2);
        w.apply_tech(f, TechId::Age2);
        assert_eq!(w.factions[f as usize].age, 2);
        assert!(!w.factions[f as usize].busy.has(TechId::Age2));
        assert!(w.events.iter().any(|e| matches!(
            e,
            GameEvent::Say {
                to: None,
                msg: Msg::AgeReached { age: 2, .. },
                ..
            }
        )));
        assert!(w.events.iter().any(|e| matches!(
            e,
            GameEvent::Play {
                sound: Sound::Age,
                ..
            }
        )));
    }

    #[test]
    fn census_caps() {
        let mut w = base_world();
        let f = human(&w);
        assert_eq!(w.factions[f as usize].pawn_cap, 10);
        assert_eq!(w.factions[f as usize].troop_cap, 6);
        // finished houses / camps raise caps (capped at 30 / 50)
        for _ in 0..6 {
            let h = w.add_building(f, BuildingKind::House, 1, 1, true);
            w.block_building(h, false);
        }
        for _ in 0..7 {
            let h = w.add_building(f, BuildingKind::Camp, 1, 1, true);
            w.block_building(h, false);
        }
        let s = w.add_building(f, BuildingKind::Barracks, 1, 1, false);
        w.block_building(s, false);
        w.census(f);
        let fac = &w.factions[f as usize];
        assert_eq!(fac.pawn_cap, 30);
        assert_eq!(fac.troop_cap, 50);
        assert_eq!(fac.sites[BuildingKind::Barracks as usize], 1);
        assert_eq!(fac.has[BuildingKind::House as usize], 6);
        assert!(w.room_for(f, UnitKind::Pawn));
    }

    #[test]
    fn takeover_and_reclaim_keep_orders_and_ai_state() {
        let mut w = base_world();
        let f = human(&w);
        let warrior = w.factions[f as usize].soldiers[0];
        let pawn = w.factions[f as usize].pawns[0];
        let goal = w.units.get(pawn).unwrap().pos + crate::math::v2(64.0, 0.0);
        w.set_order(pawn, Order::Move(crate::world::MoveOrder::to(goal)));
        w.set_order(warrior, Order::Move(crate::world::MoveOrder::to(goal)));
        w.events.clear();
        w.takeover(f);
        let fac = &w.factions[f as usize];
        assert!(!fac.human && fac.owned && !fac.connected);
        // soldiers stop, pawns keep working
        assert!(w.units.get(warrior).unwrap().order.is_idle());
        assert!(matches!(w.units.get(pawn).unwrap().order, Order::Move(_)));
        assert!(w.events.iter().any(|e| matches!(
            e,
            GameEvent::Say { msg: Msg::PlayerLeft { .. }, to: None, .. }
        )));
        assert_eq!(
            Msg::PlayerLeft {
                color: KingdomColor::Blue
            }
            .text(),
            "Regno Blu: il giocatore è uscito, ora lo guida l'IA"
        );
        // AI-driven: no fog for its decisions
        assert!(w.explored(f, crate::math::v2(10.0, 10.0)));
        // the AI gives an order, then the player comes back: orders and AiState are kept
        w.set_order(warrior, Order::Move(crate::world::MoveOrder::to(goal)));
        let step = w.factions[f as usize].ai.step;
        w.events.clear();
        w.reclaim(f);
        let fac = &w.factions[f as usize];
        assert!(fac.human && fac.connected && fac.ai.wave.is_none());
        assert_eq!(fac.ai.step, step);
        assert!(matches!(w.units.get(warrior).unwrap().order, Order::Move(_)));
        assert!(w.events.iter().any(|e| matches!(
            e,
            GameEvent::Say { msg: Msg::PlayerBack { .. }, to: None, .. }
        )));
        // reclaim twice / takeover of an AI kingdom: no-ops
        w.events.clear();
        w.reclaim(f);
        let ai = (f + 1) % 5;
        w.takeover(ai);
        assert!(w.events.is_empty());
        // commands from the AI-driven kingdom still execute (the room filters them), dead kingdoms are ignored
        w.eliminate_faction(ai, Some(f));
        assert!(!w.factions[ai as usize].alive);
        assert!(w.units.values().all(|u| u.faction != ai || u.dead));
        assert!(w.is_visible(ai, crate::math::v2(5000.0, 5000.0)));
        let res = w.factions[ai as usize].res;
        assert_eq!(
            w.exec_command(ai, &Command::DebugGift { res: None }),
            Ok(())
        );
        assert_eq!(w.factions[ai as usize].res, res);
        w.reclaim(ai);
        assert!(!w.factions[ai as usize].human);
    }

    #[test]
    fn elimination_and_end() {
        let mut w = base_world();
        let f = human(&w);
        let others: Vec<FactionIdx> = (0..5).filter(|i| *i != f).collect();
        for (i, o) in others.iter().enumerate() {
            let castle = w.factions[*o as usize].castle.unwrap();
            w.destroy_building(
                castle,
                DamageSource {
                    unit: None,
                    faction: Some(f),
                    arrow: false,
                },
            );
            assert!(!w.factions[*o as usize].alive);
            if i < 3 {
                assert!(w.outcome.is_none());
            }
        }
        assert_eq!(w.outcome.map(|o| o.winner), Some(Some(f)));
        assert!(w.events.iter().any(|e| matches!(
            e,
            GameEvent::Play {
                sound: Sound::Win,
                ..
            }
        )));
        assert_eq!(w.factions[f as usize].razed, 4);
    }

    #[test]
    fn rates_count_points_and_cap_meat() {
        let mut w = base_world();
        let f = human(&w);
        // no gatherers: only points count
        w.estimate_rates(f);
        assert_eq!(w.factions[f as usize].rate, [0; 4]);
        if let Some(i) = w.points.iter().position(|p| p.kind.def().res.is_some()) {
            w.points[i].owner = Some(f);
            let r = w.points[i].kind.def().res.unwrap();
            w.estimate_rates(f);
            assert_eq!(w.factions[f as usize].rate[r as usize], 30);
        }
    }
}
