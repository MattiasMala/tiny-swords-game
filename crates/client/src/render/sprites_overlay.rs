// OWNER: C2 (sprites) — world overlays of `drawOverlays` (spec 07 §8.5, 01 §9.3-§9.5): selection ellipses, unit /
// building HP bars (pack bar art, kingdom-coloured fill), construction big bar, capture progress bar, selection
// brackets (Cursor_04 corners), castle name plates (SmallRibbons + kingdom name), sheltered-pawns indicator, rally
// pennant, and the placement ghost (`Ghost`: sprite at alpha 0.65 + footprint fill). C4 draws the order markers,
// range rings, dashed rally line and footprint outline (control/draw.rs).
//
// The 2D overlays are laid out in world px around an entity foot; here the same 2D rectangles are mapped onto the
// billboard plane of that foot (so they project exactly like the 2D game) and pulled towards the camera so they stay
// on top (unlit, not fogged). Text (castle names, `×n`) uses pooled UI labels positioned by projection.

use crate::assets::{GameAssets, SheetId, SpriteAtlas};
use crate::control::Selection;
use crate::mirror::Mirror;
use crate::render::sprites::{
    Basis, DrawStage, Flavor, Ghost, Layer, Painter, SpriteBatches, SpriteOpts, drawable,
    foot_world,
};
use crate::render::sprites_world::{NONE, Scenery, Sheets, building_foot};
use bevy::prelude::*;
use ts_sim::TILE;
use ts_sim::cmd::EntityRef;
use ts_sim::defs::{BuildingKind, CAPTURE_TIME, KingdomColor, NodeKind, UnitKind};

/// Overlays are pulled this far (world units) towards the camera.
const PULL: f32 = 2.0;
/// The placement ghost (footprint fill + sprite) is drawn over the buildings it overlaps, like the 2D `Ctl.drawWorld`.
const GHOST_PULL: f32 = 3.0;

/// `COLOR_CSS` as linear rgba (vertex colours are linear).
pub fn css_linear(c: KingdomColor) -> [f32; 4] {
    let [r, g, b] = c.rgb();
    let l = Color::srgb_u8(r, g, b).to_linear();
    [l.red, l.green, l.blue, 1.0]
}

fn srgba(r: u8, g: u8, b: u8, a: f32) -> [f32; 4] {
    let l = Color::srgb_u8(r, g, b).to_linear();
    [l.red, l.green, l.blue, a]
}

/// One pooled screen label.
#[derive(Component)]
struct WorldLabel(usize);

#[derive(Component)]
struct WorldLabelText;

#[derive(Component)]
struct WorldLabelIcon;

/// One screen label anchored to a world point. Projected in `PostUpdate` with the camera of the frame being rendered
/// (the camera eases in `PostUpdate`, so a projection done in `Update` would trail the sprites by one frame).
#[derive(Clone, Debug)]
pub struct LabelReq {
    /// world anchor
    pub world: Vec3,
    /// horizontal offset of the label's left edge (centre when `centred`) from the anchor, in sprite px
    pub dx: f32,
    pub text: String,
    /// font size in sprite px (scaled by the on-screen size of a sprite px)
    pub size: f32,
    pub color: Color,
    pub icon: Option<Handle<Image>>,
    /// icon side in sprite px
    pub icon_size: f32,
    pub centred: bool,
}

/// Screen labels requested this frame (cleared in `SpriteSystems::Prepare`).
#[derive(Resource, Default)]
pub struct LabelRequests {
    pub list: Vec<LabelReq>,
}

fn clear_labels(mut req: ResMut<LabelRequests>) {
    req.list.clear();
}

pub struct SpritesOverlayPlugin;

impl Plugin for SpritesOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LabelRequests>()
            .add_systems(
                Update,
                (
                    clear_labels.in_set(crate::render::sprites::SpriteSystems::Prepare),
                    draw_overlays.in_set(DrawStage::Late),
                )
                    .run_if(in_state(crate::AppState::InGame)),
            )
            .add_systems(
                PostUpdate,
                place_labels
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::ui::UiSystems::Prepare)
                    .run_if(in_state(crate::AppState::InGame)),
            )
            .add_systems(OnExit(crate::AppState::InGame), hide_labels);
    }
}

