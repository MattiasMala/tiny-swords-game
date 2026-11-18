// OWNER: S6 (net) — see docs/spec/08-net.md §A.5 (lobby rules and strings), §A.6 (match start), §A.10-A.11 (commands,
// disconnects, spectators), §B.2 R2-R7 (authoritative room, session tokens, reconnection, lobby protocol, messages,
// snapshots, robustness) + addenda N1-N4; 04b §12 (takeover/reclaim, World::takeover / World::reclaim).
//
// A `Room` is I/O-free: the server wires it to WebSockets (one room is enough), the client runs a `Solo` room
// in-browser. Feed it `connect` / `receive` / `disconnect`, call `tick()` every `TICK_DT`, then send
// `drain_outbox()` and close the sockets in `drain_closed()`.
//
// RECONNECT POLICY (task decision, deviates from 08 N1's 20 s grace — `RECONNECT_GRACE` keeps it configurable):
//   * identity = session token (server-generated random string passed to `connect`, returned in `Welcome`,
//     stored by the client in localStorage `tinyswords.session`);
//   * on disconnect during a match the AI takes over IMMEDIATELY (`World::takeover`: banner `il giocatore è
//     uscito`, soldiers idle, AI fast-forward);
//   * the human can RECLAIM the kingdom for the WHOLE match by reconnecting with the same token
//     (`World::reclaim`, banner `il giocatore è tornato`, `Init{rejoin: true}` + full snapshot); if the kingdom fell
//     meanwhile they re-enter as spectator (`Init{you: None}`);
//   * a second connection presenting a token already attached replaces the first (old socket → `drain_closed`);
//   * new players cannot join a running match (`Partita già in corso.`); the match keeps running with nobody
//     connected and is discarded after `ABANDON_TIMEOUT` s (lobby reopens).
//
// S6 TODO: rate limiting, per-connection backpressure (latest-wins snapshots), lobby leadership edge cases,
// spectator snapshots for eliminated players, match end → lobby reset, logging hooks for the server.
#![allow(dead_code, unused)]

use crate::World;
use crate::cmd::Command;
use crate::defs::{AiLevel, FactionIdx, KingdomColor};
use crate::proto::{self, ClientMsg, ConnId, LobbyPlayer, ServerMsg};
use crate::snapshot::{Viewer, ViewerCache, build_snapshot};
use crate::world::{MatchSetup, SetupPlayer};
use crate::{PROTOCOL_VERSION, TICK_DT};

/// Seconds a disconnected kingdom stays human-controlled before the AI takes over (0 = immediately).
pub const RECONNECT_GRACE: f32 = 0.0;
/// Seconds a running match is kept with no human connected (reload/reconnect of the last player still works).
pub const ABANDON_TIMEOUT: f32 = 300.0;
/// Lobby size (one per kingdom).
pub const MAX_PLAYERS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomKind {
    /// in-browser single player: test-mode commands allowed, one player
    Solo,
    /// server lobby (up to 5 players)
    Lobby,
}

#[derive(Clone, Debug)]
struct Session {
    id: u32,
    token: String,
    name: String,
    color: Option<KingdomColor>,
    conn: Option<ConnId>,
    in_lobby: bool,
    faction: Option<FactionIdx>,
    /// game time of the disconnect (grace handling)
    gone_t: Option<f32>,
}

#[derive(Clone, Debug)]
struct Conn {
    id: ConnId,
    fresh_token: String,
    session: Option<u32>,
    cache: ViewerCache,
}

/// Lobby + match + sessions, without I/O.
#[derive(Debug)]
pub struct Room {
    pub kind: RoomKind,
    pub urls: Vec<String>,
    pub level: AiLevel,
    /// Forced match seed (dev CLI `--seed`, tests); `None` = derived from the room entropy.
    pub seed: Option<u32>,
    sessions: Vec<Session>,
    conns: Vec<Conn>,
    next_session: u32,
    world: Option<World>,
    /// (conn, seq, faction, command) waiting for the next tick
    pending: Vec<(ConnId, u32, FactionIdx, Command)>,
    outbox: Vec<(ConnId, ServerMsg)>,
    closed: Vec<ConnId>,
    entropy: u64,
    over_sent: bool,
    empty_t: f32,
}

