// OWNER: S4 (faction) — see docs/spec/04a-factions-economy.md §3 (faction data, setup), §4 (census, caps),
// §5.5 (canAfford/pay/refund), §6 (applyTech), §7 (updateFaction), §8 (market prices), §11.3 (estimateRates),
// §13 (elimination, end of match, reconnect), addenda E1-E7; 04b §12 (takeover / reclaim) + addenda A1, A6;
// 08-net addenda N1.
//
// Stub fns S4 must fill: update_faction (MINIMAL: census every 1 s, trades decay, AI trickle + think dispatch —
// verify order and add `estimate_rates`), estimate_rates (STUB), apply_tech (MINIMAL: no unit/building rescale,
// no events), eliminate_faction (MINIMAL). Done (verify): census, can_afford, pay, refund, room_for, check_end,
// takeover, reclaim.
#![allow(dead_code, unused)]

use crate::TICK_DT;
use crate::World;
use crate::defs::*;
use crate::events::{GameEvent, Msg, dur};
use crate::slab::UnitId;
use crate::world::{DamageSource, MatchOutcome};

impl World {
    /// `updateFaction(f, dt)` (04a §7, 04b §11): AI trickle, exploration (`explore`, for any human-OWNED faction,
    /// addenda A6), 1-s tick (census, rates for humans, trades decay), AI think (`ai_tick`) every `level.think`.
    pub fn update_faction(&mut self, f: FactionIdx) {
        let dt = TICK_DT;
        if !self.factions[f as usize].alive {
            return;
        }
        let (human, owned) = (
            self.factions[f as usize].human,
            self.factions[f as usize].owned,
        );
        if !human {
            let trickle = self.factions[f as usize].ai.level.def().trickle;
            for r in Res::ALL {
                self.factions[f as usize].res[*r] += trickle * dt as f64;
            }
        }
        if owned {
            let fac = &mut self.factions[f as usize];
            fac.fog.see_t -= dt;
            if fac.fog.see_t <= 0.0 {
                fac.fog.see_t = EXPLORE_PERIOD;
                self.explore(f);
            }
        }
        let fac = &mut self.factions[f as usize];
        fac.tick_t -= dt;
        if fac.tick_t <= 0.0 {
            fac.tick_t = CENSUS_PERIOD;
            self.census(f);
            if human {
                self.estimate_rates(f);
            }
            for t in self.factions[f as usize].trades.iter_mut() {
                *t = (*t - TRADE_DECAY).max(0.0);
            }
        }
        if !human {
            let fac = &mut self.factions[f as usize];
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
        let mut pawns = Vec::new();
        let mut soldiers = Vec::new();
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
            pc += b.def().pawns;
            tc += b.def().troops;
        }
        let fac = &mut self.factions[f as usize];
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
        self.factions[f as usize].res.can_afford(c)
    }

    /// `pay(f, cost)` (no check).
    pub fn pay(&mut self, f: FactionIdx, c: &Cost) {
        self.factions[f as usize].res.pay(c);
    }

    /// `refund(f, cost)`.
    pub fn refund(&mut self, f: FactionIdx, c: &Cost) {
        self.factions[f as usize].res.refund(c);
    }

    /// `roomFor(f, type)`: pawns need `pawn_n < pawn_cap`; every other kind (scout included) is a troop.
    pub fn room_for(&self, f: FactionIdx, k: UnitKind) -> bool {
        let fac = &self.factions[f as usize];
        if k == UnitKind::Pawn {
            fac.pawn_n < fac.pawn_cap
        } else {
            fac.troop_n < fac.troop_cap
        }
    }

    /// Training speed of a building kind for `f` (`trainSpeed(f.has[kind] || 1)`).
    pub fn train_speed_of(&self, f: FactionIdx, k: BuildingKind) -> f32 {
        train_speed(self.factions[f as usize].has[k as usize] as u32)
    }

    /// `applyTech(f, id)` (04a §6.2): mark researched; age techs → `age`, banner `${name}: ${AGE}` 4 s to all,
    /// play `age` (humans); others → mods, rescale speed/maxHp of units and maxHp of buildings, humans: play
    /// `research` + banner `Ricerca completata`. MINIMAL: techs + age + mods only.
    pub fn apply_tech(&mut self, f: FactionIdx, t: TechId) {
        let fac = &mut self.factions[f as usize];
        fac.techs.insert(t);
        if let Some(a) = t.age_target() {
            fac.age = a;
        } else {
            fac.mods.apply_tech(t);
        }
    }

    /// `estimateRates(f)` (04a §11.3): income per minute for the HUD (humans, every 1 s). STUB: zero.
    pub fn estimate_rates(&mut self, f: FactionIdx) {
        self.factions[f as usize].rate = [0; 4];
    }

    /// `eliminateFaction(f, by)` (04a §13.1): kill units and destroy buildings without credit, free points,
    /// banner `${name} è caduto!` 4 s, play `win` for a human eliminator, end check. MINIMAL: complete except
    /// event ordering details.
    pub fn eliminate_faction(&mut self, f: FactionIdx, by: Option<FactionIdx>) {
        if !self.factions[f as usize].alive {
            return;
        }
        self.factions[f as usize].alive = false;
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
        let color = self.factions[f as usize].color;
        self.emit(GameEvent::Say {
            to: None,
            msg: Msg::Fallen { color },
            color: Some(f),
            dur: dur::FALLEN,
        });
        if let Some(b) = by.filter(|b| self.factions[*b as usize].human) {
            self.emit(GameEvent::Play {
                to: b,
                sound: Sound::Win,
            });
        }
        self.check_end();
    }

    /// Server-side end (04a §13.2): ≤ 1 kingdom alive → `outcome` (per-player win/lose is derived client-side).
    /// Demo battles never end here (the client restarts them).
    pub fn check_end(&mut self) {
        if self.outcome.is_some() || self.factions.is_empty() {
            return;
        }
        let alive: Vec<FactionIdx> = self
            .factions
            .iter()
            .filter(|f| f.alive)
            .map(|f| f.idx)
            .collect();
        if alive.len() <= 1 {
            self.outcome = Some(MatchOutcome {
                winner: alive.first().copied(),
                time: self.time,
            });
        }
    }

    /// AI takeover when the player's connection is gone (04b §12.3, addenda A1, 08 N1): human = false, soldiers
    /// idle, AI state fast-forward (`ai_reset_for_takeover`), banner `il giocatore è uscito` (4 s, all).
    pub fn takeover(&mut self, f: FactionIdx) {
        let fac = &mut self.factions[f as usize];
        fac.connected = false;
        if !fac.alive || !fac.human {
            return;
        }
        fac.human = false;
        self.census(f);
        let soldiers = self.factions[f as usize].soldiers.clone();
        for u in soldiers {
            self.idle(u);
        }
        self.ai_reset_for_takeover(f);
        let color = self.factions[f as usize].color;
        self.emit(GameEvent::Say {
            to: None,
            msg: Msg::PlayerLeft { color },
            color: Some(f),
            dur: dur::PLAYER,
        });
    }

    /// Reclaim by the returning player (04b §12.3): human = true, wave forgotten, alarm cleared, explore and
    /// census/rates on the next tick; units keep their orders; banner `il giocatore è tornato` (4 s, all).
    pub fn reclaim(&mut self, f: FactionIdx) {
        let fac = &mut self.factions[f as usize];
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