/// Maps a 2D-game rectangle (world px, y down) around the reference foot `(fx, fy)` onto the billboard plane.
struct Ov<'a, 'b> {
    p: &'a mut Painter<'b>,
    atlas: &'a SpriteAtlas,
    foot: Vec3,
    fx: f32,
    fy: f32,
}

impl Ov<'_, '_> {
    /// Draws region `sh` (whole frame) into the 2D rect `(x, y, w, h)`.
    fn rect(&mut self, sh: SheetId, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        self.rect_src(sh, None, x, y, w, h, color);
    }

    #[allow(clippy::too_many_arguments)]
    fn rect_src(
        &mut self,
        sh: SheetId,
        src: Option<[f32; 4]>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
    ) {
        if sh == NONE || w <= 0.0 || h <= 0.0 {
            return;
        }
        let f = *self.atlas.frame(sh, 0);
        let src = src.unwrap_or([0.0, 0.0, f.w as f32, f.h as f32]);
        self.p.overlay_rect(
            &f,
            src,
            self.foot,
            x - self.fx,
            x + w - self.fx,
            self.fy - (y + h),
            self.fy - y,
            color,
            PULL,
        );
    }

    /// `smallBar(x, y, w, ratio, s, fill)`.
    fn small_bar(&mut self, s: &Sheets, x: f32, y: f32, w: f32, ratio: f32, sc: f32, fill: [f32; 4]) {
        let bx = x - 49.0 * sc;
        let by = y - 22.0 * sc;
        let bw = w + 98.0 * sc;
        let cap = 64.0 * sc;
        let h = 64.0 * sc;
        self.rect(s.smallbar[0], bx, by, cap, h, [1.0; 4]);
        self.rect(s.smallbar[1], bx + cap, by, (bw - 2.0 * cap).max(0.0), h, [1.0; 4]);
        self.rect(s.smallbar[2], bx + bw - cap, by, cap, h, [1.0; 4]);
        let fw = (w - 20.0 * sc) * ratio.clamp(0.0, 1.0);
        if fw > 0.5 {
            self.rect(s.white, x + 10.0 * sc, y + 8.0 * sc, fw, 6.0 * sc, fill);
        }
    }

    /// `bigBar(x, y, w, ratio, s)`.
    fn big_bar(&mut self, s: &Sheets, x: f32, y: f32, w: f32, ratio: f32, sc: f32) {
        let bx = x - 40.0 * sc;
        let by = y - 9.0 * sc;
        let bw = w + 80.0 * sc;
        let cap = 64.0 * sc;
        let h = 64.0 * sc;
        self.rect(s.bigbar[0], bx, by, cap, h, [1.0; 4]);
        self.rect(s.bigbar[1], bx + cap, by, (bw - 2.0 * cap).max(0.0), h, [1.0; 4]);
        self.rect(s.bigbar[2], bx + bw - cap, by, cap, h, [1.0; 4]);
        let fw = (w - 22.0 * sc) * ratio.clamp(0.0, 1.0);
        if fw > 0.5 {
            self.rect(s.bigbar_fill, x + 11.0 * sc, y + 13.0 * sc, fw, 21.0 * sc, [1.0; 4]);
        }
    }

    /// `drawBrackets(x, y, w, h, s)`: Cursor_04 corners, 32·s px.
    fn brackets(&mut self, s: &Sheets, x: f32, y: f32, w: f32, h: f32, sc: f32) {
        let c = 32.0 * sc;
        self.rect(s.bracket[0], x, y, c, c, [1.0; 4]);
        self.rect(s.bracket[1], x + w - c, y, c, c, [1.0; 4]);
        self.rect(s.bracket[2], x, y + h - c, c, c, [1.0; 4]);
        self.rect(s.bracket[3], x + w - c, y + h - c, c, c, [1.0; 4]);
    }