impl Room {
    pub fn new(kind: RoomKind, urls: Vec<String>) -> Room {
        Room {
            kind,
            urls,
            level: AiLevel::Normal,
            sessions: Vec::new(),
            conns: Vec::new(),
            next_session: 1,
            world: None,
            pending: Vec::new(),
            outbox: Vec::new(),
            closed: Vec::new(),
            seed: None,
            entropy: 0x9e37_79b9_7f4a_7c15,
            over_sent: false,
            empty_t: 0.0,
        }
    }

    /// Mixes external randomness into the match seed (server: OS random; browser: `Math.random`).
    pub fn add_entropy(&mut self, v: u64) {
        self.entropy = (self.entropy ^ v)
            .wrapping_mul(0x0100_0000_01b3)
            .rotate_left(17);
    }

    /// The running match, if any (solo client reads it for debugging; never mutate from outside).
    pub fn world(&self) -> Option<&World> {
        self.world.as_ref()
    }

    /// New socket. `fresh_token` = random session token the server generated for it (used if the client has none).
    pub fn connect(&mut self, conn: ConnId, fresh_token: String) {
        for b in fresh_token.bytes() {
            self.add_entropy(b as u64);
        }
        self.conns.push(Conn {
            id: conn,
            fresh_token,
            session: None,
            cache: ViewerCache::default(),
        });
    }

    fn session_of(&self, conn: ConnId) -> Option<usize> {
        let sid = self.conns.iter().find(|c| c.id == conn)?.session?;
        self.sessions.iter().position(|s| s.id == sid)
    }

    fn send(&mut self, conn: ConnId, m: ServerMsg) {
        self.outbox.push((conn, m));
    }

    fn lobby_msg(&self) -> ServerMsg {
        let players: Vec<LobbyPlayer> = self
            .sessions
            .iter()
            .filter(|s| s.in_lobby)
            .filter_map(|s| {
                s.color.map(|color| LobbyPlayer {
                    id: s.id,
                    name: s.name.clone(),
                    color,
                    connected: s.conn.is_some(),
                })
            })
            .collect();
        let leader = players.first().map(|p| p.id);
        ServerMsg::Lobby {
            players,
            level: self.level,
            leader,
        }
    }

    fn broadcast_lobby(&mut self) {
        let m = self.lobby_msg();
        let conns: Vec<ConnId> = self
            .sessions
            .iter()
            .filter(|s| s.in_lobby)
            .filter_map(|s| s.conn)
            .collect();
        for c in conns {
            self.send(c, m.clone());
        }
    }

    fn is_leader(&self, si: usize) -> bool {
        self.sessions
            .iter()
            .find(|s| s.in_lobby && s.color.is_some())
            .is_some_and(|s| s.id == self.sessions[si].id)
    }

    fn init_msg(&self, si: usize, rejoin: bool) -> Option<ServerMsg> {
        let w = self.world.as_ref()?;
        let you = self.sessions[si].faction.filter(|f| w.faction(*f).alive);
        Some(ServerMsg::Init {
            seed: w.seed,
            setup: w.setup.clone(),
            colors: w.factions.iter().map(|f| f.color).collect(),
            you,
            rejoin,
            map_hash: w.map.hash(),
        })
    }

