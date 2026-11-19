//! Drawing primitives and widgets of the original UI (spec 06 §2: assets.js + ui.js:128-170), on top of `Paint`.

use super::paint::{Align, Hit, Measure, OUTLINE, Paint, Tip};
use crate::assets::GameAssets;
use bevy::prelude::*;
use ts_sim::defs::{
    AGE_SHORT, BuildingKind, Cost, KingdomColor, PointKind, Res, Sound, TechIcon, UnitKind,
};

pub const UIP: &str = "UI Elements/UI Elements/";
pub const STOREP: &str = "UI Elements/UI Banners from the store page/";

// ---- palette (§0.2) ----
pub const INK: Color = Color::srgb(0x4a as f32 / 255.0, 0x3a as f32 / 255.0, 0x33 as f32 / 255.0);
pub const RED: Color = Color::srgb(0xc0 as f32 / 255.0, 0x30 as f32 / 255.0, 0x2b as f32 / 255.0);
pub const ACCENT: Color = Color::srgb(0x8a as f32 / 255.0, 0x3b as f32 / 255.0, 0x2e as f32 / 255.0);
pub const GREEN: Color = Color::srgb(0x2f as f32 / 255.0, 0x7a as f32 / 255.0, 0x3a as f32 / 255.0);
pub const GREY: Color = Color::srgb(0x9a as f32 / 255.0, 0x8f as f32 / 255.0, 0x80 as f32 / 255.0);
pub const GOLD: Color = Color::srgb(1.0, 0xe2 as f32 / 255.0, 0x7a as f32 / 255.0);
pub const TOAST: Color = Color::srgb(1.0, 0xd9 as f32 / 255.0, 0xcf as f32 / 255.0);
pub const PARCH: Color = Color::srgb(0xf1 as f32 / 255.0, 0xe6 as f32 / 255.0, 0xc8 as f32 / 255.0);
pub const MUTED: Color = Color::srgb(0xd9 as f32 / 255.0, 0xcf as f32 / 255.0, 0xb0 as f32 / 255.0);
pub const DIMBLUE: Color = Color::srgb(0x9a as f32 / 255.0, 0xa3 as f32 / 255.0, 0xb5 as f32 / 255.0);
pub const ERA_ON: Color = Color::srgb(1.0, 0xe9 as f32 / 255.0, 0xa3 as f32 / 255.0);
pub const SALMON: Color = Color::srgb(0xe7 as f32 / 255.0, 0x9a as f32 / 255.0, 0x8a as f32 / 255.0);
pub const CREAM: Color = Color::srgb(0xf6 as f32 / 255.0, 0xea as f32 / 255.0, 0xd0 as f32 / 255.0);
pub const LIGHT_RED: Color = Color::srgb(1.0, 0x8a as f32 / 255.0, 0x7a as f32 / 255.0);
pub const HP_OK: Color = Color::srgb(0x7b as f32 / 255.0, 0xe0 as f32 / 255.0, 0x6a as f32 / 255.0);
pub const HP_LOW: Color = Color::srgb(1.0, 0x5a as f32 / 255.0, 0x4a as f32 / 255.0);
pub const WHITE: Color = Color::WHITE;
pub const DARK: Color = OUTLINE;

/// `COLOR_CSS[color]`.
pub fn faction_color(c: KingdomColor) -> Color {
    let [r, g, b] = c.rgb();
    Color::srgb_u8(r, g, b)
}

/// Slices `[srcX, srcW]` (same for x and y).
pub type Slices = [(f32, f32); 3];
pub const G3: Slices = [(0.0, 64.0), (128.0, 64.0), (256.0, 64.0)];
pub const G7: Slices = [(0.0, 128.0), (192.0, 64.0), (320.0, 128.0)];
pub const G1: Slices = [(0.0, 64.0), (64.0, 64.0), (128.0, 64.0)];

/// `PANEL` table (§2.3).
#[derive(Clone, Copy, Debug)]
pub struct PanelDef {
    pub path: &'static str,
    pub xs: Slices,
    pub pad: [f32; 4],
}

