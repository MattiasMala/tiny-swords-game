//! Presentation events emitted by the sim. OWNER: foundation.
//!
//! Spec 07 §9.1, 08 §A.9 and R5: the sim pushes `GameEvent`s into `World.events` during a tick; the room drains them
//! after `World::step` and routes them: public events to every viewer that may see the spot (fog culling is the
//! snapshot builder's job), private ones (`to = Some(f)`) only to that kingdom's player. Camera culling happens on
//! the client (03a addenda U5, 07 addenda L1) — the sim never looks at a camera.
//!
//! Text is structured ([`Msg`]) so it travels compactly and the client formats the Italian string with
//! [`Msg::text`] (all strings verbatim from the specs).

use crate::defs::{
    AGE_NAME, BuildingKind, FactionIdx, FxKind, KingdomColor, PointKind, PropKind, Res, Sound,
    TechId,
};
use crate::slab::UnitId;
use serde::{Deserialize, Serialize};

/// A user-visible message (banner or toast).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Msg {
    /// `Distruggi i castelli nemici!` (match start, main.js:94)
    DestroyEnemyCastles,
    /// `${f.name}: ${AGE_NAME[age]}` (factions.js:106)
    AgeReached { color: KingdomColor, age: u8 },
    /// `Ricerca completata: ${label}` (factions.js:122)
    ResearchDone(TechId),
    /// `Costruzione finita: ${label}` (entities.js:1134)
    BuildingDone(BuildingKind),
    /// `Hai conquistato: ${label}` (entities.js:209)
    PointCaptured(PointKind),
    /// `Hai perso: ${label}` (entities.js:210)
    PointLost(PointKind),
    /// `${f.name} ti attacca!` (factions.js:745)
    Attacks { color: KingdomColor },
    /// `${f.name} è caduto!` (factions.js:910)
    Fallen { color: KingdomColor },
    /// `${f.name}: il giocatore è uscito, ora lo guida l'IA` (net.js:147)
    PlayerLeft { color: KingdomColor },
    /// `${f.name}: il giocatore è tornato` (new, 08 addenda N3)
    PlayerBack { color: KingdomColor },
    /// `Esplorazione finita: a piedi non resta nulla da scoprire` (entities.js:729)
    ExploreDone,
    /// `Pedoni al massimo` (entities.js:1195)
    PawnsMax,
    /// `Servono più case`
    NeedHouses,
    /// `Truppe al massimo`
    TroopsMax,
    /// `Servono più accampamenti`
    NeedCamps,
    /// `Forziere trovato: +${amount} ${RES_LABEL[kind].toLowerCase()}` (entities.js:1287)
    ChestFound { res: Res, amount: u32 },
    /// Pre-formatted text (command refusals and anything not listed above).
    Text(String),
}

impl Msg {
    /// The Italian string shown to the player.
    pub fn text(&self) -> String {
        match self {
            Msg::DestroyEnemyCastles => "Distruggi i castelli nemici!".to_string(),
            Msg::AgeReached { color, age } => {
                format!(
                    "{}: {}",
                    color.kingdom_name(),
                    AGE_NAME.get(*age as usize).copied().unwrap_or("")
                )
            }
            Msg::ResearchDone(t) => format!("Ricerca completata: {}", t.def().label),
            Msg::BuildingDone(b) => format!("Costruzione finita: {}", b.def().label),
            Msg::PointCaptured(p) => format!("Hai conquistato: {}", p.def().label),
            Msg::PointLost(p) => format!("Hai perso: {}", p.def().label),
            Msg::Attacks { color } => format!("{} ti attacca!", color.kingdom_name()),
            Msg::Fallen { color } => format!("{} è caduto!", color.kingdom_name()),
            Msg::PlayerLeft { color } => crate::proto::player_left_text(&color.kingdom_name()),
            Msg::PlayerBack { color } => crate::proto::player_back_text(&color.kingdom_name()),
            Msg::ExploreDone => {
                "Esplorazione finita: a piedi non resta nulla da scoprire".to_string()
            }
            Msg::PawnsMax => "Pedoni al massimo".to_string(),
            Msg::NeedHouses => "Servono più case".to_string(),
            Msg::TroopsMax => "Truppe al massimo".to_string(),
            Msg::NeedCamps => "Servono più accampamenti".to_string(),
            Msg::ChestFound { res, amount } => format!(
                "Forziere trovato: +{} {}",
                amount,
                res.label().to_lowercase()
            ),
            Msg::Text(s) => s.clone(),
        }
    }
}

