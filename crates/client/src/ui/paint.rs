//! Immediate-mode painter (spec 06 §0: the JS UI is immediate mode, redrawn every frame).
//!
//! Every frame the UI code emits a list of primitives (`Prim`: images with a source rect, solid rects, labels) in
//! draw order, with the same formulas as the canvas version. `sync_pool` maps them onto a pool of absolutely
//! positioned bevy_ui nodes (one root node per primitive, `GlobalZIndex` = draw order) and only touches a node's
//! components when its primitive changed, so a static HUD costs no layout work.
//!
//! Interaction follows the JS `UI` object (§3): `hit()` registers a rectangle, returns hover/down/clicked, feeds the
//! keyboard focus navigation of the next frame and the "pointer over UI" flag.

use crate::assets::GameAssets;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::text::{FontCx, FontSource, FontWeight, LayoutCx, LineBreak};
use bevy::ui::UiTransform;

/// First `GlobalZIndex` used by the UI primitives (world-space overlays of other modules stay below).
pub const Z_BASE: i32 = 1000;
/// Text outline colour `#1b1f33`.
pub const OUTLINE: Color = Color::srgb(0.106, 0.122, 0.2);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// One draw call.
#[derive(Clone, Debug)]
pub enum Prim {
    Img {
        img: Handle<Image>,
        src: Option<Rect>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
        flip: bool,
        rot: f32,
    },
    Txt {
        s: String,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        align: Align,
        shadow: Option<(Vec2, Color)>,
    },
}

/// Cursor shapes (§12).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum CursorKind {
    #[default]
    Arrow,
    Hand,
    No,
    Attack,
    Axe,
    Pick,
    Knife,
    Hammer,
}

/// Tooltip descriptor (`UI.tip`).
#[derive(Clone, Debug, Default)]
pub struct Tip {
    pub x: f32,
    pub y: f32,
    pub below: bool,
    pub title: String,
    pub key: Option<String>,
    pub desc: String,
    pub cost: Option<ts_sim::defs::Cost>,
    pub lock: Option<String>,
}

/// Result of `hit()`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hit {
    pub hover: bool,
    pub down: bool,
    pub clicked: bool,
}

/// Raw input of this frame (logical pixels, origin top-left).
#[derive(Clone, Debug, Default)]
pub struct Io {
    pub w: f32,
    pub h: f32,
    pub mouse: Vec2,
    pub has_mouse: bool,
    /// left button held
    pub lmb: bool,
    /// left press edge
    pub pressed: bool,
    pub released: bool,
    pub rpressed: bool,
    pub moved: bool,
    pub shift: bool,
    pub dt: f32,
    /// real time since start (s)
    pub clock: f64,
}

/// Painter + immediate-mode interaction state (JS `UI` minus the game fields that live in `UiState`).
#[derive(Resource, Default)]
pub struct Paint {
    pub prims: Vec<Prim>,
    /// `ctx.globalAlpha`
    pub alpha: f32,
    pub hot: bool,
    hot_acc: bool,
    pub locked: bool,
    pub cursor: CursorKind,
    pub tip: Option<Tip>,
    pub kbd: bool,
    pub focus: Option<Vec2>,
    pub focus_rect: Option<Rect>,
    rects: Vec<Rect>,
    last: Vec<Rect>,
    pub nav: Option<(f32, f32)>,
    pub enter: bool,
    pub io: Io,
    /// text width cache: (text, size*8) → width
    widths: HashMap<(String, u32), f32>,
}

impl Paint {
    /// `UI.begin()`.
    pub fn begin(&mut self, io: Io) {
        self.prims.clear();
        self.alpha = 1.0;
        self.hot_acc = false;
        self.cursor = CursorKind::Arrow;
        self.tip = None;
        self.locked = false;
        self.last = core::mem::take(&mut self.rects);
        self.focus_rect = None;
        self.io = io;
        if let Some((dx, dy)) = self.nav.take() {
            self.step(dx, dy);
        }
        if self.widths.len() > 6000 {
            self.widths.clear();
        }
    }

    /// `UI.end()`.
    pub fn end(&mut self) {
        self.hot = self.hot_acc;
        self.enter = false;
    }

    pub fn set_hot(&mut self) {
        self.hot_acc = true;
    }

    pub fn is_hot(&self) -> bool {
        self.hot_acc
    }

