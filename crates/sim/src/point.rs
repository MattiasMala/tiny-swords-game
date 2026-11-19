// OWNER: S3 (economy) — see docs/spec/03b-gather-buildings.md §11 (capture points: data, updatePoints every 0.5 s,
// capturePoint, UI strings) and 01 §4 (POINT_DEF, constants); 04a §13.1 (points freed on elimination).
#![allow(dead_code)]

use crate::TICK_DT;
use crate::World;
use crate::defs::*;
use crate::events::{GameEvent, Msg, dur};
use crate::math::{V2, v2};
use crate::slab::PointId;
use crate::world::CapturePoint;

impl World {
    /// `updatePoints(dt)` (§11.3): continuous income `0.5*dt` (+ `gathered`) to the owner of a resource point;
    /// every 0.5 s (`point_t`): count non-pawn active units within 300 px (`|dx| <= 300` and circle centred 64 px
    /// above the anchor); contested → frozen; nobody/owner → progress decays; challenger → +0.5, capture at 8 s.
    pub fn update_points(&mut self) {
        let dt = TICK_DT;
        self.point_t -= dt;
        let tick = self.point_t <= 0.0;
        if tick {
            self.point_t = CAPTURE_TICK;
        }
        for i in 0..self.points.len() {
            let (owner, res, pos) = {
                let p = &self.points[i];
                (p.owner, p.kind.def().res, p.pos)
            };
            if let (Some(o), Some(r)) = (owner, res)
                && let Some(f) = self.factions.get_mut(o as usize)
            {
                f.res[r] += POINT_INCOME as f64 * dt as f64;
                f.gathered += POINT_INCOME as f64 * dt as f64;
            }
            if !tick {
                continue;
            }
            let centre = point_sight_centre(&self.points[i]);
            let mut one: Option<FactionIdx> = None;
            let mut many = false;
            for u in self.units.values() {
                if u.dead
                    || u.inside.is_some()
                    || u.kind == UnitKind::Pawn
                    || (u.pos.x - pos.x).abs() > CAPTURE_RADIUS
                    || u.pos.dist(centre) > CAPTURE_RADIUS
                {
                    continue;
                }
                if one.is_some_and(|o| o != u.faction) {
                    many = true;
                    break;
                }
                one = Some(u.faction);
            }
            if many {
                continue; // contested: everything frozen
            }
            let p = &mut self.points[i];
            match one {
                Some(c) if Some(c) != owner => {
                    if p.by != Some(c) {
                        p.by = Some(c);
                        p.t = 0.0;
                    }
                    p.t += CAPTURE_TICK;
                    if p.t >= CAPTURE_TIME {
                        self.capture_point(PointId(i as u32), c);
                    }
                }
                _ => {
                    p.t = (p.t - CAPTURE_TICK).max(0.0);
                    if p.t == 0.0 {
                        p.by = None;
                    }
                }
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
        if let Some(o) = old.filter(|o| *o != f && self.factions[*o as usize].human) {
            self.emit(GameEvent::Alert {
                to: o,
                x: pos.x.round(),
                y: pos.y.round(),
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

/// Owner line of the point info panel (§11.5): `È tuo` / `Lo tiene il Regno X` / `Libero`.
/// `owner_name` = name of the owning kingdom (`Regno Rosso`), `mine` = the viewer owns it.
pub fn point_owner_text(owner_name: Option<&str>, mine: bool) -> String {
    match owner_name {
        Some(_) if mine => "È tuo".to_string(),
        Some(n) => format!("Lo tiene il {n}"),
        None => "Libero".to_string(),
    }
}

/// Capture progress line (§11.5), shown when `by && t > 0`:
/// `Lo stai conquistando: N%` / `Lo sta conquistando il Regno X: N%`.
pub fn point_progress_text(by_name: &str, mine: bool, t: f32) -> String {
    let pct = (100.0 * t / CAPTURE_TIME).round() as i32;
    if mine {
        format!("Lo stai conquistando: {pct}%")
    } else {
        format!("Lo sta conquistando il {by_name}: {pct}%")
    }
}

/// Body text of the point info panel (§11.5): description + income / zone + capture rule.
pub fn point_body_text(kind: PointKind) -> String {
    let d = kind.def();
    let mid = if d.res.is_some() {
        format!(" +{} al minuto.", (POINT_INCOME * 60.0).round() as i32)
    } else {
        format!(" Entro {} caselle.", OUTPOST_RADIUS as i32)
    };
    format!(
        "{}{} Si conquista tenendoci accanto delle truppe per {} secondi, senza nemici attorno.",
        d.desc, mid, CAPTURE_TIME as i32
    )
}

/// Centre of the capture circle and of the sight disc of a point (`(p.x, p.y - 64)`).
pub fn point_sight_centre(p: &CapturePoint) -> V2 {
    v2(p.pos.x, p.pos.y - crate::TILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gather::tests::{run, spawn, test_world};

    /// The test world's mine sits at cells (60..61, 20..21); its capture circle is centred at (3904, 1340).
    #[test]
    fn capture_timing_income_and_contest() {
        let mut w = test_world();
        // pawns never count
        spawn(&mut w, 0, UnitKind::Pawn, 60, 23);
        run(&mut w, 9.0);
        assert_eq!(w.points[0].owner, None);
        assert_eq!(w.points[0].t, 0.0);
        let s = spawn(&mut w, 0, UnitKind::Scout, 62, 23);
        run(&mut w, 7.0);
        assert_eq!(w.points[0].owner, None);
        assert_eq!(w.points[0].by, Some(0));
        run(&mut w, 1.1);
        assert_eq!(
            w.points[0].owner,
            Some(0),
            "captured after 8 s of sole presence"
        );
        assert!(w.events.iter().any(|e| matches!(
            e,
            GameEvent::Say {
                to: Some(0),
                msg: Msg::PointCaptured(PointKind::Mine),
                ..
            }
        )));
        // income 0.5/s (30/min) of gold, counted as gathered
        let (g0, gathered0) = (w.factions[0].res[Res::Gold], w.factions[0].gathered);
        run(&mut w, 10.0);
        let got = w.factions[0].res[Res::Gold] - g0;
        assert!((got - 5.0).abs() < 1e-3, "income {got}");
        assert!((w.factions[0].gathered - gathered0 - 5.0).abs() < 1e-3);
        // contested: progress frozen
        let e = spawn(&mut w, 1, UnitKind::Warrior, 60, 18);
        run(&mut w, 3.0);
        w.kill_unit(s, crate::world::DamageSource::NONE);
        run(&mut w, 0.1);
        // now the enemy alone: counts up from 0
        let t_before = w.points[0].t;
        assert!(t_before <= 0.5);
        if w.units.get(e).is_some_and(|u| !u.dead) {
            run(&mut w, 4.0);
            assert_eq!(w.points[0].by, Some(1));
            assert!(w.points[0].t > 3.0);
            // the challenger leaves: progress decays 1 s/s, then `by` is cleared
            w.kill_unit(e, crate::world::DamageSource::NONE);
            run(&mut w, 5.0);
            assert_eq!(w.points[0].t, 0.0);
            assert_eq!(w.points[0].by, None);
            assert_eq!(w.points[0].owner, Some(0));
        }
    }

    #[test]
    fn point_strings() {
        assert_eq!(point_owner_text(None, false), "Libero");
        assert_eq!(point_owner_text(Some("Regno Blu"), true), "È tuo");
        assert_eq!(
            point_owner_text(Some("Regno Rosso"), false),
            "Lo tiene il Regno Rosso"
        );
        assert_eq!(
            point_progress_text("Regno Rosso", false, 4.0),
            "Lo sta conquistando il Regno Rosso: 50%"
        );
        assert_eq!(
            point_progress_text("", true, 2.0),
            "Lo stai conquistando: 25%"
        );
        assert_eq!(
            point_body_text(PointKind::Mine),
            "Rende oro a chi la tiene, senza Pedoni. +30 al minuto. Si conquista tenendoci accanto delle \
             truppe per 8 secondi, senza nemici attorno."
        );
        assert_eq!(
            point_body_text(PointKind::Outpost),
            "Chi lo tiene può costruire nei dintorni, torri e palizzate comprese. Entro 9 caselle. Si \
             conquista tenendoci accanto delle truppe per 8 secondi, senza nemici attorno."
        );
    }
}
