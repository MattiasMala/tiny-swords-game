//! Main menu and multiplayer lobby (spec 06 §4-§5, ui.js:553-647).

use super::kit::{ACCENT, INK, Ui, WHITE, panel};
use super::paint::Align;
use super::{Env, UiState};
use crate::AppState;
use crate::link::LinkRequest;
use ts_sim::defs::{AiLevel, KingdomColor, UnitKind};
use ts_sim::proto::ClientMsg;

/// `LEVEL_UNIT`.
fn level_unit(l: AiLevel) -> UnitKind {
    match l {
        AiLevel::Easy => UnitKind::Pawn,
        AiLevel::Normal => UnitKind::Warrior,
        AiLevel::Hard => UnitKind::Knight,
    }
}

/// `drawMenu`.
pub fn draw_menu(u: &mut Ui, st: &mut UiState, env: &mut Env, force_lobby: bool) {
    let (w, h) = (u.w(), u.h());
    let s = (w / 980.0).min(h / 760.0).min(1.35);
    let pw = 720.0 * s;
    let ph = 600.0 * s;
    let px = (w - pw) / 2.0;
    let py = (h - ph) / 2.0 + 28.0 * s;
    u.p.locked = st.help;
    u.store_banner(px, py, pw, ph, s);
    if *env.state.get() == AppState::Lobby || force_lobby {
        draw_lobby(u, st, env, s, px, py, pw);
    } else {
        draw_main(u, st, env, s, px, py);
    }
    // help (i)
    let r = u.tiny_button(px + pw - 56.0 * s, py + 84.0 * s, 44.0 * s, false, false, Some(11));
    if r.clicked {
        st.help = true;
        st.help_fresh = true;
    }
    u.p.set_hot();
    u.p.locked = false;
}

/// `kingdomRow(y, s, selected, info)` → clicked colour.
fn kingdom_row(
    u: &mut Ui,
    y: f32,
    s: f32,
    selected: Option<KingdomColor>,
    info: &dyn Fn(KingdomColor) -> (String, UnitKind, bool),
) -> Option<KingdomColor> {
    let w = u.w();
    let mut out = None;
    for (i, &c) in KingdomColor::ALL.iter().enumerate() {
        let cx = w / 2.0 + (i as f32 - 2.0) * 124.0 * s;
        let r = u.p.hit(cx - 52.0 * s, y - 4.0 * s, 104.0 * s, 140.0 * s);
        let (text, kind, dimmed) = info(c);
        u.panel(&panel::STORE_SLOTS, cx - 46.0 * s, y, 92.0 * s, 108.0 * s, 0.6 * s);
        u.pennant(c, cx, y - 2.0 * s, 0.62 * s);
        let a0 = u.p.alpha;
        if dimmed {
            u.p.alpha = a0 * 0.4;
        }
        u.avatar(c, kind, cx - 32.0 * s, y + 42.0 * s, 64.0 * s);
        u.p.alpha = a0;
        u.plain(text, cx, y + 124.0 * s, 16.0 * s, INK, Align::Center);
        if selected == Some(c) {
            u.brackets(cx - 56.0 * s, y - 10.0 * s, 112.0 * s, 126.0 * s, 0.9 * s);
        }
        if r.clicked {
            u.play(ts_sim::defs::Sound::Click);
            out = Some(c);
        }
    }
    out
}

