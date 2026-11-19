// OWNER: S3 (economy) — see docs/spec/03b-gather-buildings.md §2.1 (makeSheep), §2.2 (killSheep), §2.3 sheep part
// of updateResources (idle/grass/move state machine at 38 px/s via map.try_move side 0, wild meadow respawn every
// 10 s, < 3 per meadow), §8.11 farm breeding hook (counts sheep with `home == Farm(b)`), addenda G1 (home by id),
// G4 (sheep keep wandering while being butchered).
#![allow(dead_code)]

use crate::TICK_DT;
use crate::World;
use crate::defs::*;
use crate::events::GameEvent;
use crate::math::{V2, v2};
use crate::slab::SheepId;
use crate::world::{Sheep, SheepHome, SheepState};

impl World {
    /// Wander centre of a home (farm pasture read from the farm each time; destroyed farm → `None`, but
    /// `destroy_building` turns those homes into `Spot`s first).
    pub fn sheep_home_pos(&self, h: SheepHome) -> Option<V2> {
        match h {
            SheepHome::Meadow(i) => self.meadows.get(i as usize).copied(),
            SheepHome::Farm(b) => self.buildings.get(b).filter(|b| !b.dead).map(|b| b.pasture),
            SheepHome::Spot(p) => Some(p),
        }
    }

    /// `makeSheep(home)` (§2.1): at `home ± (90, 60)` [RNG] (or `nearest_passable(home)` if not passable),
    /// state idle, timer `rand(0.5, 3)`, flip [RNG].
    pub fn make_sheep(&mut self, home: SheepHome) -> Option<SheepId> {
        let h = self.sheep_home_pos(home)?;
        let mut p = v2(
            h.x + self.rng.range(-90.0, 90.0),
            h.y + self.rng.range(-60.0, 60.0),
        );
        if !self.map.passable(self.map.cell_of(p), 0) {
            p = self.map.nearest_passable(h);
        }
        let t = self.rng.range(0.5, 3.0);
        let flip = self.rng.chance(0.5);
        Some(self.sheep.insert(Sheep {
            pos: p,
            home,
            state: SheepState::Idle,
            t,
            anim_t: 0.0,
            flip,
            target: p,
            users: 0,
            dead: false,
        }))
    }

    /// `killSheep(s)` (§2.2): dead + dust1 (removed at the end of `update_sheep`).
    pub fn kill_sheep(&mut self, id: SheepId) {
        let Some(s) = self.sheep.get_mut(id) else {
            return;
        };
        if s.dead {
            return;
        }
        s.dead = true;
        let p = s.pos;
        self.emit(GameEvent::fx(FxKind::Dust1, p.x, p.y));
    }

    /// Sheep part of `updateResources(dt)` (§2.3) + wild respawn (`sheep_t`, 10 s, random meadow [RNG]).
    /// Sheep are side 0: every building and palisade blocks them.
    pub fn update_sheep(&mut self) {
        let dt = TICK_DT;
        for id in self.sheep.ids() {
            let Some(s) = self.sheep.get(id) else {
                continue;
            };
            if s.dead {
                continue;
            }
            let home = s.home;
            let s = self.sheep.get_mut(id).expect("checked");
            s.anim_t += dt;
            s.t -= dt;
            if s.state == SheepState::Move {
                let dvec = s.target - s.pos;
                let d = dvec.len();
                if d < 4.0 || s.t <= 0.0 {
                    s.state = SheepState::Idle;
                    s.t = self.rng.range(1.0, 3.0);
                    s.anim_t = 0.0;
                } else {
                    let step = dvec * (SHEEP_SPEED * dt / d);
                    s.pos = self.map.try_move(s.pos, step.x, step.y, 0, false);
                    s.flip = dvec.x < 0.0;
                }
            } else if s.t <= 0.0 {
                // JS: `state === 'idle' && Math.random() < 0.45` (no draw from 'grass')
                if s.state == SheepState::Idle && self.rng.chance(0.45) {
                    s.state = SheepState::Grass;
                    s.t = 1.2;
                    s.anim_t = 0.0;
                } else {
                    s.state = SheepState::Move;
                    s.t = 4.0;
                    s.anim_t = 0.0;
                    let jx = self.rng.range(-120.0, 120.0);
                    let jy = self.rng.range(-80.0, 80.0);
                    let h = self.sheep_home_pos(home);
                    let s = self.sheep.get_mut(id).expect("checked");
                    let h = h.unwrap_or(s.pos);
                    s.target = v2(h.x + jx, h.y + jy);
                }
            }
        }
        self.sheep.retain(|_, s| !s.dead);
        self.sheep_t -= dt;
        if self.sheep_t <= 0.0 {
            self.sheep_t = SHEEP_RESPAWN;
            if let Some(m) = self.rng.pick_index(self.meadows.len()) {
                let home = SheepHome::Meadow(m as u16);
                let n = self.sheep.values().filter(|s| s.home == home).count();
                if n < SHEEP_PER_MEADOW
                    && let Some(id) = self.make_sheep(home)
                    && let Some(p) = self.sheep.get(id).map(|s| s.pos)
                {
                    self.emit(GameEvent::fx(FxKind::Dust1, p.x, p.y));
                }
            }
        }
    }
}
