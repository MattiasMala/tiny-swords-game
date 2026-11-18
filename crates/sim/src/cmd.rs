//! Player commands (typed `execCommand`). OWNER: foundation (types); execution lives in `command.rs` (S4).
//!
//! Spec 04a §12 (complete list, payloads, validation, refusal strings) and 08 §A.10. Every local player action —
//! solo or network — becomes a [`Command`] executed by `World::exec_command(f, &cmd) -> Result<(), String>`; an
//! `Err` carries the Italian refusal the client shows as toast + `deny` sound (sent back in
//! `ServerMsg::CommandResult`). Malformed payloads are silently ignored (`Ok(())`), exactly like the JS.
//! The AI issues the same commands (04a §14).
//!
//! Test-mode commands (`Debug*`, spec 04a §15.8 / 05 §15.2) are accepted only by a solo room (`Room` checks
//! [`Command::is_debug`] — the server ignores them silently, 05 addenda C5).

use crate::defs::{BuildingKind, OUTPOST_RADIUS, Res, TechId, UnitKind};
use crate::slab::{BuildingId, NodeId, PointId, SheepId, UnitId};
use serde::{Deserialize, Serialize};

/// Max ids in one command (`ids array ≤ 200 else ignored`).
pub const MAX_IDS: usize = 200;

/// Reference to any targetable entity (JS `targetId` resolved through `G.index`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EntityRef {
    Unit(UnitId),
    Building(BuildingId),
    /// tree / gold / stone (small or big)
    Node(NodeId),
    Sheep(SheepId),
    Point(PointId),
}

/// `order` kinds (`orderUnits`, entities.js:353-406).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum OrderKind {
    /// formation move, ignores enemies
    Move,
    /// attack-move (soldiers fight on the way; pawns/monks/scouts get a plain move)
    AMove,
    /// `stop`: idle in place
    Stop,
    /// scouts only: autonomous exploration
    Explore,
    /// target enemy unit/building (monks become escorting idle)
    Attack,
    /// pawns: gather the target node/sheep
    Gather,
    /// pawns: build or repair an own building
    Build,
    /// pawns: deliver load to an own finished building
    Drop,
    /// monks: heal an own unit
    Heal,
}

/// Queue entry identity (cancel guard; JS `q.unit || q.tech`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QueueWhat {
    Unit(UnitKind),
    Tech(TechId),
}

/// Market operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TradeOp {
    Sell,
    Buy,
}

/// Every player command (04a §12 table + test mode).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// `order(ids[≤200], kind, x, y, targetId)` → `orderUnits`. Always `Ok`.
    Order {
        units: Vec<UnitId>,
        kind: OrderKind,
        x: f32,
        y: f32,
        target: Option<EntityRef>,
    },
    /// `place(kind, cx, cy, builderIds)`: kind ∈ BUILD_MENU, top-left cell. Only the first [`MAX_IDS`] builders
    /// get `build` (JS `d.slice(0, 200)`: a longer list is truncated, NOT a reason to ignore the command).
    Place {
        kind: BuildingKind,
        cx: i32,
        cy: i32,
        builders: Vec<UnitId>,
    },
    /// `jobs(res, n)`: Workers sliders → `setWorkers`.
    Jobs { res: Res, n: u16 },
    /// `job(ids[≤200], res)`: send the listed own pawns to the nearest free node of `res`.
    Job { pawns: Vec<UnitId>, res: Res },
    /// `recall(ids[≤200])`: non-pawn units move to `(castle.x, castle.y1 + 130)`.
    Recall { units: Vec<UnitId> },
    /// `train(buildingId, unit)`.
    Train {
        building: BuildingId,
        unit: UnitKind,
    },
    /// `research(buildingId, tech)`.
    Research { building: BuildingId, tech: TechId },
    /// `cancel(buildingId, index, expected)`: removed only if `queue[index]` still matches; full refund.
    Cancel {
        building: BuildingId,
        index: u8,
        expected: QueueWhat,
    },
    /// `rally(buildingId, x, y, targetId)`: target kept if a node/sheep or an own building.
    Rally {
        building: BuildingId,
        x: f32,
        y: f32,
        target: Option<EntityRef>,
    },
    /// `move(buildingId, cx, cy)`: instant free relocation (never the castle).
    Move {
        building: BuildingId,
        cx: i32,
        cy: i32,
    },
    /// `demolish(buildingId)`: site refunds the full cost, finished building nothing.
    Demolish { building: BuildingId },
    /// `trade(marketId, sell|buy, res)`: res ∈ wood, meat, stone; 100 units.
    Trade {
        market: BuildingId,
        op: TradeOp,
        res: Res,
    },
    /// TEST MODE (solo only): `+1000` of one resource (`None` = all four, `+1000 di tutte e quattro`).
    DebugGift { res: Option<Res> },
    /// TEST MODE: freeze / unfreeze sim time (commands still execute while frozen).
    DebugFreeze { on: bool },
    /// TEST MODE: remove the fog for this player (`Togli la nebbia...`).
    DebugFogOff { on: bool },
}

