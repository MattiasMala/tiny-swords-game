// OWNER: C3 (ui) — spec 06-ui.md (§0 font/palette/label, §4 main menu, §5 lobby, §6+ HUD, panels, screens, §13 notes),
// 05-control-loop.md (UI input routing).
//
// SKELETON: plain bevy_ui screens with the real strings, headless `ui_widgets::Button` + `On<Activate>`:
//   * Menu  — `TINY SWORDS`, `La Guerra dei Cinque Regni`, `Scegli il tuo regno` (5 kingdoms), `Difficoltà dei
//             regni avversari` (3 levels), `GIOCA` (solo: local room), `MULTIGIOCATORE` (remote link → Lobby), note.
//   * Lobby — `MULTIGIOCATORE`, `Ingresso nella partita…` / player count, players, `INIZIA` (leader) or the waiting
//             line, LAN address, back (`Menu`).
// C3 TODO: the real art (store banner, ribbons, pennants, avatars, animated level units, 9-slice buttons), HUD,
// panels, screens, help overlay, toasts/banners from `GameEventMsg`, scale `s` formulas.

use crate::AppState;
use crate::assets::GameAssets;
use crate::link::{LinkRequest, LobbyState, NetOut};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::{Activate, Button as WButton, observe};
use ts_sim::defs::{AiLevel, KingdomColor};
use ts_sim::proto::ClientMsg;

/// Ink colour of text on parchment (`INK`).
pub const INK: Color = Color::srgb(0.106, 0.122, 0.2);
/// Heading colour `#8a3b2e`.
pub const HEADING: Color = Color::srgb(0.541, 0.231, 0.18);
const PANEL: Color = Color::srgb(0.96, 0.91, 0.78);
const BTN: Color = Color::srgb(0.27, 0.55, 0.78);
const BTN_SEL: Color = Color::srgb(0.95, 0.78, 0.3);

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Menu), spawn_menu)
            .add_systems(OnEnter(AppState::Lobby), spawn_lobby)
            .add_systems(
                Update,
                (
                    button_feedback,
                    refresh_menu.run_if(in_state(AppState::Menu)),
                    refresh_lobby.run_if(in_state(AppState::Lobby)),
                ),
            );
    }
}

/// Button that selects a kingdom colour in the menu.
#[derive(Component)]
struct ColorChoice(KingdomColor);

/// Button that selects the AI level in the menu.
#[derive(Component)]
struct LevelChoice(AiLevel);

/// Base colour of a button (feedback multiplies it).
#[derive(Component, Clone, Copy)]
struct BtnColor(Color);

#[derive(Component)]
struct NoteText;

#[derive(Component)]
struct LobbyText;

#[derive(Component)]
struct LobbyAddress;

#[derive(Component)]
struct StartButton;

fn text(font: &Handle<Font>, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        TextLayout::justify(Justify::Center),
    )
}

fn button(font: &Handle<Font>, label: &str, w: f32, h: f32, size: f32) -> impl Bundle {
    (
        WButton,
        Hovered::default(),
        BtnColor(BTN),
        Node {
            width: px(w),
            height: px(h),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(BTN),
        children![text(font, label, size, Color::WHITE)],
    )
}

fn panel() -> impl Bundle {
    (
        Node {
            width: px(720),
            max_width: percent(96),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(14),
            padding: UiRect::all(px(24)),
            border_radius: BorderRadius::all(px(12)),
            ..default()
        },
        BackgroundColor(PANEL),
    )
}

fn root(state: AppState) -> impl Bundle {
    (
        DespawnOnExit(state),
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
    )
}

fn row() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        column_gap: px(12),
        justify_content: JustifyContent::Center,
        flex_wrap: FlexWrap::Wrap,
        row_gap: px(8),
        ..default()
    }
}

fn spawn_menu(mut commands: Commands, ga: Res<GameAssets>) {
    let f = ga.font.clone();
    commands.spawn(root(AppState::Menu)).with_children(|r| {
        r.spawn(panel()).with_children(|p| {
            p.spawn(text(&f, "TINY SWORDS", 42.0, INK));
            p.spawn(text(&f, "La Guerra dei Cinque Regni", 22.0, INK));
            p.spawn(text(&f, "Scegli il tuo regno", 17.0, HEADING));
            p.spawn(row()).with_children(|row| {
                for &c in KingdomColor::ALL {
                    row.spawn((ColorChoice(c), button(&f, c.name_it(), 104.0, 44.0, 16.0)))
                        .observe(move |_a: On<Activate>, mut lobby: ResMut<LobbyState>| {
                            lobby.color = c;
                        });
                }
            });
            p.spawn(text(&f, "Difficoltà dei regni avversari", 17.0, HEADING));
            p.spawn(row()).with_children(|row| {
                for &l in AiLevel::ALL {
                    row.spawn((LevelChoice(l), button(&f, l.label(), 160.0, 44.0, 16.0)))
                        .observe(move |_a: On<Activate>, mut lobby: ResMut<LobbyState>| {
                            lobby.level = l;
                        });
                }
            });
            p.spawn(row()).with_children(|row| {
                row.spawn((
                    button(&f, "GIOCA", 200.0, 64.0, 24.0),
                    observe(
                        |_a: On<Activate>,
                         lobby: Res<LobbyState>,
                         mut req: MessageWriter<LinkRequest>| {
                            req.write(LinkRequest::StartSolo {
                                color: lobby.color,
                                level: lobby.level,
                            });
                        },
                    ),
                ));
                row.spawn((
                    button(&f, "MULTIGIOCATORE", 320.0, 64.0, 24.0),
                    observe(|_a: On<Activate>, mut req: MessageWriter<LinkRequest>| {
                        req.write(LinkRequest::ConnectRemote);
                    }),
                ));
            });
            p.spawn((NoteText, text(&f, "", 16.0, HEADING)));
        });
    });
}