    /// A message from a client.
    pub fn receive(&mut self, conn: ConnId, msg: ClientMsg) {
        match msg {
            ClientMsg::Hello {
                protocol,
                token,
                name,
            } => self.hello(conn, protocol, token, name),
            ClientMsg::Join { color } => {
                let Some(si) = self.session_of(conn) else {
                    return;
                };
                if self.world.is_some() {
                    self.send(
                        conn,
                        ServerMsg::No {
                            why: proto::NO_PLAYING.to_string(),
                        },
                    );
                    return;
                }
                if !self.sessions[si].in_lobby {
                    let n = self.sessions.iter().filter(|s| s.in_lobby).count();
                    let cap = if self.kind == RoomKind::Solo {
                        1
                    } else {
                        MAX_PLAYERS
                    };
                    if n >= cap {
                        self.send(
                            conn,
                            ServerMsg::No {
                                why: proto::NO_FULL.to_string(),
                            },
                        );
                        return;
                    }
                    let taken: Vec<KingdomColor> = self
                        .sessions
                        .iter()
                        .filter(|s| s.in_lobby)
                        .filter_map(|s| s.color)
                        .collect();
                    let pick = color.filter(|c| !taken.contains(c)).or_else(|| {
                        KingdomColor::ALL
                            .iter()
                            .copied()
                            .find(|c| !taken.contains(c))
                    });
                    self.sessions[si].color = pick;
                    self.sessions[si].in_lobby = true;
                }
                self.broadcast_lobby();
            }
            ClientMsg::Pick { color } => {
                let Some(si) = self.session_of(conn) else {
                    return;
                };
                if self.world.is_some() || !self.sessions[si].in_lobby {
                    return;
                }
                if self
                    .sessions
                    .iter()
                    .any(|s| s.in_lobby && s.color == Some(color))
                {
                    return;
                }
                self.sessions[si].color = Some(color);
                self.broadcast_lobby();
            }
            ClientMsg::SetLevel { level } => {
                let Some(si) = self.session_of(conn) else {
                    return;
                };
                if self.world.is_none() && self.is_leader(si) {
                    self.level = level;
                    self.broadcast_lobby();
                }
            }
            ClientMsg::Start => {
                let Some(si) = self.session_of(conn) else {
                    return;
                };
                if self.world.is_none() && self.is_leader(si) {
                    self.start_match();
                }
            }
            ClientMsg::Command { seq, cmd } => {
                let faction = self
                    .session_of(conn)
                    .and_then(|si| self.sessions[si].faction);
                let allowed = !cmd.is_debug() || self.kind == RoomKind::Solo;
                match (faction, &self.world, allowed) {
                    (Some(f), Some(w), true) if w.faction(f).alive && w.faction(f).human => {
                        self.pending.push((conn, seq, f, cmd))
                    }
                    _ => self.send(conn, ServerMsg::CommandResult { seq, err: None }),
                }
            }
            ClientMsg::Leave => {
                let Some(si) = self.session_of(conn) else {
                    return;
                };
                let s = self.sessions.remove(si);
                if let (Some(f), Some(w)) = (s.faction, self.world.as_mut()) {
                    w.takeover(f);
                    w.factions[f as usize].owned = false;
                }
                if let Some(c) = self.conns.iter_mut().find(|c| c.id == conn) {
                    c.session = None;
                }
                self.broadcast_lobby();
            }
            ClientMsg::Ping { t } => self.send(conn, ServerMsg::Pong { t }),
        }
    }

    fn hello(&mut self, conn: ConnId, protocol: u32, token: Option<String>, name: String) {
        if protocol != PROTOCOL_VERSION {
            self.send(
                conn,
                ServerMsg::No {
                    why: proto::VERSION_MISMATCH.to_string(),
                },
            );
            return;
        }
        let Some(ci) = self.conns.iter().position(|c| c.id == conn) else {
            return;
        };
        let existing = token
            .as_ref()
            .and_then(|t| self.sessions.iter().position(|s| &s.token == t));
        let si = match existing {
            Some(si) => {
                if let Some(old) = self.sessions[si].conn.filter(|o| *o != conn) {
                    self.closed.push(old);
                    self.conns.retain(|c| c.id != old);
                }
                si
            }
            None => {
                if self.world.is_some() {
                    self.send(
                        conn,
                        ServerMsg::No {
                            why: proto::NO_PLAYING.to_string(),
                        },
                    );
                    return;
                }
                let id = self.next_session;
                self.next_session += 1;
                let token = self
                    .conns
                    .iter()
                    .find(|c| c.id == conn)
                    .map(|c| c.fresh_token.clone())
                    .unwrap_or_default();
                self.sessions.push(Session {
                    id,
                    token,
                    name: name.clone(),
                    color: None,
                    conn: None,
                    in_lobby: false,
                    faction: None,
                    gone_t: None,
                });
                self.sessions.len() - 1
            }
        };
        let s = &mut self.sessions[si];
        s.conn = Some(conn);
        s.gone_t = None;
        if !name.is_empty() {
            s.name = name;
        }
        let (sid, token) = (s.id, s.token.clone());
        if let Some(c) = self.conns.iter_mut().find(|c| c.id == conn) {
            c.session = Some(sid);
            c.cache.reset();
        }
        let urls = self.urls.clone();
        self.send(
            conn,
            ServerMsg::Welcome {
                you: sid,
                token,
                urls,
            },
        );
        if self.world.is_some() {
            if let Some(f) = self.sessions[si].faction {
                if let Some(w) = self.world.as_mut() {
                    w.reclaim(f);
                }
                if let Some(m) = self.init_msg(si, true) {
                    self.send(conn, m);
                }
            }
        } else if self.sessions[si].in_lobby {
            self.broadcast_lobby();
        }
    }

