//! Reusable Feathers-styled building blocks for spatial node graphs.
//!
//! This module owns presentation only: canvas styling, node chrome, socket hit targets, and the
//! anti-aliased wire material. Domain panels retain their semantic graph model and commands.

use crate::theme;
use bevy::{
    asset::embedded_asset,
    feathers::cursor::EntityCursor,
    prelude::*,
    reflect::TypePath,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
    ui_render::prelude::{UiMaterial, UiMaterialPlugin},
    window::SystemCursorIcon,
};

pub(crate) const NODE_WIDTH: f32 = 224.0;
pub(crate) const NODE_HEADER_HEIGHT: f32 = 30.0;
pub(crate) const PORT_ROW_HEIGHT: f32 = 24.0;
pub(crate) const SOCKET_HIT_SIZE: f32 = 20.0;
const SOCKET_SIZE: f32 = 10.0;

pub(crate) struct FeathersNodeGraphPlugin;

impl Plugin for FeathersNodeGraphPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/node_graph_wire.wgsl");
        app.add_plugins(UiMaterialPlugin::<GraphWireMaterial>::default())
            .add_systems(Update, update_socket_visuals);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphSocketSide {
    Input,
    Output,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FeathersGraphSocket {
    pub(crate) color: Color,
}

#[derive(Component, Debug, Clone, Copy)]
struct FeathersGraphSocketDot;

#[derive(Debug, Clone)]
pub(crate) struct GraphNodeProps {
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) position: Vec2,
    pub(crate) selected: bool,
    pub(crate) muted: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphPortProps {
    pub(crate) label: String,
    pub(crate) side: GraphSocketSide,
    pub(crate) color: Color,
}

/// Anti-aliased cubic wire drawn across a graph canvas.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct GraphWireMaterial {
    #[uniform(0)]
    pub(crate) start: Vec2,
    #[uniform(0)]
    pub(crate) control_start: Vec2,
    #[uniform(0)]
    pub(crate) control_end: Vec2,
    #[uniform(0)]
    pub(crate) end: Vec2,
    #[uniform(0)]
    pub(crate) color: Vec4,
    #[uniform(0)]
    pub(crate) width: f32,
}

impl Default for GraphWireMaterial {
    fn default() -> Self {
        Self {
            start: Vec2::ZERO,
            control_start: Vec2::ZERO,
            control_end: Vec2::ZERO,
            end: Vec2::ZERO,
            color: Vec4::new(0.61, 0.47, 1.0, 0.78),
            width: 2.0,
        }
    }
}

impl UiMaterial for GraphWireMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://aestra_editor/feathers/shaders/node_graph_wire.wgsl".into()
    }
}

pub(crate) fn graph_canvas(size: Vec2) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Relative,
            overflow: Overflow::clip(),
            width: Val::Px(size.x),
            height: Val::Px(size.y),
            min_width: Val::Px(size.x),
            min_height: Val::Px(size.y),
            ..default()
        },
        BackgroundColor(theme::VIEWPORT),
    )
}

