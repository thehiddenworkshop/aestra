//! Reusable Feathers-styled building blocks for spatial node graphs.
//!
//! This module owns presentation only: canvas styling, node chrome, socket hit targets, and the
//! anti-aliased wire material. Domain panels retain their semantic graph model and commands.

use super::{
    button::{FeathersActionButton, PendingFeathersActivation},
    icon::load_svg_icon,
    scenes,
    tooltip::EditorTooltip,
};
use crate::theme;
use bevy::{
    asset::embedded_asset,
    ecs::query::{QueryData, QueryFilter},
    feathers::cursor::{EntityCursor, OverrideCursor},
    input::mouse::{MouseScrollUnit, MouseWheel},
    picking::events::{Drag, DragEnd, DragStart, Pointer, Press},
    picking::pointer::PointerButton,
    prelude::*,
    reflect::TypePath,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
    ui::RelativeCursorPosition,
    ui_render::prelude::{UiMaterial, UiMaterialPlugin},
    ui_widgets::Activate,
    window::{CursorMoved, PrimaryWindow, SystemCursorIcon, Window},
};
use bevy_resvg::prelude::{SvgColor, SvgFile, UiSvg};
use std::collections::HashMap;

pub(crate) const NODE_WIDTH: f32 = 224.0;
pub(crate) const NODE_HEADER_HEIGHT: f32 = 30.0;
pub(crate) const PORT_ROW_HEIGHT: f32 = 24.0;
pub(crate) const SOCKET_HIT_SIZE: f32 = 20.0;
const SOCKET_SIZE: f32 = 10.0;
const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 2.0;
const FRAME_PADDING: f32 = 42.0;
const GRID_SPACING: f32 = 32.0;

pub(crate) struct FeathersNodeGraphPlugin;