/// `levelRow(y, s, color, selected, enabled)` → clicked level.
fn level_row(
    u: &mut Ui,
    y: f32,
    s: f32,
    color: KingdomColor,
    selected: AiLevel,
    enabled: bool,
) -> Option<AiLevel> {
    let w = u.w();
    let clock = u.p.io.clock;
    let mut out = None;
    for (i, &l) in AiLevel::ALL.iter().enumerate() {
        let cx = w / 2.0 + (i as f32 - 1.0) * 190.0 * s;
        let clicked = enabled && u.p.hit(cx - 80.0 * s, y - 4.0 * s, 160.0 * s, 136.0 * s).clicked;
        u.panel(&panel::BANNER_SLOTS, cx - 72.0 * s, y, 144.0 * s, 100.0 * s, 0.7 * s);
        let k = level_unit(l);
        let fps = u
            .atlas
            .sheet_id(&super::kit::unit_idle_sheet(color, k))
            .map(|id| u.atlas.sheet(id).fps)
            .filter(|f| *f > 0.0)
            .unwrap_or(8.0) as f64;
        let frame = (clock * fps).floor() as usize;
        u.unit_frame(color, k, frame, cx, y + 86.0 * s, 0.9 * s);
        u.plain(l.label(), cx, y + 118.0 * s, 16.0 * s, INK, Align::Center);
        if selected == l {
            u.brackets(cx - 82.0 * s, y - 10.0 * s, 164.0 * s, 120.0 * s, 0.9 * s);
        }
        if clicked {
            u.play(ts_sim::defs::Sound::Click);
            out = Some(l);
        }
    }
    out
}

/// `drawMainMenu`.
fn draw_main(u: &mut Ui, _st: &mut UiState, env: &mut Env, s: f32, px: f32, py: f32) {
    let w = u.w();
    let color = env.lobby.color;
    u.store_ribbon(color, w / 2.0, py + 4.0 * s, 560.0 * s, s, "TINY SWORDS", 42.0 * s);
    u.plain("La Guerra dei Cinque Regni", w / 2.0, py + 84.0 * s, 22.0 * s, INK, Align::Center);
    u.plain("Scegli il tuo regno", w / 2.0, py + 124.0 * s, 17.0 * s, ACCENT, Align::Center);
    let info = |c: KingdomColor| (c.name_it().to_string(), UnitKind::Warrior, false);
    if let Some(c) = kingdom_row(u, py + 146.0 * s, s, Some(color), &info) {
        env.lobby.color = c;
    }
    u.plain(
        "Difficoltà dei regni avversari",
        w / 2.0,
        py + 312.0 * s,
        17.0 * s,
        ACCENT,
        Align::Center,
    );
    if let Some(l) = level_row(u, py + 332.0 * s, s, color, env.lobby.level, true) {
        env.lobby.level = l;
    }
    if u.big_button(w / 2.0 - 270.0 * s, py + 480.0 * s, 200.0 * s, 64.0 * s, 0.8 * s, "GIOCA", false, Some(7)) {
        env.lobby.note = None;
        env.req.write(LinkRequest::StartSolo {
            color: env.lobby.color,
            level: env.lobby.level,
        });
    }
    if u.big_button(
        w / 2.0 - 50.0 * s,
        py + 480.0 * s,
        320.0 * s,
        64.0 * s,
        0.8 * s,
        "MULTIGIOCATORE",
        false,
        Some(5),
    ) {
        env.lobby.note = None;
        env.req.write(LinkRequest::ConnectRemote);
    }
    let off = !env.audio.enabled;
    if u.tiny_button(px + 56.0 * s, py + 84.0 * s, 44.0 * s, false, off, Some(12)).clicked {
        env.audio.enabled = !env.audio.enabled;
    }
    if let Some(note) = env.lobby.note.clone() {
        u.text(note, w / 2.0, py + 626.0 * s, 16.0 * s, WHITE, Align::Center);
    }
}

