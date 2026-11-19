//! The JS `Ctl` functions (control.js) over one SystemParam: selection, orders, context actions, shortcuts,
//! placement and pick-move (05 §7-§10, §13.1). Everything only talks to the sim through `NetOut::command`; the local
//! feedback (markers, sounds, toasts of local refusals) stays client-side (05 §17.4).

use super::pick::{entity_pos, entity_valid, knows, pick, pick_hits};
use crate::render::sprites::SpriteHits;
use super::scene::{SceneIndex, building_cells};
use super::view::ViewInfo;
use super::{
    ClickAction, DOUBLE_CLICK, GROUP_DOUBLE, MarkKind, Markers, PlaceMode, Placement, Selection,
};
use crate::link::{LobbyState, NetOut};
use crate::mirror::Mirror;
use crate::render::camera::CameraRig;
use crate::ui::{Cursor, UiState};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use ts_sim::TILE;
use ts_sim::cmd::{self, Command, EntityRef, OrderKind};
use ts_sim::defs::{
    Act, BUILD_RADIUS, BuildingKind, DEFENSE_RADIUS, FactionIdx, NodeKind, OUTPOST_RADIUS, Sound,
    UnitKind,
};
use ts_sim::slab::{BuildingId, UnitId};
use ts_sim::snapshot_types::{BuildingView, UF_INSIDE, UnitView};

/// Context action identity (05 §8.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActionId {
    /// X — `Attacco in marcia`
    AttackMove,
    /// E — `Esplora`
    Explore,
    /// F — `Ferma`
    Stop,
    /// G — `Punto di raccolta`
    Rally,
    /// V — `Sposta`
    Move,
    /// Canc — `Demolisci` / `Annulla cantiere`
    Demolish,
}

/// One context action of the selection panel.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextAction {
    pub id: ActionId,
    /// keyboard code that triggers it from the world (`None` for V: the V key goes through `moveMode`)
    pub code: Option<KeyCode>,
    /// key label shown on the button
    pub key: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
    /// highlighted (mode active)
    pub on: bool,
}

/// `Ctl.actions()` (control.js:281-308).
pub fn context_actions(mirror: &Mirror, sel: &Selection, place: &Placement) -> Vec<ContextAction> {
    let mut out = Vec::new();
    let mine = sel.mine(mirror);
    if !mine.is_empty() {
        let defs: Vec<&'static ts_sim::defs::UnitDef> = mine
            .iter()
            .filter_map(|id| mirror.view.units.get(id))
            .map(|u| u.kind.def())
            .collect();
        if defs.iter().any(|d| d.dmg > 0.0) {
            out.push(ContextAction {
                id: ActionId::AttackMove,
                code: Some(KeyCode::KeyX),
                key: "X",
                title: "Attacco in marcia",
                desc: "Poi clic sul terreno: le unità vanno lì in formazione, al passo del più lento, attaccando chi incontrano; poi proseguono da sole sui nemici vicini.",
                on: place.action == ClickAction::AttackMove,
            });
        }
        if defs.iter().any(|d| d.scout) {
            out.push(ContextAction {
                id: ActionId::Explore,
                code: Some(KeyCode::KeyE),
                key: "E",
                title: "Esplora",
                desc: "L'Esploratore va da solo, una dopo l'altra, nelle zone mai viste più vicine, finché ce ne sono.",
                on: false,
            });
        }
        out.push(ContextAction {
            id: ActionId::Stop,
            code: Some(KeyCode::KeyF),
            key: "F",
            title: "Ferma",
            desc: "Annulla l'ordine in corso.",
            on: false,
        });
        return out;
    }
    let Some(bid) = sel.own_building(mirror) else {
        return out;
    };
    let Some(b) = mirror.view.buildings.get(&bid) else {
        return out;
    };
    let d = b.kind.def();
    if b.done && !d.trains.is_empty() {
        out.push(ContextAction {
            id: ActionId::Rally,
            code: Some(KeyCode::KeyG),
            key: "G",
            title: "Punto di raccolta",
            desc: "Poi clic sul terreno o su una risorsa (anche col tasto destro, direttamente).",
            on: place.action == ClickAction::Rally,
        });
    }
    if b.kind != BuildingKind::Castle {
        out.push(ContextAction {
            id: ActionId::Move,
            code: None,
            key: "V",
            title: "Sposta",
            desc: "Scegli la nuova casella col mouse o con le frecce (Invio per posare), dentro il raggio in cui si può costruire.",
            on: place.mode == PlaceMode::Move(bid),
        });
        out.push(ContextAction {
            id: ActionId::Demolish,
            code: Some(KeyCode::Delete),
            key: "Canc",
            title: if b.done {
                "Demolisci"
            } else {
                "Annulla cantiere"
            },
            desc: if b.done {
                "Abbatte l'edificio, senza rimborso."
            } else {
                "Rimuove il cantiere e restituisce le risorse."
            },
            on: false,
        });
    }
    out
}

