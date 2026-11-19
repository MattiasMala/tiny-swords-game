//! `Ctl.drawWorld` / `Ctl.drawScreen` (05 §9.6, §13; 07 §8.6): order markers, placement range rings, pick-move
//! highlight, the rally line of a selected trainer, and the screen-space drag-select rectangle. World overlays are
//! gizmos following the terrain heights, drawn over everything (`depth_bias = -1`, the JS draws them after the
//! entities). The ghost building itself is drawn by the sprite renderer (C2, `render::sprites::Ghost`).

use super::ctx::{Ctx, building_foot, building_y1};
use super::frame::FrameInput;
use super::view::ground_h;
use super::{ClickAction, DRAG_MIN, MARKER_LIFE};
use crate::render::consts::PX;
use bevy::prelude::*;
use ts_sim::TILE;
use ts_sim::defs::{BUILD_RADIUS, BuildingKind, DEFENSE_RADIUS, OUTPOST_RADIUS};
use ts_sim::map::Map;

/// Solid overlay lines (markers, brackets), 3 px.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct CtlGizmos;

/// Dashed overlay lines (range rings `[14, 10]`, rally line).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct CtlDashGizmos;

/// Registers the gizmo groups (called by the plugin).
pub fn setup_gizmos(app: &mut App) {
    app.init_gizmo_group::<CtlGizmos>()
        .init_gizmo_group::<CtlDashGizmos>()
        .add_systems(Startup, configure_gizmos);
}

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (c, _) = store.config_mut::<CtlGizmos>();
    c.line.width = 3.0;
    c.depth_bias = -1.0;
    let (c, _) = store.config_mut::<CtlDashGizmos>();
    c.line.width = 3.0;
    c.line.style = GizmoLineStyle::Dashed {
        gap_scale: 10.0 / 3.0,
        line_scale: 14.0 / 3.0,
    };
    c.depth_bias = -1.0;
}

/// Ground point (px) → world, lifted a little above the terrain.
fn gp(map: Option<&Map>, x: f32, y: f32) -> Vec3 {
    Vec3::new(x * PX, ground_h(map, x, y) + 0.04, y * PX)
}

/// Polyline of a ground circle (px centre / radius) following the terrain.
fn ground_circle(map: Option<&Map>, c: Vec2, r: f32, segs: usize) -> Vec<Vec3> {
    (0..=segs)
        .map(|i| {
            let a = i as f32 / segs as f32 * std::f32::consts::TAU;
            gp(map, c.x + a.cos() * r, c.y + a.sin() * r)
        })
        .collect()
}

/// Ground rectangle outline (px).
fn ground_rect(map: Option<&Map>, r: Rect) -> Vec<Vec3> {
    vec![
        gp(map, r.min.x, r.min.y),
        gp(map, r.max.x, r.min.y),
        gp(map, r.max.x, r.max.y),
        gp(map, r.min.x, r.max.y),
        gp(map, r.min.x, r.min.y),
    ]
}

