//! Selection panel and context actions (spec 06 §7, ui.js:257-457), plus the bridges to the world input (C4's
//! `ControlAction` requests).

use super::kit::{ACCENT, GREEN, INK, IconSpec, RED, Ui, WHITE, faction_color, panel, res_icon, ui_icon, x_icon};
use super::paint::{Align, Tip};
use super::screens::free_building;
use super::{Env, UiState, View};
use crate::control::{ActionId, ClickAction, ControlAction, PlaceMode, context_actions};
use bevy::prelude::*;
use ts_sim::TILE;
use ts_sim::cmd::{Command, EntityRef, QueueWhat};
use ts_sim::defs::{
    BIG_USERS, BuildingKind, CAPTURE_TIME, KingdomColor, Mods, NodeKind, OUTPOST_RADIUS,
    POINT_INCOME, Res, SHEEP_MEAT, Sound, TRAIN_BONUS, UnitKind, buy_price, sell_price,
    train_speed,
};
use ts_sim::slab::{BuildingId, NodeId, UnitId};
use ts_sim::snapshot_types::{BuildingView, UnitView};

// ---------------------------------------------------------------------------------------------- bridges to C4

/// `Ctl.nextIdle()`.
pub fn next_idle(_st: &mut UiState, env: &mut Env) {
    env.ctl.write(ControlAction::NextIdle);
}

/// Minimap right click: normal right-click order at a world point.
pub fn order_at(_st: &mut UiState, env: &mut Env, p: Vec2) {
    env.ctl.write(ControlAction::OrderAt(p));
}

/// `Ctl.recall(units)` (refusals toasted by control).
pub fn recall(_st: &mut UiState, env: &mut Env, ids: &[UnitId]) -> Option<String> {
    env.ctl.write(ControlAction::Recall(Some(ids.to_vec())));
    None
}

/// `Ctl.select(ids)` + `centerOnSelection()`.
pub fn select_and_center(env: &mut Env, ids: &[UnitId]) {
    env.ctl.write(ControlAction::Select(
        ids.iter().map(|u| EntityRef::Unit(*u)).collect(),
    ));
    env.ctl.write(ControlAction::CenterSelection);
}

/// `Ctl.startPlacing(kind)`.
pub fn start_placing(env: &mut Env, kind: BuildingKind, keyboard: bool) {
    env.ctl.write(ControlAction::StartPlacing { kind, keyboard });
}

/// Shortcut 5..7 (V / Tab / Backspace) → `None` (control toasts its own refusals).
pub fn run_shortcut(i: usize, _st: &mut UiState, env: &mut Env) -> Option<String> {
    let a = match i {
        5 => ControlAction::MoveMode,
        6 => ControlAction::SelectArmy,
        7 => ControlAction::Recall(None),
        _ => return None,
    };
    env.ctl.write(a);
    None
}

/// `UI.open` side effect: `G.placing = null; Ctl.mode = null`.
pub fn cancel_modes(env: &mut Env) {
    env.place.cancel();
}

/// V shortcut highlighted (`pickmove` or moving a building).
pub fn move_mode_active(env: &Env) -> bool {
    env.place.action == ClickAction::PickMove || matches!(env.place.mode, PlaceMode::Move(_))
}

/// Hint line (placing / modes).
pub fn hint_text(env: &Env) -> Option<&'static str> {
    env.place.hint()
}

// ---------------------------------------------------------------------------------------------- helpers

fn unit_color(env: &Env, un: &UnitView) -> KingdomColor {
    env.color_of(un.faction)
}

fn hp_text(hp: u16, max: u16) -> String {
    format!("{} / {}", hp, max)
}

/// Kind and bigness of a node id (canonical order: trees, golds small + big, stones small + big).
pub fn node_info(env: &Env, id: NodeId) -> Option<(NodeKind, bool)> {
    let g = env.mirror.world.as_ref()?;
    let mut i = id.0 as usize;
    if i < g.trees.len() {
        return Some((NodeKind::Tree, false));
    }
    i -= g.trees.len();
    for kind in [NodeKind::Gold, NodeKind::Stone] {
        let small = if kind == NodeKind::Gold {
            g.golds.len()
        } else {
            g.stones.len()
        };
        if i < small {
            return Some((kind, false));
        }
        i -= small;
        let bigs = g.bigs.iter().filter(|(k, _)| *k == kind).count();
        if i < bigs {
            return Some((kind, true));
        }
        i -= bigs;
    }
    None
}

