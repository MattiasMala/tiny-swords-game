//! Management screens (spec 06 §10, panels.js): Unità, Invia, Lavoratori, Ricerche, Costruisci, Mercato,
//! Modalità test.

use super::hud::hud_scale;
use super::kit::{
    ACCENT, CREAM, DIMBLUE, ERA_ON, GOLD, INK, IconSpec, LIGHT_RED, MUTED, PARCH, RED, SALMON, TOAST, Ui,
    WHITE, faction_color, panel, res_icon, ui_icon, x_icon,
};
use super::paint::{Align, CursorKind, Tip};
use super::{Env, Screen, SendState, UiState, View, minimap, selection};
use bevy::prelude::*;
use std::collections::BTreeMap;
use ts_sim::cmd::{
    CODA_PIENA, Command, NESSUN_PEDONE_LIBERO, NON_DISPONIBILE, NON_HAI_PEDONI, OrderKind, QueueWhat,
    RISORSE_INSUFFICIENTI, TradeOp, richiede,
};
use ts_sim::defs::{
    AGE_NAME, Act, BUILD_MENU, BuildingKind, Cost, QUEUE_MAX, Res, Sound, TREE_ROWS, TechId,
    TechSet, UnitKind, buy_price, sell_price,
};
use ts_sim::slab::UnitId;
use ts_sim::snapshot_types::{BuildingView, EconomyView, UnitView};

/// Frame returned by `manageFrame`.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub s: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Paper,
    Dark,
    Wood,
}

/// `drawManage` (panels.js:400-411).
pub fn draw_manage(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View) {
    u.p.tip = None;
    let Some(scr) = st.screen else {
        return;
    };
    match scr {
        Screen::Unita => draw_units(u, st, env, v),
        Screen::Invia => draw_send(u, st, env, v),
        Screen::Lavoratori => draw_workers(u, st, env, v),
        Screen::Ricerche => draw_tree(u, st, env, v),
        Screen::Costruisci => draw_build(u, st, env, v),
        Screen::Mercato => draw_market(u, st, env, v),
        Screen::Test => draw_test(u, st, env, v),
    }
    if let Some(tip) = u.p.tip.take() {
        let s = hud_scale(u.w(), u.h());
        u.tooltip(&tip, s, v.color, &v.res);
    }
}

/// `manageFrame(title, w0, h0, style)`.
fn manage_frame(u: &mut Ui, st: &mut UiState, v: &View, title: &str, w0: f32, h0: f32, style: Style) -> Frame {
    let (ww, wh) = (u.w(), u.h());
    let s = (ww / (w0 + 40.0)).min(wh / (h0 + 70.0)).min(1.3);
    let w = w0 * s;
    let h = h0 * s;
    let x = (ww - w) / 2.0;
    let y = (wh - h) / 2.0 - 6.0 * s;
    u.p.dim(0.35);
    match style {
        Style::Paper => {
            u.panel(&panel::BANNER, x, y, w, h, 0.8 * s);
            u.plain(title, ww / 2.0, y + 32.0 * s, 27.0 * s, INK, Align::Center);
        }
        Style::Dark | Style::Wood => {
            let d = if style == Style::Wood {
                panel::WOOD
            } else {
                panel::SPECIAL
            };
            u.panel(&d, x, y, w, h, 0.9 * s);
            u.big_ribbon(v.color, ww / 2.0, y + 14.0 * s, 330.0 * s, 0.75 * s, title, 25.0 * s);
        }
    }
    let close = u
        .tiny_button(x + w - 36.0 * s, y + 34.0 * s, 40.0 * s, false, true, Some(9))
        .clicked;
    if close || (!st.fresh && u.p.io.pressed && !u.p.over(x, y, w, h)) {
        st.screen = None;
    }
    st.fresh = false;
    if st.toast_t > 0.0 {
        u.text(st.toast_text.clone(), ww / 2.0, y + h + 20.0 * s, 18.0 * s, TOAST, Align::Center);
    }
    Frame { x, y, w, h, s }
}

// ---------------------------------------------------------------------------------------------- shared rules

/// Own units (live list, creation order).
fn mine<'a>(env: &'a Env, v: &View) -> Vec<&'a UnitView> {
    env.mirror
        .view
        .units
        .values()
        .filter(|un| Some(un.faction) == v.you)
        .collect()
}

/// Own buildings (not ghosts).
fn own_buildings<'a>(env: &'a Env, v: &View) -> impl Iterator<Item = &'a BuildingView> {
    let ghosts = &env.mirror.view.ghosts;
    let you = v.you;
    env.mirror
        .view
        .buildings
        .values()
        .filter(move |b| Some(b.faction) == you && !ghosts.contains(&b.id))
}

/// `freeBuilding(f, kind)`: finished own building of that kind with the shortest queue (first on ties).
pub fn free_building<'a>(env: &'a Env, v: &View, kind: BuildingKind) -> Option<&'a BuildingView> {
    let mut best: Option<&BuildingView> = None;
    for b in own_buildings(env, v) {
        if b.kind == kind && b.done && best.is_none_or(|o| b.queue.len() < o.queue.len()) {
            best = Some(b);
        }
    }
    best
}

pub fn can_afford(res: &[u32; 4], c: &Cost) -> bool {
    Res::ALL
        .iter()
        .all(|r| res[*r as usize] as f64 >= c.get(*r) as f64)
}

/// `ageLock(f, def)`.
pub fn age_lock(age: u8, need: u8) -> Option<String> {
    (age < need).then(|| richiede(AGE_NAME.get(need as usize).copied().unwrap_or("")))
}