    /// `UI.over`.
    pub fn over(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        let m = self.io.mouse;
        !self.locked && self.io.has_mouse && m.x >= x && m.x < x + w && m.y >= y && m.y < y + h
    }

    /// `UI.area`: blocks world input under panels.
    pub fn area(&mut self, x: f32, y: f32, w: f32, h: f32) {
        if self.over(x, y, w, h) {
            self.hot_acc = true;
        }
    }

    /// `UI.hit`.
    pub fn hit(&mut self, x: f32, y: f32, w: f32, h: f32) -> Hit {
        let hover = if self.kbd {
            let hv = !self.locked
                && self
                    .focus
                    .is_some_and(|f| f.x >= x && f.x < x + w && f.y >= y && f.y < y + h);
            if hv {
                self.focus_rect = Some(Rect::new(x, y, x + w, y + h));
            }
            hv
        } else {
            self.over(x, y, w, h)
        };
        if !self.locked {
            self.rects.push(Rect::new(x, y, x + w, y + h));
        }
        if hover {
            self.hot_acc = true;
            self.cursor = CursorKind::Hand;
        }
        Hit {
            hover,
            down: hover && !self.kbd && self.io.lmb,
            clicked: hover && if self.kbd { self.enter } else { self.io.pressed },
        }
    }

    /// `UI.step`: move the keyboard focus to the nearest button in direction (dx, dy).
    fn step(&mut self, dx: f32, dy: f32) {
        if self.last.is_empty() {
            return;
        }
        let mid = |r: &Rect| r.center();
        let cur = self.focus.and_then(|f| {
            self.last
                .iter()
                .copied()
                .find(|r| f.x >= r.min.x && f.x < r.max.x && f.y >= r.min.y && f.y < r.max.y)
        });
        let mut best: Option<Rect> = None;
        let mut bd = f32::INFINITY;
        for r in &self.last {
            let p = mid(r);
            let d = match cur {
                None => (p - Vec2::new(self.io.w / 2.0, self.io.h / 2.0)).length(),
                Some(c) => {
                    let p0 = mid(&c);
                    let along = (p.x - p0.x) * dx + (p.y - p0.y) * dy;
                    let across = ((p.x - p0.x) * dy).abs() + ((p.y - p0.y) * dx).abs();
                    if *r == c || along < 4.0 || across > along * 2.0 {
                        continue;
                    }
                    along + across * 1.5
                }
            };
            if d < bd {
                bd = d;
                best = Some(*r);
            }
        }
        if let Some(b) = best {
            self.focus = Some(mid(&b));
        }
    }

    // ------------------------------------------------------------------ primitives

    fn a(&self, c: Color) -> Color {
        if self.alpha >= 1.0 {
            c
        } else {
            c.with_alpha(c.alpha() * self.alpha)
        }
    }

    /// Image `src` (or the whole image) drawn into `(x, y, w, h)`.
    #[allow(clippy::too_many_arguments)]
    pub fn image(
        &mut self,
        img: &Handle<Image>,
        src: Option<Rect>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        tint: Color,
    ) {
        if w <= 0.0 || h <= 0.0 || self.alpha <= 0.0 {
            return;
        }
        let color = self.a(tint);
        self.prims.push(Prim::Img {
            img: img.clone(),
            src,
            x,
            y,
            w,
            h,
            color,
            flip: false,
            rot: 0.0,
        });
    }

    /// Rotated image (radians, around its centre).
    #[allow(clippy::too_many_arguments)]
    pub fn image_rot(
        &mut self,
        img: &Handle<Image>,
        src: Option<Rect>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        tint: Color,
        rot: f32,
        flip: bool,
    ) {
        if w <= 0.0 || h <= 0.0 || self.alpha <= 0.0 {
            return;
        }
        let color = self.a(tint);
        self.prims.push(Prim::Img {
            img: img.clone(),
            src,
            x,
            y,
            w,
            h,
            color,
            flip,
            rot,
        });
    }