impl Plugin for FeathersNodeGraphPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/node_graph_wire.wgsl");
        embedded_asset!(app, "shaders/node_graph_grid.wgsl");
        app.add_plugins(UiMaterialPlugin::<GraphWireMaterial>::default())
            .add_plugins(UiMaterialPlugin::<GraphGridMaterial>::default())
            .init_resource::<GraphViewportMemory>()
            .init_resource::<GraphPanGesture>()
            .add_observer(queue_graph_frame_activation)
            .add_observer(queue_graph_collapse_activation)
            .add_observer(begin_graph_node_press)
            .add_observer(begin_graph_node_drag)
            .add_observer(drag_graph_node)
            .add_observer(end_graph_node_drag)
            .add_systems(
                Update,
                (
                    attach_graph_grid_materials,
                    remember_graph_viewports,
                    restore_graph_viewports,
                    restore_graph_nodes,
                    handle_graph_frame_buttons,
                    handle_graph_collapse_buttons,
                    navigate_graph_viewports,
                    sync_graph_viewport_transforms,
                    update_socket_visuals,
                )
                    .chain(),
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphFrameTarget {
    All,
    Selection,
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

#[derive(Component, Debug, Clone)]
pub(crate) struct FeathersGraphViewport {
    key: String,
    pan: Vec2,
    zoom: f32,
    content_size: Vec2,
    selection_bounds: Option<Rect>,
    frame_request: Option<GraphFrameTarget>,
}

impl FeathersGraphViewport {
    pub(crate) fn project_graph_point(&self, point: Vec2) -> Vec2 {
        self.pan + point * self.zoom
    }
}

#[derive(Component, Debug, Clone, Copy)]
struct FeathersGraphCanvas {
    viewport: Entity,
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

#[derive(Debug, Clone, Copy)]
struct GraphView {
    pan: Vec2,
    zoom: f32,
}

#[derive(Debug, Clone, Copy)]
struct GraphNodeView {
    position: Vec2,
    collapsed: bool,
}

#[derive(Resource, Default)]
struct GraphViewportMemory {
    views: HashMap<String, GraphView>,
    nodes: HashMap<(String, String), GraphNodeView>,
}

#[derive(Resource, Default)]
struct GraphPanGesture {
    viewport: Option<Entity>,
    button: Option<MouseButton>,
    cursor_position: Option<Vec2>,
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
    pub(crate) subtitle: String,
    pub(crate) position: Vec2,
    pub(crate) selected: bool,
    pub(crate) muted: bool,
    pub(crate) collapse_icon: Handle<SvgFile>,
    pub(crate) expand_icon: Handle<SvgFile>,
    pub(crate) collapse_label: String,
    pub(crate) expand_label: String,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct FeathersGraphNode {
    graph_key: String,
    node_key: String,
    position: Vec2,
    selected: bool,
    collapsed: bool,
    dragging: bool,
    suppress_release_click: bool,
}

impl FeathersGraphNode {
    pub(crate) fn position(&self) -> Vec2 {
        self.position
    }

    fn begin_drag(&mut self) {
        self.dragging = true;
        self.suppress_release_click = false;
    }

    fn note_drag_motion(&mut self) {
        self.suppress_release_click = true;
    }

    fn end_drag(&mut self) {
        self.dragging = false;
    }

    pub(crate) fn consume_suppressed_release_click(&mut self) -> bool {
        std::mem::take(&mut self.suppress_release_click)
    }
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

/// Viewport-space grid whose graph coordinates follow pan and zoom without finite edges.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct GraphGridMaterial {
    #[uniform(0)]
    pan: Vec2,
    #[uniform(0)]
    zoom: f32,
    #[uniform(0)]
    spacing: f32,
}

impl Default for GraphGridMaterial {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
            spacing: GRID_SPACING,
        }
    }
}

impl UiMaterial for GraphGridMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://aestra_editor/feathers/shaders/node_graph_grid.wgsl".into()
    }
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
        EntityCursor::System(SystemCursorIcon::Grab),
        FeathersGraphViewport {
            key: props.key,
            pan: Vec2::ZERO,
            zoom: 1.0,
            content_size: props.content_size,
            selection_bounds: props.selection_bounds,
            frame_request: Some(GraphFrameTarget::All),
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
            SvgColor(theme::TEXT),
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
        memory.views.insert(
            viewport.key.clone(),
            GraphView {
                pan: viewport.pan,
                zoom: viewport.zoom,
            },
        );
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
) {
    for (entity, mut graph_node, mut style) in &mut nodes {
        let key = (graph_node.graph_key.clone(), graph_node.node_key.clone());
        let Some(saved) = memory.nodes.get(&key) else {
            continue;
        };
        graph_node.position = saved.position;
        graph_node.collapsed = saved.collapsed;
        style.left = Val::Px(saved.position.x);
        style.top = Val::Px(saved.position.y);
        apply_graph_node_collapse(entity, saved.collapsed, &mut bodies, &mut icons);
    }
}

fn begin_graph_node_press(
    press: On<Pointer<Press>>,
    mut nodes: Query<&mut FeathersGraphNode>,
    parents: Query<&ChildOf>,
    controls: Query<(), Or<(With<FeathersGraphSocket>, With<GraphCollapseAction>)>>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Some(entity) = graph_node_from_target(
        press.event_target(),
        &nodes.as_readonly(),
        &parents,
        &controls,
    ) else {
        return;
    };
    if let Ok(mut node) = nodes.get_mut(entity) {
        // A new press starts a new click-or-drag gesture. This clears a stale release guard when a
        // platform did not synthesize a Click after the previous drag.
        node.suppress_release_click = false;
    }
}

fn begin_graph_node_drag(
    mut drag: On<Pointer<DragStart>>,
    mut nodes: Query<&mut FeathersGraphNode>,
    parents: Query<&ChildOf>,
    controls: Query<(), Or<(With<FeathersGraphSocket>, With<GraphCollapseAction>)>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if drag.button != PointerButton::Primary || keys.pressed(KeyCode::Space) {
        return;
    }
    let Some(entity) = graph_node_from_target(
        drag.event_target(),
        &nodes.as_readonly(),
        &parents,
        &controls,
    ) else {
        return;
    };
    let Ok(mut node) = nodes.get_mut(entity) else {
        return;
    };
    node.begin_drag();
    drag.propagate(false);
}

fn drag_graph_node(
    mut drag: On<Pointer<Drag>>,
    mut nodes: Query<(&mut FeathersGraphNode, &mut Node, &ComputedNode)>,
    parents: Query<&ChildOf>,
    controls: Query<(), Or<(With<FeathersGraphSocket>, With<GraphCollapseAction>)>>,
    viewports: Query<&FeathersGraphViewport>,
    mut memory: ResMut<GraphViewportMemory>,
    mut override_cursor: ResMut<OverrideCursor>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some(entity) = graph_node_from_target(
        drag.event_target(),
        &nodes.as_readonly(),
        &parents,
        &controls,
    ) else {
        return;
    };
    let Ok((mut graph_node, mut style, computed)) = nodes.get_mut(entity) else {
        return;
    };
    if !graph_node.dragging {
        return;
    }
    // Drag is emitted only after picking has recognized a real drag. Arm the guard here rather
    // than waiting for DragEnd: Click ordering differs by backend on pointer release.
    graph_node.note_drag_motion();
    let zoom = viewports
        .iter()
        .find(|viewport| viewport.key == graph_node.graph_key)
        .map_or(1.0, |viewport| viewport.zoom.max(MIN_ZOOM));
    graph_node.position += graph_drag_delta(drag.delta * computed.inverse_scale_factor, zoom);
    style.left = Val::Px(graph_node.position.x);
    style.top = Val::Px(graph_node.position.y);
    memory.nodes.insert(
        (graph_node.graph_key.clone(), graph_node.node_key.clone()),
        GraphNodeView {
            position: graph_node.position,
            collapsed: graph_node.collapsed,
        },
    );
    override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::Grabbing));
    drag.propagate(false);
}