/// `techLock(f, def)`.
fn tech_lock(eco: &EconomyView, t: TechId) -> Option<String> {
    let d = t.def();
    age_lock(eco.age, d.age).or_else(|| {
        d.req
            .filter(|r| !TechSet(eco.techs).has(*r))
            .map(|r| richiede(r.def().label))
    })
}

/// `trainBlock(f, b, type)`.
pub fn train_block(eco: &EconomyView, res: &[u32; 4], b: &BuildingView, k: UnitKind) -> Option<String> {
    let d = k.def();
    if d.from != b.kind || !b.done {
        return Some(NON_DISPONIBILE.to_string());
    }
    age_lock(eco.age, d.age).or_else(|| {
        if b.queue.len() >= QUEUE_MAX {
            Some(CODA_PIENA.to_string())
        } else if can_afford(res, &d.cost) {
            None
        } else {
            Some(RISORSE_INSUFFICIENTI.to_string())
        }
    })
}

fn act_icon(a: Act) -> String {
    match a {
        Act::Idle => x_icon("hourglass"),
        Act::Marching | Act::Fleeing => x_icon("boot"),
        Act::Fighting => ui_icon(5),
        Act::ChopWood => res_icon(Res::Wood),
        Act::MineGold => res_icon(Res::Gold),
        Act::Butcher => res_icon(Res::Meat),
        Act::Build | Act::Repair => ui_icon(1),
        Act::Carry => x_icon("barrow"),
        Act::Heal => x_icon("herbs"),
        Act::Guard | Act::Sheltered => x_icon("shield"),
        Act::MineStone => res_icon(Res::Stone),
        Act::Exploring => x_icon("spyglass"),
    }
}

fn hp_ratio(un: &UnitView) -> f32 {
    un.hp as f32 / (un.max_hp.max(1)) as f32
}

// ---------------------------------------------------------------------------------------------- Unità