    /// Solid rectangle.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.image(&Handle::default(), None, x, y, w, h, color);
    }

    /// Line segment of width `lw` (rotated thin rect).
    pub fn line(&mut self, a: Vec2, b: Vec2, lw: f32, color: Color) {
        let d = b - a;
        let len = d.length();
        if len < 0.01 {
            return;
        }
        let c = (a + b) * 0.5;
        let rot = d.y.atan2(d.x);
        self.image_rot(
            &Handle::default(),
            None,
            c.x - len / 2.0,
            c.y - lw / 2.0,
            len,
            lw,
            color,
            rot,
            false,
        );
    }

    /// `label(ctx, str, x, y, size, color, align, outline)`: `y` is the vertical centre of the text.
    #[allow(clippy::too_many_arguments)]
    pub fn label(
        &mut self,
        s: impl Into<String>,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        align: Align,
        outline: Option<Color>,
    ) {
        if self.alpha <= 0.0 || size <= 0.5 {
            return;
        }
        let s: String = s.into();
        if s.is_empty() {
            return;
        }
        if let Some(oc) = outline {
            // canvas stroke of width max(2, size/5): four diagonal copies (two nodes with a shadow each)
            let o = (size / 10.0).max(1.0);
            let oc = self.a(oc);
            for dy in [-o, o] {
                self.prims.push(Prim::Txt {
                    s: s.clone(),
                    x: x - o,
                    y: y + dy,
                    size,
                    color: oc,
                    align,
                    shadow: Some((Vec2::new(2.0 * o, 0.0), oc)),
                });
            }
        }
        let color = self.a(color);
        self.prims.push(Prim::Txt {
            s,
            x,
            y,
            size,
            color,
            align,
            shadow: None,
        });
    }

    /// `dim(ctx, a)`: full-screen `rgba(16,20,36,a)` + hot.
    pub fn dim(&mut self, a: f32) {
        let (w, h) = (self.io.w, self.io.h);
        self.rect(0.0, 0.0, w, h, Color::srgba(16.0 / 255.0, 20.0 / 255.0, 36.0 / 255.0, a));
        self.hot_acc = true;
    }
}

// ---------------------------------------------------------------------------------------------- text measurement

/// Synchronous text measurement with the same font stack bevy_text uses (parley), cached.
pub struct Measure<'a> {
    pub fcx: &'a mut FontCx,
    pub lcx: &'a mut LayoutCx,
    pub fonts: &'a Assets<Font>,
    pub font: Handle<Font>,
}

impl Measure<'_> {
    fn raw(&mut self, s: &str, size: f32) -> Option<f32> {
        let src = FontSource::Handle(self.font.clone());
        let family = src.resolve_font_family(self.fonts).ok()?;
        let mut b = self.lcx.0.ranged_builder(&mut self.fcx.context, s, 1.0, true);
        b.push_default(parley::StyleProperty::FontFamily(family));
        b.push_default(parley::StyleProperty::FontSize(size));
        b.push_default(parley::StyleProperty::FontWeight(FontWeight(600).into()));
        let mut layout = b.build(s);
        layout.break_all_lines(None);
        Some(layout.width())
    }
}

impl Paint {
    /// `textWidth(ctx, str, size)` (logical px).
    pub fn text_width(&mut self, m: &mut Measure, s: &str, size: f32) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        let key = (s.to_string(), (size * 8.0).round() as u32);
        if let Some(w) = self.widths.get(&key) {
            return *w;
        }
        match m.raw(s, size) {
            Some(w) => {
                self.widths.insert(key, w);
                w
            }
            // font not registered yet: rough estimate, not cached
            None => s.chars().count() as f32 * size * 0.55,
        }
    }

    /// `wrapText(ctx, str, width, size)`: greedy wrap on single spaces.
    pub fn wrap(&mut self, m: &mut Measure, s: &str, width: f32, size: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in s.split(' ') {
            let next = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && self.text_width(m, &next, size) > width {
                lines.push(core::mem::replace(&mut line, word.to_string()));
            } else {
                line = next;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        lines
    }
}

// ---------------------------------------------------------------------------------------------- node pool

#[derive(Clone, PartialEq, Debug)]
struct ImgState {
    img: AssetId<Image>,
    src: Option<Rect>,
    r: [f32; 4],
    color: Color,
    flip: bool,
    rot: f32,
    z: i32,
}

#[derive(Clone, PartialEq, Debug)]
struct TxtState {
    s: String,
    pos: [f32; 3],
    size: f32,
    color: Color,
    align: Align,
    shadow: Option<(Vec2, Color)>,
    z: i32,
}

#[derive(Component, Default)]
pub struct ImgSlot {
    state: Option<ImgState>,
    shown: bool,
}

#[derive(Component, Default)]
pub struct TxtSlot {
    state: Option<TxtState>,
    shown: bool,
}

/// Marks text slots that carry a `TextShadow` (outline halves).
#[derive(Component)]
pub struct ShadowSlot;

#[derive(Resource, Default)]
pub struct Pool {
    imgs: Vec<Entity>,
    txts: Vec<Entity>,
    shadows: Vec<Entity>,
    pub font: Handle<Font>,
}

/// Width of the box used to centre / right-align a label.
const ALIGN_BOX: f32 = 4000.0;

fn txt_node(x: f32, y: f32, size: f32, align: Align) -> (f32, f32, Val) {
    let top = (y - size * 0.62).round();
    match align {
        Align::Left => (x.round(), top, Val::Auto),
        Align::Center => ((x - ALIGN_BOX / 2.0).round(), top, px(ALIGN_BOX)),
        Align::Right => ((x - ALIGN_BOX).round(), top, px(ALIGN_BOX)),
    }
}

fn justify(a: Align) -> Justify {
    match a {
        Align::Left => Justify::Left,
        Align::Center => Justify::Center,
        Align::Right => Justify::Right,
    }
}

fn text_bundle(font: &Handle<Font>, st: &TxtState) -> impl Bundle {
    let (l, t, w) = txt_node(st.pos[0], st.pos[1], st.size, st.align);
    (
        TxtSlot {
            state: Some(st.clone()),
            shown: true,
        },
        Node {
            position_type: PositionType::Absolute,
            left: px(l),
            top: px(t),
            width: w,
            ..default()
        },
        Text::new(st.s.clone()),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(st.size),
            weight: FontWeight(600),
            ..default()
        },
        TextColor(st.color),
        TextLayout::new(justify(st.align), LineBreak::NoWrap),
        GlobalZIndex(st.z),
        Pickable::IGNORE,
    )
}