fn graph_drag_delta(screen_delta: Vec2, zoom: f32) -> Vec2 {
    screen_delta / zoom.max(MIN_ZOOM)
}

fn end_graph_node_drag(
    mut drag: On<Pointer<DragEnd>>,
    mut nodes: Query<&mut FeathersGraphNode>,
    parents: Query<&ChildOf>,
    controls: Query<(), Or<(With<FeathersGraphSocket>, With<GraphCollapseAction>)>>,
    mut override_cursor: ResMut<OverrideCursor>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some(entity) = graph_node_from_target(
        drag.event_target(),
        &nodes.as_readonly(),
        &parents,
        &controls,
    ) else {
        return;
    };
    let Ok(mut node) = nodes.get_mut(entity) else {
        return;
    };
    node.end_drag();
    override_cursor.0 = None;
    drag.propagate(false);
}

fn graph_node_from_target<D: QueryData, F: QueryFilter>(
    mut entity: Entity,
    nodes: &Query<D, F>,
    parents: &Query<&ChildOf>,
    controls: &Query<(), Or<(With<FeathersGraphSocket>, With<GraphCollapseAction>)>>,
) -> Option<Entity> {
    loop {
        if controls.contains(entity) {
            return None;
        }
        if nodes.contains(entity) {
            return Some(entity);
        }
        entity = parents.get(entity).ok()?.parent();
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
        node.collapsed = !node.collapsed;
        let collapsed = node.collapsed;
        memory.nodes.insert(
            (node.graph_key.clone(), node.node_key.clone()),
            GraphNodeView {
                position: node.position,
                collapsed,
            },
        );
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

fn navigate_graph_viewports(
    mut cursor_moved: MessageReader<CursorMoved>,
    mut wheel: MessageReader<MouseWheel>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut gesture: ResMut<GraphPanGesture>,
    mut override_cursor: ResMut<OverrideCursor>,
    primary_window: Single<(Entity, &Window), With<PrimaryWindow>>,
    mut viewports: Query<(
        Entity,
        &RelativeCursorPosition,
        &ComputedNode,
        &mut FeathersGraphViewport,
    )>,
) {
    let cursor_position = cursor_moved
        .read()
        .filter(|event| event.window == primary_window.0)
        .last()
        .map(|event| event.position)
        .or_else(|| primary_window.1.cursor_position());
    let scroll_delta = wheel.read().fold(0.0, |sum, event| {
        let scale = match event.unit {
            MouseScrollUnit::Line => 1.0,
            MouseScrollUnit::Pixel => 0.02,
        };
        sum + event.y * scale
    });
    let hovered = viewports
        .iter()
        .find_map(|(entity, cursor, _, _)| cursor.cursor_over().then_some(entity));
    let space = keys.pressed(KeyCode::Space);
    let was_panning = gesture.viewport.is_some();

    if !was_panning && let Some(cursor_position) = cursor_position {
        gesture.cursor_position = Some(cursor_position);
    }

    if !was_panning {
        let button = if buttons.just_pressed(MouseButton::Middle) {
            Some(MouseButton::Middle)
        } else if space && buttons.just_pressed(MouseButton::Left) {
            Some(MouseButton::Left)
        } else {
            None
        };
        if let (Some(viewport), Some(button)) = (hovered, button) {
            gesture.viewport = Some(viewport);
            gesture.button = Some(button);
        }
    }

    if let (Some(entity), Some(button)) = (gesture.viewport, gesture.button) {
        // Apply cursor movement before testing release. Otherwise the final movement and button-up
        // arriving in the same frame leaves the canvas visibly trailing a fast pointer.
        if was_panning && let Some(cursor_position) = cursor_position {
            let pointer_delta = gesture
                .cursor_position
                .replace(cursor_position)
                .map_or(Vec2::ZERO, |previous| cursor_position - previous);
            if pointer_delta != Vec2::ZERO
                && let Ok((_, _, _, mut viewport)) = viewports.get_mut(entity)
            {
                // CursorMoved is already expressed in logical window pixels, matching UiTransform.
                // Pan is screen-space, so neither display scale nor graph zoom belongs here.
                viewport.pan += pointer_delta;
                viewport.frame_request = None;
            }
        }
        let still_active = buttons.pressed(button) && (button != MouseButton::Left || space);
        if still_active {
            override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::Grabbing));
        } else {
            gesture.viewport = None;
            gesture.button = None;
            gesture.cursor_position = None;
            override_cursor.0 = None;
        }
    }

    if gesture.viewport.is_none()
        && let Some(entity) = hovered
        && let Ok((_, _, _, mut viewport)) = viewports.get_mut(entity)
    {
        if keys.just_pressed(KeyCode::Home) {
            viewport.frame_request = Some(GraphFrameTarget::All);
        } else if keys.just_pressed(KeyCode::KeyF) {
            viewport.frame_request = Some(GraphFrameTarget::Selection);
        }
    }

    if gesture.viewport.is_some() || scroll_delta == 0.0 {
        return;
    }
    let Some(entity) = hovered else {
        return;
    };
    let Ok((_, cursor, computed, mut viewport)) = viewports.get_mut(entity) else {
        return;
    };
    let Some(normalized) = cursor.normalized else {
        return;
    };
    let cursor = (normalized + Vec2::splat(0.5)) * computed.size();
    let view = zoomed_graph_view_at(
        GraphView {
            pan: viewport.pan,
            zoom: viewport.zoom,
        },
        cursor,
        scroll_delta,
    );
    viewport.pan = view.pan;
    viewport.zoom = view.zoom;
    viewport.frame_request = None;
}