fn draw_units(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View) {
    let Frame { x, y, w, h, s } = manage_frame(u, st, v, "Unità", 1000.0, 630.0, Style::Paper);
    let Some(eco) = env.mirror.view.economy.clone() else {
        return;
    };
    let units = &env.mirror.view.units;
    st.marks.retain(|id| units.contains_key(id));
    let all: Vec<UnitView> = mine(env, v).into_iter().cloned().collect();
    let pawns = all.iter().filter(|un| un.kind == UnitKind::Pawn).count();
    let ww = u.w();
    u.plain(
        format!(
            "Pedoni {}/{}  ·  Truppe {}/{}   —   + addestra  ·  − / + spunta una unità di quel tipo  ·  clic su un'unità o sul nome per spuntare",
            pawns,
            eco.pawn_cap,
            all.len() - pawns,
            eco.troop_cap
        ),
        ww / 2.0,
        y + 62.0 * s,
        13.0 * s,
        ACCENT,
        Align::Center,
    );
    let by_type: Vec<(UnitKind, Vec<&UnitView>)> = UnitKind::ALL
        .iter()
        .map(|&k| (k, all.iter().filter(|un| un.kind == k).collect()))
        .collect();
    let left = x + 44.0 * s;
    let tiles_x = left + 306.0 * s;
    let tiles_w = x + w - 52.0 * s - tiles_x;
    let top = y + 84.0 * s;
    let body_h = h - 84.0 * s - 158.0 * s;
    let mut tile = 44.0 * s;
    let mut per;
    loop {
        per = ((tiles_w / tile).floor() as usize).max(1);
        let rows: usize = by_type
            .iter()
            .map(|(_, l)| l.len().div_ceil(per).max(1))
            .sum();
        if rows as f32 * (tile + 4.0 * s) <= body_h || tile <= 24.0 * s {
            break;
        }
        tile -= 2.0 * s;
    }
    let row_h = tile + 4.0 * s;
    let mut ry = top;
    // queued per unit kind across own buildings
    let mut queued: BTreeMap<UnitKind, usize> = BTreeMap::new();
    for b in own_buildings(env, v) {
        for q in &b.queue {
            if let QueueWhat::Unit(k) = q.what {
                *queued.entry(k).or_default() += 1;
            }
        }
    }
    let mut cmd: Option<Command> = None;
    let mut why: Option<String> = None;
    for (kind, list) in &by_type {
        let d = kind.def();
        let picked: Vec<UnitId> = list
            .iter()
            .filter(|un| st.marks.contains(&un.id))
            .map(|un| un.id)
            .collect();
        let all_marked = !list.is_empty() && picked.len() == list.len();
        let head = u.p.hit(left, ry, 168.0 * s, row_h);
        let a0 = u.p.alpha;
        if list.is_empty() {
            u.p.alpha = a0 * 0.55;
        }
        u.portrait(v.color, *kind, left, ry + 2.0 * s, tile * 0.9);
        u.plain(
            format!("{}  {}", d.label, list.len()),
            left + tile + 4.0 * s,
            ry + row_h / 2.0,
            14.5 * s,
            if head.hover && !list.is_empty() { ACCENT } else { INK },
            Align::Left,
        );
        u.p.alpha = a0;
        if head.clicked && !list.is_empty() {
            for un in list {
                if all_marked {
                    st.marks.remove(&un.id);
                } else {
                    st.marks.insert(un.id);
                }
            }
            u.play(Sound::Click);
        }
        // training
        let b = free_building(env, v, d.from);
        let lock = match b {
            None => Some(richiede(d.from.def().label)),
            Some(b) => train_block(&eco, &v.res, b, *kind),
        };
        let nq = queued.get(kind).copied().unwrap_or(0);
        let tx = left + 186.0 * s;
        let tcy = ry + row_h / 2.0;
        let tr = u.tiny_button(tx, tcy, (34.0 * s).min(tile * 0.8), false, lock.is_some(), None);
        u.text("+", tx, tcy - 2.0 * s, 19.0 * s, WHITE, Align::Center);
        if nq > 0 {
            u.text(nq.to_string(), tx + 12.0 * s, tcy + 11.0 * s, 12.5 * s, GOLD, Align::Center);
        }
        if tr.hover {
            u.p.tip = Some(Tip {
                x: tx + 130.0 * s,
                y: ry,
                title: format!("Addestra {}", d.label),
                cost: Some(d.cost),
                desc: if nq > 0 {
                    format!("{} In coda: {}.", d.desc, nq)
                } else {
                    d.desc.to_string()
                },
                lock: lock.clone(),
                ..Default::default()
            });
        }
        if tr.clicked {
            match (&lock, b) {
                (Some(l), _) => why = Some(l.clone()),
                (None, Some(b)) => {
                    cmd = Some(Command::Train {
                        building: b.id,
                        unit: *kind,
                    })
                }
                _ => {}
            }
        }
        // − / + marks
        if !list.is_empty() {
            let bs = (26.0 * s).min(tile * 0.62);
            let minus = u.tiny_button(left + 226.0 * s, tcy, bs, true, true, None);
            let plus = u.tiny_button(left + 286.0 * s, tcy, bs, true, false, None);
            u.text("−", left + 226.0 * s, tcy - 2.0 * s, 16.0 * s, WHITE, Align::Center);
            u.text("+", left + 286.0 * s, tcy - 2.0 * s, 16.0 * s, WHITE, Align::Center);
            u.plain(
                picked.len().to_string(),
                left + 256.0 * s,
                tcy,
                15.0 * s,
                if picked.is_empty() { INK } else { ACCENT },
                Align::Center,
            );
            if minus.hover || plus.hover {
                u.p.tip = Some(Tip {
                    x: left + 256.0 * s,
                    y: ry,
                    title: d.label.to_string(),
                    desc: format!(
                        "Spuntate {} su {}: − ne toglie una, + ne aggiunge una. Poi, in basso, cosa farne.",
                        picked.len(),
                        list.len()
                    ),
                    ..Default::default()
                });
            }
            if minus.clicked
                && let Some(last) = picked.last()
            {
                st.marks.remove(last);
            }
            if plus.clicked {
                let mut best: Option<&UnitView> = None;
                for un in list.iter().filter(|un| !st.marks.contains(&un.id)) {
                    if best.is_none_or(|b| hp_ratio(un) > hp_ratio(b)) {
                        best = Some(un);
                    }
                }
                if let Some(b) = best {
                    st.marks.insert(b.id);
                }
            }
        }
        // unit tiles
        for (i, un) in list.iter().enumerate() {
            let cx = tiles_x + ((i % per) as f32 + 0.5) * tile;
            let cy = ry + ((i / per) as f32 + 0.5) * row_h;
            let r = u.tiny_button(cx, cy, tile * 0.94, true, hp_ratio(un) < 0.35, None);
            u.portrait(v.color, *kind, cx - tile * 0.34, cy - tile * 0.38, tile * 0.68);
            u.hp_strip(hp_ratio(un), cx - tile * 0.32, cy + tile * 0.3, tile * 0.64, (4.0 * s).max(2.0));
            u.icon(&act_icon(un.act), cx + tile * 0.27, cy - tile * 0.25, tile * 0.44);
            if st.marks.contains(&un.id) {
                u.brackets(cx - tile * 0.5, cy - tile * 0.5, tile, tile, tile / 80.0);
            }
            if r.hover {
                u.p.tip = Some(Tip {
                    x: cx,
                    y: cy - tile * 0.2,
                    title: d.label.to_string(),
                    desc: format!("{}  ·  vita {}/{}", un.act.label(), un.hp, un.max_hp),
                    ..Default::default()
                });
            }
            if r.clicked && !st.marks.remove(&un.id) {
                st.marks.insert(un.id);
            }
        }
        ry += list.len().div_ceil(per).max(1) as f32 * row_h;
    }
    // bottom bar
    let marked: Vec<&UnitView> = all.iter().filter(|un| st.marks.contains(&un.id)).collect();
    let mp: Vec<UnitId> = marked
        .iter()
        .filter(|un| un.kind == UnitKind::Pawn)
        .map(|un| un.id)
        .collect();
    let ms: Vec<UnitId> = marked
        .iter()
        .filter(|un| un.kind != UnitKind::Pawn)
        .map(|un| un.id)
        .collect();
    let all_ids: Vec<UnitId> = marked.iter().map(|un| un.id).collect();
    let by = y + h - 104.0 * s;
    if u.big_button(left, by - 34.0 * s, 118.0 * s, 36.0 * s, 0.5 * s, "Tutte", false, None) {
        for un in &all {
            st.marks.insert(un.id);
        }
    }
    if u.big_button(left, by + 8.0 * s, 118.0 * s, 36.0 * s, 0.5 * s, "Nessuna", true, None) {
        st.marks.clear();
    }
    u.plain(
        format!("Pedoni spuntati: {}", mp.len()),
        left + 290.0 * s,
        by - 46.0 * s,
        13.0 * s,
        ACCENT,
        Align::Center,
    );
    for (i, &k) in Res::ALL.iter().enumerate() {
        if u.action_button(
            left + 185.0 * s + i as f32 * 70.0 * s,
            by,
            s,
            IconSpec::Res(k),
            v.color,
            k.label(),
            !mp.is_empty(),
        ) {
            cmd = Some(Command::Job {
                pawns: mp.clone(),
                res: k,
            });
        }
    }
    u.plain(
        format!("Truppe spuntate: {}", ms.len()),
        left + 580.0 * s,
        by - 46.0 * s,
        13.0 * s,
        ACCENT,
        Align::Center,
    );
    if u.action_button(left + 480.0 * s, by, s, IconSpec::X("stop"), v.color, "Ferma", !all_ids.is_empty()) {
        cmd = Some(Command::Order {
            units: all_ids.clone(),
            kind: OrderKind::Stop,
            x: 0.0,
            y: 0.0,
            target: None,
        });
    }
    if u.action_button(
        left + 550.0 * s,
        by,
        s,
        IconSpec::Build(BuildingKind::Castle),
        v.color,
        "Richiama",
        !ms.is_empty(),
    ) {
        why = selection::recall(st, env, &ms);
    }
    if u.action_button(left + 620.0 * s, by, s, IconSpec::Ui(5), v.color, "Invia", !ms.is_empty()) {
        let castle = own_buildings(env, v).find(|b| b.kind == BuildingKind::Castle);
        let (cx, cy) = castle.map_or((0.0, 0.0), |b| {
            let d = b.kind.def();
            (
                (b.cx as f32 + d.w as f32 * 0.5) * ts_sim::TILE,
                (b.cy + d.h) as f32 * ts_sim::TILE,
            )
        });
        st.send = Some(SendState {
            ids: ms.clone(),
            x: cx,
            y: cy,
            kbd: u.p.kbd,
            go: false,
        });
        st.screen = Some(Screen::Invia);
        st.fresh = true;
        u.p.focus = None;
    }
    let a0 = u.p.alpha;
    if marked.is_empty() {
        u.p.alpha = a0 * 0.4;
    }
    if u.big_button(
        x + w - 286.0 * s,
        by - 28.0 * s,
        236.0 * s,
        56.0 * s,
        0.6 * s,
        "Seleziona sulla mappa",
        false,
        None,
    ) && !marked.is_empty()
    {
        selection::select_and_center(env, &all_ids);
        st.screen = None;
    }
    u.p.alpha = a0;
    if let Some(c) = cmd {
        env.command(c);
    }
    st.say(why);
}