    /// `smallRibbon(colorIdx, 0, cx, cy, w, s)` (the label is drawn by the UI label pool).
    fn ribbon(&mut self, s: &Sheets, ci: usize, cx: f32, cy: f32, w: f32, sc: f32) {
        let x = cx - w * 0.5;
        let y = cy - 33.0 * sc;
        let cap = 64.0 * sc;
        let h = 64.0 * sc;
        let r = s.ribbon[ci.min(4)];
        self.rect(r[0], x, y, cap, h, [1.0; 4]);
        self.rect(r[1], x + cap, y, (w - 2.0 * cap).max(0.0), h, [1.0; 4]);
        self.rect(r[2], x + w - cap, y, cap, h, [1.0; 4]);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_overlays(
    mirror: Res<Mirror>,
    atlas: Res<SpriteAtlas>,
    sheets: Res<Sheets>,
    basis: Res<Basis>,
    sel: Res<Selection>,
    ghost: Res<Ghost>,
    scenery: Res<Scenery>,
    ga: Res<GameAssets>,
    time: Res<Time<Real>>,
    mut batches: ResMut<SpriteBatches>,
    mut labels: ResMut<LabelRequests>,
) {
    let Some(map) = drawable(&mirror, &atlas, &basis) else {
        return;
    };
    if !sheets.ready || batches.pages == 0 {
        return;
    }
    let now = time.elapsed_secs_f64();
    let color_of = |f: u8| {
        mirror
            .colors
            .get(f as usize)
            .copied()
            .unwrap_or(KingdomColor::Blue)
    };
    let mut p = Painter {
        batches: &mut batches,
        basis: &basis,
        atlas: &atlas,
    };

    // ---- units: selection ellipse + HP bar ----
    for (id, u) in &mirror.view.units {
        if u.flags & ts_sim::snapshot_types::UF_INSIDE != 0 {
            continue;
        }
        let selected = sel.units.contains(id);
        let hurt = u.hp < u.max_hp;
        if !selected && !hurt {
            continue;
        }
        let pos = mirror
            .unit_pos_interp(*id, now)
            .unwrap_or(Vec2::new(u.x, u.y));
        if !basis.in_view(pos.x, pos.y, 120.0) {
            continue;
        }
        let d = u.kind.def();
        let own = Some(u.faction) == mirror.you;
        if selected {
            let big = d.siege || u.kind == UnitKind::Knight;
            let sh = if big { sheets.ring_big } else { sheets.ring };
            if sh != NONE {
                let f = *atlas.frame(sh, 0);
                let c = foot_world(map, pos.x, pos.y + 1.0);
                let col = if own {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    srgba(0xff, 0x5a, 0x4a, 1.0)
                };
                p.ground_quad(
                    Layer::Dynamic,
                    Flavor::Overlay,
                    &f,
                    c,
                    f.w as f32 * 0.5,
                    f.h as f32 * 0.5,
                    col,
                );
            }
        }
        let foot = foot_world(map, pos.x, pos.y);
        let mut ov = Ov {
            p: &mut p,
            atlas: &atlas,
            foot,
            fx: pos.x,
            fy: pos.y,
        };
        let ratio = u.hp as f32 / u.max_hp.max(1) as f32;
        ov.small_bar(
            &sheets,
            pos.x - 24.0,
            pos.y - d.bar_y,
            48.0,
            ratio,
            0.5,
            css_linear(color_of(u.faction)),
        );
    }

    // ---- buildings ----
    let sel_building = sel.building;
    for (id, b) in &mirror.view.buildings {
        let d = b.kind.def();
        let fp = building_foot(b);
        if !basis.in_view(fp.x, fp.y, d.w as f32 * 40.0 + 64.0) {
            continue;
        }
        let selected = sel_building == Some(*id);
        let own = Some(b.faction) == mirror.you;
        let wide = d.w > 1;
        let top = fp.y - d.ay as f32;
        let foot = foot_world(map, fp.x, fp.y);
        let mut ov = Ov {
            p: &mut p,
            atlas: &atlas,
            foot,
            fx: fp.x,
            fy: fp.y,
        };
        // sheltered pawns: avatar + '×n' (own buildings)
        if own && b.sheltered > 0 {
            let (bx, by) = if b.kind == BuildingKind::Castle {
                (fp.x + 98.0, top - 14.0)
            } else {
                (fp.x - 34.0, top - 22.0)
            };
            // avatar (34 px) with its top-left at (bx, by), label '×n' to its right, centred on the avatar row
            let avatar = {
                let ci = color_of(b.faction).index();
                let n = ci * 5 + 4 + 1;
                ga.image(&format!(
                    "UI Elements/UI Elements/Human Avatars/Avatars_{n:02}.png"
                ))
            };
            labels.list.push(LabelReq {
                world: foot + basis.right * (bx - fp.x) + basis.up * (fp.y - (by + 17.0)),
                dx: 0.0,
                text: format!("×{}", b.sheltered),
                size: 17.0,
                color: Color::srgb_u8(0xff, 0xf7, 0xc9),
                icon: avatar,
                icon_size: 34.0,
                centred: false,
            });
        }
        if !b.done && wide {
            let prog = if d.work > 0.0 {
                b.progress / d.work
            } else {
                1.0
            };
            ov.big_bar(&sheets, fp.x - 45.0, fp.y - 60.0, 90.0, prog, 0.4);
        }
        let damaged = b.hp < b.max_hp;
        if b.done && (damaged || selected) {
            let ratio = b.hp as f32 / b.max_hp.max(1) as f32;
            let (bw, sc) = if wide { (88.0, 0.7) } else { (44.0, 0.5) };
            ov.small_bar(
                &sheets,
                fp.x - bw * 0.5,
                top + 18.0,
                bw,
                ratio,
                sc,
                css_linear(color_of(b.faction)),
            );
        }
        if b.kind == BuildingKind::Castle {
            let c = color_of(b.faction);
            ov.ribbon(&sheets, c.index(), fp.x, top + 4.0, 190.0, 0.8);
            labels.list.push(LabelReq {
                world: foot + basis.up * (fp.y - (top + 4.0 - 1.0)),
                dx: 0.0,
                text: c.kingdom_name(),
                size: 15.0,
                color: Color::WHITE,
                icon: None,
                icon_size: 0.0,
                centred: true,
            });
        }
        if selected {
            let x0 = b.cx as f32 * TILE;
            let y0 = b.cy as f32 * TILE;
            let (w, h) = (d.w as f32 * TILE, d.h as f32 * TILE);
            ov.brackets(
                &sheets,
                x0 - 10.0,
                y0 - 14.0,
                w + 20.0,
                h + 28.0,
                if wide { 1.0 } else { 0.7 },
            );
            // rally pennant of an own finished trainer
            if own
                && b.done
                && !d.trains.is_empty()
                && let Some(r) = b.rally
            {
                let pen = sheets.pennant[color_of(b.faction).index()];
                if pen != NONE {
                    let f = *atlas.frame(pen, 0);
                    let s = 0.45;
                    let rf = foot_world(map, r.x, r.y);
                    p.frame(Layer::Dynamic, Flavor::Lit, &f, 45.0, 108.0, rf, SpriteOpts {
                        scale: s,
                        lift: (50.0 - 108.0 * s).max(0.0),
                        fold: false,
                        ..default()
                    });
                }
            }
        }
    }

    // ---- capture points: progress bar + brackets ----
    for pv in &mirror.view.points {
        let Some(&(pos, kind)) = scenery.points.get(pv.id.0 as usize) else {
            continue;
        };
        if !basis.in_view(pos.x, pos.y, 200.0) {
            continue;
        }
        let def = kind.def();
        let top = pos.y - def.ay as f32;
        let foot = foot_world(map, pos.x, pos.y);
        let mut ov = Ov {
            p: &mut p,
            atlas: &atlas,
            foot,
            fx: pos.x,
            fy: pos.y,
        };
        if let Some(by) = pv.by
            && pv.t > 0.0
        {
            ov.small_bar(
                &sheets,
                pos.x - 44.0,
                top + 22.0,
                88.0,
                pv.t / CAPTURE_TIME,
                0.7,
                css_linear(color_of(by)),
            );
        }
        if sel.other == Some(EntityRef::Point(pv.id)) {
            ov.brackets(
                &sheets,
                pos.x - 64.0 - 10.0,
                pos.y + 4.0 - 128.0 - 14.0,
                128.0 + 20.0,
                128.0 + 28.0,
                1.0,
            );
        }
    }

    // ---- other selection (resource node / sheep) ----
    let other = match sel.other {
        Some(EntityRef::Node(n)) => scenery.nodes.get(n.0 as usize).map(|i| {
            (
                Vec2::new(i.ex, i.ey),
                i.big,
                i.kind != NodeKind::Tree && i.big,
            )
        }),
        Some(EntityRef::Sheep(s)) => mirror
            .view
            .sheep
            .get(&s)
            .map(|v| (Vec2::new(v.x, v.y), false, false)),
        _ => None,
    };
    if let Some((pos, big, _)) = other {
        let foot = foot_world(map, pos.x, pos.y);
        let mut ov = Ov {
            p: &mut p,
            atlas: &atlas,
            foot,
            fx: pos.x,
            fy: pos.y,
        };
        if big {
            ov.brackets(&sheets, pos.x - 74.0, pos.y - 80.0, 148.0, 148.0, 1.0);
        } else {
            ov.brackets(&sheets, pos.x - 30.0, pos.y - 54.0, 60.0, 64.0, 0.7);
        }
    }

    // ---- placement ghost ----
    if let Some(kind) = ghost.kind {
        let me = mirror.you.unwrap_or(0);
        let color = color_of(me);
        let sh = sheets.building(color, kind, ghost.variant);
        let d = kind.def();
        let mut cells = vec![(ghost.cell, ghost.valid)];
        cells.extend(ghost.chain.iter().copied());
        for (cell, ok) in cells {
            let fx = (cell.x as f32 + d.w as f32 * 0.5) * TILE;
            let fy = (cell.y + d.h) as f32 * TILE - 4.0;
            let foot = foot_world(map, fx, fy);
            // footprint fill
            if sheets.white != NONE {
                let f = *atlas.frame(sheets.white, 0);
                let col = if ok {
                    srgba(120, 255, 140, 0.28)
                } else {
                    srgba(255, 70, 60, 0.38)
                };
                let h = foot.y + 0.03;
                p.ground_rect(
                    Layer::Dynamic,
                    Flavor::Overlay,
                    &f,
                    cell.x as f32,
                    cell.y as f32,
                    (cell.x + d.w) as f32,
                    (cell.y + d.h) as f32,
                    h,
                    col,
                    GHOST_PULL,
                );
            }
            if sh != NONE {
                p.sprite(Layer::Dynamic, Flavor::Translucent, sh, 0, foot, SpriteOpts {
                    color: [1.0, 1.0, 1.0, 0.65],
                    pull: GHOST_PULL,
                    ..default()
                });
            }
        }
    }
}

/// Positions pooled UI labels (castle names, sheltered counts, floating "+N"); spawns more when needed. Runs in
/// `PostUpdate` after transform propagation: the projection uses the camera transform of the frame being rendered.
#[allow(clippy::too_many_arguments)]
fn place_labels(
    mut commands: Commands,
    req: Res<LabelRequests>,
    ga: Res<GameAssets>,
    basis: Res<Basis>,
    cams: Query<(&Camera, &GlobalTransform), With<crate::render::camera::RtsCamera>>,
    mut q: Query<(&WorldLabel, &mut Node, &mut Visibility, &Children)>,
    mut texts: Query<
        (&mut Text, &mut TextFont, &mut TextColor, &mut TextShadow),
        With<WorldLabelText>,
    >,
    mut icons: Query<(&mut ImageNode, &mut Node), (With<WorldLabelIcon>, Without<WorldLabel>)>,
) {
    let view = cams.single().ok().and_then(|(cam, tf)| {
        let vp = cam.logical_viewport_size()?;
        Some((cam.clip_from_view() * tf.to_matrix().inverse(), vp))
    });
    let project = |p: Vec3| -> Option<Vec2> {
        let (clip_from_world, vp) = view?;
        let c = clip_from_world * p.extend(1.0);
        if c.w <= 1e-4 {
            return None;
        }
        let ndc = c.truncate() / c.w;
        Some(Vec2::new((ndc.x * 0.5 + 0.5) * vp.x, (0.5 - ndc.y * 0.5) * vp.y))
    };
    let mut have = 0usize;
    for (wl, mut node, mut vis, children) in &mut q {
        have = have.max(wl.0 + 1);
        let placed = req.list.get(wl.0).and_then(|r| {
            let s = project(r.world)?;
            // screen px per sprite px at the anchor
            let k = project(r.world + basis.right * 100.0).map_or(1.0, |b| (b - s).length() / 100.0);
            Some((r, s, k))
        });
        let Some((r, s, k)) = placed else {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            continue;
        };
        let (text, color, icon, centred) = (&r.text, &r.color, &r.icon, r.centred);
        let size = (r.size * k).clamp(6.0, 40.0);
        let icon_w = if icon.is_some() {
            (r.icon_size * k).clamp(8.0, 80.0)
        } else {
            0.0
        };
        let pos = s + Vec2::new(r.dx * k, 0.0);
        let width = text.chars().count() as f32 * size * 0.62 + icon_w;
        node.left = px(if centred { pos.x - width * 0.5 } else { pos.x });
        node.top = px(pos.y - if icon.is_some() { icon_w.max(size * 1.2) * 0.5 } else { size * 0.7 });
        *vis = Visibility::Inherited;
        for c in children.iter() {
            if let Ok((mut t, mut tf, mut tc, mut sh)) = texts.get_mut(c) {
                if t.0 != *text {
                    t.0 = text.clone();
                }
                let cur = match tf.font_size {
                    FontSize::Px(v) => v,
                    _ => 0.0,
                };
                if (cur - size).abs() > 0.4 {
                    tf.font_size = FontSize::Px(size);
                }
                if tc.0 != *color {
                    tc.0 = *color;
                    sh.color = Color::srgb_u8(0x1b, 0x1f, 0x33).with_alpha(color.alpha());
                }
            }
            if let Ok((mut img, mut n)) = icons.get_mut(c) {
                match icon {
                    Some(h) => {
                        if img.image != *h {
                            img.image = h.clone();
                        }
                        let a = color.alpha();
                        if img.color.alpha() != a {
                            img.color = Color::WHITE.with_alpha(a);
                        }
                        n.display = Display::Flex;
                        n.width = px(icon_w);
                        n.height = px(icon_w);
                    }
                    None => n.display = Display::None,
                }
            }
        }
    }
    for i in have..req.list.len() {
        commands
            .spawn((
                WorldLabel(i),
                Node {
                    position_type: PositionType::Absolute,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(2),
                    ..default()
                },
                GlobalZIndex(-5),
                Visibility::Hidden,
                bevy::picking::Pickable::IGNORE,
            ))
            .with_children(|p| {
                p.spawn((
                    WorldLabelIcon,
                    ImageNode::default(),
                    Node {
                        display: Display::None,
                        ..default()
                    },
                    bevy::picking::Pickable::IGNORE,
                ));
                p.spawn((
                    WorldLabelText,
                    Text::new(""),
                    TextFont {
                        font: ga.font.clone().into(),
                        font_size: FontSize::Px(15.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    TextShadow {
                        offset: Vec2::new(1.5, 1.5),
                        color: Color::srgb_u8(0x1b, 0x1f, 0x33),
                    },
                    bevy::picking::Pickable::IGNORE,
                ));
            });
    }
}

fn hide_labels(mut q: Query<&mut Visibility, With<WorldLabel>>) {
    for mut v in &mut q {
        *v = Visibility::Hidden;
    }
}