fn img_bundle(st: &ImgState, img: Handle<Image>) -> impl Bundle {
    (
        ImgSlot {
            state: Some(st.clone()),
            shown: true,
        },
        Node {
            position_type: PositionType::Absolute,
            left: px(st.r[0]),
            top: px(st.r[1]),
            width: px(st.r[2]),
            height: px(st.r[3]),
            ..default()
        },
        ImageNode {
            image: img,
            rect: st.src,
            color: st.color,
            flip_x: st.flip,
            image_mode: NodeImageMode::Stretch,
            ..default()
        },
        UiTransform {
            rotation: Rot2::radians(st.rot),
            ..default()
        },
        GlobalZIndex(st.z),
        Pickable::IGNORE,
    )
}

type ImgQ<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut ImgSlot,
        &'static mut Node,
        &'static mut ImageNode,
        &'static mut UiTransform,
        &'static mut GlobalZIndex,
    ),
    Without<TxtSlot>,
>;

type TxtQ<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut TxtSlot,
        &'static mut Node,
        &'static mut Text,
        &'static mut TextFont,
        &'static mut TextColor,
        &'static mut TextLayout,
        &'static mut GlobalZIndex,
        Option<&'static mut TextShadow>,
    ),
    Without<ImgSlot>,
>;

#[allow(clippy::too_many_arguments)]
fn apply_txt(
    e: Entity,
    st: TxtState,
    q: &mut TxtQ,
) -> bool {
    let Ok((mut slot, mut node, mut text, mut font, mut color, mut layout, mut z, shadow)) =
        q.get_mut(e)
    else {
        return false;
    };
    if !slot.shown {
        slot.shown = true;
        node.display = Display::Flex;
    }
    if slot.state.as_ref() == Some(&st) {
        return true;
    }
    let old = slot.state.take();
    if old.as_ref().map(|o| &o.s) != Some(&st.s) {
        text.0.clone_from(&st.s);
    }
    if old.as_ref().map(|o| (o.pos, o.size, o.align)) != Some((st.pos, st.size, st.align)) {
        let (l, t, w) = txt_node(st.pos[0], st.pos[1], st.size, st.align);
        node.left = px(l);
        node.top = px(t);
        node.width = w;
    }
    if old.as_ref().map(|o| o.size) != Some(st.size) {
        font.font_size = FontSize::Px(st.size);
    }
    if old.as_ref().map(|o| o.align) != Some(st.align) {
        layout.justify = justify(st.align);
    }
    if old.as_ref().map(|o| o.color) != Some(st.color) {
        color.0 = st.color;
    }
    if old.as_ref().map(|o| o.z) != Some(st.z) {
        z.0 = st.z;
    }
    if let (Some(mut sh), Some((off, c))) = (shadow, st.shadow)
        && (sh.offset != off || sh.color != c)
    {
        sh.offset = off;
        sh.color = c;
    }
    slot.state = Some(st);
    true
}