// ---------------------------------------------------------------------------------------------- Invia

fn draw_send(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View) {
    let Frame { x, y, w, s, .. } = manage_frame(u, st, v, "Dove le mando?", 1000.0, 720.0, Style::Paper);
    let Some(mut t) = st.send.clone() else {
        if st.screen == Some(Screen::Invia) {
            st.screen = Some(Screen::Unita);
        }
        return;
    };
    let units: Vec<(UnitId, f32, f32)> = t
        .ids
        .iter()
        .filter_map(|id| env.mirror.view.units.get(id))
        .filter(|un| Some(un.faction) == v.you)
        .map(|un| (un.id, un.x, un.y))
        .collect();
    if units.is_empty() {
        if st.screen == Some(Screen::Invia) {
            st.screen = Some(Screen::Unita);
        }
        return;
    }
    if st.screen != Some(Screen::Invia) {
        return;
    }
    let mp = minimap::map_px(&env.mirror);
    let mw = 900.0 * s;
    let mh = mw * mp.y / mp.x;
    let mx = x + (w - mw) / 2.0;
    let my = y + 88.0 * s;
    let m = u.p.io.mouse;
    u.icon(&x_icon("map"), x + 78.0 * s, y + 44.0 * s, 52.0 * s);
    let ww = u.w();
    u.plain(
        format!(
            "{} unità: clic sul punto d'arrivo, oppure Frecce e Invio. Ci vanno da sole, attaccando chi incontrano.  Esc: torna a Unità",
            units.len()
        ),
        ww / 2.0,
        my - 24.0 * s,
        13.5 * s,
        ACCENT,
        Align::Center,
    );
    let k = minimap::draw_map(u, st, env, mx, my, mw, mh, s * 1.5, true);
    let inside = u.p.over(mx, my, mw, mh);
    let lw = (2.0 * s).max(1.5);
    let ring = env.gfx.ring(lw / (5.0 * s + lw / 2.0));
    for &(_, ux, uy) in &units {
        u.ring(&ring, mx + ux * k.x, my + uy * k.y, 5.0 * s + lw / 2.0, WHITE);
    }
    if inside && u.p.io.moved {
        t.kbd = false;
    }
    if inside && !t.kbd {
        t.x = (m.x - mx) / k.x;
        t.y = (m.y - my) / k.y;
    }
    if inside || t.kbd {
        let px = mx + t.x * k.x;
        let py = my + t.y * k.y;
        let lw = (2.5 * s).max(2.0);
        let ring = env.gfx.ring(lw / (13.0 * s + lw / 2.0));
        u.ring(&ring, px, py, 13.0 * s + lw / 2.0, Color::srgb_u8(0xff, 0x6a, 0x5a));
        u.icon(&ui_icon(5), px, py, 34.0 * s);
        u.p.cursor = CursorKind::Attack;
    }
    let go = t.go || (inside && u.p.io.pressed);
    t.go = false;
    st.send = Some(t.clone());
    if !go {
        return;
    }
    env.command(Command::Order {
        units: units.iter().map(|e| e.0).collect(),
        kind: OrderKind::AMove,
        x: t.x.round(),
        y: t.y.round(),
        target: None,
    });
    u.play(Sound::Ack);
    st.toast(format!("{} unità in marcia", units.len()));
    st.screen = None;
    st.send = None;
    st.marks.clear();
}

// ---------------------------------------------------------------------------------------------- Lavoratori

