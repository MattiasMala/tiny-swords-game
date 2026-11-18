// OWNER: S3 (economy) — see docs/spec/03b-gather-buildings.md §11 (capture points: data, updatePoints every 0.5 s,
// capturePoint, UI strings) and 01 §4 (POINT_DEF, constants); 04a §13.1 (points freed on elimination).
//
// Stub fns S3 must fill: update_points (MINIMAL: income only), capture_point (MINIMAL: owner change + events).
#![allow(dead_code, unused)]

use crate::TICK_DT;
use crate::World;
use crate::defs::*;
use crate::events::{GameEvent, Msg, dur};
use crate::slab::PointId;

impl World {
    /// `updatePoints(dt)` (§11.3): continuous income `0.5*dt` (+ `gathered`) to the owner of a resource point;
    /// every 0.5 s (`point_t`): count non-pawn active units within 300 px (`|dx| <= 300` and circle centred 64 px
    /// above the anchor); contested → frozen; nobody/owner → progress decays; challenger → +0.5, capture at 8 s.
    /// MINIMAL: income only.
    pub fn update_points(&mut self) {
        let dt = TICK_DT as f64;
        for i in 0..self.points.len() {
            let p = &self.points[i];
            if let (Some(o), Some(r)) = (p.owner, p.kind.def().res) {
                let f = &mut self.factions[o as usize];
                f.res[r] += POINT_INCOME as f64 * dt;
                f.gathered += POINT_INCOME as f64 * dt;
            }
        }
    }

    /// `capturePoint(p, f)` (§11.4): owner, reset, dust2 ×1.6, human capturer: play build + banner `Hai
    /// conquistato`, human loser: alert + banner `Hai perso`.
    pub fn capture_point(&mut self, p: PointId, f: FactionIdx) {
        let Some(pt) = self.points.get_mut(p.0 as usize) else {
            return;
        };
        let old = pt.owner;
        pt.owner = Some(f);
        pt.by = None;
        pt.t = 0.0;
        let (pos, kind) = (pt.pos, pt.kind);
        self.emit(GameEvent::Fx {
            fx: FxKind::Dust2,
            x: pos.x,
            y: pos.y,
            scale: 1.6,
            dy: 0.0,
            follow: None,
            color: None,
        });
        if self.factions[f as usize].human {
            self.emit(GameEvent::Play {
                to: f,
                sound: Sound::Build,
            });
            self.emit(GameEvent::Say {
                to: Some(f),
                msg: Msg::PointCaptured(kind),
                color: Some(f),
                dur: dur::CAPTURED,
            });
        }
        if let Some(o) = old.filter(|o| self.factions[*o as usize].human) {
            self.emit(GameEvent::Alert {
                to: o,
                x: pos.x,
                y: pos.y,
            });
            self.emit(GameEvent::Say {
                to: Some(o),
                msg: Msg::PointLost(kind),
                color: Some(f),
                dur: dur::LOST,
            });
        }
    }
}