/// Maps this frame's primitives onto the node pool.
pub fn sync_pool(
    mut commands: Commands,
    paint: Res<Paint>,
    mut pool: ResMut<Pool>,
    mut imgs: ImgQ,
    mut txts: TxtQ,
) {
    let (mut ni, mut nt, mut ns) = (0usize, 0usize, 0usize);
    let font = pool.font.clone();
    for (i, p) in paint.prims.iter().enumerate() {
        let z = Z_BASE + i as i32;
        match p {
            Prim::Img {
                img,
                src,
                x,
                y,
                w,
                h,
                color,
                flip,
                rot,
            } => {
                let st = ImgState {
                    img: img.id(),
                    src: *src,
                    r: [x.round(), y.round(), w.round().max(1.0), h.round().max(1.0)],
                    color: *color,
                    flip: *flip,
                    rot: *rot,
                    z,
                };
                if ni >= pool.imgs.len() {
                    let e = commands.spawn(img_bundle(&st, img.clone())).id();
                    pool.imgs.push(e);
                } else {
                    let e = pool.imgs[ni];
                    if let Ok((mut slot, mut node, mut im, mut tf, mut zi)) = imgs.get_mut(e) {
                        if !slot.shown {
                            slot.shown = true;
                            node.display = Display::Flex;
                        }
                        if slot.state.as_ref() != Some(&st) {
                            let old = slot.state.take();
                            if old.as_ref().map(|o| o.r) != Some(st.r) {
                                node.left = px(st.r[0]);
                                node.top = px(st.r[1]);
                                node.width = px(st.r[2]);
                                node.height = px(st.r[3]);
                            }
                            if old.as_ref().map(|o| (o.img, o.src, o.color, o.flip))
                                != Some((st.img, st.src, st.color, st.flip))
                            {
                                if im.image.id() != st.img {
                                    im.image = img.clone();
                                }
                                im.rect = st.src;
                                im.color = st.color;
                                im.flip_x = st.flip;
                            }
                            if old.as_ref().map(|o| o.rot) != Some(st.rot) {
                                tf.rotation = Rot2::radians(st.rot);
                            }
                            if old.as_ref().map(|o| o.z) != Some(st.z) {
                                zi.0 = st.z;
                            }
                            slot.state = Some(st);
                        }
                    }
                }
                ni += 1;
            }
            Prim::Txt {
                s,
                x,
                y,
                size,
                color,
                align,
                shadow,
            } => {
                let st = TxtState {
                    s: s.clone(),
                    pos: [*x, *y, 0.0],
                    size: *size,
                    color: *color,
                    align: *align,
                    shadow: *shadow,
                    z,
                };
                if let Some((off, c)) = shadow {
                    if ns >= pool.shadows.len() {
                        let e = commands
                            .spawn((
                                text_bundle(&font, &st),
                                ShadowSlot,
                                TextShadow {
                                    offset: *off,
                                    color: *c,
                                },
                            ))
                            .id();
                        pool.shadows.push(e);
                    } else {
                        apply_txt(pool.shadows[ns], st, &mut txts);
                    }
                    ns += 1;
                } else {
                    if nt >= pool.txts.len() {
                        let e = commands.spawn(text_bundle(&font, &st)).id();
                        pool.txts.push(e);
                    } else {
                        apply_txt(pool.txts[nt], st, &mut txts);
                    }
                    nt += 1;
                }
            }
        }
    }
    // hide the unused slots
    for &e in &pool.imgs[ni..] {
        if let Ok((mut slot, mut node, ..)) = imgs.get_mut(e)
            && slot.shown
        {
            slot.shown = false;
            node.display = Display::None;
        }
    }
    for &e in pool.txts[nt..].iter().chain(pool.shadows[ns..].iter()) {
        if let Ok((mut slot, mut node, ..)) = txts.get_mut(e)
            && slot.shown
        {
            slot.shown = false;
            node.display = Display::None;
        }
    }
}

/// Convenience: image handle by path relative to `assets/` (default handle when missing).
pub fn img(ga: &GameAssets, rel: &str) -> Handle<Image> {
    ga.image(rel).unwrap_or_default()
}
