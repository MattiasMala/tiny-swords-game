//! Help, pause / network menu, game over, reconnecting overlay (spec 06 §11, 08 addenda N1/N3).

use super::kit::{ACCENT, INK, MUTED, PARCH, Ui, WHITE, panel};
use super::paint::Align;
use super::{Env, MatchResult, UiState, View};
use crate::link::LinkRequest;
use ts_sim::defs::{AGE_NAME, KingdomColor};

/// `HELP_LINES` (ui.js:5-18).
pub const HELP_LINES: [(&str, &str); 12] = [
    (
        "Clic · trascina",
        "seleziona unità ed edifici (doppio clic: tutte le uguali in vista)",
    ),
    (
        "Clic destro",
        "muovi, attacca, raccogli, costruisci, ripara, punto di raccolta",
    ),
    (
        "Frecce o WASD · bordo dello schermo",
        "sposta la visuale · con Maiusc più svelta, con Ctrl veloce",
    ),
    ("Rotellina · +  e  −", "ingrandisci e rimpicciolisci la visuale"),
    (
        "Frecce o WASD + Invio o Spazio",
        "nelle schermate: scegli e premi · posando un edificio: muovi e posa",
    ),
    ("Maiusc + 1-9 · 1-9", "crea · richiama un gruppo di unità"),
    (".  ·  H  ·  Spazio", "pedone inattivo · castello · centra la selezione"),
    (
        "U · L · I · K · C",
        "schermate: unità · lavoratori · ricerche · mercato · costruisci",
    ),
    (
        "V · Tab · Backspace",
        "sposta un edificio · tutte le truppe · richiamale alla base",
    ),
    (
        "X · E · F · G",
        "con una selezione: attacco in marcia · esplora · ferma · punto di raccolta",
    ),
    ("Esc · P · Canc", "annulla o deseleziona · pausa · demolisci"),
    (
        "M · F1 · F2 · F3 · F4",
        "audio · aiuto · luci · risoluzione · modalità test (risorse, tempo, nebbia)",
    ),
];

/// `HELP_NOTES` (ui.js:19-27).
pub const HELP_NOTES: [&str; 7] = [
    "I Pedoni raccolgono legna, oro, pietra e cibo: clic destro su alberi, giacimenti e pecore, o schermata Lavoratori.",
    "Da soli lavorano solo nel territorio esplorato. Se vedono truppe nemiche si rifugiano nelle case finché non passano.",
    "Le Case danno posto ai Pedoni, gli Accampamenti alle truppe. Più fabbriche dello stesso tipo addestrano più in fretta.",
    "Punti da conquistare: tienici accanto delle truppe per qualche secondo. Rendono risorse; attorno agli Avamposti si costruisce.",
    "Torri e Palizzate solo vicino al castello o a un tuo Avamposto. Le tue unità passano fra i tuoi edifici, i nemici no.",
    "In marcia d'attacco l'esercito va in formazione: mischia e Arieti davanti, tiratori dietro, Monaci in fondo.",
    "Vince l'ultimo regno rimasto: abbatti i castelli nemici e difendi il tuo.",
];

/// `drawHelp` (ui.js:500-514) with the `fresh` guard of addendum UI1.
pub fn draw_help(u: &mut Ui, st: &mut UiState) {
    let (w, h) = (u.w(), u.h());
    let h0 = 132.0 + HELP_LINES.len() as f32 * 32.0 + HELP_NOTES.len() as f32 * 28.0;
    let s = (w / 980.0).min(h / (h0 + 20.0)).min(1.3);
    let pw = 880.0 * s;
    let ph = h0 * s;
    let x = (w - pw) / 2.0;
    let y = (h - ph) / 2.0;
    u.p.dim(0.45);
    u.panel(&panel::BANNER, x, y, pw, ph, 0.8 * s);
    u.plain("Comandi", w / 2.0, y + 32.0 * s, 28.0 * s, INK, Align::Center);
    for (i, (k, d)) in HELP_LINES.iter().enumerate() {
        let ly = y + 80.0 * s + i as f32 * 32.0 * s;
        u.plain(*k, x + 36.0 * s, ly, 15.0 * s, ACCENT, Align::Left);
        u.plain(*d, x + 330.0 * s, ly, 15.0 * s, INK, Align::Left);
    }
    for (i, n) in HELP_NOTES.iter().enumerate() {
        let ly = y + (100.0 + HELP_LINES.len() as f32 * 32.0 + i as f32 * 28.0) * s;
        u.plain(format!("• {n}"), x + 36.0 * s, ly, 14.0 * s, INK, Align::Left);
    }
    let close = u
        .tiny_button(x + pw - 34.0 * s, y + 32.0 * s, 40.0 * s, false, true, Some(9))
        .clicked;
    let outside = u.p.io.pressed && !u.p.over(x, y, pw, ph) && !st.help_fresh;
    if close || outside {
        st.help = false;
    }
    st.help_fresh = false;
}

