// OWNER: S6 (net) — see docs/spec/08-net.md §A.5 (lobby rules and strings), §A.6 (match start), §A.10-A.11 (commands,
// disconnects, spectators), §B.2 R2-R7 (authoritative room, session tokens, reconnection, lobby protocol, messages,
// snapshots, robustness) + addenda N1-N4; 04b §12 (takeover/reclaim, World::takeover / World::reclaim).
//
// A `Room` is I/O-free: the server wires it to WebSockets (one room is enough), the client runs a `Solo` room
// in-browser. Feed it `connect` / `receive` / `disconnect`, call `tick()` every `TICK_DT` (also in the lobby: it
// runs the lobby timers), then send `drain_outbox()`, close the sockets in `drain_closed()` and print
// `drain_log()`. Before each `tick()` the transport reports congested sockets with `set_congested` (a socket that
// still has an unsent snapshot gets no new one: latest-wins; its private events are held back, cosmetic ones
// dropped).
//
// RECONNECT POLICY (task decision, deviates from 08 N1's 20 s grace — `Room::reconnect_grace` keeps it
// configurable):
//   * identity = session token (server-generated random string passed to `connect`, returned in `Welcome`,
//     stored by the client in localStorage `tinyswords.session`);
//   * on disconnect during a match the AI takes over IMMEDIATELY (`World::takeover`: banner `il giocatore è
//     uscito`, soldiers idle, AI fast-forward);
//   * the human can RECLAIM the kingdom for the WHOLE match by reconnecting with the same token
//     (`World::reclaim`, banner `il giocatore è tornato`, `Init{rejoin: true}` + full snapshot at once); if the
//     kingdom fell meanwhile they re-enter as spectator (`Init{you: None}`);
//   * a second connection presenting a token already attached replaces the first (old socket → `drain_closed`);
//   * a reload in the lobby keeps the slot (colour, order) for `lobby_grace` s;
//   * a voluntary `Leave` invalidates the token (in a match the kingdom goes to the AI for good);
//   * new players cannot join a running match (`Partita già in corso.`); the leader leaving does not end it; the
//     match keeps running with nobody connected and is discarded after `abandon_timeout` s (finished match:
//     `finished_timeout` s; nobody left holding a token: at once), then a new lobby opens.
#![allow(clippy::collapsible_if)]

use crate::World;
use crate::cmd::Command;
use crate::defs::{AiLevel, FactionIdx, KingdomColor};
use crate::events::GameEvent;
use crate::proto::{self, ClientMsg, ConnId, LobbyPlayer, ServerMsg};
use crate::snapshot::{Viewer, ViewerCache, build_snapshot};
use crate::world::{MatchSetup, SetupPlayer};
use crate::{PROTOCOL_VERSION, TICK_DT};
use std::collections::VecDeque;

/// Seconds a disconnected kingdom stays human-controlled before the AI takes over (0 = immediately).
pub const RECONNECT_GRACE: f32 = 0.0;
/// Seconds a running match is kept with no human connected (reload/reconnect of the last player still works).
pub const ABANDON_TIMEOUT: f32 = 300.0;
/// Seconds a finished match is kept with no human connected.
pub const FINISHED_TIMEOUT: f32 = 30.0;
/// Seconds a disconnected lobby member keeps its slot (page reload in the lobby).
pub const LOBBY_GRACE: f32 = 20.0;
/// Lobby size (one per kingdom).
pub const MAX_PLAYERS: usize = 5;
/// Commands per second a network player may send (token bucket refill) and the burst size.
pub const CMD_RATE: f32 = 25.0;
pub const CMD_BURST: f32 = 50.0;
/// Events held back for a congested connection (cosmetic ones are dropped first).
pub const MAX_BACKLOG: usize = 512;
/// Expired tokens remembered (to answer `SESSION_EXPIRED` instead of `Partita già in corso.`).
const EXPIRED_MEMORY: usize = 64;
/// Max name length kept from `Hello`.
const MAX_NAME: usize = 24;
/// Test-mode command in a network match (main.js:258).
pub const TEST_SOLO_ONLY: &str = "La modalità test c'è solo nelle partite da soli";

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
    /// room clock at the disconnect
    gone_t: Option<f32>,
    /// what this player's client already knows (survives reconnects: ghosts, last-seen nodes)
    cache: ViewerCache,
    /// events not delivered yet (connection congested)
    backlog: Vec<GameEvent>,
}

#[derive(Clone, Debug)]
struct Conn {
    id: ConnId,
    fresh_token: String,
    session: Option<u32>,
    /// an unsent snapshot is still queued on the socket (reported by the transport)
    congested: bool,
    /// command rate-limit bucket
    bucket: f32,
}

/// Lobby + match + sessions, without I/O.
#[derive(Debug)]
pub struct Room {
    pub kind: RoomKind,
    pub urls: Vec<String>,
    pub level: AiLevel,
    /// Forced match seed (dev CLI `--seed`, tests); `None` = derived from the room entropy.
    pub seed: Option<u32>,
    /// see the module header
    pub reconnect_grace: f32,
    pub abandon_timeout: f32,
    pub finished_timeout: f32,
    pub lobby_grace: f32,
    sessions: Vec<Session>,
    conns: Vec<Conn>,
    next_session: u32,
    world: Option<World>,
    /// (conn, seq, faction, command) waiting for the next tick
    pending: Vec<(ConnId, u32, FactionIdx, Command)>,
    outbox: Vec<(ConnId, ServerMsg)>,
    closed: Vec<ConnId>,
    log: Vec<String>,
    expired: VecDeque<String>,
    entropy: u64,
    over_sent: bool,
    empty_t: f32,
    /// room clock (s), advances every tick, also in the lobby
    clock: f32,
}