fn draw_workers(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View) {
    let Frame { x, y, w, s, .. } = manage_frame(u, st, v, "Lavoratori", 700.0, 500.0, Style::Paper);
    let Some(eco) = env.mirror.view.economy.clone() else {
        return;
    };
    let pawns: Vec<UnitView> = mine(env, v)
        .into_iter()
        .filter(|un| un.kind == UnitKind::Pawn)
        .cloned()
        .collect();
    let n_all = pawns.len().max(1) as f32;
    let free = pawns
        .iter()
        .filter(|p| matches!(p.act, Act::Idle | Act::Carry))
        .count();
    let building = pawns
        .iter()
        .filter(|p| matches!(p.act, Act::Build | Act::Repair))
        .count();
    let hidden = pawns
        .iter()
        .filter(|p| matches!(p.act, Act::Sheltered | Act::Fleeing))
        .count();
    let moving = pawns.iter().filter(|p| p.act == Act::Marching).count();
    let mut head = format!(
        "Pedoni {}/{}  ·  Liberi {}  ·  Al cantiere {}",
        pawns.len(),
        eco.pawn_cap,
        free,
        building
    );
    if hidden > 0 {
        head.push_str(&format!("  ·  Al riparo {hidden}"));
    }
    if moving > 0 {
        head.push_str(&format!("  ·  In viaggio {moving}"));
    }
    let ww = u.w();
    u.plain(head, ww / 2.0, y + 66.0 * s, 15.0 * s, if hidden > 0 { RED } else { ACCENT }, Align::Center);
    if !u.p.io.lmb {
        st.slide = None;
    }
    let clock = u.p.io.clock;
    let mut sends: Vec<(Res, u16)> = Vec::new();
    let mut why: Option<String> = None;
    let tx = x + 250.0 * s;
    let tw = 300.0 * s;
    for (i, &k) in Res::ALL.iter().enumerate() {
        let ry = y + 122.0 * s + i as f32 * 68.0 * s;
        let n = pawns.iter().filter(|p| p.job == Some(k)).count();
        let max = n + free;
        let rate = eco.rates[i];
        u.icon(&res_icon(k), x + 78.0 * s, ry, 46.0 * s);
        u.plain(
            k.label(),
            x + 110.0 * s,
            ry - if rate != 0 { 8.0 * s } else { 0.0 },
            17.0 * s,
            INK,
            Align::Left,
        );
        if rate != 0 {
            u.plain(format!("+{rate}/min"), x + 110.0 * s, ry + 13.0 * s, 12.5 * s, super::kit::GREEN, Align::Left);
        }
        if u.tiny_button(tx - 38.0 * s, ry, 34.0 * s, false, true, None).clicked && n > 0 {
            sends.push((k, n as u16 - 1));
        }
        u.text("−", tx - 38.0 * s, ry - 2.0 * s, 20.0 * s, WHITE, Align::Center);
        if u.tiny_button(tx + tw + 38.0 * s, ry, 34.0 * s, false, false, None).clicked {
            if free > 0 {
                sends.push((k, n as u16 + 1));
            } else {
                why = Some(NESSUN_PEDONE_LIBERO.to_string());
            }
        }
        u.text("+", tx + tw + 38.0 * s, ry - 2.0 * s, 20.0 * s, WHITE, Align::Center);
        u.big_bar(tx, ry - 14.0 * s, tw, n as f32 / n_all, 0.55 * s);
        u.p.rect(
            (tx + tw * max as f32 / n_all - s).round(),
            ry - 22.0 * s,
            (2.0 * s).max(2.0),
            44.0 * s,
            Color::srgba(74.0 / 255.0, 58.0 / 255.0, 51.0 / 255.0, 0.75),
        );
        if u.p.over(tx - 12.0 * s, ry - 22.0 * s, tw + 24.0 * s, 44.0 * s) {
            u.p.set_hot();
            u.p.cursor = CursorKind::Hand;
            if u.p.io.pressed {
                st.slide = Some(k);
            }
        }
        let want = if st.slide == Some(k) {
            (((u.p.io.mouse.x - tx) / tw * n_all).round()).clamp(0.0, max as f32) as usize
        } else {
            n
        };
        if want != n {
            sends.push((k, want as u16));
        }
        let knob = 34.0 * s;
        let kx = tx + tw * want as f32 / n_all;
        u.draw(
            &format!("{}Buttons/TinyRoundBlueButton.png", super::kit::UIP),
            None,
            (kx - knob / 2.0).round(),
            (ry - knob / 2.0).round(),
            knob.round(),
            knob.round(),
        );
        u.plain(n.to_string(), tx + tw + 82.0 * s, ry, 22.0 * s, INK, Align::Center);
    }
    let fy = y + 122.0 * s + 4.0 * 68.0 * s;
    u.avatar(v.color, UnitKind::Pawn, x + 56.0 * s, fy - 22.0 * s, 44.0 * s);
    u.plain(
        format!("Liberi: {free}"),
        x + 110.0 * s,
        fy,
        17.0 * s,
        if free > 0 { ACCENT } else { INK },
        Align::Left,
    );
    let lines = u.wrap(
        "Alzando un cursore parte il pedone libero più vicino a quella risorsa; abbassandolo, i pedoni tolti restano liberi. I nuovi pedoni non vengono assegnati da soli.",
        w - 330.0 * s,
        12.5 * s,
    );
    for (i, l) in lines.into_iter().enumerate() {
        u.plain(l, x + 250.0 * s, fy - 12.0 * s + i as f32 * 17.0 * s, 12.5 * s, INK, Align::Left);
    }
    // `send(k, v)` with the 0.5 s dedupe
    for (k, n) in sends {
        if st
            .sent
            .is_some_and(|(sk, sv, t)| sk == k && sv == n && clock - t < 0.5)
        {
            continue;
        }
        st.sent = Some((k, n, clock));
        env.command(Command::Jobs { res: k, n });
    }
    st.say(why);
}

// ---------------------------------------------------------------------------------------------- Ricerche

struct TechState {
    b: Option<ts_sim::slab::BuildingId>,
    done: bool,
    busy: bool,
    lock: Option<String>,
    poor: bool,
}