fn sync_graph_viewport_transforms(
    mut viewports: Query<(Entity, &ComputedNode, &mut FeathersGraphViewport)>,
    mut canvases: Query<(&FeathersGraphCanvas, &mut UiTransform)>,
    graph_nodes: Query<(&FeathersGraphNode, &ComputedNode)>,
) {
    for (entity, computed, mut viewport) in &mut viewports {
        let viewport_size = computed.size();
        if viewport_size.min_element() > 0.0
            && let Some(target) = viewport.frame_request
        {
            let live_bounds = graph_node_bounds(
                &graph_nodes,
                &viewport.key,
                target == GraphFrameTarget::Selection,
            );
            let bounds = live_bounds.unwrap_or_else(|| match target {
                GraphFrameTarget::All => Rect::from_corners(Vec2::ZERO, viewport.content_size),
                GraphFrameTarget::Selection => viewport
                    .selection_bounds
                    .unwrap_or_else(|| Rect::from_corners(Vec2::ZERO, viewport.content_size)),
            });
            let maximum_zoom = if target == GraphFrameTarget::All {
                1.0
            } else {
                MAX_ZOOM
            };
            let view = framed_graph_view(bounds, viewport_size, maximum_zoom);
            viewport.pan = view.pan;
            viewport.zoom = view.zoom;
            viewport.frame_request = None;
        }
        for (canvas, mut transform) in &mut canvases {
            if canvas.viewport == entity {
                *transform =
                    graph_canvas_transform(viewport.pan, viewport.zoom, viewport.content_size);
            }
        }
    }
}