// ---------------------------------------------------------------------------------------------- panel

/// `drawSelection(s, right)`.
pub fn draw_selection(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View, s: f32, right: f32) {
    let (ww, wh) = (u.w(), u.h());
    let h = 148.0 * s;
    let w = (660.0 * s).min(right - 40.0 * s);
    let x = (12.0 * s).max(((ww - w) / 2.0).min(right - w - 14.0 * s));
    let y = wh - h - 16.0 * s;
    if w < 260.0 * s {
        return;
    }
    u.p.area(x, y, w, h);
    u.panel(&panel::PAPER, x, y, w, h, 0.6 * s);
    let units: Vec<UnitView> = env
        .sel
        .units
        .iter()
        .filter_map(|id| env.mirror.view.units.get(id))
        .cloned()
        .collect();
    let actions = context_actions(&env.mirror, &env.sel, &env.place);
    if units.len() > 1 {
        draw_group(u, env, &units, x, y, w - actions.len() as f32 * 50.0 * s, s);
    } else if units.len() == 1 {
        draw_unit(u, env, v, &units[0], x, y, w, s);
    } else if let Some(b) = env
        .sel
        .building
        .and_then(|b| env.mirror.view.buildings.get(&b))
        .cloned()
    {
        draw_building(u, env, v, &b, x, y, w, s);
    } else if let Some(o) = env.sel.other {
        match o {
            EntityRef::Point(p) => draw_point(u, env, v, p.0 as usize, x, y, w, s),
            EntityRef::Node(n) => draw_node(u, env, Some(n), x, y, s),
            EntityRef::Sheep(_) => draw_node(u, env, None, x, y, s),
            _ => draw_summary(u, env, v, x, y, s),
        }
    } else {
        draw_summary(u, env, v, x, y, s);
    }
    draw_actions(u, st, env, v, &actions, x, y, w, s);
}

fn draw_summary(u: &mut Ui, env: &Env, v: &View, x: f32, y: f32, s: f32) {
    let eco = env.mirror.view.economy.as_ref();
    let mine_pts = env
        .mirror
        .view
        .points
        .iter()
        .filter(|p| p.owner.is_some() && p.owner == v.you)
        .count();
    let me = v
        .you
        .and_then(|y| env.mirror.view.factions.iter().find(|f| f.idx == y));
    u.plain("Nessuna selezione", x + 20.0 * s, y + 30.0 * s, 19.0 * s, INK, Align::Left);
    u.plain(
        "Clic o trascina per selezionare · clic destro per dare ordini · F1 per i comandi",
        x + 20.0 * s,
        y + 62.0 * s,
        13.5 * s,
        INK,
        Align::Left,
    );
    u.plain(
        format!(
            "Pedoni {}   Truppe {}   Punti conquistati {}/{}   Nemici abbattuti {}   Edifici distrutti {}",
            eco.map_or(0, |e| e.pawn_n),
            eco.map_or(0, |e| e.troop_n),
            mine_pts,
            env.mirror.view.points.len(),
            me.map_or(0, |f| f.kills),
            me.map_or(0, |f| f.razed),
        ),
        x + 20.0 * s,
        y + 92.0 * s,
        14.0 * s,
        ACCENT,
        Align::Left,
    );
    let names: Vec<&str> = env
        .mirror
        .view
        .factions
        .iter()
        .filter(|f| f.alive)
        .map(|f| f.color.name_it())
        .collect();
    u.plain(
        format!("Regni rimasti: {}", names.join(", ")),
        x + 20.0 * s,
        y + 118.0 * s,
        13.5 * s,
        INK,
        Align::Left,
    );
}

