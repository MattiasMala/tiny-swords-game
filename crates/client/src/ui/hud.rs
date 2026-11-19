//! In-game HUD (spec 06 §6, §8, §11.4): top bar, era and clock, kingdom ribbon, idle-pawn and alert buttons,
//! top-right buttons, the 8 shortcuts, hints / toast line, announcement ribbon, test-mode notice, tooltip.

use super::kit::{GOLD, GREEN, GREY, INK, IconSpec, RED, TOAST, Ui, WHITE, panel, x_icon};
use super::paint::{Align, Tip};
use super::{Env, Screen, UiState, View, minimap, selection};
use ts_sim::defs::{AGE_NAME, Res, UnitKind};
use ts_sim::snapshot_types::UF_INSIDE;

/// HUD scale `G.us` (logical px: dpr = 1).
pub fn hud_scale(w: f32, h: f32) -> f32 {
    (w / 1500.0).min(h / 850.0).clamp(0.72, 1.5)
}

/// `drawHUD`.
pub fn draw_hud(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View) {
    let s = hud_scale(u.w(), u.h());
    draw_top_bar(u, st, env, v, s);
    let right = minimap::draw_minimap(u, st, env, v, s);
    selection::draw_selection(u, st, env, v, s, right);
    draw_shortcuts(u, st, env, v, s);
    draw_hints(u, st, env, s);
    if !st.over {
        draw_announcement(u, st, s);
    }
    draw_test_notice(u, st, env, s);
    if let Some(tip) = u.p.tip.take() {
        u.tooltip(&tip, s, v.color, &v.res);
    }
}

/// `drawSpectator` (ui.js:483-490).
pub fn draw_spectator(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View) {
    let s = hud_scale(u.w(), u.h());
    let w = u.w();
    if u.tiny_button(w - 40.0 * s, 38.0 * s, 44.0 * s, false, false, Some(10)).clicked {
        st.paused = true;
    }
    let off = !env.audio.enabled;
    if u.tiny_button(w - 92.0 * s, 38.0 * s, 44.0 * s, false, off, Some(12)).clicked {
        env.audio.enabled = !env.audio.enabled;
    }
    minimap::draw_minimap(u, st, env, v, s);
    draw_announcement(u, st, s);
    u.small_ribbon(
        v.color,
        0,
        w / 2.0,
        36.0 * s,
        520.0 * s,
        0.9 * s,
        "Spettatore · frecce o minimappa per guardare",
        15.0 * s,
    );
}