fn graph_node_bounds(
    nodes: &Query<(&FeathersGraphNode, &ComputedNode)>,
    graph_key: &str,
    selection_only: bool,
) -> Option<Rect> {
    nodes
        .iter()
        .filter(|(node, computed)| {
            node.graph_key == graph_key
                && (!selection_only || node.selected)
                && computed.size().min_element() > 0.0
        })
        .map(|(node, computed)| Rect::from_corners(node.position, node.position + computed.size()))
        .reduce(|left, right| Rect::from_corners(left.min.min(right.min), left.max.max(right.max)))
}

fn zoomed_graph_view_at(mut view: GraphView, cursor: Vec2, scroll_delta: f32) -> GraphView {
    let anchor = (cursor - view.pan) / view.zoom;
    view.zoom = (view.zoom * (scroll_delta * 0.12).exp()).clamp(MIN_ZOOM, MAX_ZOOM);
    view.pan = cursor - anchor * view.zoom;
    view
}

fn framed_graph_view(bounds: Rect, viewport_size: Vec2, maximum_zoom: f32) -> GraphView {
    let extent = bounds.size().max(Vec2::splat(1.0));
    let available = (viewport_size - Vec2::splat(FRAME_PADDING * 2.0)).max(Vec2::splat(1.0));
    let zoom = (available.x / extent.x)
        .min(available.y / extent.y)
        .clamp(MIN_ZOOM, maximum_zoom);
    GraphView {
        pan: viewport_size * 0.5 - bounds.center() * zoom,
        zoom,
    }
}

fn graph_canvas_transform(pan: Vec2, zoom: f32, content_size: Vec2) -> UiTransform {
    let centered_scale_offset = content_size * (zoom - 1.0) * 0.5;
    UiTransform {
        translation: Val2::px(
            pan.x + centered_scale_offset.x,
            pan.y + centered_scale_offset.y,
        ),
        scale: Vec2::splat(zoom),
        ..default()
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
    let collapse_label = props.collapse_label.clone();
    let expand_label = props.expand_label.clone();
    let mut node = parent.spawn((
        marker,
        FeathersGraphNode {
            graph_key,
            node_key,
            position: props.position,
            selected: props.selected,
            collapsed: false,
            dragging: false,
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
                flex_direction: FlexDirection::Column,
                padding: UiRect::vertical(Val::Px(5.0)),
                ..default()
            },
        ))
        .with_children(|children| body(entity, children));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_vec2_close(actual: Vec2, expected: Vec2) {
        assert!(
            actual.distance(expected) < 0.001,
            "expected {expected:?}, got {actual:?}"
        );
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

        for zoom in [0.25, 0.7, 1.0, 1.8] {
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
            };
            let wire_endpoint_screen = viewport.project_graph_point(graph_point);

            assert_vec2_close(wire_endpoint_screen, node_socket_screen);
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
            collapsed: false,
            dragging: false,
            suppress_release_click: false,
        };

        node.begin_drag();
        node.note_drag_motion();
        node.end_drag();

        assert!(node.consume_suppressed_release_click());
        assert!(!node.consume_suppressed_release_click());
    }
}