fn tech_state(env: &Env, v: &View, eco: &EconomyView, id: TechId) -> TechState {
    let d = id.def();
    let b = free_building(env, v, d.at);
    let done = TechSet(eco.techs).has(id);
    let busy = TechSet(eco.busy).has(id);
    let lock = if done || busy {
        None
    } else {
        match b {
            None => Some(richiede(d.at.def().label)),
            Some(b) => tech_lock(eco, id).or_else(|| (b.queue.len() >= QUEUE_MAX).then(|| CODA_PIENA.to_string())),
        }
    };
    let poor = !done && !busy && lock.is_none() && !can_afford(&v.res, &d.cost);
    TechState {
        b: b.map(|b| b.id),
        done,
        busy,
        lock,
        poor,
    }
}

fn draw_tree(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View) {
    let Frame { x, y, w, h, s } = manage_frame(u, st, v, "RICERCHE", 980.0, 660.0, Style::Dark);
    let Some(eco) = env.mirror.view.economy.clone() else {
        return;
    };
    let left = x + 40.0 * s;
    let label_w = 178.0 * s;
    let col_w = (w - 80.0 * s - label_w) / 3.0;
    let top = y + 112.0 * s;
    let nrows = TREE_ROWS.len() as f32;
    let row_h = (h - 112.0 * s - 52.0 * s) / nrows;
    for a in 1..=3u8 {
        let cx = left + label_w + (a - 1) as f32 * col_w;
        let on = a <= eco.age;
        u.p.rect(
            (cx + 4.0 * s).round(),
            (top - 46.0 * s).round(),
            (col_w - 8.0 * s).round(),
            (row_h * nrows + 50.0 * s).round(),
            if on {
                Color::srgba(1.0, 1.0, 1.0, 0.08)
            } else {
                Color::srgba(0.0, 0.0, 0.0, 0.25)
            },
        );
        u.text(
            format!("{}  ·  {}", ["I", "II", "III"][a as usize - 1], AGE_NAME[a as usize]),
            cx + col_w / 2.0,
            top - 26.0 * s,
            15.5 * s,
            if on { ERA_ON } else { DIMBLUE },
            Align::Center,
        );
    }
    // positions
    let mut pos: BTreeMap<TechId, Vec2> = BTreeMap::new();
    for (r, &kind) in TREE_ROWS.iter().enumerate() {
        for a in 1..=3u8 {
            let leads = |id: TechId| {
                TechId::ALL
                    .iter()
                    .any(|o| o.def().req == Some(id) && o.def().age > a) as u8
            };
            let mut ids: Vec<TechId> = TechId::ALL
                .iter()
                .copied()
                .filter(|t| t.def().at == kind && t.def().age == a)
                .collect();
            ids.sort_by_key(|t| leads(*t));
            let n = ids.len() as f32;
            for (i, id) in ids.into_iter().enumerate() {
                pos.insert(
                    id,
                    Vec2::new(
                        left + label_w + (a as f32 - 0.5) * col_w + (i as f32 - (n - 1.0) / 2.0) * 60.0 * s,
                        top + (r as f32 + 0.5) * row_h,
                    ),
                );
            }
        }
    }
    let techs = TechSet(eco.techs);
    for &id in TechId::ALL {
        let Some(req) = id.def().req else { continue };
        let (Some(a), Some(b)) = (pos.get(&req), pos.get(&id)) else {
            continue;
        };
        let c = if techs.has(req) {
            GOLD
        } else {
            Color::srgba(1.0, 1.0, 1.0, 0.28)
        };
        u.p.line(*a, *b, (3.0 * s).max(2.0), c);
    }
    for (r, &kind) in TREE_ROWS.iter().enumerate() {
        let cy = top + (r as f32 + 0.5) * row_h;
        let has = free_building(env, v, kind).is_some();
        let a0 = u.p.alpha;
        if !has {
            u.p.alpha = a0 * 0.45;
        }
        u.building_thumb(v.color, kind, left + 26.0 * s, cy, 44.0 * s);
        u.p.alpha = a0;
        u.text(
            kind.def().label,
            left + 56.0 * s,
            cy - if has { 0.0 } else { 8.0 * s },
            13.5 * s,
            if has { PARCH } else { DIMBLUE },
            Align::Left,
        );
        if !has {
            u.text("da costruire", left + 56.0 * s, cy + 9.0 * s, 11.5 * s, SALMON, Align::Left);
        }
    }
    let mut cmd = None;
    let mut why = None;
    for &id in TechId::ALL {
        let Some(&p) = pos.get(&id) else { continue };
        let ts = tech_state(env, v, &eco, id);
        let d = id.def();
        let size = 46.0 * s;
        let (cx, cy) = (p.x, p.y);
        let a0 = u.p.alpha;
        if ts.lock.is_some() {
            u.p.alpha = a0 * 0.42;
        }
        let r = u.tiny_button(cx, cy, size, true, ts.poor, None);
        u.spec(d.icon.into(), v.color, cx, cy - s, size * 0.5);
        u.p.alpha = a0;
        if ts.done {
            let bx = cx + size * 0.32;
            let byy = cy + size * 0.3;
            let br = 8.5 * s;
            let disc = env.gfx.disc.clone();
            u.p.image(&disc, None, bx - br, byy - br, 2.0 * br, 2.0 * br, Color::srgb_u8(0x3f, 0x9d, 0x4a));
            let lw = 2.4 * s;
            let p0 = Vec2::new(bx - br * 0.45, byy);
            let p1 = Vec2::new(bx - br * 0.1, byy + br * 0.38);
            let p2 = Vec2::new(bx + br * 0.5, byy - br * 0.35);
            u.p.line(p0, p1, lw, WHITE);
            u.p.line(p1, p2, lw, WHITE);
        } else if ts.busy {
            let mut ratio = 0.0;
            for b in own_buildings(env, v) {
                for q in &b.queue {
                    if q.what == QueueWhat::Tech(id) {
                        ratio = q.t / q.total.max(0.001);
                    }
                }
            }
            u.small_bar(cx - size * 0.42, cy + size * 0.3, size * 0.84, ratio, 0.5 * s, faction_color(v.color));
        }
        if r.hover {
            let note = if ts.done {
                "Già ricercata. "
            } else if ts.busy {
                "Ricerca in corso. "
            } else {
                ""
            };
            u.p.tip = Some(Tip {
                x: cx,
                y: cy - size * 0.2,
                title: d.label.to_string(),
                cost: (!ts.done).then_some(d.cost),
                desc: format!("{note}{}", d.desc),
                lock: ts.lock.clone(),
                ..Default::default()
            });
        }
        if r.clicked && !ts.done && !ts.busy {
            match (ts.lock, ts.b) {
                (Some(l), _) => why = Some(l),
                (None, Some(b)) => cmd = Some(Command::Research { building: b, tech: id }),
                _ => {}
            }
        }
    }
    let ww = u.w();
    u.text(
        "Clic su una ricerca per avviarla nell'edificio che la offre  ·  rosso: mancano le risorse  ·  spento: manca un requisito",
        ww / 2.0,
        y + h - 30.0 * s,
        12.5 * s,
        MUTED,
        Align::Center,
    );
    if let Some(c) = cmd {
        env.command(c);
    }
    st.say(why);
}