pub fn draw_world(ctx: Ctx, mut solid: Gizmos<CtlGizmos>, mut dash: Gizmos<CtlDashGizmos>) {
    let map = ctx.mirror.map();
    // order markers: ellipse 26×11 shrinking by 60 %, alpha 1 - t/0.6
    for m in &ctx.marks.list {
        let k = (m.t / MARKER_LIFE).clamp(0.0, 1.0);
        let r = 26.0 * (1.0 - 0.6 * k);
        let col = m.kind.color().with_alpha(1.0 - k);
        solid.linestrip(ground_circle(map, m.pos, r, 24), col);
    }
    // rally line of a selected own finished trainer (dashed white 0.6) + pennant pole
    if let Some(bid) = ctx.sel.own_building(&ctx.mirror)
        && let Some(b) = ctx.mirror.view.buildings.get(&bid)
        && b.done
        && !b.kind.def().trains.is_empty()
        && let Some(r) = b.rally
    {
        let from = gp(map, building_foot(b).x, building_y1(b));
        let to = gp(map, r.x, r.y);
        let col = Color::srgba(1.0, 1.0, 1.0, 0.6);
        dash.line(from, to, col);
        let top = to + Vec3::Y * (50.0 * PX);
        solid.line(to, top, Color::WHITE);
        let flag = ctx
            .mirror
            .colors
            .get(b.faction as usize)
            .map_or(Color::WHITE, |c| kingdom_color(*c));
        solid.linestrip(
            [
                top,
                top + Vec3::new(18.0 * PX, -7.0 * PX, 0.0),
                top + Vec3::new(0.0, -14.0 * PX, 0.0),
            ],
            flag,
        );
    }
    // pick-move keyboard highlight (brackets around the footprint)
    if ctx.place.action == ClickAction::PickMove
        && let Some(b) = ctx.place.pick.and_then(|b| ctx.mirror.view.buildings.get(&b))
    {
        let d = b.kind.def();
        let r = Rect::new(
            b.cx as f32 * TILE - 10.0,
            b.cy as f32 * TILE - 14.0,
            (b.cx + d.w) as f32 * TILE + 10.0,
            (b.cy + d.h) as f32 * TILE + 14.0,
        );
        solid.linestrip(ground_rect(map, r), Color::WHITE);
    }
    // placement: range rings + footprint outline
    if let Some(kind) = ctx.place.kind().filter(|_| ctx.place.placing()) {
        let d = kind.def();
        let ring = Color::srgba(1.0, 1.0, 1.0, 0.75);
        let me = ctx.mirror.you;
        if let Some(c) = ctx.mirror.view.buildings.values().find(|b| {
            Some(b.faction) == me
                && b.kind == BuildingKind::Castle
                && !ctx.mirror.view.ghosts.contains(&b.id)
        }) {
            let cd = c.kind.def();
            let centre = Vec2::new(
                (c.cx as f32 + cd.w as f32 / 2.0) * TILE,
                (c.cy as f32 + cd.h as f32 / 2.0) * TILE,
            );
            let r = if d.defense {
                DEFENSE_RADIUS
            } else {
                BUILD_RADIUS
            } * TILE;
            dash.linestrip(ground_circle(map, centre, r, 256), ring);
        }
        for p in &ctx.mirror.view.points {
            if p.owner == me
                && p.kind.def().zone
                && let Some(info) = ctx.scene.points.get(p.id.0 as usize)
            {
                dash.linestrip(
                    ground_circle(map, info.g * TILE, OUTPOST_RADIUS * TILE, 96),
                    ring,
                );
            }
        }
        let c = ctx.place.cell;
        let fr = Rect::new(
            c.x as f32 * TILE,
            c.y as f32 * TILE,
            (c.x + d.w) as f32 * TILE,
            (c.y + d.h) as f32 * TILE,
        );
        let col = if ctx.place.ok {
            Color::srgba(120.0 / 255.0, 1.0, 140.0 / 255.0, 0.9)
        } else {
            Color::srgba(1.0, 70.0 / 255.0, 60.0 / 255.0, 0.9)
        };
        solid.linestrip(ground_rect(map, fr), col);
        // corner brackets `(x0-8, y0-8, w+16, h+16)`
        solid.linestrip(ground_rect(map, fr.inflate(8.0)), Color::srgba(1.0, 1.0, 1.0, 0.5));
    }
}

/// Kingdom colour for small overlays.
fn kingdom_color(c: ts_sim::defs::KingdomColor) -> Color {
    use ts_sim::defs::KingdomColor as K;
    match c {
        K::Blue => Color::srgb_u8(0x3f, 0x7c, 0xd6),
        K::Red => Color::srgb_u8(0xd6, 0x45, 0x3f),
        K::Yellow => Color::srgb_u8(0xe8, 0xc0, 0x3a),
        K::Purple => Color::srgb_u8(0x9a, 0x55, 0xc9),
        K::Black => Color::srgb_u8(0x40, 0x40, 0x48),
    }
}

/// Screen-space drag rectangle (`Ctl.drawScreen`): shown once the travel exceeds `DRAG_MIN`.
#[derive(Component)]
pub struct DragBox;

pub fn drag_box(
    mut commands: Commands,
    fi: Res<FrameInput>,
    mut q: Query<(Entity, &mut Node), With<DragBox>>,
) {
    let rect = match (fi.drag, fi.mouse) {
        (Some(a), Some(b)) if a.distance(b) > DRAG_MIN => Some(Rect::from_corners(a, b)),
        _ => None,
    };
    match (rect, q.single_mut()) {
        (Some(r), Ok((_, mut n))) => {
            n.left = Val::Px(r.min.x);
            n.top = Val::Px(r.min.y);
            n.width = Val::Px(r.width());
            n.height = Val::Px(r.height());
            n.display = Display::Flex;
        }
        (Some(r), Err(_)) => {
            commands.spawn((
                DragBox,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(r.min.x),
                    top: Val::Px(r.min.y),
                    width: Val::Px(r.width()),
                    height: Val::Px(r.height()),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BorderColor::all(Color::WHITE),
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12)),
                GlobalZIndex(900),
                bevy::picking::Pickable::IGNORE,
            ));
        }
        (None, Ok((_, mut n))) => {
            if n.display != Display::None {
                n.display = Display::None;
            }
        }
        (None, Err(_)) => {}
    }
}
