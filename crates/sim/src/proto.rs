//! Network protocol (binary `postcard` over WebSocket binary frames). OWNER: foundation.
//!
//! Spec 08-net §B.2 R3-R5 + addenda N1, N3. The same messages are used in solo play: the client always talks to a
//! `room::Room` (in-browser for solo, on the server for multiplayer), so there is a single code path.
//!
//! Flow: client connects → server `Room::connect(conn, fresh_token)`; client sends `Hello{protocol, token, name}`
//! (stored token from `localStorage["tinyswords.session"]`, if any) → `Welcome{you, token, urls}` (the token to
//! store) → lobby messages (`Join`/`Pick`/`SetLevel`/`Start`, server broadcasts `Lobby`) → `Init` at match start
//! (or on reclaim with `rejoin = true`) → `Snapshot` every tick, `CommandResult` per command, `MatchOver`.

use crate::cmd::Command;
use crate::defs::{AiLevel, FactionIdx, KingdomColor};
use crate::snapshot_types::Snapshot;
use crate::world::MatchSetup;
use serde::{Deserialize, Serialize};

/// Connection id assigned by the transport (server socket counter / solo constant).
pub type ConnId = u32;

/// A lobby entry (`{id, color}` + name and presence).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LobbyPlayer {
    /// lobby slot id (stable for the session, not the socket)
    pub id: u32,
    pub name: String,
    pub color: KingdomColor,
    pub connected: bool,
}

/// Client → server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    /// First message: protocol version (mismatch → `No{VERSION_MISMATCH}`), stored session token, player name.
    Hello {
        protocol: u32,
        token: Option<String>,
        name: String,
    },
    /// Enter the lobby asking for a kingdom (free colour, else first free in COLORS order).
    Join { color: Option<KingdomColor> },
    /// Choose a free kingdom colour in the lobby (ignored if taken / playing).
    Pick { color: KingdomColor },
    /// Lobby leader only: AI difficulty.
    SetLevel { level: AiLevel },
    /// Lobby leader only (solo: the only player): start the match.
    Start,
    /// A player command; answered with `CommandResult{seq}`.
    Command { seq: u32, cmd: Command },
    /// Voluntary leave (back button): invalidates the token.
    Leave,
    /// Keep-alive / latency probe; answered with `Pong` (same `t`).
    Ping { t: u32 },
}

/// Server → client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ServerMsg {
    /// Reply to `Hello`: your lobby id, the session token to store, LAN urls (lobby shows `urls[0]`).
    Welcome {
        you: u32,
        token: String,
        urls: Vec<String>,
    },
    /// Lobby state broadcast: players in lobby order, AI level, leader id.
    Lobby {
        players: Vec<LobbyPlayer>,
        level: AiLevel,
        leader: Option<u32>,
    },
    /// Refusal (`Partita già in corso.`, `Partita al completo.`, version mismatch, session expired).
    No {
        why: String,
    },
    /// Match start or reclaim: the client regenerates the map with `mapgen::generate(seed, &setup)` and checks
    /// `map_hash`; `colors[i]` = colour of faction `i` (base order); `you = None` → eliminated spectator.
    Init {
        seed: u32,
        setup: MatchSetup,
        colors: Vec<KingdomColor>,
        you: Option<FactionIdx>,
        rejoin: bool,
        map_hash: u64,
    },
    /// Per-recipient, fog-filtered state of one tick.
    Snapshot(Box<Snapshot>),
    /// Result of `Command{seq}`: `err = Some(refusal)` → toast + deny sound, exactly like solo.
    CommandResult {
        seq: u32,
        err: Option<String>,
    },
    /// ≤ 1 kingdom alive (winner = last one standing, `None` if all fell).
    MatchOver {
        winner: Option<FactionIdx>,
    },
    Pong {
        t: u32,
    },
}

// ---- strings (verbatim / 08 addenda N3) ----

/// `join` refused while a match runs (mid-game join stays forbidden for new players).
pub const NO_PLAYING: &str = "Partita già in corso.";
/// lobby full (5 players).
pub const NO_FULL: &str = "Partita al completo.";
/// protocol mismatch.
pub const VERSION_MISMATCH: &str = "Versione diversa dal server: ricarica la pagina.";
/// stored token refers to a finished match.
pub const SESSION_EXPIRED: &str = "La partita a cui eri collegato è finita.";
/// client overlay while reconnecting.
pub const RECONNECTING: &str = "Riconnessione in corso…";
/// socket lost.
pub const CONNECTION_LOST: &str = "Connessione col server persa.";
/// no server reachable from the multiplayer menu.
pub const NO_SERVER: &str =
    "Multigiocatore: avvia il server di Tiny Swords e apri l'indirizzo che stampa.";
/// server shutting down.
pub const SERVER_CLOSED: &str = "Il server ha chiuso la partita.";
/// `localStorage` key of the session token.
pub const SESSION_KEY: &str = "tinyswords.session";

/// `` `${name}: il giocatore è uscito, ora lo guida l'IA` `` (net.js:147; `name` = kingdom name, e.g. `Regno Blu`).
pub fn player_left_text(name: &str) -> String {
    format!("{name}: il giocatore è uscito, ora lo guida l'IA")
}

/// `` `${name}: il giocatore è tornato` `` (new, 04b addenda A3 / 08 addenda N3).
pub fn player_back_text(name: &str) -> String {
    format!("{name}: il giocatore è tornato")
}

/// Encodes any protocol message with postcard.
pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_allocvec(msg).unwrap_or_default()
}

/// Decodes a protocol message; `Err` on malformed input (the server drops such frames).
pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, postcard::Error> {
    postcard::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::OrderKind;
    use crate::slab::UnitId;

    #[test]
    fn roundtrip() {
        let m = ClientMsg::Command {
            seq: 7,
            cmd: Command::Order {
                units: vec![UnitId::default()],
                kind: OrderKind::AMove,
                x: 10.5,
                y: -3.0,
                target: None,
            },
        };
        let b = encode(&m);
        assert_eq!(decode::<ClientMsg>(&b).unwrap(), m);
        let s = ServerMsg::Snapshot(Box::default());
        assert_eq!(decode::<ServerMsg>(&encode(&s)).unwrap(), s);
        assert!(decode::<ServerMsg>(&[250, 1, 2]).is_err());
        assert_eq!(
            player_left_text("Regno Blu"),
            "Regno Blu: il giocatore è uscito, ora lo guida l'IA"
        );
    }
}