/// `drawTopBar` (ui.js:174-224).
fn draw_top_bar(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View, s: f32) {
    let w = u.w();
    let Some(eco) = env.mirror.view.economy.clone() else {
        return;
    };
    let bw = 800.0 * s;
    let x = w / 2.0 - bw / 2.0;
    let y = 12.0 * s;
    let h = 58.0 * s;
    let col = bw / 6.0;
    u.p.area(x, y, bw, h);
    u.panel(&panel::PAPER, x, y, bw, h, 0.6 * s);
    for (i, &k) in Res::ALL.iter().enumerate() {
        let r = eco.rates[i];
        let cx = x + col * i as f32;
        u.icon(&super::kit::res_icon(k), cx + 30.0 * s, y + h / 2.0, 40.0 * s);
        u.plain(eco.res[i].to_string(), cx + 54.0 * s, y + 21.0 * s, 19.0 * s, INK, Align::Left);
        u.plain(
            format!("+{r}/min"),
            cx + 54.0 * s,
            y + 43.0 * s,
            12.5 * s,
            if r > 0 { GREEN } else { GREY },
            Align::Left,
        );
        if u.p.over(cx, y, col, h) {
            u.p.tip = Some(Tip {
                x: x + col * (i as f32 + 0.5),
                y: y + h + 34.0 * s,
                below: true,
                title: k.label().to_string(),
                desc: format!(
                    "Rendita attesa: +{r} al minuto. Conta chi ci lavora adesso, quanto porta a ogni viaggio e quanto dura il giro fra la risorsa e il punto di consegna, più i punti conquistati."
                ),
                ..Default::default()
            });
        }
    }
    // pawns
    let np = eco.pawn_n;
    let cx = x + col * 4.0;
    u.avatar(v.color, UnitKind::Pawn, cx + 8.0 * s, y + 8.0 * s, 42.0 * s);
    u.plain(
        format!("{}/{}", np, eco.pawn_cap),
        cx + 54.0 * s,
        y + h / 2.0 + s,
        19.0 * s,
        if np >= eco.pawn_cap { RED } else { INK },
        Align::Left,
    );
    if u.p.over(cx, y, col, h) {
        u.p.tip = Some(Tip {
            x: x + col * 4.5,
            y: y + h + 34.0 * s,
            below: true,
            title: "Pedoni".to_string(),
            desc: format!(
                "{} su {} posti. Ogni Casa ne aggiunge 5, fino a 30.",
                np, eco.pawn_cap
            ),
            ..Default::default()
        });
    }
    // troops
    let nt = eco.troop_n;
    let cx = x + col * 5.0;
    u.icon(&x_icon("tent"), cx + 30.0 * s, y + h / 2.0, 40.0 * s);
    u.plain(
        format!("{}/{}", nt, eco.troop_cap),
        cx + 54.0 * s,
        y + h / 2.0 + s,
        19.0 * s,
        if nt >= eco.troop_cap { RED } else { INK },
        Align::Left,
    );
    if u.p.over(cx, y, col, h) {
        u.p.tip = Some(Tip {
            x: x + col * 5.5,
            y: y + h + 34.0 * s,
            below: true,
            title: "Truppe".to_string(),
            desc: format!(
                "{} su {} posti. Ogni Accampamento ne aggiunge 8, ogni edificio che addestra truppe 2, fino a 50.",
                nt, eco.troop_cap
            ),
            ..Default::default()
        });
    }
    // era & clock
    u.sword_banner(v.color, w / 2.0, y + h + 34.0 * s, 400.0 * s, 0.5 * s, "", 15.0 * s);
    let age = AGE_NAME.get(eco.age as usize).copied().unwrap_or("");
    u.plain(age, w / 2.0 - 34.0 * s, y + h + 27.0 * s, 15.0 * s, INK, Align::Center);
    u.icon(&x_icon("hourglass"), w / 2.0 + 78.0 * s, y + h + 27.0 * s, 24.0 * s);
    let t = env.mirror.view.time.max(0.0).floor() as u32;
    u.plain(
        format!("{:02}:{:02}", t / 60, t % 60),
        w / 2.0 + 94.0 * s,
        y + h + 27.0 * s,
        15.0 * s,
        INK,
        Align::Left,
    );

    // top-left: kingdom ribbon, idle pawns, under attack
    u.small_ribbon(
        v.color,
        1,
        150.0 * s,
        36.0 * s,
        250.0 * s,
        0.9 * s,
        &v.color.kingdom_name(),
        17.0 * s,
    );
    let idle = env
        .mirror
        .view
        .units
        .values()
        .filter(|un| {
            Some(un.faction) == v.you
                && un.kind == UnitKind::Pawn
                && un.act == ts_sim::defs::Act::Idle
                && un.flags & UF_INSIDE == 0
        })
        .count();
    if idle > 0 {
        let r = u.small_button(48.0 * s, 102.0 * s, 96.0 * s, false, false);
        u.avatar(v.color, UnitKind::Pawn, 28.0 * s, 80.0 * s + r.dy, 40.0 * s);
        u.text(idle.to_string(), 70.0 * s, 122.0 * s, 16.0 * s, GOLD, Align::Left);
        if r.hover {
            u.p.tip = Some(Tip {
                x: 170.0 * s,
                y: 250.0 * s,
                title: "Pedoni inattivi".to_string(),
                key: Some(".".to_string()),
                desc: "Seleziona il prossimo pedone senza un compito.".to_string(),
                ..Default::default()
            });
        }
        if r.clicked {
            u.play(ts_sim::defs::Sound::Click);
            selection::next_idle(st, env);
        }
    }
    if let Some(at) = st.alert_at
        && env.mirror.view.time - st.alert_t < 30.0
    {
        let bx = if idle > 0 { 120.0 } else { 48.0 } * s;
        let r = u.small_button(bx, 102.0 * s, 96.0 * s, false, true);
        u.icon(&x_icon("bell"), bx, 100.0 * s + r.dy, 44.0 * s);
        if r.hover {
            u.p.tip = Some(Tip {
                x: 170.0 * s,
                y: 250.0 * s,
                title: "Sotto attacco".to_string(),
                desc: "Porta la visuale dove il regno è stato colpito.".to_string(),
                ..Default::default()
            });
        }
        if r.clicked {
            u.play(ts_sim::defs::Sound::Click);
            env.rig.look_at(at);
        }
    }

    // top-right
    let bx = w - 40.0 * s;
    let size = 44.0 * s;
    let cy = 38.0 * s;
    if u.tiny_button(bx, cy, size, false, false, Some(10)).clicked {
        st.paused = true;
    }
    let off = !env.audio.enabled;
    if u.tiny_button(bx - 52.0 * s, cy, size, false, off, Some(12)).clicked {
        env.audio.enabled = !env.audio.enabled;
    }
    if u.tiny_button(bx - 104.0 * s, cy, size, false, false, Some(11)).clicked {
        st.help = true;
        st.help_fresh = true;
    }
}