/// `drawUnitInfo`.
#[allow(clippy::too_many_arguments)]
fn draw_unit(u: &mut Ui, env: &Env, v: &View, un: &UnitView, x: f32, y: f32, w: f32, s: f32) {
    let c = unit_color(env, un);
    let mine = Some(un.faction) == v.you;
    let d = un.kind.def();
    u.panel(&panel::BANNER_SLOTS, x + 14.0 * s, y + 16.0 * s, 96.0 * s, 96.0 * s, 0.7 * s);
    u.portrait(c, un.kind, x + 20.0 * s, y + 20.0 * s, 84.0 * s);
    let name = if mine {
        d.label.to_string()
    } else {
        format!("{} · {}", d.label, c.kingdom_name())
    };
    u.small_ribbon(c, 1, x + 240.0 * s, y + 30.0 * s, 240.0 * s, 0.85 * s, &name, 15.0 * s);
    let bw = (260.0 * s).min(w - 150.0 * s);
    u.big_bar(x + 128.0 * s, y + 52.0 * s, bw, un.hp as f32 / un.max_hp.max(1) as f32, 0.6 * s);
    u.text(hp_text(un.hp, un.max_hp), x + 128.0 * s + bw / 2.0, y + 68.0 * s, 13.0 * s, WHITE, Align::Center);
    // stats with the owner's modifiers (enemy techs are not replicated: base values for them)
    let mods = if mine {
        env.mirror
            .view
            .economy
            .as_ref()
            .map(|e| e.mods)
            .unwrap_or_default()
    } else {
        Mods::default()
    };
    let mut stats: Vec<(String, String)> = Vec::new();
    if d.dmg > 0.0 {
        stats.push((ui_icon(5), (mods.unit_dmg(un.kind)).round().to_string()));
        stats.push((ui_icon(6), mods.unit_armor(un.kind).round().to_string()));
    }
    if d.dmg > 0.0 && d.range > 100.0 {
        stats.push((x_icon("arrows"), (mods.unit_range(un.kind) / TILE).round().to_string()));
    }
    stats.push((x_icon("boot"), (mods.unit_speed(un.kind) / 10.0).round().to_string()));
    if d.heal > 0.0 {
        let mut t = (d.heal * mods.heal).round().to_string();
        if mine && d.faith > 0.0 {
            t.push_str(&format!("  ·  riserva {}/{}", un.faith, d.faith.round()));
        }
        stats.push((x_icon("herbs"), t));
    }
    let step = (72.0 * s).min((w - 150.0 * s) / stats.len() as f32);
    for (i, (icon, val)) in stats.into_iter().enumerate() {
        u.icon(&icon, x + 140.0 * s + i as f32 * step, y + 96.0 * s, 28.0 * s);
        u.plain(val, x + 156.0 * s + i as f32 * step, y + 97.0 * s, 15.0 * s, INK, Align::Left);
    }
    if mine {
        u.plain(un.act.label(), x + 128.0 * s, y + 124.0 * s, 13.5 * s, ACCENT, Align::Left);
    }
}

/// `drawGroupInfo`.
fn draw_group(u: &mut Ui, env: &mut Env, units: &[UnitView], x: f32, y: f32, w: f32, s: f32) {
    let size = 46.0 * s;
    let cols = (((w - 24.0 * s) / size).floor() as usize).max(1);
    let shown = units.len().min(cols * 3 - 1);
    let shift = u.p.io.shift;
    for (i, un) in units.iter().take(shown).enumerate() {
        let cx = x + 14.0 * s + ((i % cols) as f32 + 0.5) * size;
        let cy = y + 14.0 * s + ((i / cols) as f32 + 0.5) * size - 3.0 * s;
        let ratio = un.hp as f32 / un.max_hp.max(1) as f32;
        let r = u.tiny_button(cx, cy, size * 0.96, true, ratio < 0.35, None);
        let c = unit_color(env, un);
        u.portrait(c, un.kind, cx - size * 0.34, cy - size * 0.36, size * 0.68);
        u.hp_strip(ratio, cx - size * 0.32, cy + size * 0.3, size * 0.64, 4.0 * s);
        if r.clicked {
            if shift {
                env.ctl.write(ControlAction::Deselect(un.id));
            } else {
                env.ctl.write(ControlAction::Select(vec![EntityRef::Unit(un.id)]));
            }
        }
    }
    if units.len() > shown {
        let i = shown;
        u.plain(
            format!("+{}", units.len() - shown),
            x + 14.0 * s + ((i % cols) as f32 + 0.5) * size,
            y + 14.0 * s + ((i / cols) as f32 + 0.5) * size,
            17.0 * s,
            INK,
            Align::Center,
        );
    }
}

