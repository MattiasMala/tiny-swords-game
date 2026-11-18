// OWNER: S3 (economy) — see docs/spec/03b-gather-buildings.md §2.1 (makeSheep), §2.2 (killSheep), §2.3 sheep part
// of updateResources (idle/grass/move state machine at 38 px/s via map.try_move side 0, wild meadow respawn every
// 10 s, < 3 per meadow), §8.11 farm breeding hook (counts sheep with `home == Farm(b)`), addenda G1 (home by id).
//
// Stub fns S3 must fill: update_sheep (STUB: removes dead sheep only). Done (verify): make_sheep, kill_sheep,
// sheep_home_pos.
#![allow(dead_code, unused)]

use crate::World;
use crate::defs::*;
use crate::events::GameEvent;
use crate::math::{V2, v2};
use crate::slab::SheepId;
use crate::world::{Sheep, SheepHome, SheepState};

impl World {
    /// Wander centre of a home (farm pasture read from the farm each time; destroyed farm → `None`).
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
        s.dead = true;
        let p = s.pos;
        self.emit(GameEvent::fx(FxKind::Dust1, p.x, p.y));
    }

    /// Sheep part of `updateResources(dt)` (§2.3) + wild respawn (`sheep_t`, 10 s, random meadow [RNG]).
    /// STUB: only removes dead sheep.
    pub fn update_sheep(&mut self) {
        self.sheep.retain(|_, s| !s.dead);
    }
}