fn spawn_lobby(mut commands: Commands, ga: Res<GameAssets>) {
    let f = ga.font.clone();
    commands.spawn(root(AppState::Lobby)).with_children(|r| {
        r.spawn(panel()).with_children(|p| {
            p.spawn(text(&f, "MULTIGIOCATORE", 38.0, INK));
            p.spawn((LobbyText, text(&f, "Ingresso nella partita…", 18.0, INK)));
            p.spawn(row()).with_children(|row| {
                row.spawn((
                    StartButton,
                    button(&f, "INIZIA", 260.0, 64.0, 24.0),
                    observe(|_a: On<Activate>, mut out: ResMut<NetOut>| {
                        out.send(ClientMsg::Start);
                    }),
                ));
                row.spawn((
                    button(&f, "Menu", 160.0, 64.0, 22.0),
                    observe(|_a: On<Activate>, mut req: MessageWriter<LinkRequest>| {
                        req.write(LinkRequest::Close { leave: true });
                    }),
                ));
            });
            p.spawn((LobbyAddress, text(&f, "", 17.0, HEADING)));
        });
    });
}

/// Hover / press / selection feedback.
#[allow(clippy::type_complexity)]
fn button_feedback(
    lobby: Res<LobbyState>,
    mut q: Query<(
        &Hovered,
        Has<Pressed>,
        &BtnColor,
        &mut BackgroundColor,
        Option<&ColorChoice>,
        Option<&LevelChoice>,
    )>,
) {
    for (hovered, pressed, base, mut bg, cc, lc) in &mut q {
        let selected =
            cc.is_some_and(|c| c.0 == lobby.color) || lc.is_some_and(|l| l.0 == lobby.level);
        let c = if selected { BTN_SEL } else { base.0 };
        let k = match (pressed, hovered.get()) {
            (true, _) => 0.8,
            (false, true) => 1.15,
            _ => 1.0,
        };
        let s = c.to_srgba();
        let v = Color::srgb(
            (s.red * k).min(1.0),
            (s.green * k).min(1.0),
            (s.blue * k).min(1.0),
        );
        if bg.0 != v {
            bg.0 = v;
        }
    }
}

fn refresh_menu(lobby: Res<LobbyState>, mut note: Query<&mut Text, With<NoteText>>) {
    if !lobby.is_changed() {
        return;
    }
    for mut t in &mut note {
        t.0 = lobby.note.clone().unwrap_or_default();
    }
}

#[allow(clippy::type_complexity)]
fn refresh_lobby(
    lobby: Res<LobbyState>,
    mut texts: ParamSet<(
        Query<&mut Text, With<LobbyText>>,
        Query<&mut Text, With<LobbyAddress>>,
    )>,
    mut start: Query<&mut Node, With<StartButton>>,
) {
    if !lobby.is_changed() {
        return;
    }
    let me = lobby
        .you
        .and_then(|id| lobby.players.iter().find(|p| p.id == id));
    let line = if me.is_none() {
        "Ingresso nella partita…".to_string()
    } else {
        let mut s = format!(
            "Giocatori: {} su {} · i regni liberi li guida l'IA",
            lobby.players.len(),
            KingdomColor::ALL.len()
        );
        for p in &lobby.players {
            let who = if Some(p.id) == lobby.you {
                "Tu"
            } else if Some(p.id) == lobby.leader {
                "Host"
            } else {
                "Giocatore"
            };
            s.push_str(&format!("\n{} — {}", p.color.name_it(), who));
        }
        if !lobby.is_leader() {
            s.push_str("\nIn attesa che l'host avvii la partita…");
        }
        s
    };
    for mut t in &mut texts.p0() {
        t.0 = line.clone();
    }
    let addr = if lobby.is_leader() {
        let where_ = lobby
            .urls
            .first()
            .cloned()
            .unwrap_or_else(|| "http://<IP di questo PC>:8000".to_string());
        format!("Gli altri giocatori aprono  {where_}")
    } else {
        String::new()
    };
    for mut t in &mut texts.p1() {
        t.0 = addr.clone();
    }
    for mut n in &mut start {
        n.display = if lobby.is_leader() {
            Display::Flex
        } else {
            Display::None
        };
    }
}