/// Lobby screen (ui.js:620-647).
fn draw_lobby(u: &mut Ui, st: &mut UiState, env: &mut Env, s: f32, px: f32, py: f32, _pw: f32) {
    let w = u.w();
    let me = env
        .lobby
        .you
        .and_then(|id| env.lobby.players.iter().find(|p| p.id == id))
        .cloned();
    let host = env.lobby.is_leader();
    let title_color = me.as_ref().map_or(env.lobby.color, |m| m.color);
    u.store_ribbon(title_color, w / 2.0, py + 4.0 * s, 560.0 * s, s, "MULTIGIOCATORE", 38.0 * s);
    if u.tiny_button(px + 56.0 * s, py + 84.0 * s, 44.0 * s, false, true, Some(9)).clicked {
        st.starting = false;
        env.req.write(LinkRequest::Close { leave: true });
        return;
    }
    let Some(me) = me else {
        u.plain("Ingresso nella partita…", w / 2.0, py + 290.0 * s, 21.0 * s, INK, Align::Center);
        return;
    };
    u.plain(
        format!(
            "Giocatori: {} su {} · i regni liberi li guida l'IA",
            env.lobby.players.len(),
            KingdomColor::ALL.len()
        ),
        w / 2.0,
        py + 84.0 * s,
        18.0 * s,
        INK,
        Align::Center,
    );
    u.plain("Scegli il tuo regno", w / 2.0, py + 124.0 * s, 17.0 * s, ACCENT, Align::Center);
    let players = env.lobby.players.clone();
    let leader = env.lobby.leader;
    let me_id = me.id;
    let taken = |c: KingdomColor| players.iter().any(|p| p.color == c);
    let info = |c: KingdomColor| {
        let p = players.iter().find(|p| p.color == c);
        match p {
            Some(p) => {
                let t = if p.id == me_id {
                    "Tu"
                } else if Some(p.id) == leader {
                    "Host"
                } else {
                    "Giocatore"
                };
                (t.to_string(), UnitKind::Warrior, false)
            }
            None => ("IA".to_string(), UnitKind::Pawn, true),
        }
    };
    // `Net.choose(color)`: only a free kingdom can be taken (the server checks again)
    if let Some(c) = kingdom_row(u, py + 146.0 * s, s, Some(me.color), &info)
        && c != me.color
        && !taken(c)
    {
        env.lobby.color = c;
        env.out.send(ClientMsg::Pick { color: c });
    }
    let head = if host {
        "Difficoltà dell'IA"
    } else {
        "Difficoltà dell'IA (la sceglie l'host)"
    };
    u.plain(head, w / 2.0, py + 312.0 * s, 17.0 * s, ACCENT, Align::Center);
    if let Some(l) = level_row(u, py + 332.0 * s, s, me.color, env.lobby.lobby_level, host) {
        env.out.send(ClientMsg::SetLevel { level: l });
    }
    if st.starting {
        u.plain("Avvio della partita…", w / 2.0, py + 512.0 * s, 19.0 * s, ACCENT, Align::Center);
    } else if host {
        if u.big_button(w / 2.0 - 130.0 * s, py + 480.0 * s, 260.0 * s, 64.0 * s, 0.8 * s, "INIZIA", false, Some(7))
        {
            st.starting = true;
            env.out.send(ClientMsg::Start);
        }
    } else {
        u.plain(
            "In attesa che l'host avvii la partita…",
            w / 2.0,
            py + 512.0 * s,
            19.0 * s,
            ACCENT,
            Align::Center,
        );
    }
    if host {
        let where_ = env
            .lobby
            .urls
            .first()
            .cloned()
            .unwrap_or_else(|| "http://<IP di questo PC>:8000".to_string());
        u.text(
            format!("Gli altri giocatori aprono  {where_}"),
            w / 2.0,
            py + 626.0 * s,
            17.0 * s,
            WHITE,
            Align::Center,
        );
    }
}

/// Native dev helper (`TS_UI_SCREEN=lobby` on the menu): fills the lobby with sample players so the lobby screen can be
/// screenshotted without a server.
pub fn dev_fake_lobby(env: &mut Env) {
    if env.lobby.you.is_some() {
        return;
    }
    let p = |id: u32, name: &str, color: KingdomColor| ts_sim::proto::LobbyPlayer {
        id,
        name: name.to_string(),
        color,
        connected: true,
    };
    env.lobby.players = vec![p(1, "Anna", KingdomColor::Red), p(2, "Bruno", KingdomColor::Blue)];
    env.lobby.you = Some(1);
    env.lobby.leader = Some(1);
    env.lobby.urls = vec!["http://192.168.1.20:8000".to_string()];
}
