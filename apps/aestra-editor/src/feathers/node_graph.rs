//! Reusable Feathers-styled building blocks for spatial node graphs.
//!
//! This module owns presentation only: canvas styling, node chrome, socket hit targets, and the
//! anti-aliased wire material. Domain panels retain their semantic graph model and commands.

use super::number_input::EditorNumberInput;
use super::{
    button::{FeathersActionButton, PendingFeathersActivation},
    icon::load_svg_icon,
    scenes,
    tooltip::{EditorTooltip, EditorTooltipSide},
};
use crate::feathers::icon::{SvgColor, SvgFile, UiSvg};
use crate::theme;
#[cfg(test)]
use bevy::picking::{
    events::{Drag, DragEnd, DragStart, Pointer},
    pointer::PointerButton,
};
use bevy::{
    asset::embedded_asset,
    feathers::controls::{FeathersCheckbox, FeathersTextInputContainer},
    prelude::*,
    ui::RelativeCursorPosition,
    ui_render::prelude::UiMaterialPlugin,
    ui_widgets::Activate,
    window::SystemCursorIcon,
};
#[cfg(test)]
use std::collections::BTreeSet;

mod drag_assist;
mod drag_context;
mod memory;
pub(crate) use memory::GraphViewportMemory;
mod drag;
use drag::*;
#[path = "node_graph/interaction/pointer_019.rs"]
mod graph_pointer;
mod node;
use node::begin_graph_node_press;
pub(crate) use node::{
    FeathersGraphNode, FeathersGraphNodePreview, GraphModifiedDragTarget, GraphModifiedNodeDrag,
    GraphNodeDragModifier, GraphPinEdit, GraphPresentationBatchEdit, GraphPresentationEdit,
};
mod marquee;
use marquee::{
    GraphMarqueeGesture, begin_graph_marquee, cancel_graph_marquee, cleanup_graph_marquee,
    end_graph_marquee, update_graph_marquee,
};
pub(crate) use marquee::{GraphMarqueeSelection, GraphSelectionMode};
#[path = "node_graph/navigation/cursor_019.rs"]
mod graph_cursor;
use graph_cursor::EntityCursor;
#[cfg(test)]
use graph_cursor::OverrideCursor;
mod navigation;
use navigation::*;
pub(crate) use navigation::{
    FeathersGraphNavigationBlocker, FeathersGraphViewport, GraphFrameTarget,
};
mod materials;
use materials::GraphGridMaterial;
pub(crate) use materials::GraphWireMaterial;
mod framing;
pub(crate) use drag_assist::spawn_controls as spawn_graph_drag_controls;
pub(crate) mod geometry;
pub(crate) mod insertion;
#[cfg(test)]
#[path = "node_graph/insertion/tests.rs"]
mod insertion_tests;
mod overlay;
pub(crate) mod placement;
mod resize;

pub(crate) const NODE_WIDTH: f32 = 224.0;
pub(crate) const NODE_HEADER_HEIGHT: f32 = 30.0;
pub(crate) const PORT_ROW_HEIGHT: f32 = 24.0;
pub(crate) const NODE_PREVIEW_SIZE: f32 = 208.0;
pub(crate) const SOCKET_HIT_SIZE: f32 = 20.0;
const SOCKET_SIZE: f32 = 10.0;
const SELECTED_BORDER_WIDTH: f32 = 2.0;
const MIN_SELECTED_BORDER_SCREEN_WIDTH: f32 = 2.0;

type GraphNodeControlFilter = Or<(
    With<FeathersGraphSocket>,
    With<GraphCollapseAction>,
    With<EditorNumberInput>,
    With<FeathersCheckbox>,
    With<FeathersTextInputContainer>,
    With<FeathersActionButton>,
)>;

pub(crate) struct FeathersNodeGraphPlugin;

/// Hosts order graph restoration/navigation after their own document lifecycle and UI rebuild.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GraphWidgetSync;

/// Shared toolbar chrome for material programs and function bodies.
pub(crate) fn graph_toolbar_bundle() -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(38.0),
            min_height: Val::Px(38.0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(Val::Px(8.0)),
            column_gap: Val::Px(4.0),
            ..default()
        },
        BackgroundColor(theme::PANEL_LIGHT),
    )
}