// ---------------------------------------------------------------------------------------------- Costruisci

fn draw_build(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View) {
    let Frame { x, y, w, h, s } = manage_frame(u, st, v, "COSTRUISCI", 840.0, 690.0, Style::Wood);
    let Some(eco) = env.mirror.view.economy.clone() else {
        return;
    };
    let ww = u.w();
    u.text(
        "Scegli l'edificio, poi dove metterlo (clic, o Frecce e Invio): ci va il pedone libero più vicino, o quelli che hai selezionato.",
        ww / 2.0,
        y + h - 54.0 * s,
        12.5 * s,
        CREAM,
        Align::Center,
    );
    let cw = 176.0 * s;
    let ch = 130.0 * s;
    let gx = x + (w - cw * 4.0) / 2.0;
    let gy = y + 92.0 * s;
    let has_pawn = mine(env, v).iter().any(|un| un.kind == UnitKind::Pawn);
    let mut chosen = None;
    let mut why = None;
    for (i, &kind) in BUILD_MENU.iter().enumerate() {
        let d = kind.def();
        let bx = gx + (i % 4) as f32 * cw;
        let by = gy + (i / 4) as f32 * ch;
        let lock = age_lock(eco.age, d.age);
        let poor = lock.is_none() && !can_afford(&v.res, &d.cost);
        let r = u.p.hit(bx + 4.0 * s, by + 4.0 * s, cw - 8.0 * s, ch - 8.0 * s);
        u.panel(&panel::WOOD_SLOTS, bx + 6.0 * s, by + 6.0 * s, cw - 12.0 * s, ch - 12.0 * s, 0.7 * s);
        let a0 = u.p.alpha;
        if lock.is_some() {
            u.p.alpha = a0 * 0.45;
        }
        u.building_thumb(v.color, kind, bx + cw / 2.0, by + 46.0 * s, 68.0 * s);
        u.text(d.label, bx + cw / 2.0, by + 88.0 * s, 13.5 * s, WHITE, Align::Center);
        u.p.alpha = a0;
        if lock.is_some() {
            u.text(AGE_NAME[d.age as usize], bx + cw / 2.0, by + 108.0 * s, 12.0 * s, LIGHT_RED, Align::Center);
        } else {
            u.cost(&v.res, &d.cost, bx + cw / 2.0, by + 108.0 * s, s, true, cw - 16.0 * s);
        }
        if r.hover {
            u.brackets(bx + 4.0 * s, by + 4.0 * s, cw - 8.0 * s, ch - 8.0 * s, 0.7 * s);
            u.p.tip = Some(Tip {
                x: bx + cw / 2.0,
                y: by + 14.0 * s,
                title: d.label.to_string(),
                cost: Some(d.cost),
                desc: d.desc.to_string(),
                lock: lock
                    .clone()
                    .or_else(|| poor.then(|| RISORSE_INSUFFICIENTI.to_string())),
                ..Default::default()
            });
        }
        if r.clicked {
            let w = lock.or_else(|| {
                if poor {
                    Some(RISORSE_INSUFFICIENTI.to_string())
                } else if !has_pawn {
                    Some(NON_HAI_PEDONI.to_string())
                } else {
                    None
                }
            });
            if w.is_none() {
                chosen = Some(kind);
            }
            why = w;
        }
    }
    st.say(why);
    if let Some(kind) = chosen {
        u.play(Sound::Click);
        st.screen = None;
        selection::start_placing(env, kind, u.p.kbd);
    }
}

// ---------------------------------------------------------------------------------------------- Mercato