/// `drawBuildingInfo`.
#[allow(clippy::too_many_arguments)]
fn draw_building(u: &mut Ui, env: &mut Env, v: &View, b: &BuildingView, x: f32, y: f32, w: f32, s: f32) {
    let c = env.color_of(b.faction);
    let mine = Some(b.faction) == v.you && !env.mirror.view.ghosts.contains(&b.id);
    let d = b.kind.def();
    u.panel(&panel::BANNER_SLOTS, x + 14.0 * s, y + 16.0 * s, 96.0 * s, 96.0 * s, 0.7 * s);
    u.building_thumb(c, b.kind, x + 62.0 * s, y + 64.0 * s, 82.0 * s);
    let name = if mine {
        d.label.to_string()
    } else {
        format!("{} · {}", d.label, c.kingdom_name())
    };
    u.small_ribbon(c, 1, x + 240.0 * s, y + 30.0 * s, 240.0 * s, 0.85 * s, &name, 15.0 * s);
    let bw = (260.0 * s).min(w - 150.0 * s);
    u.big_bar(x + 128.0 * s, y + 52.0 * s, bw, b.hp as f32 / b.max_hp.max(1) as f32, 0.6 * s);
    u.text(hp_text(b.hp, b.max_hp), x + 128.0 * s + bw / 2.0, y + 68.0 * s, 13.0 * s, WHITE, Align::Center);
    if !b.done {
        u.plain(
            format!("In costruzione: {}%", (100.0 * b.progress / d.work).floor().clamp(0.0, 100.0)),
            x + 128.0 * s,
            y + 100.0 * s,
            14.5 * s,
            ACCENT,
            Align::Left,
        );
        return;
    }
    if !mine {
        return;
    }
    if b.queue.is_empty() {
        if d.trade {
            let step = (96.0 * s).min((w - 190.0 * s) / 3.0);
            let size = (13.5 * s).min(step * 0.15);
            u.icon(&x_icon("scales"), x + 142.0 * s, y + 104.0 * s, 34.0 * s);
            let trades = env
                .mirror
                .view
                .economy
                .as_ref()
                .map_or([0.0; 4], |e| e.trades);
            for (i, k) in [Res::Wood, Res::Meat, Res::Stone].iter().enumerate() {
                let tr = trades[*k as usize] as f64;
                u.icon(&res_icon(*k), x + 180.0 * s + i as f32 * step, y + 104.0 * s, 28.0 * s);
                u.plain(
                    format!("{}/{}", sell_price(tr).round(), buy_price(tr).round()),
                    x + 194.0 * s + i as f32 * step,
                    y + 105.0 * s,
                    size,
                    INK,
                    Align::Left,
                );
            }
            if w > 400.0 * s {
                u.plain(
                    "oro per vendere / comprare 100",
                    x + 128.0 * s,
                    y + 128.0 * s,
                    12.0 * s,
                    ACCENT,
                    Align::Left,
                );
            }
            return;
        }
        let trains = !d.trains.is_empty();
        let hint = if trains {
            "Clic destro sulla mappa: punto di raccolta"
        } else {
            d.desc
        };
        let mut lines: Vec<String> = u.wrap(hint, w - 150.0 * s, 13.5 * s).into_iter().take(2).collect();
        if trains && b.kind != BuildingKind::Castle {
            let same = env
                .mirror
                .view
                .buildings
                .values()
                .filter(|o| {
                    o.faction == b.faction
                        && o.kind == b.kind
                        && o.done
                        && !env.mirror.view.ghosts.contains(&o.id)
                })
                .count() as u32;
            let l = if same > 1 {
                format!(
                    "{} ×{}: addestramento +{}%",
                    d.label,
                    same,
                    ((train_speed(same) - 1.0) * 100.0).round()
                )
            } else {
                format!("Un'altra uguale: +{}% di velocità", (TRAIN_BONUS * 100.0).round())
            };
            if lines.len() < 2 {
                lines.push(l);
            } else {
                lines[1] = l;
            }
        }
        for (i, l) in lines.into_iter().enumerate() {
            u.plain(
                l,
                x + 128.0 * s,
                y + 98.0 * s + i as f32 * 18.0 * s,
                13.5 * s,
                if i > 0 && trains { ACCENT } else { INK },
                Align::Left,
            );
        }
        return;
    }
    let mut cancel: Option<usize> = None;
    for (i, item) in b.queue.iter().enumerate() {
        let cx = x + 152.0 * s + i as f32 * 54.0 * s;
        let cy = y + 116.0 * s;
        let r = u.small_button(cx, cy, 74.0 * s, false, false);
        let (spec, name) = match item.what {
            QueueWhat::Unit(k) => (IconSpec::Unit(k), k.def().label),
            QueueWhat::Tech(t) => (IconSpec::from(t.def().icon), t.def().label),
        };
        u.spec(spec, c, cx, cy - 2.0 * s + r.dy, 28.0 * s);
        if r.hover {
            u.p.tip = Some(Tip {
                x: cx,
                y: y + 30.0 * s,
                title: name.to_string(),
                desc: "Clic per annullare: le risorse tornano indietro.".to_string(),
                ..Default::default()
            });
        }
        if r.clicked {
            cancel = Some(i);
        }
    }
    let q = b.queue[0];
    u.small_bar(
        x + 128.0 * s + bw + 20.0 * s,
        y + 104.0 * s,
        (40.0 * s).max(w - bw - 170.0 * s),
        q.t / q.total.max(0.001),
        0.8 * s,
        faction_color(c),
    );
    if let QueueWhat::Unit(k) = q.what
        && q.t >= q.total
    {
        u.plain(
            if k == UnitKind::Pawn {
                "In attesa di case"
            } else {
                "In attesa di accampamenti"
            },
            x + 128.0 * s + bw + 20.0 * s,
            y + 132.0 * s,
            13.0 * s,
            RED,
            Align::Left,
        );
    }
    if let Some(i) = cancel {
        let item = b.queue[i];
        env.command(Command::Cancel {
            building: b.id,
            index: i as u8,
            expected: item.what,
        });
        u.play(Sound::Click);
    }
}