impl Room {
    pub fn new(kind: RoomKind, urls: Vec<String>) -> Room {
        Room {
            kind,
            urls,
            level: AiLevel::Normal,
            seed: None,
            reconnect_grace: RECONNECT_GRACE,
            abandon_timeout: ABANDON_TIMEOUT,
            finished_timeout: FINISHED_TIMEOUT,
            lobby_grace: LOBBY_GRACE,
            sessions: Vec::new(),
            conns: Vec::new(),
            next_session: 1,
            world: None,
            pending: Vec::new(),
            outbox: Vec::new(),
            closed: Vec::new(),
            log: Vec::new(),
            expired: VecDeque::new(),
            entropy: 0x9e37_79b9_7f4a_7c15,
            over_sent: false,
            empty_t: 0.0,
            clock: 0.0,
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

    /// A match is running (or finished but not discarded yet).
    pub fn playing(&self) -> bool {
        self.world.is_some()
    }

    /// Number of attached sockets.
    pub fn connection_count(&self) -> usize {
        self.conns.len()
    }

    /// New socket. `fresh_token` = random session token the server generated for it (used if the client has none).
    pub fn connect(&mut self, conn: ConnId, fresh_token: String) {
        for b in fresh_token.bytes() {
            self.add_entropy(b as u64);
        }
        self.add_entropy(conn as u64);
        self.conns.retain(|c| c.id != conn);
        self.conns.push(Conn {
            id: conn,
            fresh_token,
            session: None,
            congested: false,
            bucket: CMD_BURST,
        });
    }

    /// Transport report: `true` while the socket still has an unsent snapshot queued (no new snapshot is built for
    /// it until it drains: latest-wins without breaking the deltas).
    pub fn set_congested(&mut self, conn: ConnId, congested: bool) {
        if let Some(c) = self.conns.iter_mut().find(|c| c.id == conn) {
            c.congested = congested;
        }
    }

    fn conn_mut(&mut self, conn: ConnId) -> Option<&mut Conn> {
        self.conns.iter_mut().find(|c| c.id == conn)
    }

    fn session_of(&self, conn: ConnId) -> Option<usize> {
        let sid = self.conns.iter().find(|c| c.id == conn)?.session?;
        self.sessions.iter().position(|s| s.id == sid)
    }

    fn send(&mut self, conn: ConnId, m: ServerMsg) {
        self.outbox.push((conn, m));
    }

    fn refuse(&mut self, conn: ConnId, why: &str) {
        self.send(
            conn,
            ServerMsg::No {
                why: why.to_string(),
            },
        );
    }

    fn log(&mut self, line: String) {
        self.log.push(line);
    }

    fn lobby_members(&self) -> impl Iterator<Item = &Session> {
        self.sessions
            .iter()
            .filter(|s| s.in_lobby && s.color.is_some())
    }

    /// Lobby leader: the first connected member in lobby order (a member reloading the page does not block the
    /// others), else the first member.
    fn leader_id(&self) -> Option<u32> {
        self.lobby_members()
            .find(|s| s.conn.is_some())
            .or_else(|| self.lobby_members().next())
            .map(|s| s.id)
    }

    fn lobby_msg(&self) -> ServerMsg {
        let players: Vec<LobbyPlayer> = self
            .lobby_members()
            .filter_map(|s| {
                s.color.map(|color| LobbyPlayer {
                    id: s.id,
                    name: s.name.clone(),
                    color,
                    connected: s.conn.is_some(),
                })
            })
            .collect();
        ServerMsg::Lobby {
            players,
            level: self.level,
            leader: self.leader_id(),
        }
    }

    fn broadcast_lobby(&mut self) {
        if self.world.is_some() {
            return;
        }
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
        self.leader_id() == Some(self.sessions[si].id)
    }

    fn init_msg(&self, si: usize, rejoin: bool) -> Option<ServerMsg> {
        let w = self.world.as_ref()?;
        let you = self.sessions[si]
            .faction
            .filter(|f| w.factions.get(*f as usize).is_some_and(|x| x.alive));
        Some(ServerMsg::Init {
            seed: w.seed,
            setup: w.setup.clone(),
            colors: w.factions.iter().map(|f| f.color).collect(),
            you,
            rejoin,
            map_hash: w.map.hash(),
        })
    }

    fn viewer_of(w: &World, faction: Option<FactionIdx>) -> Option<Viewer> {
        match faction {
            Some(f) if w.factions.get(f as usize).is_some_and(|x| x.alive) => {
                Some(Viewer::Player(f))
            }
            Some(_) => Some(Viewer::Spectator),
            None => None,
        }
    }

    /// A message from a client.
    pub fn receive(&mut self, conn: ConnId, msg: ClientMsg) {
        if !self.conns.iter().any(|c| c.id == conn) {
            return;
        }
        match msg {
            ClientMsg::Hello {
                protocol,
                token,
                name,
            } => self.hello(conn, protocol, token, name),
            ClientMsg::Join { color } => self.join(conn, color),
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
                if self.world.is_none() && self.sessions[si].in_lobby && self.is_leader(si) {
                    self.level = level;
                    self.broadcast_lobby();
                }
            }
            ClientMsg::Start => {
                let Some(si) = self.session_of(conn) else {
                    return;
                };
                if self.world.is_none() && self.sessions[si].in_lobby && self.is_leader(si) {
                    self.start_match();
                }
            }
            ClientMsg::Command { seq, cmd } => self.command(conn, seq, cmd),
            ClientMsg::Leave => self.leave(conn),
            ClientMsg::Ping { t } => self.send(conn, ServerMsg::Pong { t }),
        }
    }

    fn hello(&mut self, conn: ConnId, protocol: u32, token: Option<String>, name: String) {
        if protocol != PROTOCOL_VERSION {
            self.refuse(conn, proto::VERSION_MISMATCH);
            self.closed.push(conn);
            return;
        }
        let Some(ci) = self.conns.iter().position(|c| c.id == conn) else {
            return;
        };
        if self.conns[ci].session.is_some() {
            return; // duplicate Hello
        }
        let name: String = name.trim().chars().take(MAX_NAME).collect();
        let existing = token
            .as_ref()
            .filter(|t| !t.is_empty())
            .and_then(|t| self.sessions.iter().position(|s| &s.token == t));
        let si = match existing {
            Some(si) => {
                if let Some(old) = self.sessions[si].conn.filter(|o| *o != conn) {
                    // a newer socket with the same token replaces the old one (reload before the old timed out)
                    self.closed.push(old);
                    self.conns.retain(|c| c.id != old);
                    self.log(format!("giocatore {old} sostituito dalla connessione {conn}"));
                }
                si
            }
            None => {
                if self.world.is_some() {
                    let expired = token
                        .as_ref()
                        .is_some_and(|t| self.expired.iter().any(|e| e == t));
                    self.refuse(
                        conn,
                        if expired {
                            proto::SESSION_EXPIRED
                        } else {
                            proto::NO_PLAYING
                        },
                    );
                    self.closed.push(conn);
                    return;
                }
                let id = self.next_session;
                self.next_session += 1;
                let token = self.conns[ci].fresh_token.clone();
                self.sessions.push(Session {
                    id,
                    token,
                    name: name.clone(),
                    color: None,
                    conn: None,
                    in_lobby: false,
                    faction: None,
                    gone_t: None,
                    cache: ViewerCache::default(),
                    backlog: Vec::new(),
                });
                self.sessions.len() - 1
            }
        };
        let rejoining = existing.is_some();
        let s = &mut self.sessions[si];
        s.conn = Some(conn);
        s.gone_t = None;
        if !name.is_empty() {
            s.name = name;
        }
        let (sid, token, faction) = (s.id, s.token.clone(), s.faction);
        if let Some(c) = self.conn_mut(conn) {
            c.session = Some(sid);
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
            let Some(f) = faction else {
                // a lobby session that was not in the lobby when the match started
                self.refuse(conn, proto::NO_PLAYING);
                self.sessions.remove(si);
                if let Some(c) = self.conn_mut(conn) {
                    c.session = None;
                }
                self.closed.push(conn);
                return;
            };
            if let Some(w) = self.world.as_mut() {
                w.reclaim(f);
            }
            let name = self.world.as_ref().map(|w| w.faction(f).name()).unwrap_or_default();
            self.log(format!("{name}: il giocatore {conn} è tornato in partita"));
            if let Some(m) = self.init_msg(si, true) {
                self.send(conn, m);
            }
            // full snapshot at once (the client mirror restarts; known ghosts are re-sent)
            self.sessions[si].cache.resync();
            self.sessions[si].backlog.clear();
            if let Some(w) = self.world.as_ref()
                && let Some(viewer) = Self::viewer_of(w, Some(f))
            {
                let snap = build_snapshot(w, viewer, &mut self.sessions[si].cache, &[]);
                let over = w.outcome.map(|o| o.winner);
                self.outbox.push((conn, ServerMsg::Snapshot(Box::new(snap))));
                if let Some(winner) = over {
                    self.outbox.push((conn, ServerMsg::MatchOver { winner }));
                }
            }
        } else if self.sessions[si].in_lobby {
            if rejoining {
                let name = self.sessions[si].name.clone();
                self.log(format!("{name} è tornato nella sala d'attesa"));
            }
            self.broadcast_lobby();
        }
    }

    fn join(&mut self, conn: ConnId, color: Option<KingdomColor>) {
        let Some(si) = self.session_of(conn) else {
            return;
        };
        if self.world.is_some() {
            // a player of the match re-sending Join after a rejoin is ignored
            if self.sessions[si].faction.is_none() {
                self.refuse(conn, proto::NO_PLAYING);
            }
            return;
        }
        if !self.sessions[si].in_lobby {
            let n = self.lobby_members().count();
            let cap = if self.kind == RoomKind::Solo {
                1
            } else {
                MAX_PLAYERS
            };
            if n >= cap {
                self.refuse(conn, proto::NO_FULL);
                return;
            }
            let taken: Vec<KingdomColor> = self.lobby_members().filter_map(|s| s.color).collect();
            let pick = color.filter(|c| !taken.contains(c)).or_else(|| {
                KingdomColor::ALL
                    .iter()
                    .copied()
                    .find(|c| !taken.contains(c))
            });
            let Some(pick) = pick else {
                self.refuse(conn, proto::NO_FULL);
                return;
            };
            // lobby order = join order
            let mut s = self.sessions.remove(si);
            s.color = Some(pick);
            s.in_lobby = true;
            let name = s.name.clone();
            self.sessions.push(s);
            self.log(format!(
                "{} entra nella sala d'attesa ({})",
                if name.is_empty() { "un giocatore" } else { &name },
                pick.kingdom_name()
            ));
        }
        self.broadcast_lobby();
    }

    fn command(&mut self, conn: ConnId, seq: u32, cmd: Command) {
        if self.kind == RoomKind::Lobby {
            let Some(c) = self.conn_mut(conn) else { return };
            if c.bucket < 1.0 {
                return; // flood: dropped without answer
            }
            c.bucket -= 1.0;
        }
        if cmd.is_debug() && self.kind != RoomKind::Solo {
            self.send(
                conn,
                ServerMsg::CommandResult {
                    seq,
                    err: Some(TEST_SOLO_ONLY.to_string()),
                },
            );
            return;
        }
        let faction = self
            .session_of(conn)
            .and_then(|si| self.sessions[si].faction);
        match (faction, &self.world) {
            (Some(f), Some(w))
                if w.factions.get(f as usize).is_some_and(|x| x.alive && x.human) =>
            {
                self.pending.push((conn, seq, f, cmd))
            }
            _ => self.send(conn, ServerMsg::CommandResult { seq, err: None }),
        }
    }

    fn leave(&mut self, conn: ConnId) {
        let Some(si) = self.session_of(conn) else {
            return;
        };
        let s = self.sessions.remove(si);
        if let Some(c) = self.conn_mut(conn) {
            c.session = None;
        }
        self.pending.retain(|p| p.0 != conn);
        match (s.faction, self.world.as_mut()) {
            (Some(f), Some(w)) => {
                w.takeover(f);
                w.factions[f as usize].owned = false;
                let name = w.faction(f).name();
                self.log(format!("{name}: il giocatore {conn} ha lasciato la partita"));
            }
            _ => {
                if s.in_lobby {
                    let name = if s.name.is_empty() { "un giocatore".to_string() } else { s.name };
                    self.log(format!("{name} lascia la sala d'attesa"));
                }
                self.broadcast_lobby();
            }
        }
    }

    fn start_match(&mut self) {
        // late arrivals that never joined the lobby are refused now, without receiving Init (08 R4)
        let late: Vec<(usize, Option<ConnId>)> = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.in_lobby || s.color.is_none())
            .map(|(i, s)| (i, s.conn))
            .collect();
        for (_, c) in &late {
            if let Some(c) = *c {
                self.refuse(c, proto::NO_PLAYING);
                self.closed.push(c);
                if let Some(cc) = self.conn_mut(c) {
                    cc.session = None;
                }
            }
        }
        for (i, _) in late.into_iter().rev() {
            self.sessions.remove(i);
        }
        let players: Vec<SetupPlayer> = self
            .sessions
            .iter()
            .filter_map(|s| {
                s.color.map(|color| SetupPlayer {
                    color,
                    name: if s.name.is_empty() {
                        color.kingdom_name()
                    } else {
                        s.name.clone()
                    },
                })
            })
            .collect();
        let setup = MatchSetup {
            players,
            level: self.level,
            solo: self.kind == RoomKind::Solo,
        };
        self.add_entropy(self.next_session as u64);
        self.add_entropy(self.clock.to_bits() as u64);
        let seed = self
            .seed
            .unwrap_or((self.entropy ^ (self.entropy >> 32)) as u32);
        let mut world = World::new(seed, &setup);
        for s in self.sessions.iter_mut() {
            s.faction = s
                .color
                .and_then(|c| world.factions.iter().position(|f| f.color == c))
                .map(|i| i as FactionIdx);
            s.cache.reset();
            s.backlog.clear();
            // a member reloading the page right now: the AI drives until they are back
            if s.conn.is_none()
                && let Some(f) = s.faction
            {
                world.takeover(f);
            }
        }
        self.log(format!(
            "partita avviata: seme {seed}, giocatori umani {}, IA {}",
            setup.players.len(),
            self.level.label()
        ));
        self.world = Some(world);
        self.pending.clear();
        self.over_sent = false;
        self.empty_t = 0.0;
        for si in 0..self.sessions.len() {
            if let Some(conn) = self.sessions[si].conn
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
        self.pending.retain(|p| p.0 != conn);
        let Some(si) = si else { return };
        if self.sessions[si].conn != Some(conn) {
            return;
        }
        self.sessions[si].conn = None;
        self.sessions[si].gone_t = Some(self.clock);
        self.sessions[si].backlog.clear();
        let grace = self.reconnect_grace;
        if let Some(w) = self.world.as_mut() {
            if let Some(f) = self.sessions[si].faction {
                if grace <= 0.0 {
                    w.takeover(f);
                } else {
                    w.factions[f as usize].connected = false;
                }
                let name = w.faction(f).name();
                self.log(format!("{name}: il giocatore {conn} si è disconnesso"));
            }
        } else if self.sessions[si].in_lobby {
            // reload in the lobby: the slot is kept for `lobby_grace`
            self.broadcast_lobby();
        } else {
            self.sessions.remove(si);
        }
    }

    /// Advances the room by one `TICK_DT`: lobby timers, the match (if running), snapshots, command results.
    pub fn tick(&mut self) {
        self.clock += TICK_DT;
        for c in self.conns.iter_mut() {
            c.bucket = (c.bucket + CMD_RATE * TICK_DT).min(CMD_BURST);
        }
        if self.world.is_none() {
            self.lobby_timers();
            return;
        }
        self.match_tick();
    }

    fn lobby_timers(&mut self) {
        let (clock, grace) = (self.clock, self.lobby_grace);
        let before = self.sessions.len();
        self.sessions.retain(|s| {
            s.conn.is_some() || s.gone_t.is_none_or(|t| clock - t < grace)
        });
        if self.sessions.len() != before {
            self.broadcast_lobby();
        }
    }

    fn match_tick(&mut self) {
        let Some(w) = self.world.as_mut() else { return };
        // grace expiry (only when `reconnect_grace > 0`)
        if self.reconnect_grace > 0.0 {
            for s in self.sessions.iter() {
                if let (None, Some(t), Some(f)) = (s.conn, s.gone_t, s.faction)
                    && self.clock - t >= self.reconnect_grace
                    && w.factions[f as usize].human
                {
                    w.takeover(f);
                }
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
        for s in self.sessions.iter_mut() {
            let Some(conn) = s.conn else { continue };
            let Some(viewer) = Self::viewer_of(w, s.faction) else {
                continue;
            };
            let congested = self
                .conns
                .iter()
                .find(|c| c.id == conn)
                .is_none_or(|c| c.congested);
            if congested {
                // hold back what must not be lost; cosmetic events are dropped
                s.backlog
                    .extend(events.iter().filter(|e| !e.is_cosmetic()).cloned());
                if s.backlog.len() > MAX_BACKLOG {
                    let extra = s.backlog.len() - MAX_BACKLOG;
                    s.backlog.drain(..extra);
                }
                continue;
            }
            let snap = if s.backlog.is_empty() {
                build_snapshot(w, viewer, &mut s.cache, &events)
            } else {
                let mut evs = core::mem::take(&mut s.backlog);
                evs.extend(events.iter().cloned());
                build_snapshot(w, viewer, &mut s.cache, &evs)
            };
            self.outbox.push((conn, ServerMsg::Snapshot(Box::new(snap))));
        }
        if let (Some(o), false) = (w.outcome, self.over_sent) {
            self.over_sent = true;
            let conns: Vec<ConnId> = self
                .sessions
                .iter()
                .filter(|s| s.faction.is_some())
                .filter_map(|s| s.conn)
                .collect();
            for c in conns {
                self.outbox
                    .push((c, ServerMsg::MatchOver { winner: o.winner }));
            }
            let who = o
                .winner
                .map(|f| w.faction(f).name())
                .unwrap_or_else(|| "nessuno".to_string());
            self.log.push(format!("partita finita: vince {who}"));
        }
        if self.kind != RoomKind::Lobby {
            return;
        }
        // lifetime: discard when nobody can come back, or nobody connected for too long
        let finished = w.outcome.is_some();
        let members = self.sessions.iter().any(|s| s.faction.is_some());
        let anyone = self
            .sessions
            .iter()
            .any(|s| s.faction.is_some() && s.conn.is_some());
        self.empty_t = if anyone { 0.0 } else { self.empty_t + TICK_DT };
        let limit = if finished {
            self.finished_timeout
        } else {
            self.abandon_timeout
        };
        if !members || self.empty_t >= limit {
            self.discard_match();
        }
    }

    /// Drops the match and reopens the lobby (tokens of the match expire).
    fn discard_match(&mut self) {
        self.world = None;
        self.pending.clear();
        self.over_sent = false;
        self.empty_t = 0.0;
        for s in self.sessions.iter().filter(|s| s.faction.is_some()) {
            self.expired.push_back(s.token.clone());
        }
        while self.expired.len() > EXPIRED_MEMORY {
            self.expired.pop_front();
        }
        // connected sessions (none normally) go back to the menu; the others are forgotten
        let gone: Vec<ConnId> = self.sessions.iter().filter_map(|s| s.conn).collect();
        for c in gone {
            self.refuse(c, proto::SESSION_EXPIRED);
            self.closed.push(c);
            if let Some(cc) = self.conn_mut(c) {
                cc.session = None;
            }
        }
        self.sessions.clear();
        self.log.push("partita chiusa: la sala d'attesa è di nuovo aperta".to_string());
    }

    /// Messages to send, in order.
    pub fn drain_outbox(&mut self) -> Vec<(ConnId, ServerMsg)> {
        core::mem::take(&mut self.outbox)
    }

    /// Sockets the room wants closed (after their pending messages).
    pub fn drain_closed(&mut self) -> Vec<ConnId> {
        core::mem::take(&mut self.closed)
    }

    /// Italian log lines for the server console.
    pub fn drain_log(&mut self) -> Vec<String> {
        core::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slab::UnitId;
    use crate::snapshot_types::Control;

    fn hello(r: &mut Room, conn: ConnId, token: Option<&str>, name: &str) {
        r.connect(conn, format!("tok-{conn}"));
        r.receive(
            conn,
            ClientMsg::Hello {
                protocol: PROTOCOL_VERSION,
                token: token.map(|t| t.to_string()),
                name: name.into(),
            },
        );
    }

    fn join(r: &mut Room, conn: ConnId, color: Option<KingdomColor>) {
        r.receive(conn, ClientMsg::Join { color });
    }

    fn last_lobby(out: &[(ConnId, ServerMsg)], to: ConnId) -> Option<(Vec<LobbyPlayer>, Option<u32>)> {
        out.iter().rev().find_map(|(c, m)| match m {
            ServerMsg::Lobby {
                players, leader, ..
            } if *c == to => Some((players.clone(), *leader)),
            _ => None,
        })
    }

    fn refused(out: &[(ConnId, ServerMsg)], to: ConnId) -> Option<String> {
        out.iter().find_map(|(c, m)| match m {
            ServerMsg::No { why } if *c == to => Some(why.clone()),
            _ => None,
        })
    }

    fn token_of(out: &[(ConnId, ServerMsg)], to: ConnId) -> String {
        out.iter()
            .find_map(|(c, m)| match m {
                ServerMsg::Welcome { token, .. } if *c == to => Some(token.clone()),
                _ => None,
            })
            .expect("welcome")
    }

    fn faction_of(r: &Room, color: KingdomColor) -> usize {
        r.world()
            .unwrap()
            .factions
            .iter()
            .position(|f| f.color == color)
            .unwrap()
    }

    fn lobby_room() -> Room {
        let mut r = Room::new(RoomKind::Lobby, vec!["http://x".into()]);
        r.seed = Some(3);
        r
    }

    #[test]
    fn lobby_flow() {
        let mut r = lobby_room();
        hello(&mut r, 1, None, "Ada");
        join(&mut r, 1, Some(KingdomColor::Red));
        hello(&mut r, 2, None, "Bob");
        join(&mut r, 2, Some(KingdomColor::Red)); // taken → first free in COLORS order
        hello(&mut r, 3, None, "Cy");
        join(&mut r, 3, None);
        let out = r.drain_outbox();
        let (players, leader) = last_lobby(&out, 3).unwrap();
        let colors: Vec<_> = players.iter().map(|p| p.color).collect();
        assert_eq!(
            colors,
            vec![KingdomColor::Red, KingdomColor::Blue, KingdomColor::Yellow]
        );
        assert_eq!(leader, Some(players[0].id));
        // pick: taken colour ignored, free colour taken
        r.receive(2, ClientMsg::Pick { color: KingdomColor::Red });
        r.receive(2, ClientMsg::Pick { color: KingdomColor::Black });
        let (players, _) = last_lobby(&r.drain_outbox(), 1).unwrap();
        assert_eq!(players[1].color, KingdomColor::Black);
        // only the leader sets the level / starts
        r.receive(2, ClientMsg::SetLevel { level: AiLevel::Hard });
        r.receive(2, ClientMsg::Start);
        assert!(!r.playing() && r.level == AiLevel::Normal);
        r.receive(1, ClientMsg::SetLevel { level: AiLevel::Hard });
        assert_eq!(r.level, AiLevel::Hard);
        // full lobby
        hello(&mut r, 4, None, "D");
        join(&mut r, 4, None);
        hello(&mut r, 5, None, "E");
        join(&mut r, 5, None);
        hello(&mut r, 6, None, "F");
        join(&mut r, 6, None);
        assert_eq!(refused(&r.drain_outbox(), 6).as_deref(), Some(proto::NO_FULL));
        // the leader leaves: leadership passes to the next member
        r.receive(1, ClientMsg::Leave);
        let (players, leader) = last_lobby(&r.drain_outbox(), 2).unwrap();
        assert_eq!(players.len(), 4);
        assert_eq!(leader, Some(players[0].id));
        assert_eq!(players[0].name, "Bob");
        // 6 sent Hello but its Join was refused: it is not part of the match and gets refused at start
        r.receive(2, ClientMsg::Start);
        assert!(r.playing());
        let out = r.drain_outbox();
        for c in [2, 3, 4, 5] {
            assert!(out.iter().any(|(cc, m)| *cc == c
                && matches!(m, ServerMsg::Init { you: Some(_), rejoin: false, .. })));
        }
        assert_eq!(refused(&out, 6).as_deref(), Some(proto::NO_PLAYING));
        assert!(!out.iter().any(|(c, m)| *c == 6 && matches!(m, ServerMsg::Init { .. })));
        let w = r.world().unwrap();
        assert_eq!(w.factions.iter().filter(|f| f.human).count(), 4);
        assert_eq!(w.setup.level, AiLevel::Hard);
        // a newcomer cannot join a running match
        hello(&mut r, 7, None, "G");
        assert_eq!(refused(&r.drain_outbox(), 7).as_deref(), Some(proto::NO_PLAYING));
        assert!(r.drain_closed().contains(&7));
    }

    #[test]
    fn reconnect_reclaims_kingdom() {
        let mut r = lobby_room();
        hello(&mut r, 1, None, "Ada");
        join(&mut r, 1, Some(KingdomColor::Purple));
        hello(&mut r, 2, None, "Bob");
        join(&mut r, 2, Some(KingdomColor::Blue));
        let out = r.drain_outbox();
        let tok = token_of(&out, 1);
        r.receive(1, ClientMsg::Start);
        r.tick();
        let out = r.drain_outbox();
        assert!(out.iter().any(|(c, m)| *c == 1 && matches!(m, ServerMsg::Snapshot(s) if s.full)));
        let f = faction_of(&r, KingdomColor::Purple);
        let units_before: Vec<UnitId> = r
            .world()
            .unwrap()
            .units
            .iter()
            .filter(|(_, u)| u.faction as usize == f)
            .map(|(id, _)| id)
            .collect();
        // page closed: the AI takes over at once, the match goes on (the leader leaving does not end it)
        r.disconnect(1);
        {
            let w = r.world().unwrap();
            assert!(!w.factions[f].human && w.factions[f].owned && !w.factions[f].connected);
        }
        for _ in 0..40 {
            r.tick();
        }
        let out = r.drain_outbox();
        assert!(r.playing());
        // Bob sees the takeover banner and the kingdom marked as disconnected
        assert!(out.iter().any(|(c, m)| *c == 2
            && matches!(m, ServerMsg::Snapshot(s) if s.events.iter().any(|e| matches!(e,
                GameEvent::Say { msg: crate::events::Msg::PlayerLeft { .. }, .. })))));
        let ctl = out.iter().rev().find_map(|(c, m)| match m {
            ServerMsg::Snapshot(s) if *c == 2 && !s.factions.is_empty() => Some(s.factions[f].control),
            _ => None,
        });
        assert_eq!(ctl, Some(Control::Disconnected));
        // page reload: same token → human again, same kingdom, same units
        hello(&mut r, 9, Some(&tok), "Ada");
        let out = r.drain_outbox();
        assert_eq!(token_of(&out, 9), tok);
        assert!(out.iter().any(|(c, m)| *c == 9
            && matches!(m, ServerMsg::Init { rejoin: true, you: Some(y), .. } if *y as usize == f)));
        let snap = out
            .iter()
            .find_map(|(c, m)| match m {
                ServerMsg::Snapshot(s) if *c == 9 => Some(s.clone()),
                _ => None,
            })
            .expect("immediate full snapshot");
        assert!(snap.full && snap.economy.is_some());
        let w = r.world().unwrap();
        assert!(w.factions[f].human && w.factions[f].connected);
        for id in &units_before {
            if w.unit_alive(*id) {
                assert!(snap.units.iter().any(|u| u.id == *id), "own unit resent");
            }
        }
        // commands work again
        r.receive(9, ClientMsg::Command { seq: 5, cmd: Command::Recall { units: vec![] } });
        r.tick();
        assert!(r
            .drain_outbox()
            .iter()
            .any(|(c, m)| *c == 9 && matches!(m, ServerMsg::CommandResult { seq: 5, .. })));
        // Join after the rejoin is ignored silently
        join(&mut r, 9, Some(KingdomColor::Purple));
        assert!(refused(&r.drain_outbox(), 9).is_none());
    }

    #[test]
    fn second_connection_replaces_first() {
        let mut r = lobby_room();
        hello(&mut r, 1, None, "Ada");
        join(&mut r, 1, None);
        let tok = token_of(&r.drain_outbox(), 1);
        r.receive(1, ClientMsg::Start);
        r.drain_outbox();
        let f = faction_of(&r, KingdomColor::Blue);
        hello(&mut r, 2, Some(&tok), "");
        assert_eq!(r.drain_closed(), vec![1]);
        // the old socket closing later changes nothing
        r.disconnect(1);
        let w = r.world().unwrap();
        assert!(w.factions[f].human && w.factions[f].connected);
        r.tick();
        let out = r.drain_outbox();
        assert!(out.iter().all(|(c, _)| *c == 2));
        assert!(!out.iter().any(|(_, m)| matches!(m, ServerMsg::Snapshot(s)
            if s.events.iter().any(|e| matches!(e, GameEvent::Say { msg: crate::events::Msg::PlayerLeft { .. }, .. })))));
    }

    #[test]
    fn reload_in_lobby_keeps_slot() {
        let mut r = lobby_room();
        hello(&mut r, 1, None, "Ada");
        join(&mut r, 1, Some(KingdomColor::Yellow));
        hello(&mut r, 2, None, "Bob");
        join(&mut r, 2, None);
        let tok = token_of(&r.drain_outbox(), 1);
        r.disconnect(1);
        let (players, leader) = last_lobby(&r.drain_outbox(), 2).unwrap();
        assert_eq!(players.len(), 2);
        assert!(!players[0].connected);
        assert_eq!(leader, Some(players[1].id), "a reloading leader does not block the start");
        for _ in 0..20 {
            r.tick();
        }
        hello(&mut r, 3, Some(&tok), "Ada");
        join(&mut r, 3, None);
        let (players, leader) = last_lobby(&r.drain_outbox(), 3).unwrap();
        assert_eq!(players[0].color, KingdomColor::Yellow);
        assert!(players[0].connected);
        assert_eq!(leader, Some(players[0].id));
        // a slot not reclaimed within the grace is freed
        r.disconnect(3);
        for _ in 0..(LOBBY_GRACE / TICK_DT) as usize + 2 {
            r.tick();
        }
        let (players, _) = last_lobby(&r.drain_outbox(), 2).unwrap();
        assert_eq!(players.len(), 1);
        // voluntary leave invalidates the token
        hello(&mut r, 4, Some(&tok), "Ada");
        assert_ne!(token_of(&r.drain_outbox(), 4), tok);
    }

    #[test]
    fn leave_abandon_and_new_lobby() {
        let mut r = lobby_room();
        r.abandon_timeout = 1.0;
        hello(&mut r, 1, None, "Ada");
        join(&mut r, 1, None);
        hello(&mut r, 2, None, "Bob");
        join(&mut r, 2, None);
        let out = r.drain_outbox();
        let (t1, t2) = (token_of(&out, 1), token_of(&out, 2));
        r.receive(1, ClientMsg::Start);
        let f1 = faction_of(&r, KingdomColor::Blue);
        // Ada leaves for good: her token is void, the AI keeps the kingdom
        r.receive(1, ClientMsg::Leave);
        assert!(!r.world().unwrap().factions[f1].owned);
        r.disconnect(1);
        hello(&mut r, 3, Some(&t1), "Ada");
        assert_eq!(refused(&r.drain_outbox(), 3).as_deref(), Some(proto::NO_PLAYING));
        // Bob's connection drops: the match is kept for `abandon_timeout`, then discarded
        r.disconnect(2);
        r.tick();
        assert!(r.playing());
        for _ in 0..30 {
            r.tick();
        }
        assert!(!r.playing());
        assert!(r.drain_log().iter().any(|l| l.contains("partita chiusa")));
        // a new lobby is open; Bob's old token is unknown there and gets a fresh session
        hello(&mut r, 4, Some(&t2), "Bob");
        join(&mut r, 4, None);
        let out = r.drain_outbox();
        assert_ne!(token_of(&out, 4), t2);
        assert_eq!(last_lobby(&out, 4).unwrap().0.len(), 1);
    }

    #[test]
    fn everyone_leaving_discards_at_once() {
        let mut r = lobby_room();
        hello(&mut r, 1, None, "Ada");
        join(&mut r, 1, None);
        r.receive(1, ClientMsg::Start);
        r.receive(1, ClientMsg::Leave);
        r.tick();
        assert!(!r.playing());
    }

    #[test]
    fn commands_rate_limit_and_test_mode() {
        let mut r = lobby_room();
        hello(&mut r, 1, None, "Ada");
        join(&mut r, 1, None);
        r.receive(1, ClientMsg::Start);
        r.drain_outbox();
        for seq in 0..200 {
            r.receive(1, ClientMsg::Command { seq, cmd: Command::Recall { units: vec![] } });
        }
        r.receive(1, ClientMsg::Command { seq: 999, cmd: Command::DebugFreeze { on: true } });
        r.tick();
        let out = r.drain_outbox();
        let n = out
            .iter()
            .filter(|(_, m)| matches!(m, ServerMsg::CommandResult { .. }))
            .count();
        assert_eq!(n, CMD_BURST as usize, "flood beyond the burst is dropped");
        // bucket refills over time
        for _ in 0..20 {
            r.tick();
        }
        r.receive(1, ClientMsg::Command { seq: 1000, cmd: Command::DebugFreeze { on: true } });
        let out = r.drain_outbox();
        assert!(out.iter().any(|(_, m)| matches!(m,
            ServerMsg::CommandResult { seq: 1000, err: Some(e) } if e == TEST_SOLO_ONLY)));
        assert!(!r.world().unwrap().frozen);
        // solo rooms allow test mode
        let mut s = Room::new(RoomKind::Solo, Vec::new());
        s.seed = Some(4);
        hello(&mut s, 1, None, "");
        join(&mut s, 1, Some(KingdomColor::Red));
        s.receive(1, ClientMsg::Start);
        s.receive(1, ClientMsg::Command { seq: 1, cmd: Command::DebugFreeze { on: true } });
        s.tick();
        assert!(s.world().unwrap().frozen);
        // a second solo player is refused
        hello(&mut s, 2, None, "");
        assert!(refused(&s.drain_outbox(), 2).is_some());
    }

    #[test]
    fn congestion_holds_snapshots_but_keeps_private_events() {
        let mut r = lobby_room();
        hello(&mut r, 1, None, "Ada");
        join(&mut r, 1, None);
        r.receive(1, ClientMsg::Start);
        r.tick();
        r.drain_outbox();
        let f = faction_of(&r, KingdomColor::Blue) as FactionIdx;
        r.set_congested(1, true);
        r.world
            .as_mut()
            .unwrap()
            .emit(GameEvent::Toast { to: f, msg: crate::events::Msg::PawnsMax });
        r.tick();
        r.tick();
        assert!(!r.drain_outbox().iter().any(|(_, m)| matches!(m, ServerMsg::Snapshot(_))));
        r.set_congested(1, false);
        r.tick();
        let out = r.drain_outbox();
        let snaps: Vec<_> = out
            .iter()
            .filter_map(|(_, m)| match m {
                ServerMsg::Snapshot(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(snaps.len(), 1);
        assert!(!snaps[0].full);
        assert!(snaps[0].events.iter().any(|e| matches!(e, GameEvent::Toast { .. })));
    }

    #[test]
    fn eliminated_player_rejoins_as_spectator() {
        let mut r = lobby_room();
        hello(&mut r, 1, None, "Ada");
        join(&mut r, 1, None);
        hello(&mut r, 2, None, "Bob");
        join(&mut r, 2, None);
        let tok = token_of(&r.drain_outbox(), 1);
        r.receive(1, ClientMsg::Start);
        let f = faction_of(&r, KingdomColor::Blue) as FactionIdx;
        r.disconnect(1);
        r.world.as_mut().unwrap().eliminate_faction(f, None);
        r.tick();
        hello(&mut r, 3, Some(&tok), "Ada");
        let out = r.drain_outbox();
        assert!(out.iter().any(|(c, m)| *c == 3 && matches!(m, ServerMsg::Init { you: None, rejoin: true, .. })));
        assert!(out.iter().any(|(c, m)| *c == 3
            && matches!(m, ServerMsg::Snapshot(s) if s.fog_off && s.you.is_none())));
    }
}