pub(crate) fn spawn_graph_toolbar_summary(parent: &mut ChildSpawnerCommands, text: String) {
    parent.spawn(Node {
        flex_grow: 1.0,
        ..default()
    });
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::TEXT_FAINT),
    ));
}

pub(crate) fn spawn_graph_tool_button<A: Component>(
    parent: &mut ChildSpawnerCommands,
    assets: &AssetServer,
    icon_path: &'static str,
    label: String,
    action: A,
) -> Entity {
    parent
        .spawn_empty()
        .apply_scene(scenes::feathers_tool_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
            EditorTooltip::description(label),
            Node {
                width: Val::Px(28.0),
                height: Val::Px(24.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_child((
            Node {
                width: Val::Px(15.0),
                height: Val::Px(15.0),
                ..default()
            },
            UiSvg(load_svg_icon(assets, icon_path)),
            SvgColor(Color::WHITE),
            Pickable::IGNORE,
        ))
        .id()
}

impl Plugin for FeathersNodeGraphPlugin {
    fn build(&self, app: &mut App) {
        geometry::register(app);
        embedded_asset!(app, "shaders/node_graph_wire.wgsl");
        embedded_asset!(app, "shaders/node_graph_grid.wgsl");
        app.add_plugins(UiMaterialPlugin::<GraphWireMaterial>::default())
            .add_plugins(UiMaterialPlugin::<GraphGridMaterial>::default())
            .init_resource::<GraphViewportMemory>()
            .init_resource::<overlay::GraphOverlays>()
            .init_resource::<drag_assist::State>()
            .init_resource::<GraphPanGesture>()
            .init_resource::<GraphMarqueeGesture>()
            .init_resource::<GraphNodeDragGesture>()
            .add_observer(queue_graph_frame_activation)
            .add_observer(queue_graph_collapse_activation)
            .add_observer(queue_graph_pin_activation)
            .add_observer(begin_graph_node_press::<GraphNodeControlFilter>)
            .add_observer(begin_graph_node_drag::<GraphNodeControlFilter>)
            .add_observer(drag_graph_node)
            .add_observer(end_graph_node_drag)
            .add_observer(begin_graph_marquee::<GraphNodeControlFilter>)
            .add_observer(update_graph_marquee)
            .add_observer(end_graph_marquee)
            .add_observer(cancel_graph_marquee)
            .add_observer(drag_assist::toggle)
            .add_systems(
                Update,
                (
                    attach_graph_grid_materials,
                    remember_graph_viewports,
                    restore_graph_viewports,
                    restore_graph_nodes,
                    overlay::reconcile,
                    overlay::sync_notice,
                    sync_graph_nodes_from_memory,
                    handle_graph_frame_buttons,
                    handle_graph_collapse_buttons,
                    handle_graph_pin_buttons,
                    navigate_graph_viewports,
                    cleanup_graph_marquee,
                    sync_graph_viewport_transforms,
                    sync_graph_selection_outlines,
                    update_socket_visuals,
                    drag_assist::sync_controls,
                    drag_assist::sync_guides,
                )
                    .chain()
                    .in_set(GraphWidgetSync),
            )
            .add_systems(
                PostUpdate,
                sync_graph_grid_materials.after(bevy::ui::UiSystems::Layout),
            );
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GraphViewportProps {
    pub(crate) key: String,
    pub(crate) content_size: Vec2,
    pub(crate) selection_bounds: Option<Rect>,
    /// Seed pan/zoom for a viewport whose `key` has no remembered camera yet (a freshly opened or
    /// split view seeded from the document camera). `None` leaves the viewport to auto-frame.
    pub(crate) initial_view: Option<(Vec2, f32)>,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct GraphFrameAction {
    key: String,
    target: GraphFrameTarget,
}

impl GraphFrameAction {
    pub(crate) fn new(key: impl Into<String>, target: GraphFrameTarget) -> Self {
        Self {
            key: key.into(),
            target,
        }
    }
}

/// Graph-space layer rendered below the node canvas with the exact same pan/zoom transform.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FeathersGraphWireLayer {
    pub(crate) viewport: Entity,
}

#[derive(Component, Debug, Clone, Copy)]
struct FeathersGraphGrid {
    viewport: Entity,
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
pub(crate) struct FeathersGraphNodePreviewToggle;

/// Returns the graph-local proxy anchor used by wires while a node is compact.
///
/// A compact node hides its body and therefore its real socket entities. Their cached UI
/// transforms must not be used: they describe the expanded body and can lag behind the compact
/// layout. Instead, all inputs converge on the left edge of the header and all outputs on the
/// right edge. Expanded nodes keep using their measured socket positions.
pub(crate) fn compact_graph_socket_offset(
    node: &FeathersGraphNode,
    computed: &ComputedNode,
    side: GraphSocketSide,
) -> Option<Vec2> {
    let size = computed.size() * computed.inverse_scale_factor;
    compact_graph_socket_offset_for_size(node.collapsed(), size, side)
}

fn compact_graph_socket_offset_for_size(
    collapsed: bool,
    size: Vec2,
    side: GraphSocketSide,
) -> Option<Vec2> {
    if !collapsed || !size.is_finite() || size.min_element() <= 0.0 {
        return None;
    }

    Some(Vec2::new(
        match side {
            GraphSocketSide::Input => 0.0,
            GraphSocketSide::Output => size.x,
        },
        size.y * 0.5,
    ))
}

#[derive(Component, Debug, Clone, Copy)]
struct FeathersGraphNodeBody {
    node: Entity,
}

#[derive(Component, Debug, Clone)]
struct GraphCollapseAction {
    node: Entity,
    collapse_label: String,
    expand_label: String,
}

#[derive(Component, Debug, Clone)]
struct GraphCollapseIcon {
    node: Entity,
    collapse_icon: Handle<SvgFile>,
    expand_icon: Handle<SvgFile>,
}

#[derive(Component, Debug, Clone)]
struct GraphPinAction {
    node: Entity,
}

#[derive(Component, Debug, Clone)]
struct GraphPinIcon {
    node: Entity,
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

pub(crate) fn spawn_graph_frame_button(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    icon_path: &'static str,
    label: String,
    action: GraphFrameAction,
) -> Entity {
    let mut button = parent.spawn_empty();
    button.apply_scene(scenes::feathers_tool_button());
    let entity = button.id();
    button
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
            EditorTooltip::description(label),
            Node {
                width: Val::Px(28.0),
                height: Val::Px(24.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_child((
            Node {
                width: Val::Px(15.0),
                height: Val::Px(15.0),
                ..default()
            },
            UiSvg(load_svg_icon(asset_server, icon_path)),
            SvgColor(Color::WHITE),
            Pickable::IGNORE,
        ));
    entity
}

fn queue_graph_frame_activation(
    activate: On<Activate>,
    actions: Query<(), (With<GraphFrameAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

fn handle_graph_frame_buttons(
    mut commands: Commands,
    actions: Query<
        (
            Entity,
            &Interaction,
            &GraphFrameAction,
            Option<&PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
    mut viewports: Query<&mut FeathersGraphViewport>,
) {
    for (entity, interaction, action, pending) in &actions {
        if *interaction != Interaction::Pressed || pending.is_none() {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>()
            .insert(Interaction::None);
        for mut viewport in &mut viewports {
            if viewport.key == action.key {
                viewport.frame_request = Some(action.target);
            }
        }
    }
}

fn remember_graph_viewports(
    mut memory: ResMut<GraphViewportMemory>,
    viewports: Query<Ref<FeathersGraphViewport>>,
) {
    for viewport in &viewports {
        if viewport.is_added() {
            continue;
        }
        let next = GraphView {
            pan: viewport.pan,
            zoom: viewport.zoom,
        };
        if memory.views.get(&viewport.key) != Some(&next) {
            memory.views.insert(viewport.key.clone(), next);
        }
    }
}

fn restore_graph_viewports(
    memory: Res<GraphViewportMemory>,
    mut viewports: Query<&mut FeathersGraphViewport, Added<FeathersGraphViewport>>,
) {
    for mut viewport in &mut viewports {
        let Some(view) = memory.views.get(&viewport.key) else {
            continue;
        };
        viewport.pan = view.pan;
        viewport.zoom = view.zoom;
        viewport.frame_request = None;
    }
}

fn attach_graph_grid_materials(
    grids: Query<
        Entity,
        (
            With<FeathersGraphGrid>,
            Without<MaterialNode<GraphGridMaterial>>,
        ),
    >,
    mut materials: ResMut<Assets<GraphGridMaterial>>,
    mut commands: Commands,
) {
    for entity in &grids {
        commands
            .entity(entity)
            .insert(MaterialNode(materials.add(GraphGridMaterial::default())));
    }
}

fn sync_graph_grid_materials(
    viewports: Query<&FeathersGraphViewport>,
    grids: Query<(&FeathersGraphGrid, &MaterialNode<GraphGridMaterial>)>,
    mut materials: ResMut<Assets<GraphGridMaterial>>,
) {
    for (grid, material) in &grids {
        let Ok(viewport) = viewports.get(grid.viewport) else {
            continue;
        };
        let Some(mut material) = materials.get_mut(&material.0) else {
            continue;
        };
        material.pan = viewport.pan;
        material.zoom = viewport.zoom;
    }
}

fn restore_graph_nodes(
    memory: Res<GraphViewportMemory>,
    mut nodes: Query<(Entity, &mut FeathersGraphNode, &mut Node), Added<FeathersGraphNode>>,
    mut bodies: Query<(&FeathersGraphNodeBody, &mut Node), Without<FeathersGraphNode>>,
    mut icons: Query<(&GraphCollapseIcon, &mut UiSvg)>,
    mut pin_icons: Query<(&GraphPinIcon, &mut SvgColor)>,
) {
    for (entity, mut graph_node, mut style) in &mut nodes {
        let key = (graph_node.graph_key.clone(), graph_node.node_key.clone());
        let Some(saved) = memory.nodes.get(&key) else {
            continue;
        };
        let position = memory
            .node_position(&graph_node.graph_key, &graph_node.node_key)
            .unwrap();
        graph_node.position = position;
        graph_node.collapsed = saved.collapsed;
        graph_node.pinned = memory.is_pinned(&graph_node.graph_key, &graph_node.node_key);
        style.left = Val::Px(position.x);
        style.top = Val::Px(position.y);
        apply_graph_node_collapse(entity, saved.collapsed, &mut bodies, &mut icons);
        apply_graph_node_pin(entity, graph_node.pinned, &mut pin_icons);
    }
}

/// Keeps every node's position (and collapse) in step with the shared memory, so a second view of
/// the same document reflects a node move or collapse live — not just when the pane rebuilds. Node
/// layout is keyed per document, so both views resolve to the same memory entry; the node being
/// dragged already matches and is skipped. Runs only when memory changed.
fn sync_graph_nodes_from_memory(
    memory: Res<GraphViewportMemory>,
    mut nodes: Query<(Entity, &mut FeathersGraphNode, &mut Node)>,
    mut bodies: Query<(&FeathersGraphNodeBody, &mut Node), Without<FeathersGraphNode>>,
    mut icons: Query<(&GraphCollapseIcon, &mut UiSvg)>,
    mut pin_icons: Query<(&GraphPinIcon, &mut SvgColor)>,
) {
    if !memory.is_changed() {
        return;
    }
    for (entity, mut graph_node, mut style) in &mut nodes {
        let key = (graph_node.graph_key.clone(), graph_node.node_key.clone());
        let Some(saved) = memory.nodes.get(&key) else {
            continue;
        };
        let position = memory
            .node_position(&graph_node.graph_key, &graph_node.node_key)
            .unwrap();
        if graph_node.position != position {
            graph_node.position = position;
            style.left = Val::Px(position.x);
            style.top = Val::Px(position.y);
        }
        if graph_node.collapsed != saved.collapsed {
            graph_node.collapsed = saved.collapsed;
            apply_graph_node_collapse(entity, saved.collapsed, &mut bodies, &mut icons);
        }
        let pinned = memory.is_pinned(&graph_node.graph_key, &graph_node.node_key);
        if graph_node.pinned != pinned {
            graph_node.pinned = pinned;
            apply_graph_node_pin(entity, pinned, &mut pin_icons);
        }
    }
}

fn queue_graph_collapse_activation(
    activate: On<Activate>,
    actions: Query<(), (With<GraphCollapseAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

fn queue_graph_pin_activation(
    activate: On<Activate>,
    actions: Query<(), (With<GraphPinAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

fn handle_graph_pin_buttons(
    mut commands: Commands,
    actions: Query<
        (
            Entity,
            &Interaction,
            &GraphPinAction,
            Option<&PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
    mut nodes: Query<&mut FeathersGraphNode>,
    mut icons: Query<(&GraphPinIcon, &mut SvgColor)>,
    mut memory: ResMut<GraphViewportMemory>,
) {
    for (entity, interaction, action, pending) in &actions {
        if *interaction != Interaction::Pressed || pending.is_none() {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>()
            .insert(Interaction::None);
        let Ok(mut node) = nodes.get_mut(action.node) else {
            continue;
        };
        // A bootstrap-only node has no authored base yet. Pinning makes its current displayed
        // position an explicit anchor, so materialize that position before storing the pin.
        if memory.node(&node.graph_key, &node.node_key).is_none() {
            memory.set_node(
                node.graph_key.clone(),
                node.node_key.clone(),
                node.position,
                node.collapsed,
            );
        }
        let before = memory.is_pinned(&node.graph_key, &node.node_key);
        let after = !before;
        memory.set_pinned(&node.graph_key, &node.node_key, after);
        node.pinned = after;
        apply_graph_node_pin(action.node, after, &mut icons);
        commands.entity(entity).insert(AccessibleLabel(if after {
            "Unpin node".into()
        } else {
            "Pin node".into()
        }));
        commands.trigger(GraphPinEdit {
            graph: node.graph_key.clone(),
            node: node.node_key.clone(),
            before,
            after,
            origin: Some(action.node),
        });
    }
}

fn apply_graph_node_pin(
    entity: Entity,
    pinned: bool,
    icons: &mut Query<(&GraphPinIcon, &mut SvgColor)>,
) {
    for (icon, mut color) in icons.iter_mut() {
        if icon.node == entity {
            color.0 = if pinned {
                theme::ACCENT
            } else {
                theme::TEXT_FAINT
            };
        }
    }
}

fn handle_graph_collapse_buttons(
    mut commands: Commands,
    actions: Query<
        (
            Entity,
            &Interaction,
            &GraphCollapseAction,
            Option<&PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
    mut nodes: Query<&mut FeathersGraphNode>,
    mut bodies: Query<(&FeathersGraphNodeBody, &mut Node), Without<FeathersGraphNode>>,
    mut icons: Query<(&GraphCollapseIcon, &mut UiSvg)>,
    mut memory: ResMut<GraphViewportMemory>,
) {
    for (entity, interaction, action, pending) in &actions {
        if *interaction != Interaction::Pressed || pending.is_none() {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>()
            .insert(Interaction::None);
        let Ok(mut node) = nodes.get_mut(action.node) else {
            continue;
        };
        let before = memory
            .node(&node.graph_key, &node.node_key)
            .unwrap_or((node.position, node.collapsed));
        node.collapsed = !node.collapsed;
        let collapsed = node.collapsed;
        memory.set_collapsed(&node.graph_key, &node.node_key, node.position, collapsed);
        commands.trigger(GraphPresentationEdit {
            graph: node.graph_key.clone(),
            node: node.node_key.clone(),
            before,
            after: (before.0, collapsed),
            origin: Some(action.node),
        });
        apply_graph_node_collapse(action.node, collapsed, &mut bodies, &mut icons);
        commands
            .entity(entity)
            .insert(AccessibleLabel(if collapsed {
                action.expand_label.clone()
            } else {
                action.collapse_label.clone()
            }));
    }
}

fn apply_graph_node_collapse(
    entity: Entity,
    collapsed: bool,
    bodies: &mut Query<(&FeathersGraphNodeBody, &mut Node), Without<FeathersGraphNode>>,
    icons: &mut Query<(&GraphCollapseIcon, &mut UiSvg)>,
) {
    for (body, mut style) in bodies.iter_mut() {
        if body.node == entity {
            style.display = if collapsed {
                Display::None
            } else {
                Display::Flex
            };
        }
    }
    for (icon, mut svg) in icons.iter_mut() {
        if icon.node == entity {
            svg.0 = if collapsed {
                icon.expand_icon.clone()
            } else {
                icon.collapse_icon.clone()
            };
        }
    }
}

/// Keep the selected edge visible when the graph canvas shrinks. The regular border contributes
/// `SELECTED_BORDER_WIDTH * zoom` screen pixels; an outline supplies only the missing amount and
/// does not change node layout or socket positions.
fn selection_outline_width(zoom: f32) -> f32 {
    (MIN_SELECTED_BORDER_SCREEN_WIDTH / zoom.max(MIN_ZOOM) - SELECTED_BORDER_WIDTH).max(0.0)
}

fn sync_graph_selection_outlines(
    viewports: Query<&FeathersGraphViewport>,
    parents: Query<&ChildOf>,
    mut nodes: Query<(Entity, &FeathersGraphNode, &mut Outline)>,
) {
    for (entity, graph_node, mut outline) in &mut nodes {
        let color = if graph_node.selected {
            theme::ACCENT
        } else {
            Color::NONE
        };
        if outline.color != color {
            outline.color = color;
        }
        if !graph_node.selected {
            continue;
        }
        let Some(viewport) = parents
            .iter_ancestors(entity)
            .find_map(|ancestor| viewports.get(ancestor).ok())
        else {
            continue;
        };
        let width = Val::Px(selection_outline_width(viewport.zoom));
        if outline.width != width {
            outline.width = width;
        }
    }
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
            Button,
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
                Button,
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

fn graph_port_row_node(side: GraphSocketSide) -> Node {
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

#[cfg(test)]
mod layout_contract_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_graph_toolbar_has_fixed_compact_chrome() {
        let mut world = World::new();
        let entity = world.spawn(graph_toolbar_bundle()).id();
        let node = world.get::<Node>(entity).unwrap();
        assert_eq!(node.height, Val::Px(38.0));
        assert_eq!(node.flex_shrink, 0.0);
        assert_eq!(node.column_gap, Val::Px(4.0));
        assert_eq!(
            world.get::<BackgroundColor>(entity).unwrap().0,
            theme::PANEL_LIGHT
        );
    }

    fn assert_vec2_close(actual: Vec2, expected: Vec2) {
        assert!(
            actual.distance(expected) < 0.001,
            "expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn compact_nodes_route_wires_to_header_edges() {
        let size = Vec2::new(180.0, 42.0);
        assert_eq!(
            compact_graph_socket_offset_for_size(true, size, GraphSocketSide::Input),
            Some(Vec2::new(0.0, 21.0))
        );
        assert_eq!(
            compact_graph_socket_offset_for_size(true, size, GraphSocketSide::Output),
            Some(Vec2::new(180.0, 21.0))
        );
    }

    #[test]
    fn expanded_or_unmeasured_nodes_do_not_use_compact_proxy_anchors() {
        assert_eq!(
            compact_graph_socket_offset_for_size(
                false,
                Vec2::new(180.0, 42.0),
                GraphSocketSide::Input,
            ),
            None
        );
        assert_eq!(
            compact_graph_socket_offset_for_size(true, Vec2::ZERO, GraphSocketSide::Output),
            None
        );
    }

    #[test]
    fn nodes_sync_from_shared_memory_across_views() {
        use bevy::ecs::system::RunSystemOnce;
        // Node layout is per document, so two views of one graph share a memory entry. Moving a
        // node (writing memory) must update every view's node entity, not just the dragged one, so
        // wires stay attached in the other view.
        let mut app = App::new();
        let mut memory = GraphViewportMemory::default();
        memory.set_node("material:p", "expression:n", Vec2::new(120.0, 40.0), false);
        app.insert_resource(memory);
        let node = || {
            (
                FeathersGraphNode {
                    graph_key: "material:p".into(),
                    node_key: "expression:n".into(),
                    position: Vec2::ZERO,
                    selected: false,
                    pinned: false,
                    collapsed: false,
                    dragging: false,
                    drag_before: None,
                    drag_modifier: None,
                    suppress_release_click: false,
                },
                Node::default(),
            )
        };
        let left = app.world_mut().spawn(node()).id();
        let right = app.world_mut().spawn(node()).id();

        app.world_mut()
            .run_system_once(sync_graph_nodes_from_memory)
            .unwrap();

        for view in [left, right] {
            assert_eq!(
                app.world().get::<FeathersGraphNode>(view).unwrap().position,
                Vec2::new(120.0, 40.0)
            );
        }
    }

    #[test]
    fn cursor_centered_zoom_keeps_the_graph_point_under_the_pointer() {
        let before = GraphView {
            pan: Vec2::new(-180.0, 75.0),
            zoom: 0.8,
        };
        let cursor = Vec2::new(430.0, 260.0);
        let anchor = (cursor - before.pan) / before.zoom;

        let after = zoomed_graph_view_at(before, cursor, 2.0);

        assert!(after.zoom > before.zoom);
        assert_vec2_close(after.pan + anchor * after.zoom, cursor);
    }

    #[test]
    fn graph_can_zoom_out_to_overview_scale_without_losing_cursor_anchor() {
        let cursor = Vec2::new(430.0, 260.0);
        let before = GraphView {
            pan: Vec2::new(-180.0, 75.0),
            zoom: 0.25,
        };
        let anchor = (cursor - before.pan) / before.zoom;

        let after = zoomed_graph_view_at(before, cursor, -20.0);
        assert_eq!(after.zoom, MIN_ZOOM);
        assert_vec2_close(after.pan + anchor * after.zoom, cursor);
        assert_vec2_close(
            graph_drag_delta(Vec2::new(8.0, 4.0), after.zoom),
            Vec2::new(100.0, 50.0),
        );
    }

    #[test]
    fn selected_outline_keeps_a_two_pixel_edge_at_overview_zoom() {
        assert_eq!(selection_outline_width(1.0), 0.0);
        for zoom in [0.5, 0.25, MIN_ZOOM] {
            let screen_width = (SELECTED_BORDER_WIDTH + selection_outline_width(zoom)) * zoom;
            assert!((screen_width - MIN_SELECTED_BORDER_SCREEN_WIDTH).abs() < 0.001);
        }
    }

    #[test]
    fn selected_outline_tracks_its_own_viewport_zoom() {
        use bevy::ecs::system::RunSystemOnce;

        let mut app = App::new();
        let viewport = app
            .world_mut()
            .spawn(FeathersGraphViewport {
                key: "view".into(),
                pan: Vec2::ZERO,
                zoom: MIN_ZOOM,
                content_size: Vec2::new(1000.0, 800.0),
                selection_bounds: None,
                frame_request: None,
                measured_frame: None,
            })
            .id();
        let node = app
            .world_mut()
            .spawn((
                ChildOf(viewport),
                FeathersGraphNode {
                    graph_key: "graph".into(),
                    node_key: "node".into(),
                    position: Vec2::ZERO,
                    selected: true,
                    pinned: false,
                    collapsed: false,
                    dragging: false,
                    drag_before: None,
                    drag_modifier: None,
                    suppress_release_click: false,
                },
                Outline::new(Val::Px(0.0), Val::ZERO, theme::ACCENT),
            ))
            .id();

        app.world_mut()
            .run_system_once(sync_graph_selection_outlines)
            .unwrap();
        assert_eq!(
            app.world().get::<Outline>(node).unwrap().width,
            Val::Px(selection_outline_width(MIN_ZOOM))
        );

        app.world_mut()
            .get_mut::<FeathersGraphViewport>(viewport)
            .unwrap()
            .zoom = 1.0;
        app.world_mut()
            .run_system_once(sync_graph_selection_outlines)
            .unwrap();
        assert_eq!(
            app.world().get::<Outline>(node).unwrap().width,
            Val::Px(0.0)
        );
    }

    #[test]
    fn frame_view_uses_overview_scale_for_wide_graphs() {
        let bounds = Rect::from_corners(Vec2::ZERO, Vec2::new(8_000.0, 2_000.0));
        let viewport = Vec2::new(960.0, 540.0);
        let view = framed_graph_view(bounds, viewport, 1.0);
        assert!(view.zoom < 0.25);
        assert!(view.zoom >= MIN_ZOOM);
        let visible_min = bounds.min * view.zoom + view.pan;
        let visible_max = bounds.max * view.zoom + view.pan;
        assert!(visible_min.x >= 0.0 && visible_min.y >= 0.0);
        assert!(visible_max.x <= viewport.x && visible_max.y <= viewport.y);
    }

    #[test]
    fn frame_view_centers_bounds_and_keeps_them_inside_the_viewport() {
        let bounds = Rect::from_corners(Vec2::new(200.0, 120.0), Vec2::new(1_400.0, 720.0));
        let viewport = Vec2::new(960.0, 540.0);

        let view = framed_graph_view(bounds, viewport, 1.0);
        let visible_min = bounds.min * view.zoom + view.pan;
        let visible_max = bounds.max * view.zoom + view.pan;

        assert_vec2_close((visible_min + visible_max) * 0.5, viewport * 0.5);
        assert!(visible_min.x >= 0.0 && visible_min.y >= 0.0);
        assert!(visible_max.x <= viewport.x && visible_max.y <= viewport.y);
    }

    #[test]
    fn canvas_transform_preserves_top_left_pan_while_scaling_about_its_center() {
        let pan = Vec2::new(84.0, -36.0);
        let size = Vec2::new(1_200.0, 800.0);
        let zoom = 1.5;

        let transform = graph_canvas_transform(pan, zoom, size);
        let translation = transform.translation.resolve(1.0, size, size);
        let visual_top_left = translation + size * (1.0 - zoom) * 0.5;

        assert_vec2_close(visual_top_left, pan);
    }

    #[test]
    fn fixed_wire_layer_projects_graph_points_like_the_zoomed_node_canvas() {
        let pan = Vec2::new(-137.0, 82.0);
        let content_size = Vec2::new(1_800.0, 960.0);
        let graph_point = Vec2::new(725.0, 318.0);

        for zoom in [MIN_ZOOM, 0.25, 0.7, 1.0, 1.8] {
            let canvas = graph_canvas_transform(pan, zoom, content_size);
            let canvas_translation = canvas.translation.resolve(1.0, content_size, content_size);
            let canvas_top_left = canvas_translation + content_size * (1.0 - canvas.scale.x) * 0.5;
            let node_socket_screen = canvas_top_left + graph_point * canvas.scale;

            let viewport = FeathersGraphViewport {
                key: "graph".into(),
                pan,
                zoom,
                content_size,
                selection_bounds: None,
                frame_request: None,
                measured_frame: None,
            };
            let wire_endpoint_screen = viewport.project_graph_point(graph_point);

            assert_vec2_close(wire_endpoint_screen, node_socket_screen);
            assert_vec2_close(
                viewport.unproject_viewport_point(wire_endpoint_screen),
                graph_point,
            );
        }
    }

    #[test]
    fn node_drag_converts_screen_motion_to_graph_motion() {
        assert_vec2_close(
            graph_drag_delta(Vec2::new(30.0, -18.0), 1.5),
            Vec2::new(20.0, -12.0),
        );
        assert_vec2_close(
            graph_drag_delta(Vec2::new(30.0, -18.0), 0.5),
            Vec2::new(60.0, -36.0),
        );
    }

    #[test]
    fn node_drag_release_click_guard_survives_until_the_release_click() {
        let mut node = FeathersGraphNode {
            graph_key: "graph".into(),
            node_key: "node".into(),
            position: Vec2::ZERO,
            selected: false,
            pinned: false,
            collapsed: false,
            dragging: false,
            drag_before: None,
            drag_modifier: None,
            suppress_release_click: false,
        };

        node.begin_drag();
        node.note_drag_motion();
        node.end_drag();

        assert!(node.consume_suppressed_release_click());
        assert!(!node.consume_suppressed_release_click());
    }

    #[test]
    fn graph_ports_anchor_inputs_left_and_outputs_right_on_the_first_body_row() {
        let input = graph_port_row_node(GraphSocketSide::Input);
        assert_eq!(input.position_type, PositionType::Relative);
        assert_eq!(input.flex_direction, FlexDirection::Row);
        assert_eq!(input.justify_content, JustifyContent::FlexStart);

        let output = graph_port_row_node(GraphSocketSide::Output);
        assert_eq!(output.position_type, PositionType::Absolute);
        assert_eq!(output.top, Val::Px(5.0));
        assert_eq!(output.right, Val::Px(0.0));
        assert_eq!(output.flex_direction, FlexDirection::RowReverse);
        assert_eq!(output.justify_content, JustifyContent::FlexStart);
    }
}