/// Banner durations used by the JS `G.announce(text, faction, dur)` calls (seconds).
pub mod dur {
    pub const START: f32 = 3.5;
    pub const AGE: f32 = 4.0;
    pub const RESEARCH: f32 = 2.6;
    pub const BUILDING_DONE: f32 = 2.2;
    pub const CAPTURED: f32 = 2.6;
    pub const LOST: f32 = 3.0;
    pub const ATTACKS: f32 = 3.5;
    pub const FALLEN: f32 = 4.0;
    pub const PLAYER: f32 = 4.0;
    /// default `G.announce` duration
    pub const DEFAULT: f32 = 3.0;
}

/// One presentation event (spec 07 §9.1 table, typed).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GameEvent {
    /// `['fx', name, x, y, scale, dy, followId, color]`: sprite FX; `dy` shifts draw order only; `follow` tracks a
    /// unit; `color` selects the per-kingdom Heal_Effect sheet. Public.
    Fx {
        fx: FxKind,
        x: f32,
        y: f32,
        scale: f32,
        dy: f32,
        follow: Option<UnitId>,
        color: Option<KingdomColor>,
    },
    /// `['prop', name, x, y]`: permanent decor added for everyone (e.g. `chest_open`). Public.
    Prop { prop: PropKind, x: f32, y: f32 },
    /// `['snd', name, x, y]`: positional sound, public (client applies distance falloff).
    Sound { sound: Sound, x: f32, y: f32 },
    /// `['play', fi, sound]`: UI sound at full volume for one kingdom. Private.
    Play { to: FactionIdx, sound: Sound },
    /// `['float', fi, x, y, '+N', kind]`: floating "+N" with resource icon. Private.
    Float {
        to: FactionIdx,
        x: f32,
        y: f32,
        amount: i32,
        res: Res,
    },
    /// `['toast', fi, text]`. Private.
    Toast { to: FactionIdx, msg: Msg },
    /// `['alert', fi, x, y]`: "under attack" minimap ping (client adds banner `Il tuo regno è sotto attacco!` +
    /// horn when off screen). Private.
    Alert { to: FactionIdx, x: f32, y: f32 },
    /// `['say', to, text, colorFi, dur]`: banner; `to = None` = everyone; `color = None` = reader's colour.
    Say {
        to: Option<FactionIdx>,
        msg: Msg,
        color: Option<FactionIdx>,
        dur: f32,
    },
}

impl GameEvent {
    /// `Some(f)` for private events, `None` for public ones.
    pub fn recipient(&self) -> Option<FactionIdx> {
        match self {
            GameEvent::Play { to, .. }
            | GameEvent::Float { to, .. }
            | GameEvent::Toast { to, .. }
            | GameEvent::Alert { to, .. } => Some(*to),
            GameEvent::Say { to, .. } => *to,
            _ => None,
        }
    }

    /// Whether faction `f` should receive this event (ignoring fog).
    pub fn is_for(&self, f: Option<FactionIdx>) -> bool {
        match self.recipient() {
            None => true,
            Some(to) => f == Some(to),
        }
    }

    /// World position of a positional public event (for fog / camera culling).
    pub fn pos(&self) -> Option<(f32, f32)> {
        match self {
            GameEvent::Fx { x, y, .. }
            | GameEvent::Prop { x, y, .. }
            | GameEvent::Sound { x, y, .. } => Some((*x, *y)),
            _ => None,
        }
    }

    /// Cosmetic events may be dropped under congestion; private notices never (08 R5).
    pub fn is_cosmetic(&self) -> bool {
        matches!(self, GameEvent::Fx { .. } | GameEvent::Sound { .. })
    }

    /// Shorthand for a plain FX event.
    pub fn fx(fx: FxKind, x: f32, y: f32) -> GameEvent {
        GameEvent::Fx {
            fx,
            x,
            y,
            scale: 1.0,
            dy: 0.0,
            follow: None,
            color: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_verbatim() {
        assert_eq!(
            Msg::AgeReached {
                color: KingdomColor::Blue,
                age: 2
            }
            .text(),
            "Regno Blu: Età Feudale"
        );
        assert_eq!(
            Msg::ChestFound {
                res: Res::Wood,
                amount: 50
            }
            .text(),
            "Forziere trovato: +50 legna"
        );
        assert_eq!(
            Msg::PlayerLeft {
                color: KingdomColor::Red
            }
            .text(),
            "Regno Rosso: il giocatore è uscito, ora lo guida l'IA"
        );
        assert_eq!(
            Msg::PlayerBack {
                color: KingdomColor::Black
            }
            .text(),
            "Regno Nero: il giocatore è tornato"
        );
        assert_eq!(
            Msg::Fallen {
                color: KingdomColor::Yellow
            }
            .text(),
            "Regno Giallo è caduto!"
        );
        assert_eq!(
            GameEvent::Toast {
                to: 2,
                msg: Msg::PawnsMax
            }
            .recipient(),
            Some(2)
        );
        assert!(GameEvent::fx(FxKind::Dust1, 0.0, 0.0).is_for(Some(3)));
    }
}