/// `drawNodeInfo` (`id = None` → a sheep).
fn draw_node(u: &mut Ui, env: &Env, id: Option<NodeId>, x: f32, y: f32, s: f32) {
    let info = id.and_then(|n| node_info(env, n));
    let (res, name, left, big) = match (id, info) {
        (None, _) => (Res::Meat, "Pecora", SHEEP_MEAT, false),
        (Some(n), Some((kind, big))) => {
            let amount = env
                .mirror
                .view
                .node_amounts
                .get(n.0 as usize)
                .copied()
                .unwrap_or(0.0);
            let (r, nm) = match (kind, big) {
                (NodeKind::Tree, _) => (Res::Wood, "Albero"),
                (NodeKind::Gold, true) => (Res::Gold, "Grande vena d'oro"),
                (NodeKind::Gold, false) => (Res::Gold, "Vena d'oro"),
                (NodeKind::Stone, true) => (Res::Stone, "Grande cava di pietra"),
                (NodeKind::Stone, false) => (Res::Stone, "Cava di pietra"),
            };
            (r, nm, amount, big)
        }
        _ => return,
    };
    u.icon(&res_icon(res), x + 62.0 * s, y + 64.0 * s, 84.0 * s);
    u.plain(name, x + 128.0 * s, y + 48.0 * s, 20.0 * s, INK, Align::Left);
    u.plain(
        format!("{}: {}", res.label(), fmt_num(left)),
        x + 128.0 * s,
        y + 78.0 * s,
        16.0 * s,
        ACCENT,
        Align::Left,
    );
    let t = if big {
        format!("Ci lavorano fino a {BIG_USERS} Pedoni insieme (clic destro per mandarli).")
    } else {
        "Clic destro con dei Pedoni selezionati per raccogliere.".to_string()
    };
    u.plain(t, x + 128.0 * s, y + 106.0 * s, 13.5 * s, INK, Align::Left);
}

/// JS number formatting of an amount (integers without decimals).
fn fmt_num(v: f32) -> String {
    if (v - v.round()).abs() < 1e-4 {
        format!("{}", v.round() as i64)
    } else {
        format!("{}", (v * 100.0).round() / 100.0)
    }
}