impl Command {
    /// Test-mode command (solo only).
    pub fn is_debug(&self) -> bool {
        matches!(
            self,
            Command::DebugGift { .. } | Command::DebugFreeze { .. } | Command::DebugFogOff { .. }
        )
    }

    /// Shape validation shared by server and solo (JS: arrays ≤ 200, finite coordinates). A command that fails
    /// it is ignored without a message.
    pub fn well_formed(&self) -> bool {
        match self {
            // JS: only move/amove need a finite point (`orderUnits` returns early otherwise); stop/explore/attack/
            // gather/build/drop/heal ignore x, y.
            Command::Order { units, kind, x, y, .. } => {
                units.len() <= MAX_IDS
                    && (!matches!(kind, OrderKind::Move | OrderKind::AMove)
                        || (x.is_finite() && y.is_finite()))
            }
            // builders are truncated to MAX_IDS by the executor (factions.js:350), never rejected
            Command::Place { .. } => true,
            Command::Job { pawns, .. } => pawns.len() <= MAX_IDS,
            Command::Recall { units } => units.len() <= MAX_IDS,
            Command::Rally { x, y, .. } => x.is_finite() && y.is_finite(),
            Command::Trade { res, .. } => res.tradable(),
            _ => true,
        }
    }
}

// ---- refusal strings (verbatim, spec 01 §2.4/§3.1/§5, 04a §5.4/§9/§12) ----

pub const NON_DISPONIBILE: &str = "Non disponibile";
pub const CODA_PIENA: &str = "Coda piena";
pub const RISORSE_INSUFFICIENTI: &str = "Risorse insufficienti";
pub const GIA_RICERCATA: &str = "Già ricercata";
pub const RICERCA_IN_CORSO: &str = "Ricerca in corso";
pub const QUI_NON_SI_PUO_COSTRUIRE: &str = "Qui non si può costruire";
pub const QUI_NON_PUO_STARE: &str = "Qui non può stare";
pub const CASTELLO_NON_SI_SPOSTA: &str = "Il castello non si sposta";
pub const CASTELLO_NON_SI_DEMOLISCE: &str = "Il castello non si demolisce";
pub const ORO_INSUFFICIENTE: &str = "Oro insufficiente";
pub const NESSUN_PEDONE_LIBERO: &str = "Nessun pedone libero";
/// network refusal of F4 (client-side toast)
pub const SOLO_TEST_MODE: &str = "La modalità test c'è solo nelle partite da soli";
/// client-side refusals (Build screen / recall) kept here for one source of truth
pub const NON_HAI_PEDONI: &str = "Non hai pedoni";
pub const NON_HAI_TRUPPE: &str = "Non hai truppe";

/// `'Richiede: ' + what` (age name, tech label or building label).
pub fn richiede(what: &str) -> String {
    format!("Richiede: {what}")
}

/// `` `${RES_LABEL[res]}: nessun posto libero nel territorio esplorato` ``.
pub fn nessun_posto_libero(res: Res) -> String {
    format!(
        "{}: nessun posto libero nel territorio esplorato",
        res.label()
    )
}

/// Zone refusal (factions.js:161-166):
/// `${defense ? 'Torri e palizzate' : 'Troppo lontano'}: entro ${r} caselle dal castello o 9 da un tuo avamposto`.
pub fn zone_refusal(defense: bool) -> String {
    let (head, r) = if defense {
        ("Torri e palizzate", 12)
    } else {
        ("Troppo lontano", 40)
    };
    format!(
        "{head}: entro {r} caselle dal castello o {} da un tuo avamposto",
        OUTPOST_RADIUS as i32
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings() {
        assert_eq!(
            zone_refusal(true),
            "Torri e palizzate: entro 12 caselle dal castello o 9 da un tuo avamposto"
        );
        assert_eq!(
            zone_refusal(false),
            "Troppo lontano: entro 40 caselle dal castello o 9 da un tuo avamposto"
        );
        assert_eq!(
            nessun_posto_libero(Res::Gold),
            "Oro: nessun posto libero nel territorio esplorato"
        );
        assert_eq!(richiede("Età Feudale"), "Richiede: Età Feudale");
    }

    #[test]
    fn shape() {
        let big = Command::Recall {
            units: vec![UnitId::default(); 201],
        };
        assert!(!big.well_formed());
        let nan = Command::Order {
            units: vec![],
            kind: OrderKind::Move,
            x: f32::NAN,
            y: 0.0,
            target: None,
        };
        assert!(!nan.well_formed());
        let stop_nan = Command::Order {
            units: vec![],
            kind: OrderKind::Stop,
            x: f32::NAN,
            y: 0.0,
            target: None,
        };
        assert!(stop_nan.well_formed());
        let many_builders = Command::Place {
            kind: BuildingKind::House,
            cx: 0,
            cy: 0,
            builders: vec![UnitId::default(); 300],
        };
        assert!(many_builders.well_formed());
        assert!(Command::DebugGift { res: None }.is_debug());
        assert!(
            !Command::Trade {
                market: BuildingId::default(),
                op: TradeOp::Sell,
                res: Res::Gold
            }
            .well_formed()
        );
    }
}