/// JS `Math.round`: halves go up (towards +∞), unlike `f32::round` (away from zero) — matters for the -0.5 cells at
/// the map's left / top edge.
#[inline]
pub(crate) fn js_round(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

/// Building foot point / footprint helpers (sim px).
pub fn building_foot(b: &BuildingView) -> Vec2 {
    let d = b.kind.def();
    Vec2::new(
        (b.cx as f32 + d.w as f32 / 2.0) * TILE,
        (b.cy + d.h) as f32 * TILE - 4.0,
    )
}

/// Bottom edge `y1` of a building footprint (px).
pub fn building_y1(b: &BuildingView) -> f32 {
    (b.cy + b.kind.def().h) as f32 * TILE
}

/// All the state the `Ctl` functions touch.
#[derive(SystemParam)]
pub struct Ctx<'w> {
    pub mirror: Res<'w, Mirror>,
    pub scene: Res<'w, SceneIndex>,
    pub view: Res<'w, ViewInfo>,
    pub sel: ResMut<'w, Selection>,
    pub place: ResMut<'w, Placement>,
    pub net: ResMut<'w, NetOut>,
    pub marks: ResMut<'w, Markers>,
    pub rig: ResMut<'w, CameraRig>,
    pub ui: ResMut<'w, UiState>,
    pub lobby: Res<'w, LobbyState>,
    pub time: Res<'w, Time<Real>>,
    pub hits: Res<'w, SpriteHits>,
}