pub mod panel {
    use super::*;
    pub const BANNER: PanelDef = PanelDef {
        path: "UI Elements/UI Elements/Banners/Banner.png",
        xs: G7,
        pad: [28.0, 60.0, 44.0, 17.0],
    };
    pub const BANNER_SLOTS: PanelDef = PanelDef {
        path: "UI Elements/UI Elements/Banners/Banner_Slots.png",
        xs: G1,
        pad: [5.0, 5.0, 2.0, 4.0],
    };
    pub const WOOD: PanelDef = PanelDef {
        path: "UI Elements/UI Elements/Wood Table/WoodTable.png",
        xs: G7,
        pad: [44.0, 43.0, 44.0, 25.0],
    };
    pub const WOOD_SLOTS: PanelDef = PanelDef {
        path: "UI Elements/UI Elements/Wood Table/WoodTable_Slots.png",
        xs: G1,
        pad: [12.0, 11.0, 12.0, 11.0],
    };
    pub const PAPER: PanelDef = PanelDef {
        path: "UI Elements/UI Elements/Papers/RegularPaper.png",
        xs: G3,
        pad: [12.0, 20.0, 12.0, 19.0],
    };
    pub const SPECIAL: PanelDef = PanelDef {
        path: "UI Elements/UI Elements/Papers/SpecialPaper.png",
        xs: G3,
        pad: [9.0, 20.0, 9.0, 21.0],
    };
    pub const STORE_SLOTS: PanelDef = PanelDef {
        path: "UI Elements/UI Banners from the store page/Banner/Slots.png",
        xs: G3,
        pad: [43.0, 8.0, 40.0, 17.0],
    };
    pub const BIG_BLUE: PanelDef = PanelDef {
        path: "UI Elements/UI Elements/Buttons/BigBlueButton_Regular.png",
        xs: G3,
        pad: [19.0, 17.0, 19.0, 17.0],
    };
    pub const BIG_BLUE_DOWN: PanelDef = PanelDef {
        path: "UI Elements/UI Elements/Buttons/BigBlueButton_Pressed.png",
        xs: G3,
        pad: [19.0, 17.0, 19.0, 17.0],
    };
    pub const BIG_RED: PanelDef = PanelDef {
        path: "UI Elements/UI Elements/Buttons/BigRedButton_Regular.png",
        xs: G3,
        pad: [19.0, 17.0, 19.0, 17.0],
    };
    pub const BIG_RED_DOWN: PanelDef = PanelDef {
        path: "UI Elements/UI Elements/Buttons/BigRedButton_Pressed.png",
        xs: G3,
        pad: [19.0, 17.0, 19.0, 17.0],
    };
}

/// Icon "spec" (`drawSpec`, §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSpec {
    Ui(u8),
    X(&'static str),
    Res(Res),
    Unit(UnitKind),
    Build(BuildingKind),
    Age(u8),
}

impl From<TechIcon> for IconSpec {
    fn from(t: TechIcon) -> Self {
        match t {
            TechIcon::Age(n) => IconSpec::Age(n),
            TechIcon::X(n) => IconSpec::X(n),
        }
    }
}

pub fn ui_icon(n: u8) -> String {
    format!("{UIP}Icons/Icon_{n:02}.png")
}

pub fn x_icon(name: &str) -> String {
    format!("Extra/Icons/{name}.png")
}

/// `RES_ICON[k]`.
pub fn res_icon(r: Res) -> String {
    match r {
        Res::Wood => ui_icon(2),
        Res::Gold => ui_icon(3),
        Res::Meat => ui_icon(4),
        Res::Stone => x_icon("stone"),
    }
}

/// Idle (portrait) sheet of a unit (spec 01 §2.5).
pub fn unit_idle_sheet(c: KingdomColor, k: UnitKind) -> String {
    let ck = c.key();
    match k {
        UnitKind::Pawn => format!("Units/{ck} Units/Pawn/Pawn_Idle.png"),
        UnitKind::Warrior => format!("Units/{ck} Units/Warrior/Warrior_Idle.png"),
        UnitKind::Lancer => format!("Units/{ck} Units/Lancer/Lancer_Idle.png"),
        UnitKind::Archer => format!("Units/{ck} Units/Archer/Archer_Idle.png"),
        UnitKind::Monk => format!("Units/{ck} Units/Monk/Idle.png"),
        UnitKind::Scout => format!("Extra/Units/{ck}/Scout_Idle.png"),
        UnitKind::Knight => format!("Extra/Units/{ck}/Knight_Idle.png"),
        UnitKind::Ram => format!("Extra/Units/{ck}/Ram_Run.png"),
        UnitKind::Catapult => format!("Extra/Units/{ck}/Catapult_Run.png"),
    }
}