/// `drawPause` (ui.js:516-528), network note per 08 addenda N3.
pub fn draw_pause(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View) {
    let (w, h) = (u.w(), u.h());
    let s = (w / 900.0).min(h / 640.0).min(1.3);
    let pw = 380.0 * s;
    let ph = 270.0 * s;
    let x = (w - pw) / 2.0;
    let y = (h - ph) / 2.0;
    let net = env.lobby.remote;
    u.p.dim(0.5);
    u.panel(&panel::SPECIAL, x, y, pw, ph, 0.9 * s);
    u.big_ribbon(
        v.color,
        w / 2.0,
        y + 16.0 * s,
        300.0 * s,
        0.75 * s,
        if net { "MENU" } else { "PAUSA" },
        26.0 * s,
    );
    if u.big_button(x + 60.0 * s, y + 80.0 * s, pw - 120.0 * s, 62.0 * s, 0.75 * s, "Riprendi", false, Some(7)) {
        st.paused = false;
    }
    if u.big_button(
        x + 60.0 * s,
        y + 166.0 * s,
        pw - 120.0 * s,
        62.0 * s,
        0.75 * s,
        if net { "Esci" } else { "Menu" },
        true,
        Some(8),
    ) {
        st.paused = false;
        env.req.write(LinkRequest::Close { leave: true });
    }
    if net {
        u.text(
            "La partita continua mentre il menu è aperto.",
            w / 2.0,
            y + 248.0 * s,
            13.0 * s,
            MUTED,
            Align::Center,
        );
    }
}

/// `drawGameOver` (ui.js:530-551).
pub fn draw_game_over(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View) {
    let (w, h) = (u.w(), u.h());
    let s = (w / 900.0).min(h / 640.0).min(1.3);
    let pw = 500.0 * s;
    let ph = 330.0 * s;
    let x = (w - pw) / 2.0;
    let y = (h - ph) / 2.0;
    let win = st.result == Some(MatchResult::Win);
    u.p.dim(0.55);
    u.panel(&panel::SPECIAL, x, y, pw, ph, 0.9 * s);
    u.big_ribbon(
        v.color,
        w / 2.0,
        y + 14.0 * s,
        380.0 * s,
        0.85 * s,
        if win { "VITTORIA!" } else { "SCONFITTA" },
        30.0 * s,
    );
    let alive: Vec<KingdomColor> = env
        .mirror
        .view
        .factions
        .iter()
        .filter(|f| f.alive)
        .map(|f| f.color)
        .collect();
    let line = if win {
        "I cinque regni sono tuoi.".to_string()
    } else if alive.len() == 1 {
        format!("Vince il {}.", alive[0].kingdom_name())
    } else {
        format!("Il {} è caduto.", v.color.kingdom_name())
    };
    u.text(line, w / 2.0, y + 84.0 * s, 19.0 * s, PARCH, Align::Center);
    let me = st
        .player_idx
        .and_then(|i| env.mirror.view.factions.iter().find(|f| f.idx == i));
    let (kills, razed, age) = me.map_or((0, 0, 1), |f| (f.kills, f.razed, f.age));
    let t = st.end_time.max(0.0).floor() as u32;
    let stats = format!(
        "Tempo {}:{:02}    Nemici abbattuti {}    Edifici distrutti {}    {}",
        t / 60,
        t % 60,
        kills,
        razed,
        AGE_NAME.get(age as usize).copied().unwrap_or("")
    );
    u.text(stats, w / 2.0, y + 122.0 * s, 14.0 * s, MUTED, Align::Center);
    let bx = x + 90.0 * s;
    let bw = pw - 180.0 * s;
    let bh = 62.0 * s;
    if env.solo() {
        if u.big_button(bx, y + 156.0 * s, bw, bh, 0.75 * s, "Rigioca", false, Some(7)) {
            env.req.write(LinkRequest::StartSolo {
                color: env.lobby.color,
                level: env.lobby.level,
            });
        }
        if u.big_button(bx, y + 236.0 * s, bw, bh, 0.75 * s, "Menu", true, Some(8)) {
            env.req.write(LinkRequest::Close { leave: true });
        }
    } else {
        let running = alive.len() > 1;
        if running && u.big_button(bx, y + 156.0 * s, bw, bh, 0.75 * s, "Guarda", false, Some(7)) {
            st.spectate = true;
            st.over = false;
        }
        if u.big_button(bx, y + 236.0 * s, bw, bh, 0.75 * s, "Esci", true, Some(8)) {
            env.req.write(LinkRequest::Close { leave: true });
        }
    }
}

/// Reconnecting overlay (08 addenda N1/N3): network game whose socket is down.
pub fn draw_reconnecting(u: &mut Ui, env: &Env) {
    if !env.lobby.remote || env.lobby.connected {
        return;
    }
    let (w, h) = (u.w(), u.h());
    let s = (w / 900.0).min(h / 640.0).min(1.3);
    u.p.dim(0.5);
    let tw = u.tw(ts_sim::proto::RECONNECTING, 22.0 * s);
    u.panel(&panel::PAPER, w / 2.0 - tw / 2.0 - 40.0 * s, h / 2.0 - 34.0 * s, tw + 80.0 * s, 68.0 * s, 0.6 * s);
    u.plain(ts_sim::proto::RECONNECTING, w / 2.0, h / 2.0, 22.0 * s, INK, Align::Center);
    let _ = WHITE;
}