impl Ctx<'_> {
    /// `pick()` at a window point: the sprite renderer's hit boxes (exact billboards) when it published any this
    /// frame, else the projected spec hitboxes.
    pub fn pick_at(&self, s: Vec2, g: Option<Vec2>) -> Option<EntityRef> {
        match pick_hits(&self.hits, &self.mirror, &self.scene, s) {
            Some(r) => r,
            None => pick(&self.mirror, &self.scene, &self.view, s, g),
        }
    }

    /// `G.clock` (real seconds).
    pub fn now(&self) -> f64 {
        self.time.elapsed_secs_f64()
    }

    pub fn me(&self) -> Option<FactionIdx> {
        self.mirror.you
    }

    fn unit(&self, id: UnitId) -> Option<&UnitView> {
        self.mirror.view.units.get(&id)
    }

    fn own_units(&self) -> impl Iterator<Item = &UnitView> {
        let me = self.me();
        self.mirror
            .view
            .units
            .values()
            .filter(move |u| Some(u.faction) == me)
    }

    fn own_castle(&self) -> Option<&BuildingView> {
        let me = self.me()?;
        self.mirror
            .view
            .buildings
            .values()
            .find(|b| b.faction == me && b.kind == BuildingKind::Castle && !self.mirror.view.ghosts.contains(&b.id))
    }

    /// `toast(why)` + `deny` (or `click` when `why` is `None`), the JS `act` / `say` pattern.
    pub fn say(&mut self, why: Option<String>) {
        match why {
            Some(w) => {
                self.ui.toast(w);
                self.ui.play(Sound::Deny);
            }
            None => self.ui.play(Sound::Click),
        }
    }

    pub fn play(&mut self, s: Sound) {
        self.ui.play(s);
    }

    /// `lookAt(x, y)`.
    pub fn look_at(&mut self, p: Vec2) {
        self.rig.look_at(p);
    }

    /// `onScreen(x, y, m)` with the local camera (05 addenda C1); `m` in world px.
    pub fn on_screen(&self, x: f32, y: f32, m: f32) -> bool {
        let map = self.mirror.map();
        let k = self.view.scale_at(map, x, y) / TILE;
        self.view.on_screen(map, x, y, m * k)
    }

    // ------------------------------------------------------------------------------------------- selection

    /// `refresh()`: drop selected ids that are gone, dead, exhausted (05 §7.1) and stale control-group ids.
    pub fn refresh(&mut self) {
        let mirror = &self.mirror;
        let scene = &self.scene;
        let sel = &mut self.sel;
        sel.units.retain(|id| {
            mirror
                .view
                .units
                .get(id)
                .is_some_and(|u| u.flags & UF_INSIDE == 0 || Some(u.faction) == mirror.you)
        });
        if let Some(b) = sel.building
            && !mirror.view.buildings.contains_key(&b)
        {
            sel.building = None;
        }
        if let Some(o) = sel.other
            && !entity_valid(mirror, scene, o)
        {
            sel.other = None;
        }
        if let Some(PlaceMode::Move(b)) = Some(self.place.mode)
            && !mirror.view.buildings.contains_key(&b)
        {
            // the moved building died while placing → placing cancelled (05 §10)
            self.place.mode = PlaceMode::None;
            self.place.kind = None;
        }
        if let Some(b) = self.place.pick
            && !mirror.view.buildings.contains_key(&b)
        {
            self.place.pick = None;
        }
    }

    /// `select(ids, add)`.
    pub fn select(&mut self, ids: &[EntityRef], add: bool) {
        if !add {
            self.sel.clear();
        }
        for e in ids {
            self.sel.add(*e);
        }
        self.place.action = ClickAction::None;
        self.refresh();
    }

    /// `clickSelect(wx, wy, add)` (05 §7.3) at the window point `s`.
    pub fn click_select(&mut self, s: Vec2, g: Option<Vec2>, add: bool) {
        let Some(e) = self.pick_at(s, g) else {
            if !add {
                self.select(&[], false);
            }
            return;
        };
        let now = self.now();
        let me = self.me();
        let own_unit = match e {
            EntityRef::Unit(id) => self.unit(id).is_some_and(|u| Some(u.faction) == me),
            _ => false,
        };
        let double = own_unit
            && self
                .sel
                .last_pick
                .is_some_and(|(id, t)| id == e && now - t < DOUBLE_CLICK);
        if double {
            let kind = match e {
                EntityRef::Unit(id) => self.unit(id).map(|u| u.kind),
                _ => None,
            };
            let ids: Vec<EntityRef> = self
                .own_units()
                .filter(|u| Some(u.kind) == kind && u.flags & UF_INSIDE == 0)
                .filter(|u| self.on_screen(u.x, u.y, 0.0))
                .map(|u| EntityRef::Unit(u.id))
                .collect();
            self.select(&ids, add);
        } else if add && own_unit && self.sel_all_own_units() {
            // shift-click toggles
            if self.sel.contains(e) {
                self.sel.remove(e);
            } else {
                self.sel.add(e);
            }
            self.refresh();
        } else {
            self.select(&[e], false);
        }
        self.sel.last_pick = Some((e, now));
        self.play(Sound::Select);
    }

    /// Every selected entity is an own unit (vacuously true when empty).
    fn sel_all_own_units(&self) -> bool {
        self.sel.building.is_none()
            && self.sel.other.is_none()
            && self.sel.units.len() == self.sel.mine(&self.mirror).len()
    }

    /// `boxSelect(x0, y0, x1, y1, add)` (05 §7.4), window px.
    pub fn box_select(&mut self, a: Vec2, b: Vec2, add: bool) {
        let r = Rect::from_corners(a, b);
        let map = self.mirror.map();
        // screen point 26 px above the foot: the renderer's anchor when it drew the unit, else projected
        let anchors: std::collections::HashMap<UnitId, Vec2> = self
            .hits
            .hits
            .iter()
            .filter_map(|h| match h.target {
                crate::render::sprites::HitTarget::Unit(id) => Some((id, h.anchor)),
                _ => None,
            })
            .collect();
        let ids: Vec<(UnitId, UnitKind)> = self
            .own_units()
            .filter(|u| u.flags & UF_INSIDE == 0)
            .filter(|u| {
                anchors
                    .get(&u.id)
                    .copied()
                    .or_else(|| self.view.project_px(map, u.x, u.y, 26.0))
                    .is_some_and(|p| r.contains(p))
            })
            .map(|u| (u.id, u.kind))
            .collect();
        let army: Vec<EntityRef> = ids
            .iter()
            .filter(|(_, k)| *k != UnitKind::Pawn)
            .map(|(id, _)| EntityRef::Unit(*id))
            .collect();
        if !ids.is_empty() || !add {
            let pick: Vec<EntityRef> = if !army.is_empty() && army.len() < ids.len() {
                army
            } else {
                ids.iter().map(|(id, _)| EntityRef::Unit(*id)).collect()
            };
            self.select(&pick, add);
        }
    }

    // ------------------------------------------------------------------------------------------- orders

    /// `order(wx, wy, forceAttackMove)` (05 §8.1); `target` = `pick` at the order point.
    pub fn order(&mut self, w: Vec2, target: Option<EntityRef>, force_amove: bool) {
        let own = self.sel.mine(&self.mirror);
        if own.is_empty() {
            return;
        }
        let me = self.me();
        let kind_of = |id: &UnitId| self.unit(*id).map(|u| u.kind);
        let pawns: Vec<UnitId> = own
            .iter()
            .copied()
            .filter(|id| kind_of(id) == Some(UnitKind::Pawn))
            .collect();
        let army: Vec<UnitId> = own
            .iter()
            .copied()
            .filter(|id| kind_of(id).is_some_and(|k| k.def().dmg > 0.0))
            .collect();
        let monks: Vec<UnitId> = own
            .iter()
            .copied()
            .filter(|id| kind_of(id) == Some(UnitKind::Monk))
            .collect();
        let except = |g: &[UnitId]| -> Vec<UnitId> {
            own.iter().copied().filter(|id| !g.contains(id)).collect()
        };
        let (wx, wy) = (js_round(w.x) as f32, js_round(w.y) as f32);
        let mut sends: Vec<(Vec<UnitId>, OrderKind, Option<EntityRef>)> = Vec::new();
        let mut mark = MarkKind::Move;
        let enemy = match target {
            Some(EntityRef::Unit(id)) => self.unit(id).is_some_and(|u| Some(u.faction) != me),
            Some(EntityRef::Building(id)) => self
                .mirror
                .view
                .buildings
                .get(&id)
                .is_some_and(|b| Some(b.faction) != me),
            _ => false,
        };
        let is_res = matches!(target, Some(EntityRef::Node(_)) | Some(EntityRef::Sheep(_)));
        if enemy {
            // monks follow the attackers instead of walking onto the target
            let mut band = army.clone();
            band.extend(monks.iter().copied());
            sends.push((band.clone(), OrderKind::Attack, target));
            sends.push((except(&band), OrderKind::Move, None));
            mark = if army.is_empty() {
                MarkKind::Move
            } else {
                MarkKind::Attack
            };
        } else if force_amove {
            sends.push((own.clone(), OrderKind::AMove, None));
            mark = MarkKind::Attack;
        } else if is_res {
            sends.push((pawns.clone(), OrderKind::Gather, target));
            sends.push((except(&pawns), OrderKind::Move, None));
            mark = if pawns.is_empty() {
                MarkKind::Move
            } else {
                MarkKind::Gather
            };
        } else if let Some(EntityRef::Building(bid)) = target
            && let Some(b) = self.mirror.view.buildings.get(&bid)
        {
            let work = !b.done || b.hp < b.max_hp;
            let kind = if work {
                OrderKind::Build
            } else if !b.kind.def().drop.is_empty() {
                OrderKind::Drop
            } else {
                OrderKind::Move
            };
            sends.push((pawns.clone(), kind, target));
            sends.push((except(&pawns), OrderKind::Move, None));
            mark = if !pawns.is_empty() && work {
                MarkKind::Gather
            } else {
                MarkKind::Move
            };
        } else if let Some(EntityRef::Unit(uid)) = target
            && self.unit(uid).is_some_and(|u| u.hp < u.max_hp)
            && !monks.is_empty()
        {
            sends.push((monks.clone(), OrderKind::Heal, target));
            sends.push((except(&monks), OrderKind::Move, None));
        } else {
            sends.push((own.clone(), OrderKind::Move, None));
        }
        for (units, kind, t) in sends {
            if units.is_empty() {
                continue;
            }
            // sim caps id arrays at 200: split bigger selections
            for chunk in units.chunks(cmd::MAX_IDS) {
                self.net.command(Command::Order {
                    units: chunk.to_vec(),
                    kind,
                    x: wx,
                    y: wy,
                    target: t,
                });
            }
        }
        self.marks.mark(mark, Vec2::new(wx, wy));
        self.play(Sound::Ack);
    }

    /// `setRally(b, wx, wy)` (05 §8.2).
    pub fn set_rally(&mut self, b: BuildingId, w: Vec2, target: Option<EntityRef>) {
        let me = self.me();
        let ok = match target {
            Some(EntityRef::Node(_)) | Some(EntityRef::Sheep(_)) => true,
            Some(EntityRef::Building(t)) => self
                .mirror
                .view
                .buildings
                .get(&t)
                .is_some_and(|tb| Some(tb.faction) == me && !tb.done),
            _ => false,
        };
        let (wx, wy) = (js_round(w.x) as f32, js_round(w.y) as f32);
        self.net.command(Command::Rally {
            building: b,
            x: wx,
            y: wy,
            target: if ok { target } else { None },
        });
        self.marks.mark(MarkKind::Rally, Vec2::new(wx, wy));
        self.play(Sound::Ack);
    }

    /// Target under a world point (for orders that do not come from the cursor: minimap right click).
    pub fn pick_world(&self, w: Vec2) -> Option<EntityRef> {
        let s = self.view.project_px(self.mirror.map(), w.x, w.y, 0.0)?;
        if !self.view.viewport.contains(s) {
            return None;
        }
        self.pick_at(s, Some(w))
    }

    /// `act(a)`: run a context action → toast + deny on refusal, else click.
    pub fn act(&mut self, id: ActionId) {
        let why = self.run_action(id);
        self.say(why);
    }

    fn run_action(&mut self, id: ActionId) -> Option<String> {
        let own = self.sel.mine(&self.mirror);
        match id {
            ActionId::AttackMove => {
                self.place.action = ClickAction::AttackMove;
                None
            }
            ActionId::Explore => {
                let scouts: Vec<UnitId> = own
                    .iter()
                    .copied()
                    .filter(|i| self.unit(*i).is_some_and(|u| u.kind.def().scout))
                    .collect();
                if !scouts.is_empty() {
                    self.net.command(Command::Order {
                        units: scouts,
                        kind: OrderKind::Explore,
                        x: 0.0,
                        y: 0.0,
                        target: None,
                    });
                }
                None
            }
            ActionId::Stop => {
                for chunk in own.chunks(cmd::MAX_IDS) {
                    self.net.command(Command::Order {
                        units: chunk.to_vec(),
                        kind: OrderKind::Stop,
                        x: 0.0,
                        y: 0.0,
                        target: None,
                    });
                }
                None
            }
            ActionId::Rally => {
                self.place.action = ClickAction::Rally;
                None
            }
            ActionId::Move => match self.sel.own_building(&self.mirror) {
                Some(b) => self.start_moving(b, false),
                None => self.move_mode(),
            },
            ActionId::Demolish => {
                if let Some(b) = self.sel.own_building(&self.mirror) {
                    self.net.command(Command::Demolish { building: b });
                    self.select(&[], false);
                }
                None
            }
        }
    }

    /// Context action triggered by a key code (`KeyX`, `KeyE`, `KeyF`, `KeyG`, `Delete`).
    pub fn act_key(&mut self, code: KeyCode) -> bool {
        let list = context_actions(&self.mirror, &self.sel, &self.place);
        if let Some(a) = list.iter().find(|a| a.code == Some(code)) {
            self.act(a.id);
            true
        } else {
            false
        }
    }

    // ------------------------------------------------------------------------------------------- shortcuts

    /// `army()`: own live units that are not pawns (scouts, monks, siege included).
    fn army(&self) -> Vec<UnitId> {
        self.own_units()
            .filter(|u| u.kind != UnitKind::Pawn)
            .map(|u| u.id)
            .collect()
    }

    /// `nextIdle()`.
    pub fn next_idle(&mut self) {
        let idle: Vec<(UnitId, Vec2)> = self
            .own_units()
            .filter(|u| u.kind == UnitKind::Pawn && u.act == Act::Idle)
            .map(|u| (u.id, Vec2::new(u.x, u.y)))
            .collect();
        if idle.is_empty() {
            return;
        }
        let i = self.sel.idle_i % idle.len();
        self.sel.idle_i = self.sel.idle_i.wrapping_add(1);
        let (id, p) = idle[i];
        self.select(&[EntityRef::Unit(id)], false);
        self.look_at(p);
    }

    /// `home()`.
    pub fn home(&mut self) {
        if let Some(c) = self.own_castle() {
            let id = c.id;
            let p = building_foot(c);
            self.select(&[EntityRef::Building(id)], false);
            self.look_at(p - Vec2::new(0.0, 80.0));
        }
    }

    /// `group(n, assign)`.
    pub fn group(&mut self, n: usize, assign: bool) {
        if n >= self.sel.groups.len() {
            return;
        }
        if assign {
            self.sel.groups[n] = self.sel.mine(&self.mirror);
            return;
        }
        let ids: Vec<UnitId> = self.sel.groups[n]
            .iter()
            .copied()
            .filter(|id| self.mirror.view.units.contains_key(id))
            .collect();
        if ids.is_empty() {
            return;
        }
        let refs: Vec<EntityRef> = ids.iter().map(|id| EntityRef::Unit(*id)).collect();
        self.select(&refs, false);
        let now = self.now();
        if self
            .sel
            .last_group
            .is_some_and(|(g, t)| g == n && now - t < GROUP_DOUBLE)
        {
            self.center_on_selection();
        }
        self.sel.last_group = Some((n, now));
    }

    /// `centerOnSelection()`.
    pub fn center_on_selection(&mut self) {
        let Some(e) = self.sel.first() else {
            return;
        };
        let Some(p) = entity_pos(&self.mirror, &self.scene, e) else {
            return;
        };
        let dy = if matches!(e, EntityRef::Building(_)) {
            80.0
        } else {
            0.0
        };
        self.look_at(p - Vec2::new(0.0, dy));
    }

    /// `selectArmy()` (Tab).
    pub fn select_army(&mut self) {
        let army = self.army();
        if army.is_empty() {
            self.say(Some(cmd::NON_HAI_TRUPPE.to_string()));
            return;
        }
        let refs: Vec<EntityRef> = army.into_iter().map(EntityRef::Unit).collect();
        self.select(&refs, false);
        self.play(Sound::Select);
    }

    /// `recall(units = army())` (Backspace / Units screen).
    pub fn recall(&mut self, units: Option<Vec<UnitId>>) {
        let units = units.unwrap_or_else(|| self.army());
        let castle = self.own_castle().map(|c| (building_foot(c).x, building_y1(c)));
        let Some((cx, cy1)) = castle.filter(|_| !units.is_empty()) else {
            self.say(Some(cmd::NON_HAI_TRUPPE.to_string()));
            return;
        };
        for chunk in units.chunks(cmd::MAX_IDS) {
            self.net.command(Command::Recall {
                units: chunk.to_vec(),
            });
        }
        self.marks.mark(MarkKind::Move, Vec2::new(cx, cy1 + 130.0));
        self.play(Sound::Ack);
    }

    // ------------------------------------------------------------------------------------------- placement

    /// `startPlacing(kind)` (05 §9.1): ghost centred on the view.
    pub fn start_placing(&mut self, kind: BuildingKind, kbd: bool) {
        let d = kind.def();
        let c = self.view.center_px;
        self.place.mode = PlaceMode::Place(kind);
        self.place.kind = Some(kind);
        self.place.cell = IVec2::new(
            js_round(c.x / TILE - d.w as f32 / 2.0),
            js_round(c.y / TILE - d.h as f32 / 2.0),
        );
        self.place.ok = false;
        self.place.why = None;
        self.place.last = None;
        self.place.kbd = kbd;
        self.place.enter = false;
        self.place.action = ClickAction::None;
        self.place.pick = None;
    }

    /// `moveMode()` (V).
    pub fn move_mode(&mut self) -> Option<String> {
        if let Some(b) = self.sel.own_building(&self.mirror) {
            return self.start_moving(b, false);
        }
        self.place.mode = PlaceMode::None;
        self.place.kind = None;
        self.place.action = ClickAction::PickMove;
        self.place.pick = None;
        None
    }

    /// `startMoving(b, kbd)`.
    pub fn start_moving(&mut self, b: BuildingId, kbd: bool) -> Option<String> {
        let Some(v) = self.mirror.view.buildings.get(&b) else {
            return None;
        };
        if v.kind == BuildingKind::Castle {
            return Some(cmd::CASTELLO_NON_SI_SPOSTA.to_string());
        }
        let (kind, cx, cy) = (v.kind, v.cx, v.cy);
        self.place.mode = PlaceMode::Move(b);
        self.place.kind = Some(kind);
        self.place.cell = IVec2::new(cx, cy);
        self.place.ok = false;
        self.place.why = None;
        self.place.last = None;
        self.place.kbd = kbd;
        self.place.enter = false;
        self.place.action = ClickAction::None;
        None
    }

    /// `zoneBlock(f, kind, cx, cy)` against the mirror (castle footprint centre / owned outposts).
    pub fn zone_block(&self, kind: BuildingKind, cx: i32, cy: i32) -> Option<String> {
        let d = kind.def();
        let me = self.me()?;
        let (x, y) = (cx as f32 + d.w as f32 / 2.0, cy as f32 + d.h as f32 / 2.0);
        let r = if d.defense {
            DEFENSE_RADIUS
        } else {
            BUILD_RADIUS
        };
        if let Some(c) = self.own_castle() {
            let cd = c.kind.def();
            let (ccx, ccy) = (
                c.cx as f32 + cd.w as f32 / 2.0,
                c.cy as f32 + cd.h as f32 / 2.0,
            );
            if (x - ccx).hypot(y - ccy) <= r {
                return None;
            }
        }
        for p in &self.mirror.view.points {
            if p.owner == Some(me)
                && p.kind.def().zone
                && let Some(info) = self.scene.points.get(p.id.0 as usize)
                && (x - info.g.x).hypot(y - info.g.y) <= OUTPOST_RADIUS
            {
                return None;
            }
        }
        Some(cmd::zone_refusal(d.defense))
    }

    /// `why(cx, cy)` of the placement step (05 §9.2).
    pub fn place_why(&self, cx: i32, cy: i32) -> Option<String> {
        let kind = self.place.kind()?;
        let d = kind.def();
        match self.place.mode {
            PlaceMode::None => None,
            PlaceMode::Move(b) => {
                if kind == BuildingKind::Castle {
                    return Some(cmd::CASTELLO_NON_SI_SPOSTA.to_string());
                }
                if let Some(z) = self.zone_block(kind, cx, cy) {
                    return Some(z);
                }
                let own = building_cells(&self.mirror, b);
                if self.scene.can_place(kind, cx, cy, &own) {
                    None
                } else {
                    Some(cmd::QUI_NON_PUO_STARE.to_string())
                }
            }
            PlaceMode::Place(_) => {
                let afford = self
                    .mirror
                    .view
                    .economy
                    .as_ref()
                    .is_some_and(|e| {
                        ts_sim::defs::Res::ALL
                            .iter()
                            .all(|r| e.res[*r as usize] as u64 >= d.cost.get(*r) as u64)
                    });
                if !afford {
                    return Some(cmd::RISORSE_INSUFFICIENTI.to_string());
                }
                if let Some(z) = self.zone_block(kind, cx, cy) {
                    return Some(z);
                }
                let seen = knows(
                    &self.mirror,
                    (cx as f32 + d.w as f32 / 2.0) * TILE,
                    (cy as f32 + d.h as f32 / 2.0) * TILE,
                );
                if self.scene.can_place(kind, cx, cy, &[]) && seen {
                    None
                } else {
                    Some(cmd::QUI_NON_SI_PUO_COSTRUIRE.to_string())
                }
            }
        }
    }

    /// `builders(kind, cx, cy)` (05 §9.3).
    pub fn builders(&self, kind: BuildingKind, cx: i32, cy: i32) -> Vec<UnitId> {
        let sel: Vec<UnitId> = self
            .sel
            .mine(&self.mirror)
            .into_iter()
            .filter(|id| self.unit(*id).is_some_and(|u| u.kind == UnitKind::Pawn))
            .collect();
        if !sel.is_empty() {
            return sel;
        }
        let d = kind.def();
        let c = Vec2::new(
            (cx as f32 + d.w as f32 / 2.0) * TILE,
            (cy as f32 + d.h as f32 / 2.0) * TILE,
        );
        let pawns: Vec<&UnitView> = self
            .own_units()
            .filter(|u| u.kind == UnitKind::Pawn)
            .collect();
        if kind == BuildingKind::Wall
            && pawns
                .iter()
                .any(|u| u.act == Act::Build && Vec2::new(u.x, u.y).distance(c) < 600.0)
        {
            // they chain to the next palisade site by themselves
            return Vec::new();
        }
        let by_dist = |acts: &[Act]| -> Vec<(f32, UnitId)> {
            let mut v: Vec<(f32, UnitId)> = pawns
                .iter()
                .filter(|u| acts.contains(&u.act))
                .map(|u| (Vec2::new(u.x, u.y).distance(c), u.id))
                .collect();
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            v
        };
        let mut list = by_dist(&[Act::Idle, Act::Carry]);
        list.extend(by_dist(&[
            Act::ChopWood,
            Act::MineGold,
            Act::Butcher,
            Act::MineStone,
        ]));
        let n = if d.work > 15.0 { 2 } else { 1 };
        list.into_iter().take(n).map(|(_, id)| id).collect()
    }

    /// Sends the `place` / `move` command for the current ghost; returns the refusal, if any.
    pub fn place_confirm(&mut self) -> Option<String> {
        let IVec2 { x: cx, y: cy } = self.place.cell;
        if let Some(why) = self.place_why(cx, cy) {
            return Some(why);
        }
        match self.place.mode {
            PlaceMode::None => None,
            PlaceMode::Move(b) => {
                self.net.command(Command::Move { building: b, cx, cy });
                None
            }
            PlaceMode::Place(kind) => {
                let builders = self.builders(kind, cx, cy);
                self.net.command(Command::Place {
                    kind,
                    cx,
                    cy,
                    builders,
                });
                None
            }
        }
    }

    /// `keyMove(dx, dy)` (05 §9.4 placing, §10 pick-move).
    pub fn key_move(&mut self, dx: i32, dy: i32) {
        let map_w = self.mirror.map().map_or(ts_sim::MAP_W as i32, |m| m.w);
        let map_h = self.mirror.map().map_or(ts_sim::MAP_H as i32, |m| m.h);
        if let Some(kind) = self.place.kind().filter(|_| self.place.placing()) {
            let d = kind.def();
            self.place.kbd = true;
            self.place.cell.x = (self.place.cell.x + dx).clamp(0, (map_w - d.w).max(0));
            self.place.cell.y = (self.place.cell.y + dy).clamp(0, (map_h - d.h).max(0));
            let c = Vec2::new(
                (self.place.cell.x as f32 + d.w as f32 / 2.0) * TILE,
                (self.place.cell.y as f32 + d.h as f32 / 2.0) * TILE,
            );
            // recentre when the ghost leaves the inner 40 % of the view
            let map = self.mirror.map();
            let inside = self.view.project_px(map, c.x, c.y, 0.0).is_some_and(|p| {
                let r = self.view.viewport;
                let (mx, my) = (r.width() * 0.3, r.height() * 0.3);
                p.x >= r.min.x + mx && p.x <= r.max.x - mx && p.y >= r.min.y + my && p.y <= r.max.y - my
            });
            if !inside {
                self.look_at(c);
            }
            return;
        }
        if self.place.action != ClickAction::PickMove {
            return;
        }
        let me = self.me();
        let cur = self
            .place
            .pick
            .and_then(|b| self.mirror.view.buildings.get(&b))
            .map(|b| (b.id, building_foot(b)));
        let origin = cur.map_or(self.view.center_px, |(_, p)| p);
        let (fx, fy) = (dx as f32, dy as f32);
        let mut best: Option<(f32, BuildingId, Vec2)> = None;
        for b in self.mirror.view.buildings.values() {
            if Some(b.faction) != me
                || b.kind == BuildingKind::Castle
                || self.mirror.view.ghosts.contains(&b.id)
                || cur.is_some_and(|(id, _)| id == b.id)
            {
                continue;
            }
            let p = building_foot(b);
            let along = (p.x - origin.x) * fx + (p.y - origin.y) * fy;
            let across = ((p.x - origin.x) * fy).abs() + ((p.y - origin.y) * fx).abs();
            if cur.is_some() && along < 8.0 {
                continue;
            }
            let d = if cur.is_some() {
                along + across * 2.0
            } else {
                p.distance(origin)
            };
            if best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, b.id, p));
            }
        }
        if let Some((_, id, p)) = best {
            self.place.pick = Some(id);
            self.look_at(p - Vec2::new(0.0, 60.0));
        }
    }

    /// `keyConfirm()`.
    pub fn key_confirm(&mut self) {
        if self.place.placing() {
            self.place.enter = true;
            return;
        }
        if self.place.action == ClickAction::PickMove {
            let b = self
                .place
                .pick
                .filter(|b| self.mirror.view.buildings.contains_key(b));
            let why = match b {
                Some(b) => self.start_moving(b, true),
                None => Some("Scegli un edificio con le frecce".to_string()),
            };
            self.say(why);
        }
    }

    // ------------------------------------------------------------------------------------------- cursor

    /// `Ctl.cursor()` (05 §8.4) for the world under the window point `s`.
    pub fn cursor(&self, s: Option<Vec2>, g: Option<Vec2>) -> Cursor {
        if self.place.placing() {
            return if self.place.ok {
                Cursor::Arrow
            } else {
                Cursor::No
            };
        }
        match self.place.action {
            ClickAction::AttackMove => return Cursor::Attack,
            ClickAction::PickMove => {
                let me = self.me();
                let hit = s.and_then(|s| self.pick_at(s, g));
                let own_bld = matches!(hit, Some(EntityRef::Building(b))
                    if self.mirror.view.buildings.get(&b).is_some_and(|v| Some(v.faction) == me && v.kind != BuildingKind::Castle));
                return if own_bld { Cursor::Hand } else { Cursor::Arrow };
            }
            _ => {}
        }
        let own = self.sel.mine(&self.mirror);
        let Some(s) = s.filter(|_| !own.is_empty()) else {
            return Cursor::Arrow;
        };
        let Some(t) = self.pick_at(s, g) else {
            return Cursor::Arrow;
        };
        let me = self.me();
        let pawn = own
            .iter()
            .any(|id| self.unit(*id).is_some_and(|u| u.kind == UnitKind::Pawn));
        let any_dmg = own
            .iter()
            .any(|id| self.unit(*id).is_some_and(|u| u.kind.def().dmg > 0.0));
        match t {
            EntityRef::Unit(id) if self.unit(id).is_some_and(|u| Some(u.faction) != me) => {
                if any_dmg { Cursor::Attack } else { Cursor::Arrow }
            }
            EntityRef::Building(id) => {
                let Some(b) = self.mirror.view.buildings.get(&id) else {
                    return Cursor::Arrow;
                };
                if Some(b.faction) != me {
                    if any_dmg { Cursor::Attack } else { Cursor::Arrow }
                } else if pawn && (!b.done || b.hp < b.max_hp) {
                    Cursor::Hammer
                } else {
                    Cursor::Arrow
                }
            }
            EntityRef::Node(n) if pawn => match self.scene.nodes.get(n.0 as usize).map(|n| n.kind) {
                Some(NodeKind::Tree) => Cursor::Axe,
                Some(NodeKind::Gold) | Some(NodeKind::Stone) => Cursor::Pick,
                None => Cursor::Arrow,
            },
            EntityRef::Sheep(_) if pawn => Cursor::Knife,
            _ => Cursor::Arrow,
        }
    }

    /// Is a placed palisade cell sequence allowed (wall drag uses the same `why`).
    pub fn wall_line(&mut self, from: IVec2, to: IVec2) {
        let n = (to.x - from.x).abs().max((to.y - from.y).abs());
        let mine_sel = self
            .sel
            .mine(&self.mirror)
            .iter()
            .any(|id| self.unit(*id).is_some_and(|u| u.kind == UnitKind::Pawn));
        for i in 1..=n {
            let k = i as f32 / n as f32;
            let cx = js_round(from.x as f32 + (to.x - from.x) as f32 * k);
            let cy = js_round(from.y as f32 + (to.y - from.y) as f32 * k);
            if self.place_why(cx, cy).is_none() {
                let builders = if mine_sel {
                    self.builders(BuildingKind::Wall, cx, cy)
                } else {
                    Vec::new()
                };
                self.net.command(Command::Place {
                    kind: BuildingKind::Wall,
                    cx,
                    cy,
                    builders,
                });
            }
        }
    }

    /// Solo match (test mode, pause semantics).
    pub fn solo(&self) -> bool {
        !self.lobby.remote
    }
}