/// `drawPointInfo`.
#[allow(clippy::too_many_arguments)]
fn draw_point(u: &mut Ui, env: &Env, v: &View, idx: usize, x: f32, y: f32, w: f32, s: f32) {
    let Some(p) = env.mirror.view.points.iter().find(|p| p.id.0 as usize == idx).copied() else {
        return;
    };
    let d = p.kind.def();
    u.panel(&panel::BANNER_SLOTS, x + 14.0 * s, y + 16.0 * s, 96.0 * s, 96.0 * s, 0.7 * s);
    u.point_thumb(p.kind, x + 62.0 * s, y + 64.0 * s, 84.0 * s);
    let rc = p.owner.map_or(v.color, |o| env.color_of(o));
    u.small_ribbon(rc, if p.owner.is_some() { 1 } else { 0 }, x + 240.0 * s, y + 30.0 * s, 240.0 * s, 0.85 * s, d.label, 15.0 * s);
    u.icon(&x_icon("banner"), x + 140.0 * s, y + 66.0 * s, 30.0 * s);
    let (t, col) = match p.owner {
        Some(o) if Some(o) == v.you => ("È tuo".to_string(), GREEN),
        Some(o) => (format!("Lo tiene il {}", env.color_of(o).kingdom_name()), ACCENT),
        None => ("Libero".to_string(), ACCENT),
    };
    u.plain(t, x + 160.0 * s, y + 66.0 * s, 15.0 * s, col, Align::Left);
    if let Some(by) = p.by
        && p.t > 0.0
    {
        let who = if Some(by) == v.you {
            "Lo stai conquistando".to_string()
        } else {
            format!("Lo sta conquistando il {}", env.color_of(by).kingdom_name())
        };
        u.plain(
            format!("{}: {}%", who, (100.0 * p.t / CAPTURE_TIME).round()),
            x + 290.0 * s,
            y + 66.0 * s,
            13.5 * s,
            RED,
            Align::Left,
        );
    }
    let text = format!(
        "{}{} Si conquista tenendoci accanto delle truppe per {} secondi, senza nemici attorno.",
        d.desc,
        if d.res.is_some() {
            format!(" +{} al minuto.", (POINT_INCOME * 60.0).round())
        } else {
            format!(" Entro {} caselle.", OUTPOST_RADIUS.round())
        },
        CAPTURE_TIME.round()
    );
    let lines = u.wrap(&text, w - 150.0 * s, 13.0 * s);
    for (i, l) in lines.into_iter().take(3).enumerate() {
        u.plain(l, x + 128.0 * s, y + 92.0 * s + i as f32 * 17.0 * s, 13.0 * s, INK, Align::Left);
    }
}

/// `drawActions`.
#[allow(clippy::too_many_arguments)]
fn draw_actions(
    u: &mut Ui,
    _st: &mut UiState,
    env: &mut Env,
    v: &View,
    actions: &[crate::control::ContextAction],
    x: f32,
    y: f32,
    w: f32,
    s: f32,
) {
    let n = actions.len();
    for (i, a) in actions.iter().enumerate() {
        let cx = x + w - 38.0 * s - (n - 1 - i) as f32 * 50.0 * s;
        let cy = y + 40.0 * s;
        let r = u.tiny_button(cx, cy, 44.0 * s, true, false, None);
        let icon = match a.id {
            ActionId::AttackMove => IconSpec::Ui(5),
            ActionId::Explore => IconSpec::X("eye"),
            ActionId::Stop => IconSpec::X("stop"),
            ActionId::Rally => IconSpec::X("flag"),
            ActionId::Move => IconSpec::X("barrow"),
            ActionId::Demolish => IconSpec::Ui(9),
        };
        u.spec(icon, v.color, cx, cy - s, 24.0 * s);
        u.text(a.key, cx - 19.0 * s, cy - 18.0 * s, 10.0 * s, WHITE, Align::Left);
        if a.on {
            u.brackets(cx - 24.0 * s, cy - 24.0 * s, 48.0 * s, 48.0 * s, 0.5 * s);
        }
        if r.hover {
            u.p.tip = Some(Tip {
                x: cx,
                y: y + 8.0 * s,
                title: a.title.to_string(),
                key: Some(a.key.to_string()),
                desc: a.desc.to_string(),
                ..Default::default()
            });
        }
        if r.clicked {
            env.ctl.write(ControlAction::Act(a.id));
        }
    }
    let _ = free_building;
    let _: Option<BuildingId> = None;
}