/// One of the 8 shortcuts (`SHORTCUTS`, panels.js:9-24).
pub struct Shortcut {
    pub key: &'static str,
    pub icon: IconSpec,
    pub title: &'static str,
    pub screen: Option<Screen>,
    pub desc: &'static str,
    pub red: bool,
}

pub const SHORTCUTS: [Shortcut; 8] = [
    Shortcut {
        key: "U",
        icon: IconSpec::Unit(UnitKind::Warrior),
        title: "Unità",
        screen: Some(Screen::Unita),
        desc: "Tutte le tue unità in un elenco: le addestri, le spunti e cambi il loro lavoro.",
        red: false,
    },
    Shortcut {
        key: "L",
        icon: IconSpec::X("pickaxe"),
        title: "Lavoratori",
        screen: Some(Screen::Lavoratori),
        desc: "Quanti pedoni su ogni risorsa, con un cursore per ciascuna.",
        red: false,
    },
    Shortcut {
        key: "I",
        icon: IconSpec::X("book"),
        title: "Ricerche",
        screen: Some(Screen::Ricerche),
        desc: "L'albero delle ricerche: un livello per ogni era.",
        red: false,
    },
    Shortcut {
        key: "K",
        icon: IconSpec::X("scales"),
        title: "Mercato",
        screen: Some(Screen::Mercato),
        desc: "Vendi e compra legna, cibo e pietra in cambio d'oro.",
        red: false,
    },
    Shortcut {
        key: "C",
        icon: IconSpec::Ui(1),
        title: "Costruisci",
        screen: Some(Screen::Costruisci),
        desc: "Scegli un edificio e poi dove metterlo: ci va il pedone libero più vicino, o quelli selezionati.",
        red: false,
    },
    Shortcut {
        key: "V",
        icon: IconSpec::X("barrow"),
        title: "Sposta edificio",
        screen: None,
        desc: "Sposta l'edificio selezionato, o il prossimo che scegli (clic, oppure Frecce e Invio); poi la nuova casella, allo stesso modo.",
        red: false,
    },
    Shortcut {
        key: "Tab",
        icon: IconSpec::X("shield"),
        title: "Tutte le truppe",
        screen: None,
        desc: "Seleziona tutti i soldati, ovunque siano.",
        red: false,
    },
    Shortcut {
        key: "Bksp",
        icon: IconSpec::Build(ts_sim::defs::BuildingKind::Castle),
        title: "Richiama alla base",
        screen: None,
        desc: "Tutte le truppe tornano davanti al castello senza fermarsi a combattere.",
        red: true,
    },
];