    fn start_match(&mut self) {
        let players: Vec<SetupPlayer> = self
            .sessions
            .iter()
            .filter(|s| s.in_lobby)
            .filter_map(|s| {
                s.color.map(|color| SetupPlayer {
                    color,
                    name: s.name.clone(),
                })
            })
            .collect();
        let setup = MatchSetup {
            players,
            level: self.level,
            solo: self.kind == RoomKind::Solo,
        };
        self.add_entropy(self.next_session as u64);
        let seed = self
            .seed
            .unwrap_or((self.entropy ^ (self.entropy >> 32)) as u32);
        let world = World::new(seed, &setup);
        for s in self.sessions.iter_mut() {
            s.faction = if s.in_lobby {
                s.color
                    .and_then(|c| world.factions.iter().position(|f| f.color == c))
                    .map(|i| i as FactionIdx)
            } else {
                None
            };
        }
        self.world = Some(world);
        self.over_sent = false;
        self.empty_t = 0.0;
        for c in self.conns.iter_mut() {
            c.cache.reset();
        }
        for si in 0..self.sessions.len() {
            if let (true, Some(conn)) = (self.sessions[si].in_lobby, self.sessions[si].conn)
                && let Some(m) = self.init_msg(si, false)
            {
                self.send(conn, m);
            }
        }
    }

    /// Socket closed / timed out.
    pub fn disconnect(&mut self, conn: ConnId) {
        let si = self.session_of(conn);
        self.conns.retain(|c| c.id != conn);
        let Some(si) = si else { return };
        if self.sessions[si].conn != Some(conn) {
            return;
        }
        self.sessions[si].conn = None;
        if let Some(w) = self.world.as_mut() {
            self.sessions[si].gone_t = Some(w.time);
            if let Some(f) = self.sessions[si].faction {
                if RECONNECT_GRACE <= 0.0 {
                    w.takeover(f);
                } else {
                    w.factions[f as usize].connected = false;
                }
            }
        } else {
            self.sessions[si].in_lobby = false;
            self.broadcast_lobby();
        }
    }