pub(crate) fn spawn_graph_node<B: Bundle>(
    parent: &mut ChildSpawnerCommands,
    props: GraphNodeProps,
    marker: B,
    body: impl FnOnce(&mut ChildSpawnerCommands),
) -> Entity {
    let border = if props.selected {
        theme::ACCENT
    } else {
        theme::BORDER_BRIGHT
    };
    let background = if props.selected {
        theme::SELECTION
    } else {
        theme::PANEL
    };
    let title_color = if props.muted {
        theme::TEXT_FAINT
    } else {
        theme::TEXT
    };
    let mut node = parent.spawn((
        marker,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(props.position.x),
            top: Val::Px(props.position.y),
            width: Val::Px(NODE_WIDTH),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(Val::Px(if props.selected { 2.0 } else { 1.0 })),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(background),
        BorderColor::all(border),
        BoxShadow::new(
            Color::srgba(0.0, 0.0, 0.0, 0.45),
            Val::Px(0.0),
            Val::Px(3.0),
            Val::Px(10.0),
            Val::Px(0.0),
        ),
    ));
    let entity = node.id();
    node.with_children(|node| {
        node.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(NODE_HEADER_HEIGHT),
                min_height: Val::Px(NODE_HEADER_HEIGHT),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(9.0)),
                column_gap: Val::Px(7.0),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|header| {
            header.spawn((
                Node {
                    width: Val::Px(3.0),
                    height: Val::Px(16.0),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(if props.muted {
                    theme::TEXT_FAINT
                } else {
                    theme::ACCENT
                }),
            ));
            header.spawn((
                Text::new(props.title),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(title_color),
            ));
            header.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            header.spawn((
                Text::new(props.subtitle),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
        node.spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::vertical(Val::Px(5.0)),
            ..default()
        })
        .with_children(body);
    });
    entity
}

pub(crate) fn spawn_graph_port<B: Bundle>(
    parent: &mut ChildSpawnerCommands,
    props: GraphPortProps,
    marker: B,
) -> Entity {
    let row_direction = match props.side {
        GraphSocketSide::Input => FlexDirection::Row,
        GraphSocketSide::Output => FlexDirection::RowReverse,
    };
    let row_align = match props.side {
        GraphSocketSide::Input => JustifyContent::FlexStart,
        GraphSocketSide::Output => JustifyContent::FlexEnd,
    };
    let mut row = parent.spawn(Node {
        width: Val::Percent(100.0),
        height: Val::Px(PORT_ROW_HEIGHT),
        min_height: Val::Px(PORT_ROW_HEIGHT),
        flex_direction: row_direction,
        justify_content: row_align,
        align_items: AlignItems::Center,
        column_gap: Val::Px(5.0),
        padding: UiRect::horizontal(Val::Px(4.0)),
        ..default()
    });
    let mut socket_entity = Entity::PLACEHOLDER;
    row.with_children(|row| {
        socket_entity = row
            .spawn((
                Button,
                marker,
                FeathersGraphSocket { color: props.color },
                EntityCursor::System(SystemCursorIcon::Crosshair),
                Node {
                    width: Val::Px(SOCKET_HIT_SIZE),
                    height: Val::Px(SOCKET_HIT_SIZE),
                    min_width: Val::Px(SOCKET_HIT_SIZE),
                    min_height: Val::Px(SOCKET_HIT_SIZE),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_child((
                FeathersGraphSocketDot,
                Node {
                    width: Val::Px(SOCKET_SIZE),
                    height: Val::Px(SOCKET_SIZE),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(SOCKET_SIZE * 0.5)),
                    ..default()
                },
                BackgroundColor(theme::PANEL_DARK),
                BorderColor::all(props.color),
                Pickable::IGNORE,
            ))
            .id();
        row.spawn((
            Text::new(props.label),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(theme::TEXT_MUTED),
            Pickable::IGNORE,
        ));
    });
    socket_entity
}

fn update_socket_visuals(
    sockets: Query<(&Interaction, &FeathersGraphSocket, &Children), Changed<Interaction>>,
    mut dots: Query<(&mut BackgroundColor, &mut BorderColor), With<FeathersGraphSocketDot>>,
) {
    for (interaction, socket, children) in &sockets {
        for child in children.iter() {
            let Ok((mut background, mut border)) = dots.get_mut(child) else {
                continue;
            };
            match *interaction {
                Interaction::None => {
                    background.0 = theme::PANEL_DARK;
                    *border = BorderColor::all(socket.color);
                }
                Interaction::Hovered => {
                    background.0 = socket.color;
                    *border = BorderColor::all(theme::TEXT);
                }
                Interaction::Pressed => {
                    background.0 = theme::TEXT;
                    *border = BorderColor::all(socket.color);
                }
            }
        }
    }
}
