// OWNER: S4 (faction) — see docs/spec/04a-factions-economy.md §12 (execCommand: every command, validation order,
// refusal strings), §5.4 (trainBlock / techBlock / ageLock / techLock), §9 (canPlace via `Map::can_place`,
// zoneBlock via `World::zone_block`, placeBlock, moveBlock), §8 (trade), §12.2 setWorkers, §12.3 job, §12.4
// recall; addenda E2 (fog check on `place` for humans), E6; 05 addenda C5 (test-mode commands, solo only).
//
// Stub fns S4 must fill: exec_command (MINIMAL: `Order` dispatch + test-mode commands; every other command is
// accepted and ignored), train_block, tech_block, place_block, move_block, set_workers.
// `cmd::Command::well_formed()` already rejects oversized id lists / non-finite coordinates.
#![allow(dead_code, unused)]

use crate::World;
use crate::cmd::{self, Command};
use crate::defs::*;
use crate::slab::BuildingId;

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
            // STUB: Place, Jobs, Job, Recall, Train, Research, Cancel, Rally, Move, Demolish, Trade (04a §12).
            _ => Ok(()),
        }
    }

    /// `trainBlock(f, b, type)` (04a §5.4): `Non disponibile` | `Richiede: AGE` | `Coda piena` |
    /// `Risorse insufficienti`. STUB: `None`.
    pub fn train_block(&self, f: FactionIdx, b: BuildingId, k: UnitKind) -> Option<String> {
        None
    }

    /// `techBlock(f, b, id)` (04a §5.4): `Non disponibile` | `Già ricercata` | `Ricerca in corso` | age/req lock |
    /// `Coda piena` | `Risorse insufficienti`. STUB: `None`.
    pub fn tech_block(&self, f: FactionIdx, b: BuildingId, t: TechId) -> Option<String> {
        None
    }

    /// `placeBlock(f, kind, cx, cy)` (04a §9.3 + E2 fog check for humans). STUB: `None`.
    pub fn place_block(
        &self,
        f: FactionIdx,
        kind: BuildingKind,
        cx: i32,
        cy: i32,
    ) -> Option<String> {
        None
    }

    /// `moveBlock(f, b, cx, cy)` (04a §9.3). STUB: `None`.
    pub fn move_block(&self, f: FactionIdx, b: BuildingId, cx: i32, cy: i32) -> Option<String> {
        None
    }

    /// `setWorkers(f, res, n)` (04a §12.2). STUB: `Ok`.
    pub fn set_workers(&mut self, f: FactionIdx, res: Res, n: u16) -> Result<(), String> {
        Ok(())
    }
}