    /// Advances the match by one `TICK_DT` (if running) and queues snapshots / command results.
    pub fn tick(&mut self) {
        let Some(w) = self.world.as_mut() else { return };
        // grace expiry
        for s in self.sessions.iter_mut() {
            if let (Some(t), Some(f)) = (s.gone_t, s.faction)
                && w.time - t >= RECONNECT_GRACE
                && w.factions[f as usize].human
                && !w.factions[f as usize].connected
            {
                w.takeover(f);
            }
        }
        let pending = core::mem::take(&mut self.pending);
        let cmds: Vec<(FactionIdx, Command)> =
            pending.iter().map(|(_, _, f, c)| (*f, c.clone())).collect();
        let results = w.step(&cmds);
        let events = w.drain_events();
        for ((conn, seq, _, _), r) in pending.iter().zip(results) {
            self.outbox.push((
                *conn,
                ServerMsg::CommandResult {
                    seq: *seq,
                    err: r.err(),
                },
            ));
        }
        let w = self.world.as_ref().expect("running");
        for c in self.conns.iter_mut() {
            let Some(sid) = c.session else { continue };
            let Some(s) = self.sessions.iter().find(|s| s.id == sid) else {
                continue;
            };
            let viewer = match s.faction {
                Some(f) if w.faction(f).alive => Viewer::Player(f),
                Some(_) => Viewer::Spectator,
                None => continue,
            };
            let snap = build_snapshot(w, viewer, &mut c.cache, &events);
            self.outbox
                .push((c.id, ServerMsg::Snapshot(Box::new(snap))));
        }
        if let (Some(o), false) = (w.outcome, self.over_sent) {
            self.over_sent = true;
            let conns: Vec<ConnId> = self
                .conns
                .iter()
                .filter(|c| c.session.is_some())
                .map(|c| c.id)
                .collect();
            for c in conns {
                self.outbox
                    .push((c, ServerMsg::MatchOver { winner: o.winner }));
            }
        }
        let anyone = self
            .sessions
            .iter()
            .any(|s| s.faction.is_some() && s.conn.is_some());
        self.empty_t = if anyone { 0.0 } else { self.empty_t + TICK_DT };
        if self.kind == RoomKind::Lobby && self.empty_t >= ABANDON_TIMEOUT {
            self.world = None;
            self.sessions.retain(|s| s.conn.is_some());
            for s in self.sessions.iter_mut() {
                s.faction = None;
                s.in_lobby = false;
            }
        }
    }

    /// Messages to send, in order.
    pub fn drain_outbox(&mut self) -> Vec<(ConnId, ServerMsg)> {
        core::mem::take(&mut self.outbox)
    }

    /// Sockets the room wants closed (replaced by a newer connection with the same token).
    pub fn drain_closed(&mut self) -> Vec<ConnId> {
        core::mem::take(&mut self.closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_flow_and_reclaim() {
        let mut r = Room::new(RoomKind::Lobby, vec!["http://x".into()]);
        r.connect(1, "tok-a".into());
        r.receive(
            1,
            ClientMsg::Hello {
                protocol: PROTOCOL_VERSION,
                token: None,
                name: "Ada".into(),
            },
        );
        r.receive(
            1,
            ClientMsg::Join {
                color: Some(KingdomColor::Red),
            },
        );
        r.receive(1, ClientMsg::Start);
        let out = r.drain_outbox();
        assert!(
            out.iter()
                .any(|(_, m)| matches!(m, ServerMsg::Welcome { .. }))
        );
        assert!(out.iter().any(|(_, m)| matches!(
            m,
            ServerMsg::Init {
                you: Some(_),
                rejoin: false,
                ..
            }
        )));
        r.tick();
        assert!(
            r.drain_outbox()
                .iter()
                .any(|(_, m)| matches!(m, ServerMsg::Snapshot(_)))
        );
        let f = r
            .world()
            .unwrap()
            .factions
            .iter()
            .position(|f| f.color == KingdomColor::Red)
            .unwrap();
        r.disconnect(1);
        assert!(!r.world().unwrap().factions[f].human);
        r.connect(2, "tok-b".into());
        r.receive(
            2,
            ClientMsg::Hello {
                protocol: PROTOCOL_VERSION,
                token: Some("tok-a".into()),
                name: String::new(),
            },
        );
        assert!(r.world().unwrap().factions[f].human);
        let out = r.drain_outbox();
        assert!(
            out.iter()
                .any(|(c, m)| *c == 2 && matches!(m, ServerMsg::Init { rejoin: true, .. }))
        );
        // a stranger cannot join the running match
        r.connect(3, "tok-c".into());
        r.receive(
            3,
            ClientMsg::Hello {
                protocol: PROTOCOL_VERSION,
                token: None,
                name: "Bob".into(),
            },
        );
        assert!(
            r.drain_outbox()
                .iter()
                .any(|(c, m)| *c == 3 && matches!(m, ServerMsg::No { .. }))
        );
    }
}
