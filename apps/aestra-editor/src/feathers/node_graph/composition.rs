//! Production canvas, node chrome and typed socket composition shared across engine boundaries.
use super::{
    composition_input::NodeButton,
    graph_cursor::EntityCursor,
    metrics::{NODE_HEADER_HEIGHT, NODE_PREVIEW_SIZE, NODE_WIDTH, PORT_ROW_HEIGHT},
    navigation::{FeathersGraphCanvas, FeathersGraphViewport, GraphFrameTarget},
    node::{FeathersGraphNode, FeathersGraphNodePreview},
    preview_toggle::FeathersGraphNodePreviewToggle,
    socket::GraphSocketSide,
    wire_layer::FeathersGraphWireLayer,
};
use crate::{
    feathers::{
        action_activation::FeathersActionButton,
        icon::{SvgColor, SvgFile, UiSvg},
        scenes,
        tooltip::{EditorTooltip, EditorTooltipSide},
    },
    theme,
};
use bevy::{prelude::*, ui::RelativeCursorPosition, window::SystemCursorIcon};

pub(crate) const SOCKET_HIT_SIZE: f32 = 20.0;
const SOCKET_SIZE: f32 = 10.0;
pub(super) const SELECTED_BORDER_WIDTH: f32 = 2.0;

#[derive(Debug, Clone)]
pub(crate) struct GraphViewportProps {
    pub(crate) key: String,
    pub(crate) content_size: Vec2,
    pub(crate) selection_bounds: Option<Rect>,
    /// Seed pan/zoom for a viewport whose `key` has no remembered camera yet (a freshly opened or
    /// split view seeded from the document camera). `None` leaves the viewport to auto-frame.
    pub(crate) initial_view: Option<(Vec2, f32)>,
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct FeathersGraphGrid {
    pub(super) viewport: Entity,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FeathersGraphSocket {
    pub(crate) color: Color,
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct FeathersGraphSocketDot;

#[derive(Debug, Clone)]
pub(crate) struct GraphNodeProps {
    pub(crate) graph_key: String,
    pub(crate) node_key: String,
    pub(crate) title: String,
    pub(crate) position: Vec2,
    pub(crate) selected: bool,
    pub(crate) pinned: bool,
    pub(crate) muted: bool,
    pub(crate) collapse_icon: Handle<SvgFile>,
    pub(crate) expand_icon: Handle<SvgFile>,
    pub(crate) pin_icon: Handle<SvgFile>,
    pub(crate) collapse_label: String,
    pub(crate) expand_label: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphNodePreviewToggleProps {
    pub(crate) visible: bool,
    pub(crate) show_icon: Handle<SvgFile>,
    pub(crate) hide_icon: Handle<SvgFile>,
    pub(crate) show_label: String,
    pub(crate) hide_label: String,
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct FeathersGraphNodeBody {
    pub(super) node: Entity,
}

#[derive(Component, Debug, Clone)]
pub(super) struct GraphCollapseAction {
    pub(super) node: Entity,
    pub(super) collapse_label: String,
    pub(super) expand_label: String,
}

#[derive(Component, Debug, Clone)]
pub(super) struct GraphCollapseIcon {
    pub(super) node: Entity,
    pub(super) collapse_icon: Handle<SvgFile>,
    pub(super) expand_icon: Handle<SvgFile>,
}

#[derive(Component, Debug, Clone)]
pub(super) struct GraphPinAction {
    pub(super) node: Entity,
}

#[derive(Component, Debug, Clone)]
pub(super) struct GraphPinIcon {
    pub(super) node: Entity,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphPortProps {
    pub(crate) label: Option<String>,
    pub(crate) tooltip_title: String,
    pub(crate) tooltip_description: String,
    pub(crate) side: GraphSocketSide,
    pub(crate) color: Color,
}

pub(crate) fn graph_canvas(size: Vec2) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Relative,
            overflow: Overflow::visible(),
            width: Val::Px(size.x),
            height: Val::Px(size.y),
            min_width: Val::Px(size.x),
            min_height: Val::Px(size.y),
            ..default()
        },
        BackgroundColor(Color::NONE),
    )
}

pub(crate) fn graph_canvas_bundle<B: Bundle>(
    viewport: Entity,
    size: Vec2,
    marker: B,
) -> impl Bundle {
    (
        graph_canvas(size),
        marker,
        FeathersGraphCanvas { viewport },
        UiTransform::IDENTITY,
    )
}

fn graph_wire_layer_bundle(viewport: Entity) -> impl Bundle {
    (
        FeathersGraphWireLayer { viewport },
        UiTransform::IDENTITY,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            overflow: Overflow::visible(),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            ..default()
        },
        Pickable::IGNORE,
    )
}

pub(crate) fn spawn_graph_viewport<B: Bundle>(
    parent: &mut ChildSpawnerCommands,
    props: GraphViewportProps,
    canvas_marker: B,
    overlay: impl FnOnce(&mut ChildSpawnerCommands),
    content: impl FnOnce(&mut ChildSpawnerCommands),
) -> Entity {
    let mut viewport = parent.spawn((
        Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            position_type: PositionType::Relative,
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(theme::VIEWPORT),
        RelativeCursorPosition::default(),
        Pickable {
            should_block_lower: true,
            is_hoverable: true,
        },
        // Empty canvas keeps the ordinary arrow. Marquee selection begins with primary drag,
        // while pan gestures switch the global cursor to `Grabbing` only while active.
        EntityCursor::System(SystemCursorIcon::Default),
        FeathersGraphViewport {
            key: props.key,
            pan: props.initial_view.map_or(Vec2::ZERO, |(pan, _)| pan),
            zoom: props.initial_view.map_or(1.0, |(_, zoom)| zoom),
            content_size: props.content_size,
            selection_bounds: props.selection_bounds,
            measured_frame: None,
            // A seeded camera is honoured immediately; only an unseeded viewport auto-frames.
            frame_request: props
                .initial_view
                .is_none()
                .then_some(GraphFrameTarget::All),
        },
    ));
    let entity = viewport.id();
    viewport.with_children(|viewport| {
        viewport.spawn((
            FeathersGraphGrid { viewport: entity },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            Pickable::IGNORE,
        ));
        viewport
            .spawn(graph_wire_layer_bundle(entity))
            .with_children(overlay);
        viewport
            .spawn(graph_canvas_bundle(
                entity,
                props.content_size,
                canvas_marker,
            ))
            .with_children(content);
    });
    entity
}

pub(crate) fn spawn_graph_node<B: Bundle>(
    parent: &mut ChildSpawnerCommands,
    props: GraphNodeProps,
    marker: B,
    body: impl FnOnce(Entity, &mut ChildSpawnerCommands),
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
    let graph_key = props.graph_key.clone();
    let node_key = props.node_key.clone();
    let collapse_icon = props.collapse_icon.clone();
    let expand_icon = props.expand_icon.clone();
    let pin_icon = props.pin_icon.clone();
    let collapse_label = props.collapse_label.clone();
    let expand_label = props.expand_label.clone();
    let mut node = parent.spawn((
        marker,
        FeathersGraphNode {
            graph_key,
            node_key,
            position: props.position,
            selected: props.selected,
            pinned: props.pinned,
            collapsed: false,
            dragging: false,
            drag_before: None,
            drag_modifier: None,
            suppress_release_click: false,
        },
        Pickable {
            should_block_lower: true,
            is_hoverable: true,
        },
        EntityCursor::System(SystemCursorIcon::Grab),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(props.position.x),
            top: Val::Px(props.position.y),
            width: Val::Px(NODE_WIDTH),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(Val::Px(if props.selected {
                SELECTED_BORDER_WIDTH
            } else {
                1.0
            })),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(background),
        BorderColor::all(border),
        Outline::new(
            Val::Px(0.0),
            Val::ZERO,
            if props.selected {
                theme::ACCENT
            } else {
                Color::NONE
            },
        ),
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
            let mut pin = header.spawn_empty();
            pin.apply_scene(scenes::feathers_tool_button());
            pin.insert((
                GraphPinAction { node: entity },
                FeathersActionButton,
                AccessibleLabel(if props.pinned {
                    "Unpin node".into()
                } else {
                    "Pin node".into()
                }),
                Node {
                    width: Val::Px(22.0),
                    height: Val::Px(22.0),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
            ))
            .with_child((
                GraphPinIcon { node: entity },
                Node {
                    width: Val::Px(11.0),
                    height: Val::Px(11.0),
                    ..default()
                },
                UiSvg(pin_icon),
                SvgColor(if props.pinned {
                    theme::ACCENT
                } else {
                    theme::TEXT_FAINT
                }),
                Pickable::IGNORE,
            ));
            let mut disclosure = header.spawn_empty();
            disclosure.apply_scene(scenes::feathers_tool_button());
            disclosure
                .insert((
                    GraphCollapseAction {
                        node: entity,
                        collapse_label: collapse_label.clone(),
                        expand_label: expand_label.clone(),
                    },
                    FeathersActionButton,
                    AccessibleLabel(collapse_label.clone()),
                    Node {
                        width: Val::Px(22.0),
                        height: Val::Px(22.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                ))
                .with_child((
                    GraphCollapseIcon {
                        node: entity,
                        collapse_icon: collapse_icon.clone(),
                        expand_icon,
                    },
                    Node {
                        width: Val::Px(11.0),
                        height: Val::Px(11.0),
                        ..default()
                    },
                    UiSvg(collapse_icon),
                    SvgColor(theme::TEXT),
                    Pickable::IGNORE,
                ));
        });
        node.spawn((
            FeathersGraphNodeBody { node: entity },
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(PORT_ROW_HEIGHT + 10.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::vertical(Val::Px(5.0)),
                ..default()
            },
        ))
        .with_children(|children| body(entity, children));
    });
    entity
}

pub(crate) fn spawn_graph_node_preview_toggle<B: Bundle>(
    parent: &mut ChildSpawnerCommands,
    props: GraphNodePreviewToggleProps,
    marker: B,
) -> Entity {
    let (icon, label) = if props.visible {
        (props.hide_icon, props.hide_label)
    } else {
        (props.show_icon, props.show_label)
    };
    let mut toggle = parent.spawn_empty();
    toggle.apply_scene(scenes::feathers_tool_button());
    toggle
        .insert((
            NodeButton,
            FeathersActionButton,
            FeathersGraphNodePreviewToggle,
            marker,
            AccessibleLabel(label.clone()),
            EditorTooltip::description(label),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(4.0),
                right: Val::Px(34.0),
                width: Val::Px(22.0),
                height: Val::Px(22.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_child((
            Node {
                width: Val::Px(12.0),
                height: Val::Px(12.0),
                ..default()
            },
            UiSvg(icon),
            SvgColor(theme::TEXT),
            Pickable::IGNORE,
        ));
    toggle.id()
}

pub(crate) fn spawn_graph_node_preview<B: Bundle>(
    parent: &mut ChildSpawnerCommands,
    marker: B,
) -> Entity {
    parent
        .spawn((
            FeathersGraphNodePreview,
            marker,
            Node {
                width: Val::Px(NODE_PREVIEW_SIZE),
                height: Val::Px(NODE_PREVIEW_SIZE),
                min_width: Val::Px(NODE_PREVIEW_SIZE),
                min_height: Val::Px(NODE_PREVIEW_SIZE),
                flex_shrink: 0.0,
                align_self: AlignSelf::Center,
                margin: UiRect::new(Val::Px(7.0), Val::Px(7.0), Val::Px(5.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::VIEWPORT),
            BorderColor::all(theme::BORDER),
            Pickable::IGNORE,
        ))
        .id()
}

pub(crate) fn spawn_graph_port<B: Bundle>(
    parent: &mut ChildSpawnerCommands,
    props: GraphPortProps,
    marker: B,
) -> Entity {
    spawn_graph_port_with(parent, props, marker, |_| {})
}

/// Spawns a typed port with an optional inline control after its label.
pub(crate) fn spawn_graph_port_with<B: Bundle>(
    parent: &mut ChildSpawnerCommands,
    props: GraphPortProps,
    marker: B,
    accessory: impl FnOnce(&mut ChildSpawnerCommands),
) -> Entity {
    let mut row = parent.spawn((graph_port_row_node(props.side), Pickable::IGNORE));
    let mut socket_entity = Entity::PLACEHOLDER;
    row.with_children(|row| {
        socket_entity = row
            .spawn((
                NodeButton,
                marker,
                FeathersGraphSocket { color: props.color },
                AccessibleLabel(format!(
                    "{}. {}",
                    props.tooltip_title, props.tooltip_description
                )),
                EditorTooltip::titled(props.tooltip_title, props.tooltip_description)
                    .with_preferred_side(match props.side {
                        GraphSocketSide::Input => EditorTooltipSide::Left,
                        GraphSocketSide::Output => EditorTooltipSide::Right,
                    })
                    // Sockets live under the pan/zoom canvas transform and clip, so anchor the
                    // tooltip beside the cursor rather than the (mis-placed, clipped) element.
                    .anchored_to_cursor(),
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
        if let Some(label) = props.label {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
                Pickable::IGNORE,
            ));
        }
        accessory(row);
    });
    socket_entity
}

pub(super) fn graph_port_row_node(side: GraphSocketSide) -> Node {
    let (flex_direction, justify_content, position_type, top, right) = match side {
        GraphSocketSide::Input => (
            FlexDirection::Row,
            JustifyContent::FlexStart,
            PositionType::Relative,
            Val::Auto,
            Val::Auto,
        ),
        GraphSocketSide::Output => (
            FlexDirection::RowReverse,
            JustifyContent::FlexStart,
            PositionType::Absolute,
            Val::Px(5.0),
            Val::Px(0.0),
        ),
    };
    Node {
        width: Val::Percent(100.0),
        height: Val::Px(PORT_ROW_HEIGHT),
        min_height: Val::Px(PORT_ROW_HEIGHT),
        flex_direction,
        justify_content,
        align_items: AlignItems::Center,
        column_gap: Val::Px(5.0),
        padding: UiRect::horizontal(Val::Px(4.0)),
        position_type,
        top,
        right,
        ..default()
    }
}