/// `buildingImg(color, kind, variant)`.
pub fn building_img(c: KingdomColor, k: BuildingKind, variant: u8) -> String {
    let d = k.def();
    let file = if k == BuildingKind::House {
        format!("{}{}", d.file, variant.min(2) + 1)
    } else {
        d.file.to_string()
    };
    if d.neutral {
        format!("Extra/Buildings/{file}.png")
    } else if d.extra {
        format!("Extra/Buildings/{}/{file}.png", c.key())
    } else {
        format!("Buildings/{} Buildings/{file}.png", c.key())
    }
}

pub fn point_img(k: PointKind) -> String {
    format!("Extra/Buildings/{}.png", k.def().sprite)
}

/// Frame-scoped UI context: painter, text measurement, assets, queued UI sounds.
pub struct Ui<'a> {
    pub p: &'a mut Paint,
    pub m: Measure<'a>,
    pub ga: &'a GameAssets,
    pub sounds: &'a mut Vec<Sound>,
}

/// smallButton result (with the press offset `dy`).
#[derive(Clone, Copy, Debug, Default)]
pub struct SmallHit {
    pub hover: bool,
    pub down: bool,
    pub clicked: bool,
    pub dy: f32,
}

impl Ui<'_> {
    pub fn w(&self) -> f32 {
        self.p.io.w
    }
    pub fn h(&self) -> f32 {
        self.p.io.h
    }

    pub fn play(&mut self, s: Sound) {
        self.sounds.push(s);
    }

    pub fn tw(&mut self, s: &str, size: f32) -> f32 {
        self.p.text_width(&mut self.m, s, size)
    }

    pub fn wrap(&mut self, s: &str, width: f32, size: f32) -> Vec<String> {
        self.p.wrap(&mut self.m, s, width, size)
    }

    /// Image by path, `src` rect, destination rect.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, path: &str, src: Option<Rect>, x: f32, y: f32, w: f32, h: f32) {
        if let Some(hd) = self.ga.image(path) {
            self.p.image(&hd, src, x, y, w, h, Color::WHITE);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_tint(
        &mut self,
        path: &str,
        src: Option<Rect>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        tint: Color,
    ) {
        if let Some(hd) = self.ga.image(path) {
            self.p.image(&hd, src, x, y, w, h, tint);
        }
    }

    /// `label(...)` with the default dark outline.
    pub fn text(&mut self, s: impl Into<String>, x: f32, y: f32, size: f32, color: Color, a: Align) {
        self.p.label(s, x, y, size, color, a, Some(OUTLINE));
    }

    /// `label(..., null)`: plain text (on parchment).
    pub fn plain(&mut self, s: impl Into<String>, x: f32, y: f32, size: f32, color: Color, a: Align) {
        self.p.label(s, x, y, size, color, a, None);
    }

    /// `drawIcon(ctx, path, cx, cy, size)`.
    pub fn icon(&mut self, path: &str, cx: f32, cy: f32, size: f32) {
        let h = (size / 2.0).round();
        self.draw(path, None, cx.round() - h, cy.round() - h, h * 2.0, h * 2.0);
    }

    /// `nineSlice` with gapped slices.
    #[allow(clippy::too_many_arguments)]
    pub fn nine(&mut self, path: &str, xs: &Slices, x: f32, y: f32, w: f32, h: f32, s: f32) {
        let Some(img) = self.ga.image(path) else {
            return;
        };
        let (mut lw, mut rw, mut th, mut bh) = (xs[0].1 * s, xs[2].1 * s, xs[0].1 * s, xs[2].1 * s);
        if lw + rw > w {
            let k = w / (lw + rw);
            lw *= k;
            rw *= k;
        }
        if th + bh > h {
            let k = h / (th + bh);
            th *= k;
            bh *= k;
        }
        let xx = [x.round(), (x + lw).round(), (x + w - rw).round(), (x + w).round()];
        let yy = [y.round(), (y + th).round(), (y + h - bh).round(), (y + h).round()];
        for j in 0..3 {
            for i in 0..3 {
                let (dw, dh) = (xx[i + 1] - xx[i], yy[j + 1] - yy[j]);
                if dw > 0.0 && dh > 0.0 {
                    let src = Rect::new(xs[i].0, xs[j].0, xs[i].0 + xs[i].1, xs[j].0 + xs[j].1);
                    self.p.image(&img, Some(src), xx[i], yy[j], dw, dh, Color::WHITE);
                }
            }
        }
    }

    /// `drawPanel(def, x, y, w, h, s)`: `(x, y, w, h)` is the visible rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn panel(&mut self, d: &PanelDef, x: f32, y: f32, w: f32, h: f32, s: f32) {
        let [l, t, r, b] = d.pad;
        self.nine(
            d.path,
            &d.xs,
            x - l * s,
            y - t * s,
            w + (l + r) * s,
            h + (t + b) * s,
            s,
        );
    }

    /// `drawStoreBanner` (menu background).
    pub fn store_banner(&mut self, x: f32, y: f32, w: f32, h: f32, s: f32) {
        let path = format!("{STOREP}Banner/Banner.png");
        let Some(im) = self.ga.image(&path) else {
            return;
        };
        let (x, y, w, h) = (x.round(), y.round(), w.round(), h.round());
        let c = (64.0 * s).round();
        let cr = (44.0 * s).round();
        let bh = (98.0 * s).round();
        let bl = (192.0 * s).round();
        let br = (172.0 * s).round();
        let mh = h - c - bh;
        let mut d = |sx: f32, sy: f32, sw: f32, sh: f32, dx: f32, dy: f32, dw: f32, dh: f32| {
            self.p
                .image(&im, Some(Rect::new(sx, sy, sx + sw, sy + sh)), dx, dy, dw, dh, Color::WHITE);
        };
        d(0.0, 0.0, 64.0, 64.0, x, y, c, c);
        d(256.0, 0.0, 64.0, 64.0, x + c, y, w - c - cr, c);
        d(640.0, 0.0, 44.0, 64.0, x + w - cr, y, cr, c);
        d(0.0, 128.0, 64.0, 64.0, x, y + c, c, mh);
        d(320.0, 128.0, 64.0, 64.0, x + c, y + c, w - c - cr, mh);
        d(640.0, 128.0, 44.0, 64.0, x + w - cr, y + c, cr, mh);
        d(0.0, 256.0, 192.0, 98.0, x, y + h - bh, bl, bh);
        d(256.0, 256.0, 64.0, 98.0, x + bl, y + h - bh, w - bl - br, bh);
        d(512.0, 256.0, 172.0, 98.0, x + w - br, y + h - bh, br, bh);
    }

    /// `threeSlice(im, xs, sy, sh, x, y, w, s)`.
    #[allow(clippy::too_many_arguments)]
    pub fn three(&mut self, path: &str, xs: &Slices, sy: f32, sh: f32, x: f32, y: f32, w: f32, s: f32) {
        let Some(im) = self.ga.image(path) else {
            return;
        };
        let lw = (xs[0].1 * s).round();
        let rw = (xs[2].1 * s).round();
        let h = (sh * s).round();
        let (x, y, w) = (x.round(), y.round(), w.round());
        let src = |i: usize| Rect::new(xs[i].0, sy, xs[i].0 + xs[i].1, sy + sh);
        self.p.image(&im, Some(src(0)), x, y, lw, h, Color::WHITE);
        if w - lw - rw > 0.0 {
            self.p.image(&im, Some(src(1)), x + lw, y, w - lw - rw, h, Color::WHITE);
        }
        self.p.image(&im, Some(src(2)), x + w - rw, y, rw, h, Color::WHITE);
    }

    /// `bigRibbon(colorIdx, cx, cy, w, s, str, size)`.
    #[allow(clippy::too_many_arguments)]
    pub fn big_ribbon(&mut self, c: KingdomColor, cx: f32, cy: f32, w: f32, s: f32, t: &str, size: f32) {
        let path = format!("{UIP}Ribbons/BigRibbons.png");
        self.three(&path, &G7, c.index() as f32 * 128.0, 128.0, cx - w / 2.0, cy - 64.0 * s, w, s);
        if !t.is_empty() {
            self.text(t, cx, cy - 2.0 * s, size, WHITE, Align::Center);
        }
    }

    /// `smallRibbon(colorIdx, variant, cx, cy, w, s, str, size)`.
    #[allow(clippy::too_many_arguments)]
    pub fn small_ribbon(
        &mut self,
        c: KingdomColor,
        variant: u8,
        cx: f32,
        cy: f32,
        w: f32,
        s: f32,
        t: &str,
        size: f32,
    ) {
        let path = format!("{UIP}Ribbons/SmallRibbons.png");
        let sy = (c.index() as f32 * 2.0 + variant as f32) * 64.0;
        self.three(&path, &G3, sy, 64.0, cx - w / 2.0, cy - 33.0 * s, w, s);
        if !t.is_empty() {
            self.text(t, cx, cy - 1.0, size, WHITE, Align::Center);
        }
    }

    /// `swordBanner(colorIdx, cx, cy, w, s, str, size)`.
    #[allow(clippy::too_many_arguments)]
    pub fn sword_banner(&mut self, c: KingdomColor, cx: f32, cy: f32, w: f32, s: f32, t: &str, size: f32) {
        let path = format!("{UIP}Swords/Swords.png");
        self.three(&path, &G7, c.index() as f32 * 128.0, 128.0, cx - w / 2.0, cy - 64.0 * s, w, s);
        if !t.is_empty() {
            self.plain(t, cx + 16.0 * s, cy - 14.0 * s, size, INK, Align::Center);
        }
    }

    /// `storeRibbon(color, cx, cy, w, s, str, size)`.
    #[allow(clippy::too_many_arguments)]
    pub fn store_ribbon(&mut self, c: KingdomColor, cx: f32, cy: f32, w: f32, s: f32, t: &str, size: f32) {
        let path = format!("{STOREP}Ribbons/Ribbon_{}.png", c.key());
        self.three(&path, &G7, 0.0, 128.0, cx - w / 2.0, cy - 68.0 * s, w, s);
        if !t.is_empty() {
            self.text(t, cx, cy, size, WHITE, Align::Center);
        }
    }

    /// `drawPennant(color, cx, y, s)`.
    pub fn pennant(&mut self, c: KingdomColor, cx: f32, y: f32, s: f32) {
        let path = format!("{STOREP}Ribbons/Ribbon_{}.png", c.key());
        self.draw(
            &path,
            Some(Rect::new(51.0, 448.0, 141.0, 556.0)),
            (cx - 45.0 * s).round(),
            y.round(),
            (90.0 * s).round(),
            (108.0 * s).round(),
        );
    }

    /// `bigBar(x, y, w, ratio, s)`.
    pub fn big_bar(&mut self, x: f32, y: f32, w: f32, ratio: f32, s: f32) {
        let base = format!("{UIP}Bars/BigBar_Base.png");
        self.three(&base, &G3, 0.0, 64.0, x - 40.0 * s, y - 9.0 * s, w + 80.0 * s, s);
        let fw = (w - 22.0 * s) * ratio.clamp(0.0, 1.0);
        if fw > 0.5 {
            let fill = format!("{UIP}Bars/BigBar_Fill.png");
            self.draw(
                &fill,
                Some(Rect::new(0.0, 22.0, 64.0, 43.0)),
                (x + 11.0 * s).round(),
                (y + 13.0 * s).round(),
                fw.round(),
                (21.0 * s).round(),
            );
        }
    }

    /// `smallBar(x, y, w, ratio, s, fill)`: `fill` = flat faction colour (source-atop tint of the fill strip).
    #[allow(clippy::too_many_arguments)]
    pub fn small_bar(&mut self, x: f32, y: f32, w: f32, ratio: f32, s: f32, fill: Color) {
        let base = format!("{UIP}Bars/SmallBar_Base.png");
        self.three(&base, &G3, 0.0, 64.0, x - 49.0 * s, y - 22.0 * s, w + 98.0 * s, s);
        let fw = (w - 20.0 * s) * ratio.clamp(0.0, 1.0);
        if fw > 0.5 {
            self.p.rect(x + 10.0 * s, y + 8.0 * s, fw, 6.0 * s, fill);
        }
    }

    /// `drawBrackets(x, y, w, h, s)`.
    pub fn brackets(&mut self, x: f32, y: f32, w: f32, h: f32, s: f32) {
        let path = format!("{UIP}Cursors/Cursor_04.png");
        let Some(im) = self.ga.image(&path) else {
            return;
        };
        let d = (32.0 * s).round();
        let (x, y, w, h) = (x.round(), y.round(), w.round(), h.round());
        for (sx, sy, dx, dy) in [
            (0.0, 0.0, x, y),
            (96.0, 0.0, x + w - d, y),
            (0.0, 96.0, x, y + h - d),
            (96.0, 96.0, x + w - d, y + h - d),
        ] {
            self.p.image(
                &im,
                Some(Rect::new(sx, sy, sx + 32.0, sy + 32.0)),
                dx,
                dy,
                d,
                d,
                Color::WHITE,
            );
        }
    }

    /// `drawAvatar(color, type, x, y, size)` (top-left anchored).
    pub fn avatar(&mut self, c: KingdomColor, k: UnitKind, x: f32, y: f32, size: f32) {
        let Some(a) = k.def().avatar else {
            return;
        };
        let n = c.index() * 5 + a as usize + 1;
        let path = format!("{UIP}Human Avatars/Avatars_{n:02}.png");
        self.draw(&path, Some(Rect::new(40.0, 40.0, 216.0, 216.0)), x, y, size, size);
    }

    /// `drawPortrait(color, type, x, y, size)`.
    pub fn portrait(&mut self, c: KingdomColor, k: UnitKind, x: f32, y: f32, size: f32) {
        if k.def().avatar.is_some() {
            self.avatar(c, k, x, y, size);
            return;
        }
        let path = unit_idle_sheet(c, k);
        let Some(e) = self.ga.entry(&path) else {
            return;
        };
        let (fw, fh) = (e.fw as f32, e.fh as f32);
        let ay = e.ay.unwrap_or(fh * 0.9);
        let side = fw.min(fh) * 0.86;
        let sx = (fw - side) / 2.0;
        let sy = ay - side * 0.9;
        self.draw(
            &path,
            Some(Rect::new(sx, sy, sx + side, sy + side)),
            x.round(),
            y.round(),
            size.round(),
            size.round(),
        );
    }

    /// Animated idle frame of a unit anchored at its foot `(x, y)` with scale `k` (menu level cards).
    pub fn unit_frame(&mut self, c: KingdomColor, k: UnitKind, frame: usize, x: f32, y: f32, scale: f32) {
        let path = unit_idle_sheet(c, k);
        let Some(e) = self.ga.entry(&path) else {
            return;
        };
        let (fw, fh) = (e.fw as f32, e.fh as f32);
        let n = e.frames.max(1) as usize;
        let i = if matches!(k, UnitKind::Ram | UnitKind::Catapult) {
            0
        } else {
            frame % n
        };
        let ax = e.ax.unwrap_or(fw / 2.0);
        let ay = e.ay.unwrap_or(fh);
        let sx = i as f32 * fw;
        self.draw(
            &path,
            Some(Rect::new(sx, 0.0, sx + fw, fh)),
            (x - ax * scale).round(),
            (y - ay * scale).round(),
            (fw * scale).round(),
            (fh * scale).round(),
        );
    }

    /// `drawBuildingThumb(color, kind, cx, cy, size)`.
    pub fn building_thumb(&mut self, c: KingdomColor, k: BuildingKind, cx: f32, cy: f32, size: f32) {
        let [sx, sy, sw, sh] = k.def().crop.map(|v| v as f32);
        let kk = (size / sw).min(size / sh);
        let path = building_img(c, k, 0);
        self.draw(
            &path,
            Some(Rect::new(sx, sy, sx + sw, sy + sh)),
            (cx - sw * kk / 2.0).round(),
            (cy - sh * kk / 2.0).round(),
            (sw * kk).round(),
            (sh * kk).round(),
        );
    }

    /// Capture point sprite crop fitted in `size` centred at `(cx, cy)`.
    pub fn point_thumb(&mut self, k: PointKind, cx: f32, cy: f32, size: f32) {
        let [sx, sy, sw, sh] = k.def().crop.map(|v| v as f32);
        let kk = (size / sw).min(size / sh);
        let path = point_img(k);
        self.draw(
            &path,
            Some(Rect::new(sx, sy, sx + sw, sy + sh)),
            (cx - sw * kk / 2.0).round(),
            (cy - sh * kk / 2.0).round(),
            (sw * kk).round(),
            (sh * kk).round(),
        );
    }

    /// `drawSpec(spec, color, cx, cy, size)`.
    pub fn spec(&mut self, sp: IconSpec, c: KingdomColor, cx: f32, cy: f32, size: f32) {
        match sp {
            IconSpec::Ui(n) => self.icon(&ui_icon(n), cx, cy, size * 1.15),
            IconSpec::X(n) => self.icon(&x_icon(n), cx, cy, size * 1.2),
            IconSpec::Res(r) => self.icon(&res_icon(r), cx, cy, size * 1.15),
            IconSpec::Unit(k) => {
                self.portrait(c, k, cx - size * 0.6, cy - size * 0.62, size * 1.2)
            }
            IconSpec::Build(k) => self.building_thumb(c, k, cx, cy, size),
            IconSpec::Age(n) => {
                self.icon(&x_icon("crown"), cx, cy - size * 0.1, size * 1.1);
                let t = AGE_SHORT.get(n as usize).copied().unwrap_or("");
                self.text(t, cx, cy + size * 0.3, size * 0.46, ERA_ON, Align::Center);
            }
        }
    }

    // ------------------------------------------------------------------ buttons

    /// `bigButton(x, y, w, h, s, text, red, icon)` → clicked.
    #[allow(clippy::too_many_arguments)]
    pub fn big_button(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        s: f32,
        t: &str,
        red: bool,
        icon: Option<u8>,
    ) -> bool {
        let r = self.p.hit(x, y, w, h);
        let def = match (red, r.down) {
            (false, false) => panel::BIG_BLUE,
            (false, true) => panel::BIG_BLUE_DOWN,
            (true, false) => panel::BIG_RED,
            (true, true) => panel::BIG_RED_DOWN,
        };
        self.panel(&def, x, y, w, h, s);
        let cy = y + h / 2.0 + if r.down { 3.0 } else { -4.0 } * s;
        let tw = self.tw(t, 24.0 * s);
        let iw = if icon.is_some() { 40.0 * s } else { 0.0 };
        if let Some(i) = icon {
            self.icon(&ui_icon(i), x + w / 2.0 - tw / 2.0 - 4.0 * s, cy, 40.0 * s);
        }
        self.text(t, x + w / 2.0 + iw / 2.0, cy, 24.0 * s, WHITE, Align::Center);
        if r.clicked {
            self.play(Sound::Click);
        }
        r.clicked
    }

    /// `smallButton(cx, cy, size, shape, red)` (hit = inner 70 %).
    pub fn small_button(&mut self, cx: f32, cy: f32, size: f32, square: bool, red: bool) -> SmallHit {
        let v = size * 0.35;
        let r = self.p.hit(cx - v, cy - v, v * 2.0, v * 2.0);
        let path = format!(
            "{UIP}Buttons/Small{}{}Button_{}.png",
            if red { "Red" } else { "Blue" },
            if square { "Square" } else { "Round" },
            if r.down { "Pressed" } else { "Regular" }
        );
        self.draw(
            &path,
            None,
            (cx - size / 2.0).round(),
            (cy - size / 2.0).round(),
            size.round(),
            size.round(),
        );
        SmallHit {
            hover: r.hover,
            down: r.down,
            clicked: r.clicked,
            dy: if r.down { 0.04 } else { -0.03 } * size,
        }
    }

    /// `tinyButton(cx, cy, size, shape, red, icon)`.
    #[allow(clippy::too_many_arguments)]
    pub fn tiny_button(
        &mut self,
        cx: f32,
        cy: f32,
        size: f32,
        square: bool,
        red: bool,
        icon: Option<u8>,
    ) -> Hit {
        let r = self.p.hit(cx - size / 2.0, cy - size / 2.0, size, size);
        let path = format!(
            "{UIP}Buttons/Tiny{}{}Button.png",
            if square { "Square" } else { "Round" },
            if red { "Red" } else { "Blue" }
        );
        let k = if r.down { 0.92 } else { 1.0 };
        self.draw(
            &path,
            None,
            (cx - size * k / 2.0).round(),
            (cy - size * k / 2.0).round(),
            (size * k).round(),
            (size * k).round(),
        );
        if let Some(i) = icon {
            self.icon(&ui_icon(i), cx, cy - size * 0.03, size * 0.74 * k);
        }
        if r.clicked {
            self.play(Sound::Click);
        }
        r
    }

    /// `actionButton(cx, cy, s, iconSpec, text, enabled, run)` → true when clicked and enabled.
    #[allow(clippy::too_many_arguments)]
    pub fn action_button(
        &mut self,
        cx: f32,
        cy: f32,
        s: f32,
        sp: IconSpec,
        c: KingdomColor,
        t: &str,
        enabled: bool,
    ) -> bool {
        let a0 = self.p.alpha;
        if !enabled {
            self.p.alpha = a0 * 0.4;
        }
        let r = self.small_button(cx, cy, 78.0 * s, true, false);
        self.spec(sp, c, cx, cy - 2.0 * s + r.dy, 30.0 * s);
        self.plain(t, cx, cy + 36.0 * s, 11.5 * s, INK, Align::Center);
        self.p.alpha = a0;
        if r.clicked {
            self.play(if enabled { Sound::Click } else { Sound::Deny });
        }
        r.clicked && enabled
    }

    /// `drawCost(f, cost, x, y, s, light, maxW)`.
    #[allow(clippy::too_many_arguments)]
    pub fn cost(&mut self, res: &[u32; 4], cost: &Cost, x: f32, y: f32, s: f32, light: bool, max_w: f32) {
        let kinds: Vec<(Res, u16)> = cost.iter().collect();
        if kinds.is_empty() {
            return;
        }
        let step = if max_w > 0.0 {
            (74.0 * s).min(max_w / kinds.len() as f32)
        } else {
            74.0 * s
        };
        let z = s.min(step / 58.0);
        let mut x = x;
        if max_w > 0.0 {
            x -= step * kinds.len() as f32 / 2.0;
        }
        for (k, v) in kinds {
            let ok = res[k as usize] as f64 >= v as f64;
            self.icon(&res_icon(k), x + 13.0 * z, y, 28.0 * z);
            if light {
                self.text(
                    v.to_string(),
                    x + 28.0 * z,
                    y,
                    15.0 * z,
                    if ok { WHITE } else { LIGHT_RED },
                    Align::Left,
                );
            } else {
                self.plain(v.to_string(), x + 28.0 * z, y, 15.0 * z, if ok { INK } else { RED }, Align::Left);
            }
            x += step;
        }
    }

    /// `drawTooltip(tip, s)`.
    pub fn tooltip(&mut self, tip: &Tip, s: f32, c: KingdomColor, res: &[u32; 4]) {
        let w = 330.0 * s;
        let lines = self.wrap(&tip.desc, w - 28.0 * s, 13.5 * s);
        let h = (34.0
            + if tip.cost.is_some() { 30.0 } else { 0.0 }
            + lines.len() as f32 * 18.0
            + if tip.lock.is_some() { 24.0 } else { 0.0 }
            + 8.0)
            * s;
        let x = (tip.x - w / 2.0).clamp(12.0 * s, (self.w() - w - 12.0 * s).max(12.0 * s));
        let y = if tip.below {
            tip.y
        } else {
            (8.0 * s).max(tip.y - h - 28.0 * s)
        };
        self.panel(&panel::PAPER, x, y, w, h, 0.6 * s);
        let title = match &tip.key {
            Some(k) => format!("{}  [{}]", tip.title, k),
            None => tip.title.clone(),
        };
        let rw = w.min(self.tw(&tip.title, 15.0 * s) + 130.0 * s);
        self.small_ribbon(c, 1, x + w / 2.0, y - 2.0 * s, rw, 0.78 * s, &title, 15.0 * s);
        let mut ty = y + 36.0 * s;
        if let Some(cost) = &tip.cost {
            self.cost(res, cost, x + 14.0 * s, ty, s, false, 0.0);
            ty += 30.0 * s;
        }
        for l in lines {
            self.plain(l, x + 14.0 * s, ty, 13.5 * s, INK, Align::Left);
            ty += 18.0 * s;
        }
        if let Some(lock) = &tip.lock {
            self.plain(lock.clone(), x + 14.0 * s, ty + 4.0 * s, 14.0 * s, RED, Align::Left);
        }
    }

    /// `hpStrip(u, x, y, w, t)` (clamped, Q8).
    pub fn hp_strip(&mut self, ratio: f32, x: f32, y: f32, w: f32, t: f32) {
        self.p.rect(x, y, w, t, DARK);
        let r = ratio.clamp(0.0, 1.0);
        let c = if r > 0.35 { HP_OK } else { HP_LOW };
        self.p.rect(x, y, w * r, t, c);
    }

    /// Circle outline of radius `r` (procedural ring texture).
    pub fn ring(&mut self, ring: &Handle<Image>, cx: f32, cy: f32, r: f32, color: Color) {
        self.p
            .image(ring, None, cx - r, cy - r, 2.0 * r, 2.0 * r, color);
    }
}