fn draw_market(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View) {
    let Frame { x, y, w, s, .. } = manage_frame(u, st, v, "Mercato", 700.0, 440.0, Style::Paper);
    let Some(eco) = env.mirror.view.economy.clone() else {
        return;
    };
    let market = free_building(env, v, BuildingKind::Market).map(|b| b.id);
    let ww = u.w();
    u.icon(&x_icon("scales"), x + 70.0 * s, y + 74.0 * s, 46.0 * s);
    u.icon(&res_icon(Res::Gold), x + w / 2.0 - 60.0 * s, y + 74.0 * s, 40.0 * s);
    let gold = v.res[Res::Gold as usize];
    u.plain(format!("Oro: {gold}"), x + w / 2.0 - 34.0 * s, y + 75.0 * s, 19.0 * s, INK, Align::Left);
    let Some(market) = market else {
        u.plain("Non hai ancora un Mercato.", ww / 2.0, y + 190.0 * s, 19.0 * s, ACCENT, Align::Center);
        u.plain(
            format!(
                "Si costruisce dall'{}: schermata Costruisci (tasto C).",
                AGE_NAME[BuildingKind::Market.def().age as usize]
            ),
            ww / 2.0,
            y + 226.0 * s,
            14.5 * s,
            INK,
            Align::Center,
        );
        return;
    };
    let mut cmd = None;
    for (i, &k) in [Res::Wood, Res::Meat, Res::Stone].iter().enumerate() {
        let ry = y + 150.0 * s + i as f32 * 82.0 * s;
        let tr = eco.trades[k as usize] as f64;
        let sell = sell_price(tr).round() as i64;
        let buy = buy_price(tr).round() as i64;
        u.icon(&res_icon(k), x + 74.0 * s, ry, 50.0 * s);
        u.plain(k.label(), x + 108.0 * s, ry - 10.0 * s, 17.0 * s, INK, Align::Left);
        u.plain(
            format!("ne hai {}", v.res[k as usize]),
            x + 108.0 * s,
            ry + 12.0 * s,
            12.5 * s,
            ACCENT,
            Align::Left,
        );
        let a0 = u.p.alpha;
        if v.res[k as usize] < 100 {
            u.p.alpha = a0 * 0.45;
        }
        if u.big_button(x + 236.0 * s, ry - 28.0 * s, 200.0 * s, 56.0 * s, 0.58 * s, &format!("Vendi 100:  +{sell}"), false, None)
        {
            cmd = Some(Command::Trade {
                market,
                op: TradeOp::Sell,
                res: k,
            });
        }
        u.icon(&x_icon("coins"), x + 262.0 * s, ry - 2.0 * s, 30.0 * s);
        u.p.alpha = if (gold as i64) < buy { a0 * 0.45 } else { a0 };
        if u.big_button(x + 452.0 * s, ry - 28.0 * s, 200.0 * s, 56.0 * s, 0.58 * s, &format!("Compra 100:  −{buy}"), true, None)
        {
            cmd = Some(Command::Trade {
                market,
                op: TradeOp::Buy,
                res: k,
            });
        }
        u.icon(&x_icon("upgrade"), x + 478.0 * s, ry - 2.0 * s, 30.0 * s);
        u.p.alpha = a0;
    }
    u.plain(
        "I prezzi sono in oro. Ogni scambio li peggiora per un po'.",
        ww / 2.0,
        y + 150.0 * s + 3.0 * 82.0 * s - 18.0 * s,
        13.0 * s,
        INK,
        Align::Center,
    );
    if let Some(c) = cmd {
        env.command(c);
    }
}

// ---------------------------------------------------------------------------------------------- Modalità test

fn draw_test(u: &mut Ui, st: &mut UiState, env: &mut Env, v: &View) {
    let Frame { x, y, w, s, .. } = manage_frame(u, st, v, "Modalità test", 700.0, 530.0, Style::Paper);
    let ww = u.w();
    let bw = 136.0 * s;
    let gap = (w - 80.0 * s - bw * 4.0) / 3.0;
    u.plain(
        "Solo nelle partite da soli. Si apre e si chiude con F4.",
        ww / 2.0,
        y + 66.0 * s,
        13.5 * s,
        ACCENT,
        Align::Center,
    );
    let mut cmd = None;
    for (i, &k) in Res::ALL.iter().enumerate() {
        let bx = x + 40.0 * s + i as f32 * (bw + gap);
        u.icon(&res_icon(k), bx + 26.0 * s, y + 116.0 * s, 44.0 * s);
        u.plain(v.res[i].to_string(), bx + 52.0 * s, y + 117.0 * s, 17.0 * s, INK, Align::Left);
        if u.big_button(bx, y + 144.0 * s, bw, 54.0 * s, 0.58 * s, "+1000", false, None) {
            cmd = Some(Command::DebugGift { res: Some(k) });
        }
    }
    if u.big_button(x + 40.0 * s, y + 212.0 * s, w - 80.0 * s, 54.0 * s, 0.58 * s, "+1000 di tutte e quattro", false, None)
    {
        cmd = Some(Command::DebugGift { res: None });
    }
    let frozen = env.mirror.view.frozen;
    if u.big_button(
        x + 40.0 * s,
        y + 292.0 * s,
        w - 80.0 * s,
        54.0 * s,
        0.58 * s,
        if frozen {
            "Tempo fermo: fallo ripartire"
        } else {
            "Ferma il tempo"
        },
        frozen,
        None,
    ) {
        cmd = Some(Command::DebugFreeze { on: !frozen });
    }
    let fog_on = !env.mirror.view.fog_off;
    if u.big_button(
        x + 40.0 * s,
        y + 360.0 * s,
        w - 80.0 * s,
        54.0 * s,
        0.58 * s,
        if fog_on {
            "Togli la nebbia: tutta la mappa in vista"
        } else {
            "Nebbia tolta: rimettila"
        },
        !fog_on,
        None,
    ) {
        cmd = Some(Command::DebugFogOff { on: fog_on });
    }
    u.plain(
        "A tempo fermo puoi guardare in giro e dare ordini: partono quando lo fai ripartire.",
        ww / 2.0,
        y + 438.0 * s,
        13.0 * s,
        INK,
        Align::Center,
    );
    if let Some(c) = cmd {
        env.command(c);
    }
}