/// `drawShortcuts` (panels.js:29-42).
fn draw_shortcuts(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View, s: f32) {
    let (w, h) = (u.w(), u.h());
    let step = 72.5 * s;
    let x0 = w - 304.0 * s;
    let y0 = h - 364.0 * s;
    u.p.area(x0, y0, step * 4.0, 146.0 * s);
    for (i, c) in SHORTCUTS.iter().enumerate() {
        let cx = x0 + ((i % 4) as f32 + 0.5) * step;
        let cy = y0 + ((i / 4) as f32 + 0.5) * 72.0 * s;
        let r = u.small_button(cx, cy, 98.0 * s, true, c.red);
        u.spec(c.icon, v.color, cx, cy - 2.0 * s + r.dy, 38.0 * s);
        u.text(c.key, cx - 27.0 * s, cy - 24.0 * s, 11.5 * s, WHITE, Align::Left);
        let active = match c.screen {
            Some(sc) => st.screen == Some(sc),
            None => i == 5 && selection::move_mode_active(env),
        };
        if active {
            u.brackets(cx - 36.0 * s, cy - 37.0 * s, 72.0 * s, 72.0 * s, 0.7 * s);
        }
        if r.hover {
            u.p.tip = Some(Tip {
                x: cx,
                y: y0 + (i / 4) as f32 * 72.0 * s + 6.0 * s,
                title: c.title.to_string(),
                key: Some(if c.key == "Bksp" { "Backspace" } else { c.key }.to_string()),
                desc: c.desc.to_string(),
                ..Default::default()
            });
        }
        if r.clicked {
            u.play(ts_sim::defs::Sound::Click);
            if let Some(sc) = c.screen {
                st.open(sc);
                selection::cancel_modes(env);
            } else {
                let why = selection::run_shortcut(i, st, env);
                st.say(why);
            }
        }
    }
}

/// `drawHints` (ui.js:242-254).
fn draw_hints(u: &mut Ui, st: &mut UiState, env: &mut Env, s: f32) {
    let (w, h) = (u.w(), u.h());
    let y = h - 188.0 * s;
    if let Some(hint) = selection::hint_text(env) {
        u.text(hint, w / 2.0, y, 16.0 * s, WHITE, Align::Center);
    } else if st.toast_t > 0.0 {
        let a0 = u.p.alpha;
        u.p.alpha = a0 * (st.toast_t * 2.0).clamp(0.0, 1.0);
        u.text(st.toast_text.clone(), w / 2.0, y, 19.0 * s, TOAST, Align::Center);
        u.p.alpha = a0;
    }
}

/// `drawAnnouncement` (ui.js:459-465).
pub fn draw_announcement(u: &mut Ui, st: &mut UiState, s: f32) {
    let Some(b) = st.banners.front().cloned() else {
        return;
    };
    let w = u.w();
    let a0 = u.p.alpha;
    u.p.alpha = a0 * (b.t * 4.0).min((b.dur - b.t) * 2.0).clamp(0.0, 1.0);
    let tw = u.tw(&b.text, 21.0 * s);
    u.big_ribbon(b.color, w / 2.0, 190.0 * s, tw + 190.0 * s, 0.7 * s, &b.text, 21.0 * s);
    u.p.alpha = a0;
}

/// Test-mode notice (ui.js:475-477).
fn draw_test_notice(u: &mut Ui, st: &mut UiState, env: &mut Env, s: f32) {
    let frozen = env.mirror.view.frozen;
    let fog_off = env.mirror.view.fog_off;
    if frozen || (fog_off && st.result.is_none()) {
        let t = format!(
            "Modalità test: {}{}{}  (F4)",
            if frozen { "tempo fermo" } else { "" },
            if frozen && fog_off { " · " } else { "" },
            if fog_off { "nebbia tolta" } else { "" }
        );
        let w = u.w();
        u.text(t, w / 2.0, 138.0 * s, 15.0 * s, GOLD, Align::Center);
    }
}
