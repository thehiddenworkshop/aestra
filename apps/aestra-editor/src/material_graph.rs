//! Projectional material graph workspace backed by semantic material commands.

use crate::{
    feathers::{
        context_menu::{
            pointer_position_in_node, should_dismiss_pointer_context_menu,
            spawn_pointer_context_menu_custom_item, spawn_pointer_context_menu_sized,
        },
        icon::load_svg_icon,
        node_graph::{
            FeathersGraphNode, FeathersGraphViewport, FeathersGraphWireLayer, GraphFrameAction,
            GraphFrameTarget, GraphNodeProps, GraphPortProps, GraphSocketSide, GraphViewportMemory,
            GraphViewportProps, GraphWireMaterial, NODE_HEADER_HEIGHT, NODE_WIDTH, PORT_ROW_HEIGHT,
            spawn_graph_frame_button, spawn_graph_node, spawn_graph_port, spawn_graph_viewport,
        },
        panel::spawn_panel_empty_state,
    },
    *,
};
use aestra_authoring::{
    MaterialAuthoringDocument, MaterialConnectionTarget, MaterialExpressionInput,
    MaterialInsertionPoint, MaterialInspectionTarget, MaterialInspector,
    MaterialOperationAvailability, MaterialOutputSocket, MaterialToolCommand, MaterialToolPlan,
    MaterialToolPlanner,
};
use aestra_compiler::{
    MaterialCompiler, MaterialGraphEdgeTarget, MaterialGraphNode, MaterialGraphOutput,
    MaterialGraphOutputKind, MaterialGraphProjection, MaterialStackModifierKind,
};
use aestra_core::{
    MaterialExpressionId, MaterialProgramId,
    material::{MaterialExpressionDomain, MaterialValueType},
};
use bevy::{
    input_focus::{FocusCause, InputFocus},
    ui_render::ui_material::MaterialNode,
};
use std::collections::{BTreeMap, BTreeSet};

const COLUMN_WIDTH: f32 = 282.0;
const CANVAS_PADDING: f32 = 34.0;
const NODE_GAP: f32 = 22.0;
const SNAP_RADIUS: f32 = 38.0;

pub(crate) struct EditorMaterialGraphPlugin;

impl Plugin for EditorMaterialGraphPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MaterialGraphGesture>()
            .init_resource::<MaterialGraphPaletteState>()
            .add_observer(begin_material_connection_drag)
            .add_observer(update_material_connection_drag)
            .add_observer(finish_material_connection_drag)
            .add_observer(stop_material_socket_click)
            .add_observer(select_material_graph_node)
            .add_observer(open_material_graph_palette)
            .add_observer(update_material_graph_palette_search)
            .add_systems(
                Update,
                (
                    open_material_graph_palette_from_keyboard,
                    dismiss_material_graph_palette,
                    focus_material_graph_palette_search,
                    handle_material_graph_palette_actions,
                    handle_material_graph_actions,
                    attach_material_graph_wire_materials,
                ),
            )
            .add_systems(
                PostUpdate,
                update_material_graph_wires.after(bevy::transform::TransformSystems::Propagate),
            );
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct MaterialGraphAction {
    program: MaterialProgramId,
    expression: MaterialExpressionId,
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphCanvas;

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphViewport {
    program: MaterialProgramId,
}

#[derive(Debug, Clone)]
struct MaterialGraphPaletteOpen {
    program: MaterialProgramId,
    menu_position: Vec2,
    graph_position: Vec2,
    graph_key: String,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct MaterialGraphPaletteState {
    open: Option<MaterialGraphPaletteOpen>,
    query: String,
}

#[derive(Component)]
struct MaterialGraphPalette;

#[derive(Component)]
struct MaterialGraphPaletteAnchor;

#[derive(Component)]
struct MaterialGraphPaletteSearch;

#[derive(Component, Debug, Clone)]
struct MaterialGraphPaletteAction {
    program: MaterialProgramId,
    kind: MaterialStackModifierKind,
    edit: MaterialGraphPaletteEdit,
    graph_position: Vec2,
    graph_key: String,
    searchable: String,
}

#[derive(Debug, Clone, Copy)]
enum MaterialGraphPaletteEdit {
    Insert(MaterialInsertionPoint),
    Wrap(MaterialConnectionTarget),
}

#[derive(Debug, Clone, Copy)]
struct MaterialGraphPaletteOption {
    kind: MaterialStackModifierKind,
    edit: MaterialGraphPaletteEdit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaterialGraphSocketKind {
    ExpressionOutput(MaterialExpressionId),
    ConnectionInput(MaterialConnectionTarget),
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphSocket {
    program: MaterialProgramId,
    kind: MaterialGraphSocketKind,
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphSocketAnchor {
    node: Entity,
    offset: Option<Vec2>,
}

#[derive(Debug, Clone, Copy)]
struct MaterialGraphSocketPosition {
    program: MaterialProgramId,
    kind: MaterialGraphSocketKind,
    graph: Vec2,
    world: Vec2,
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphWire {
    program: MaterialProgramId,
    source: MaterialExpressionId,
    target: MaterialConnectionTarget,
    color: Vec4,
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphGhostWire {
    program: MaterialProgramId,
}

#[derive(Resource, Debug, Default)]
enum MaterialGraphGesture {
    #[default]
    Idle,
    Connecting {
        program: MaterialProgramId,
        source: MaterialExpressionId,
        cursor: Vec2,
        snap_target: Option<MaterialConnectionTarget>,
    },
}

#[derive(Debug)]
struct MaterialGraphLayout {
    nodes: BTreeMap<MaterialExpressionId, Vec2>,
    output: Vec2,
    size: Vec2,
}

fn handle_material_graph_actions(
    mut actions: Query<
        (&Interaction, &MaterialGraphAction, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
    inspector: Res<MaterialStackInspectorState>,
) {
    for (interaction, action, mut background) in &mut actions {
        match *interaction {
            Interaction::Hovered => background.0 = theme::BUTTON_HOVER,
            Interaction::None => {
                background.0 = if inspector.selected == Some((action.program, action.expression)) {
                    theme::SELECTION
                } else {
                    theme::PANEL
                };
            }
            Interaction::Pressed => {
                background.0 = theme::ACCENT_DIM;
            }
        }
    }
}

fn select_material_graph_node(
    mut click: On<Pointer<Click>>,
    actions: Query<&MaterialGraphAction>,
    mut graph_nodes: Query<&mut FeathersGraphNode>,
    parents: Query<&ChildOf>,
    sockets: Query<(), With<MaterialGraphSocket>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut session: ResMut<EditorSession>,
    mut layout: ResMut<WorkspaceLayout>,
) {
    if click.button != PointerButton::Primary || keys.pressed(KeyCode::Space) {
        return;
    }
    let mut entity = click.event_target();
    let action = loop {
        if sockets.contains(entity) {
            return;
        }
        if let Ok(action) = actions.get(entity) {
            break action;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    if graph_nodes
        .get_mut(entity)
        .is_ok_and(|mut node| node.consume_suppressed_release_click())
    {
        click.propagate(false);
        return;
    }
    if inspector.selected != Some((action.program, action.expression)) {
        inspector.selected = Some((action.program, action.expression));
        session.ui_revision += 1;
    }
    reveal_dock_panel(&mut layout, &mut session, DockPanel::Properties);
    click.propagate(false);
}

fn open_material_graph_palette(
    mut click: On<Pointer<Click>>,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    graph_nodes: Query<(), With<FeathersGraphNode>>,
    palette_surfaces: Query<(), With<MaterialGraphPalette>>,
    parents: Query<&ChildOf>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let mut entity = click.event_target();
    loop {
        if graph_nodes.contains(entity) || palette_surfaces.contains(entity) {
            return;
        }
        if let Ok((marker, viewport, computed, transform)) = viewports.get(entity) {
            let menu_position =
                pointer_position_in_node(click.pointer_location.position, computed, transform);
            palette.open = Some(MaterialGraphPaletteOpen {
                program: marker.program,
                menu_position,
                graph_position: viewport.unproject_viewport_point(menu_position),
                graph_key: material_graph_view_key(marker.program),
            });
            palette.query.clear();
            session.ui_revision += 1;
            click.propagate(false);
            return;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    }
}

fn open_material_graph_palette_from_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &RelativeCursorPosition,
        &ComputedNode,
    )>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut session: ResMut<EditorSession>,
) {
    if !keys.just_pressed(KeyCode::Tab) || palette.open.is_some() {
        return;
    }
    let Some((marker, viewport, cursor, computed)) = viewports
        .iter()
        .find(|(_, _, cursor, _)| cursor.cursor_over())
    else {
        return;
    };
    let Some(normalized) = cursor.normalized else {
        return;
    };
    let menu_position = (normalized + Vec2::splat(0.5)) * computed.size();
    palette.open = Some(MaterialGraphPaletteOpen {
        program: marker.program,
        menu_position,
        graph_position: viewport.unproject_viewport_point(menu_position),
        graph_key: material_graph_view_key(marker.program),
    });
    palette.query.clear();
    session.ui_revision += 1;
}

fn dismiss_material_graph_palette(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    surfaces: Query<&RelativeCursorPosition, With<MaterialGraphPalette>>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut session: ResMut<EditorSession>,
) {
    if should_dismiss_pointer_context_menu(
        palette.open.is_some(),
        buttons.just_pressed(MouseButton::Left),
        keys.just_pressed(KeyCode::Escape),
        surfaces.iter().any(RelativeCursorPosition::cursor_over),
    ) {
        palette.open = None;
        palette.query.clear();
        session.ui_revision += 1;
    }
}

fn update_material_graph_palette_search(
    change: On<ValueChange<String>>,
    searches: Query<(), With<MaterialGraphPaletteSearch>>,
    mut items: Query<(&MaterialGraphPaletteAction, &mut Node)>,
    mut palette: ResMut<MaterialGraphPaletteState>,
) {
    if !searches.contains(change.source) {
        return;
    }
    palette.query = change.value.trim().to_lowercase();
    for (action, mut node) in &mut items {
        node.display = if palette.query.is_empty() || action.searchable.contains(&palette.query) {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn focus_material_graph_palette_search(
    searches: Query<Entity, Added<MaterialGraphPaletteSearch>>,
    mut focus: ResMut<InputFocus>,
) {
    if let Some(entity) = searches.iter().next() {
        focus.set(entity, FocusCause::Navigated);
    }
}

fn begin_material_connection_drag(
    mut event: On<Pointer<DragStart>>,
    sockets: Query<(&MaterialGraphSocket, &ComputedNode)>,
    viewports: Query<(Entity, &MaterialGraphViewport)>,
    wire_layers: Query<(Entity, &FeathersGraphWireLayer)>,
    mut materials: ResMut<Assets<GraphWireMaterial>>,
    mut gesture: ResMut<MaterialGraphGesture>,
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if event.button != PointerButton::Primary || keys.pressed(KeyCode::Space) {
        return;
    }
    let Ok((socket, computed)) = sockets.get(event.event_target()) else {
        return;
    };
    let MaterialGraphSocketKind::ExpressionOutput(source) = socket.kind else {
        return;
    };
    let cursor = event.pointer_location.position / computed.inverse_scale_factor;
    *gesture = MaterialGraphGesture::Connecting {
        program: socket.program,
        source,
        cursor,
        snap_target: None,
    };
    if let Some((viewport, _)) = viewports
        .iter()
        .find(|(_, viewport)| viewport.program == socket.program)
        && let Some((wire_layer, _)) = wire_layers
            .iter()
            .find(|(_, wire_layer)| wire_layer.viewport == viewport)
    {
        let material = materials.add(GraphWireMaterial::default());
        commands.spawn((
            MaterialGraphGhostWire {
                program: socket.program,
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            MaterialNode(material),
            Pickable::IGNORE,
            ChildOf(wire_layer),
        ));
    }
    event.propagate(false);
}

fn update_material_connection_drag(
    mut event: On<Pointer<Drag>>,
    sockets: Query<&ComputedNode, With<MaterialGraphSocket>>,
    mut gesture: ResMut<MaterialGraphGesture>,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Ok(computed) = sockets.get(event.event_target()) else {
        return;
    };
    if let MaterialGraphGesture::Connecting { cursor, .. } = &mut *gesture {
        *cursor = event.pointer_location.position / computed.inverse_scale_factor;
        event.propagate(false);
    }
}

fn finish_material_connection_drag(
    mut event: On<Pointer<DragEnd>>,
    sockets: Query<(), With<MaterialGraphSocket>>,
    ghosts: Query<Entity, With<MaterialGraphGhostWire>>,
    mut gesture: ResMut<MaterialGraphGesture>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary || sockets.get(event.event_target()).is_err() {
        return;
    }
    let previous = std::mem::replace(&mut *gesture, MaterialGraphGesture::Idle);
    let MaterialGraphGesture::Connecting {
        program,
        source,
        snap_target,
        ..
    } = previous
    else {
        return;
    };
    for ghost in &ghosts {
        if let Ok(mut entity) = commands.get_entity(ghost) {
            entity.despawn();
        }
    }
    if let Some(target) = snap_target {
        apply_material_connection(
            &mut session,
            &mut catalog,
            &mut material_history,
            &mut history_ledger,
            program,
            source,
            target,
        );
    }
    event.propagate(false);
}

fn stop_material_socket_click(
    mut event: On<Pointer<Click>>,
    sockets: Query<(), With<MaterialGraphSocket>>,
) {
    if sockets.get(event.event_target()).is_ok() {
        event.propagate(false);
    }
}

fn apply_material_connection(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    material_history: &mut MaterialProgramEditHistory,
    history_ledger: &mut EditorHistoryLedger,
    program: MaterialProgramId,
    source: MaterialExpressionId,
    target: MaterialConnectionTarget,
) {
    let result = apply_material_tool_command(
        &session.effect,
        catalog,
        material_history,
        program,
        "Connect material nodes",
        MaterialToolCommand::ConnectMaterialExpression {
            program,
            source,
            target,
        },
    );
    match result {
        Ok(_) => {
            history_ledger.record_material_edit(session);
            session.status = "Connected material nodes".into();
        }
        Err(error) => session.status = format!("Material connection failed: {error}"),
    }
    session.ui_revision += 1;
}

fn handle_material_graph_palette_actions(
    mut commands: Commands,
    actions: Query<
        (
            Entity,
            &Interaction,
            &MaterialGraphPaletteAction,
            Option<&PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
    mut graph_memory: ResMut<GraphViewportMemory>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut palette: ResMut<MaterialGraphPaletteState>,
) {
    for (entity, interaction, action, pending) in &actions {
        if *interaction != Interaction::Pressed || pending.is_none() {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>()
            .insert(Interaction::None);
        let command = match action.edit {
            MaterialGraphPaletteEdit::Insert(placement) => {
                MaterialToolCommand::InsertMaterialOperation {
                    program: action.program,
                    kind: action.kind,
                    placement,
                }
            }
            MaterialGraphPaletteEdit::Wrap(target) => MaterialToolCommand::WrapMaterialExpression {
                program: action.program,
                target,
                kind: action.kind,
            },
        };
        let result = apply_material_tool_command(
            &session.effect,
            &mut catalog,
            &mut material_history,
            action.program,
            "Add material graph node",
            command,
        );
        match result {
            Ok(plan) => {
                history_ledger.record_material_edit(&mut session);
                let position =
                    action.graph_position - Vec2::new(NODE_WIDTH * 0.5, NODE_HEADER_HEIGHT * 0.5);
                for expression in &plan.created_expressions {
                    graph_memory.place_node(
                        action.graph_key.clone(),
                        format!("expression:{expression}"),
                        position,
                    );
                }
                if let Some(expression) = plan.created_expressions.last().copied() {
                    inspector.selected = Some((action.program, expression));
                }
                session.status = format!("Added {} node", action.kind.display_name());
            }
            Err(error) => session.status = format!("Could not add material node: {error}"),
        }
        palette.open = None;
        palette.query.clear();
        session.ui_revision += 1;
    }
}

fn apply_material_tool_command(
    effect: &aestra_core::EffectAsset,
    catalog: &mut ProjectEffectCatalog,
    material_history: &mut MaterialProgramEditHistory,
    program: MaterialProgramId,
    label: &str,
    command: MaterialToolCommand,
) -> Result<MaterialToolPlan, String> {
    let programs = catalog.material_programs_for_effect(effect)?;
    let current = programs
        .iter()
        .find(|candidate| candidate.id == program)
        .cloned()
        .ok_or_else(|| format!("Material program {program} is unavailable"))?;
    let document = MaterialAuthoringDocument::new(effect.clone(), programs);
    let plan = MaterialToolPlanner::plan(&document, command).map_err(|error| error.to_string())?;
    let replacement = plan
        .replacement_program(program)
        .cloned()
        .ok_or_else(|| format!("material tool plan omitted replacement program {program}"))?;
    material_history.execute_replacement(effect, catalog, label, current, replacement)?;
    Ok(plan)
}

fn attach_material_graph_wire_materials(
    wires: Query<(Entity, &MaterialGraphWire), Without<MaterialNode<GraphWireMaterial>>>,
    mut materials: ResMut<Assets<GraphWireMaterial>>,
    mut commands: Commands,
) {
    for (entity, wire) in &wires {
        let material = materials.add(GraphWireMaterial {
            color: wire.color,
            ..default()
        });
        commands.entity(entity).insert(MaterialNode(material));
    }
}

fn update_material_graph_wires(
    mut materials: ResMut<Assets<GraphWireMaterial>>,
    wires: Query<(&MaterialGraphWire, &MaterialNode<GraphWireMaterial>)>,
    ghosts: Query<(&MaterialGraphGhostWire, &MaterialNode<GraphWireMaterial>)>,
    mut sockets: Query<(
        &MaterialGraphSocket,
        &UiGlobalTransform,
        &mut MaterialGraphSocketAnchor,
    )>,
    graph_nodes: Query<(&FeathersGraphNode, &ComputedNode, &UiGlobalTransform)>,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    mut gesture: ResMut<MaterialGraphGesture>,
) {
    // Socket offsets are graph-local and invariant under pan and zoom. Cache them after layout,
    // then project from graph state directly so a render frame can never mix UI transforms from
    // two adjacent zoom layouts.
    let mut socket_positions = Vec::with_capacity(sockets.iter().len());
    for (socket, transform, mut anchor) in &mut sockets {
        let Ok((node, computed, node_transform)) = graph_nodes.get(anchor.node) else {
            continue;
        };
        let (_, _, world) = transform.to_scale_angle_translation();
        let offset = *anchor
            .offset
            .get_or_insert_with(|| viewport_local_position(computed, node_transform, world));
        socket_positions.push(MaterialGraphSocketPosition {
            program: socket.program,
            kind: socket.kind,
            graph: node.position() + offset,
            world,
        });
    }

    for (wire, material) in &wires {
        let Some(start_graph) = socket_graph_position(
            &socket_positions,
            wire.program,
            MaterialGraphSocketKind::ExpressionOutput(wire.source),
        ) else {
            continue;
        };
        let Some(end_graph) = socket_graph_position(
            &socket_positions,
            wire.program,
            MaterialGraphSocketKind::ConnectionInput(wire.target),
        ) else {
            continue;
        };
        let Some((_, viewport, _, _)) = viewports
            .iter()
            .find(|(marker, _, _, _)| marker.program == wire.program)
        else {
            continue;
        };
        let start = viewport.project_graph_point(start_graph);
        let end = viewport.project_graph_point(end_graph);
        update_wire_material(&mut materials, &material.0, start, end, wire.color, 2.0);
    }

    let MaterialGraphGesture::Connecting {
        program,
        source,
        cursor,
        snap_target,
    } = &mut *gesture
    else {
        return;
    };
    let Some(start_graph) = socket_graph_position(
        &socket_positions,
        *program,
        MaterialGraphSocketKind::ExpressionOutput(*source),
    ) else {
        return;
    };

    let document = catalog
        .material_programs_for_effect(&session.effect)
        .ok()
        .map(|programs| MaterialAuthoringDocument::new(session.effect.clone(), programs));
    let mut nearest: Option<(f32, MaterialConnectionTarget, Vec2)> = None;
    for socket in &socket_positions {
        let MaterialGraphSocketKind::ConnectionInput(target) = socket.kind else {
            continue;
        };
        if socket.program != *program {
            continue;
        }
        let distance = socket.world.distance(*cursor);
        if distance > SNAP_RADIUS
            || nearest
                .as_ref()
                .is_some_and(|(nearest_distance, _, _)| distance >= *nearest_distance)
        {
            continue;
        }
        let valid = document.as_ref().is_some_and(|document| {
            MaterialToolPlanner::plan(
                document,
                MaterialToolCommand::ConnectMaterialExpression {
                    program: *program,
                    source: *source,
                    target,
                },
            )
            .is_ok()
        });
        if valid {
            nearest = Some((distance, target, socket.graph));
        }
    }
    *snap_target = nearest.map(|(_, target, _)| target);
    let Some((_, viewport, computed, transform)) = viewports
        .iter()
        .find(|(marker, _, _, _)| marker.program == *program)
    else {
        return;
    };
    let start = viewport.project_graph_point(start_graph);
    let end = nearest.map_or_else(
        || viewport_local_position(computed, transform, *cursor),
        |(_, _, graph)| viewport.project_graph_point(graph),
    );
    for (ghost, material) in &ghosts {
        if ghost.program != *program {
            continue;
        }
        let color = if snap_target.is_some() {
            Vec4::new(0.45, 1.0, 0.72, 1.0)
        } else {
            Vec4::new(0.76, 0.70, 0.92, 0.72)
        };
        update_wire_material(&mut materials, &material.0, start, end, color, 2.5);
    }
}

fn viewport_local_position(
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
    world_position: Vec2,
) -> Vec2 {
    transform.try_inverse().map_or(world_position, |inverse| {
        inverse.transform_point2(world_position) + computed.size() * 0.5
    })
}

fn socket_graph_position(
    sockets: &[MaterialGraphSocketPosition],
    program: MaterialProgramId,
    kind: MaterialGraphSocketKind,
) -> Option<Vec2> {
    sockets
        .iter()
        .find(|socket| socket.program == program && socket.kind == kind)
        .map(|socket| socket.graph)
}

fn set_wire_points(material: &mut GraphWireMaterial, start: Vec2, end: Vec2) {
    let control = (end.x - start.x).abs().mul_add(0.5, 54.0).min(220.0);
    material.start = start;
    material.control_start = start + Vec2::new(control, 0.0);
    material.control_end = end - Vec2::new(control, 0.0);
    material.end = end;
}

fn update_wire_material(
    materials: &mut Assets<GraphWireMaterial>,
    handle: &Handle<GraphWireMaterial>,
    start: Vec2,
    end: Vec2,
    color: Vec4,
    width: f32,
) {
    const POSITION_EPSILON_SQUARED: f32 = 0.0001;
    let unchanged = materials.get(handle).is_some_and(|material| {
        material.start.distance_squared(start) <= POSITION_EPSILON_SQUARED
            && material.end.distance_squared(end) <= POSITION_EPSILON_SQUARED
            && material.color == color
            && material.width == width
    });
    if unchanged {
        return;
    }
    if let Some(mut material) = materials.get_mut(handle) {
        set_wire_points(&mut material, start, end);
        material.color = color;
        material.width = width;
    }
}

pub(crate) fn spawn_material_graph_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    inspector: &MaterialStackInspectorState,
    palette: &MaterialGraphPaletteState,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            let projection = selected_projection(session, catalog);
            spawn_header(panel, projection.as_ref().ok(), localizer, asset_server);
            let Ok((_program_name, projection)) = projection else {
                spawn_panel_empty_state(
                    panel,
                    &localizer.text("material-graph-empty"),
                    &localizer.text("material-graph-empty-description"),
                    theme::ACCENT,
                );
                return;
            };
            let layout = layout_graph(&projection);
            let graph_key = material_graph_view_key(projection.program);
            let selection_bounds = selected_graph_node_bounds(&layout, &projection, inspector);
            let viewport = spawn_graph_viewport(
                panel,
                GraphViewportProps {
                    key: graph_key.clone(),
                    content_size: layout.size,
                    selection_bounds,
                },
                MaterialGraphCanvas,
                |overlay| spawn_graph_wires(overlay, &projection),
                |canvas| {
                    for node in &projection.nodes {
                        let position = layout
                            .nodes
                            .get(&node.expression)
                            .copied()
                            .unwrap_or(Vec2::splat(CANVAS_PADDING));
                        spawn_expression_node(
                            canvas,
                            projection.program,
                            node,
                            position,
                            inspector,
                            localizer,
                            asset_server,
                            &graph_key,
                        );
                    }
                    spawn_output_node(
                        canvas,
                        projection.program,
                        &projection.outputs,
                        layout.output,
                        localizer,
                        asset_server,
                        &graph_key,
                    );
                },
            );
            panel
                .commands()
                .entity(viewport)
                .insert(MaterialGraphViewport {
                    program: projection.program,
                });
            if let Some(open) = palette
                .open
                .as_ref()
                .filter(|open| open.program == projection.program)
            {
                let options = material_graph_palette_options(
                    session,
                    catalog,
                    projection.program,
                    &projection,
                    &layout,
                    open.graph_position.x,
                );
                panel.commands().entity(viewport).with_children(|viewport| {
                    spawn_material_graph_palette(
                        viewport,
                        open,
                        &options,
                        &palette.query,
                        localizer,
                    );
                });
            }
        });
}

fn material_graph_palette_options(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    program: MaterialProgramId,
    projection: &MaterialGraphProjection,
    layout: &MaterialGraphLayout,
    graph_x: f32,
) -> Vec<MaterialGraphPaletteOption> {
    let Ok(programs) = catalog.material_programs_for_effect(&session.effect) else {
        return Vec::new();
    };
    let document = MaterialAuthoringDocument::new(session.effect.clone(), programs);
    let Ok(report) =
        MaterialInspector::inspect(&document, MaterialInspectionTarget::Program(program))
    else {
        return Vec::new();
    };
    select_palette_operations(
        &document,
        program,
        projection,
        &report.operations,
        layout,
        graph_x,
    )
}

fn select_palette_operations(
    document: &MaterialAuthoringDocument,
    program: MaterialProgramId,
    projection: &MaterialGraphProjection,
    operations: &[MaterialOperationAvailability],
    layout: &MaterialGraphLayout,
    graph_x: f32,
) -> Vec<MaterialGraphPaletteOption> {
    MaterialStackModifierKind::INSERTABLE
        .into_iter()
        .filter_map(|kind| {
            let insert = operations
                .iter()
                .filter(|operation| operation.kind == kind)
                .map(|operation| {
                    (
                        (insertion_position_x(operation.placement, layout) - graph_x).abs(),
                        MaterialGraphPaletteEdit::Insert(operation.placement),
                    )
                })
                .min_by(|left, right| left.0.total_cmp(&right.0));
            let wrap = projection
                .edges
                .iter()
                .filter_map(|edge| edge_target(&edge.target))
                .filter(|target| {
                    MaterialToolPlanner::plan(
                        document,
                        MaterialToolCommand::WrapMaterialExpression {
                            program,
                            target: *target,
                            kind,
                        },
                    )
                    .is_ok()
                })
                .map(|target| {
                    (
                        (connection_target_x(target, layout) - graph_x).abs(),
                        MaterialGraphPaletteEdit::Wrap(target),
                    )
                })
                .min_by(|left, right| left.0.total_cmp(&right.0));
            match (insert, wrap) {
                (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
                (Some(candidate), None) | (None, Some(candidate)) => Some(candidate),
                (None, None) => None,
            }
            .map(|(_, edit)| MaterialGraphPaletteOption { kind, edit })
        })
        .collect()
}

fn insertion_position_x(placement: MaterialInsertionPoint, layout: &MaterialGraphLayout) -> f32 {
    match placement {
        MaterialInsertionPoint::Start => CANVAS_PADDING,
        MaterialInsertionPoint::End => layout.output.x,
        MaterialInsertionPoint::Before(expression) => layout
            .nodes
            .get(&expression)
            .map_or(layout.output.x, |position| position.x - COLUMN_WIDTH * 0.5),
        MaterialInsertionPoint::After(expression) => layout
            .nodes
            .get(&expression)
            .map_or(layout.output.x, |position| position.x + COLUMN_WIDTH * 0.5),
    }
}

fn connection_target_x(target: MaterialConnectionTarget, layout: &MaterialGraphLayout) -> f32 {
    match target {
        MaterialConnectionTarget::ExpressionInput { expression, .. } => layout
            .nodes
            .get(&expression)
            .map_or(layout.output.x, |position| position.x),
        MaterialConnectionTarget::ProgramOutput(_) => layout.output.x,
    }
}

fn spawn_material_graph_palette(
    parent: &mut ChildSpawnerCommands,
    open: &MaterialGraphPaletteOpen,
    options: &[MaterialGraphPaletteOption],
    query: &str,
    localizer: &Localizer,
) {
    spawn_pointer_context_menu_sized(
        parent,
        open.menu_position,
        268.0,
        MaterialGraphPaletteAnchor,
        MaterialGraphPalette,
        |menu| {
            spawn_search_field(
                menu,
                query,
                &localizer.text("material-graph-search-nodes"),
                &localizer.text("material-graph-clear-search"),
                MaterialGraphPaletteSearch,
            );
            if options.is_empty() {
                menu.spawn((
                    Text::new(localizer.text("material-graph-no-compatible-nodes")),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                    Node {
                        padding: UiRect::all(Val::Px(10.0)),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                return;
            }
            for option in options {
                let label = option.kind.display_name();
                let category = material_graph_node_category(option.kind);
                let searchable = format!("{category} {label}").to_lowercase();
                spawn_pointer_context_menu_custom_item(
                    menu,
                    label,
                    MaterialGraphPaletteAction {
                        program: open.program,
                        kind: option.kind,
                        edit: option.edit,
                        graph_position: open.graph_position,
                        graph_key: open.graph_key.clone(),
                        searchable,
                    },
                    |item| {
                        item.spawn(Node {
                            min_height: Val::Px(34.0),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::FlexStart,
                            justify_content: JustifyContent::Center,
                            ..default()
                        })
                        .with_children(|content| {
                            content.spawn((
                                Text::new(label),
                                TextFont {
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(theme::TEXT),
                                Pickable::IGNORE,
                            ));
                            content.spawn((
                                Text::new(category),
                                TextFont {
                                    font_size: FontSize::Px(8.0),
                                    ..default()
                                },
                                TextColor(theme::TEXT_FAINT),
                                Pickable::IGNORE,
                            ));
                        });
                    },
                );
            }
        },
    );
}

fn material_graph_node_category(kind: MaterialStackModifierKind) -> &'static str {
    match kind {
        MaterialStackModifierKind::PanUv
        | MaterialStackModifierKind::RotateUv
        | MaterialStackModifierKind::ScaleUv => "UV",
        MaterialStackModifierKind::Remap | MaterialStackModifierKind::Smoothstep => "Math",
        MaterialStackModifierKind::RadialMask
        | MaterialStackModifierKind::Dissolve
        | MaterialStackModifierKind::DissolveEdge => "Mask",
        MaterialStackModifierKind::SoftParticle => "Depth",
        MaterialStackModifierKind::BaseTexture
        | MaterialStackModifierKind::Fresnel
        | MaterialStackModifierKind::DepthFade => "Material",
    }
}

fn selected_projection(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
) -> Result<(String, MaterialGraphProjection), String> {
    let selected_renderer = match session.selection.primary {
        SemanticTarget::Renderer(id) => session
            .effect
            .emitters
            .iter()
            .flat_map(|emitter| &emitter.renderers)
            .find(|renderer| renderer.id == id),
        _ => session
            .selection
            .emitter(&session.effect)
            .and_then(|id| {
                session
                    .effect
                    .emitters
                    .iter()
                    .find(|emitter| emitter.id == id)
            })
            .and_then(|emitter| {
                emitter
                    .renderers
                    .iter()
                    .find(|renderer| renderer.enabled)
                    .or_else(|| emitter.renderers.first())
            }),
    }
    .ok_or_else(|| "selected emitter has no renderer".to_owned())?;
    let instance = session
        .effect
        .material_instances
        .iter()
        .find(|instance| instance.id == selected_renderer.material)
        .ok_or_else(|| "selected renderer does not use a semantic material".to_owned())?;
    let programs = catalog.material_programs_for_effect(&session.effect)?;
    let program = programs
        .iter()
        .find(|program| program.id == instance.program.id())
        .ok_or_else(|| "selected material program is unavailable".to_owned())?;
    let compiler = MaterialCompiler;
    let ir = compiler.compile(program).ok();
    Ok((
        program.name.clone(),
        compiler.project_graph(program, ir.as_ref()),
    ))
}

fn spawn_header(
    parent: &mut ChildSpawnerCommands,
    projection: Option<&(String, MaterialGraphProjection)>,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(38.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(14.0)),
                column_gap: Val::Px(10.0),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT),
        ))
        .with_children(|header| {
            header.spawn((
                Text::new(localizer.text("material-graph-edit-hint")),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
            ));
            header.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if let Some((name, graph)) = projection {
                let key = material_graph_view_key(graph.program);
                spawn_graph_frame_button(
                    header,
                    asset_server,
                    "icons/frame-all.svg",
                    localizer.text("material-graph-frame-all"),
                    GraphFrameAction::new(key.clone(), GraphFrameTarget::All),
                );
                spawn_graph_frame_button(
                    header,
                    asset_server,
                    "icons/frame-selection.svg",
                    localizer.text("material-graph-frame-selection"),
                    GraphFrameAction::new(key, GraphFrameTarget::Selection),
                );
                header.spawn((
                    Text::new(format!(
                        "{name}  ·  {} NODES  ·  {} LINKS",
                        graph.nodes.len(),
                        graph.edges.len()
                    )),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                ));
            }
        });
}

fn material_graph_view_key(program: MaterialProgramId) -> String {
    format!("material:{program}")
}

fn selected_graph_node_bounds(
    layout: &MaterialGraphLayout,
    graph: &MaterialGraphProjection,
    inspector: &MaterialStackInspectorState,
) -> Option<Rect> {
    let (program, expression) = inspector.selected?;
    if program != graph.program {
        return None;
    }
    let node = graph
        .nodes
        .iter()
        .find(|node| node.expression == expression)?;
    let position = *layout.nodes.get(&expression)?;
    Some(Rect::from_corners(
        position,
        position
            + Vec2::new(
                NODE_WIDTH,
                node_height(node.inputs.len(), node.disabled || !node.reachable),
            ),
    ))
}

fn spawn_graph_wires(parent: &mut ChildSpawnerCommands, graph: &MaterialGraphProjection) {
    for edge in &graph.edges {
        let Some(target) = edge_target(&edge.target) else {
            continue;
        };
        parent.spawn((
            MaterialGraphWire {
                program: graph.program,
                source: edge.source,
                target,
                color: wire_color(edge.value_type),
            },
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
    }
}

fn spawn_expression_node(
    parent: &mut ChildSpawnerCommands,
    program: MaterialProgramId,
    node: &MaterialGraphNode,
    position: Vec2,
    inspector: &MaterialStackInspectorState,
    localizer: &Localizer,
    asset_server: &AssetServer,
    graph_key: &str,
) {
    let selected = inspector.selected == Some((program, node.expression));
    let subtitle = type_domain(node.value_type, node.evaluation_domain);
    spawn_graph_node(
        parent,
        GraphNodeProps {
            graph_key: graph_key.to_owned(),
            node_key: format!("expression:{}", node.expression),
            title: node.label.clone(),
            subtitle,
            position,
            selected,
            muted: !node.reachable || node.disabled,
            collapse_icon: load_svg_icon(asset_server, "icons/chevron-down.svg"),
            expand_icon: load_svg_icon(asset_server, "icons/chevron-right.svg"),
            collapse_label: localizer.text("material-graph-collapse-node"),
            expand_label: localizer.text("material-graph-expand-node"),
        },
        (
            Button,
            MaterialGraphAction {
                program,
                expression: node.expression,
            },
        ),
        |graph_node, body| {
            if node.disabled || !node.reachable {
                let state = match (node.disabled, node.reachable) {
                    (true, false) => format!(
                        "{} · {}",
                        localizer.text("material-graph-disabled"),
                        localizer.text("material-graph-unreachable")
                    ),
                    (true, true) => localizer.text("material-graph-disabled"),
                    (false, false) => localizer.text("material-graph-unreachable"),
                    (false, true) => String::new(),
                };
                body.spawn((
                    Text::new(state),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.58, 0.30)),
                    Node {
                        margin: UiRect::horizontal(Val::Px(9.0)),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
            }
            for port in &node.inputs {
                let Some(target) = input_target(node.expression, &port.name) else {
                    continue;
                };
                spawn_graph_port(
                    body,
                    GraphPortProps {
                        label: port.name.replace('_', " "),
                        side: GraphSocketSide::Input,
                        color: socket_color(port.value_type),
                    },
                    (
                        MaterialGraphSocket {
                            program,
                            kind: MaterialGraphSocketKind::ConnectionInput(target),
                        },
                        MaterialGraphSocketAnchor {
                            node: graph_node,
                            offset: None,
                        },
                    ),
                );
            }
            spawn_graph_port(
                body,
                GraphPortProps {
                    label: "result".into(),
                    side: GraphSocketSide::Output,
                    color: socket_color(node.value_type),
                },
                (
                    MaterialGraphSocket {
                        program,
                        kind: MaterialGraphSocketKind::ExpressionOutput(node.expression),
                    },
                    MaterialGraphSocketAnchor {
                        node: graph_node,
                        offset: None,
                    },
                ),
            );
        },
    );
}

fn spawn_output_node(
    parent: &mut ChildSpawnerCommands,
    program: MaterialProgramId,
    outputs: &[MaterialGraphOutput],
    position: Vec2,
    localizer: &Localizer,
    asset_server: &AssetServer,
    graph_key: &str,
) {
    spawn_graph_node(
        parent,
        GraphNodeProps {
            graph_key: graph_key.to_owned(),
            node_key: "output".to_owned(),
            title: localizer.text("material-graph-outputs"),
            subtitle: "FRAGMENT".into(),
            position,
            selected: false,
            muted: false,
            collapse_icon: load_svg_icon(asset_server, "icons/chevron-down.svg"),
            expand_icon: load_svg_icon(asset_server, "icons/chevron-right.svg"),
            collapse_label: localizer.text("material-graph-collapse-node"),
            expand_label: localizer.text("material-graph-expand-node"),
        },
        (),
        |graph_node, body| {
            for output in outputs {
                let target = MaterialConnectionTarget::ProgramOutput(match output.kind {
                    MaterialGraphOutputKind::Color => MaterialOutputSocket::Color,
                    MaterialGraphOutputKind::Alpha => MaterialOutputSocket::Alpha,
                });
                spawn_graph_port(
                    body,
                    GraphPortProps {
                        label: format!("{:?}", output.kind).to_lowercase(),
                        side: GraphSocketSide::Input,
                        color: socket_color(output.value_type),
                    },
                    (
                        MaterialGraphSocket {
                            program,
                            kind: MaterialGraphSocketKind::ConnectionInput(target),
                        },
                        MaterialGraphSocketAnchor {
                            node: graph_node,
                            offset: None,
                        },
                    ),
                );
            }
        },
    );
}

fn layout_graph(graph: &MaterialGraphProjection) -> MaterialGraphLayout {
    let inputs = graph
        .nodes
        .iter()
        .map(|node| {
            (
                node.expression,
                node.inputs
                    .iter()
                    .map(|port| port.source)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut depths = BTreeMap::new();
    for node in &graph.nodes {
        expression_depth(node.expression, &inputs, &mut depths, &mut BTreeSet::new());
    }
    let mut columns = BTreeMap::<usize, Vec<&MaterialGraphNode>>::new();
    for node in &graph.nodes {
        columns
            .entry(depths.get(&node.expression).copied().unwrap_or_default())
            .or_default()
            .push(node);
    }
    let mut positions = BTreeMap::new();
    let mut maximum_y = CANVAS_PADDING;
    for (depth, nodes) in columns {
        let mut y = CANVAS_PADDING + 34.0;
        for node in nodes {
            positions.insert(
                node.expression,
                Vec2::new(CANVAS_PADDING + depth as f32 * COLUMN_WIDTH, y),
            );
            y += node_height(node.inputs.len(), node.disabled || !node.reachable) + NODE_GAP;
        }
        maximum_y = maximum_y.max(y);
    }
    let output_depth = depths.values().copied().max().unwrap_or_default() + 1;
    let output = Vec2::new(
        CANVAS_PADDING + output_depth as f32 * COLUMN_WIDTH,
        CANVAS_PADDING + 88.0,
    );
    let width = output.x + NODE_WIDTH + CANVAS_PADDING;
    let height = maximum_y.max(output.y + 130.0) + CANVAS_PADDING;
    MaterialGraphLayout {
        nodes: positions,
        output,
        size: Vec2::new(width.max(720.0), height.max(420.0)),
    }
}

fn expression_depth(
    expression: MaterialExpressionId,
    inputs: &BTreeMap<MaterialExpressionId, Vec<MaterialExpressionId>>,
    memo: &mut BTreeMap<MaterialExpressionId, usize>,
    visiting: &mut BTreeSet<MaterialExpressionId>,
) -> usize {
    if let Some(depth) = memo.get(&expression) {
        return *depth;
    }
    if !visiting.insert(expression) {
        return 0;
    }
    let depth = inputs
        .get(&expression)
        .into_iter()
        .flatten()
        .filter(|source| inputs.contains_key(source))
        .map(|source| expression_depth(*source, inputs, memo, visiting) + 1)
        .max()
        .unwrap_or_default();
    visiting.remove(&expression);
    memo.insert(expression, depth);
    depth
}

fn node_height(input_count: usize, has_state: bool) -> f32 {
    40.0 + (input_count.max(1) + 1) as f32 * PORT_ROW_HEIGHT + if has_state { 18.0 } else { 0.0 }
}

fn input_target(expression: MaterialExpressionId, name: &str) -> Option<MaterialConnectionTarget> {
    let input = match name {
        "left" => MaterialExpressionInput::Left,
        "right" => MaterialExpressionInput::Right,
        "start" => MaterialExpressionInput::Start,
        "end" => MaterialExpressionInput::End,
        "factor" => MaterialExpressionInput::Factor,
        "value" => MaterialExpressionInput::Value,
        "min" => MaterialExpressionInput::Minimum,
        "max" => MaterialExpressionInput::Maximum,
        "input_min" => MaterialExpressionInput::InputMinimum,
        "input_max" => MaterialExpressionInput::InputMaximum,
        "output_min" => MaterialExpressionInput::OutputMinimum,
        "output_max" => MaterialExpressionInput::OutputMaximum,
        "edge_min" => MaterialExpressionInput::EdgeMinimum,
        "edge_max" => MaterialExpressionInput::EdgeMaximum,
        "normal" => MaterialExpressionInput::Normal,
        "view" => MaterialExpressionInput::View,
        "power" => MaterialExpressionInput::Power,
        "radius" => MaterialExpressionInput::Radius,
        "softness" => MaterialExpressionInput::Softness,
        "threshold" => MaterialExpressionInput::Threshold,
        "edge_width" => MaterialExpressionInput::EdgeWidth,
        "scene_depth" => MaterialExpressionInput::SceneDepth,
        "pixel_depth" => MaterialExpressionInput::PixelDepth,
        "fade_distance" => MaterialExpressionInput::FadeDistance,
        "invert" => MaterialExpressionInput::Invert,
        "speed" => MaterialExpressionInput::Speed,
        "time" => MaterialExpressionInput::Time,
        "center" => MaterialExpressionInput::Center,
        "angle" => MaterialExpressionInput::Angle,
        "scale" => MaterialExpressionInput::Scale,
        "texture" => MaterialExpressionInput::Texture,
        "uv" => MaterialExpressionInput::Uv,
        "source" => MaterialExpressionInput::Source,
        "alpha" => MaterialExpressionInput::SourceAlpha,
        _ => return None,
    };
    Some(MaterialConnectionTarget::ExpressionInput { expression, input })
}

fn edge_target(target: &MaterialGraphEdgeTarget) -> Option<MaterialConnectionTarget> {
    match target {
        MaterialGraphEdgeTarget::Input { expression, port } => input_target(*expression, port),
        MaterialGraphEdgeTarget::Output(MaterialGraphOutputKind::Color) => Some(
            MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color),
        ),
        MaterialGraphEdgeTarget::Output(MaterialGraphOutputKind::Alpha) => Some(
            MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha),
        ),
    }
}

fn type_domain(
    value_type: Option<MaterialValueType>,
    domain: Option<MaterialExpressionDomain>,
) -> String {
    match (value_type, domain) {
        (Some(value_type), Some(domain)) => format!("{value_type:?} · {domain:?}").to_uppercase(),
        _ => "UNRESOLVED".into(),
    }
}

fn socket_color(value_type: Option<MaterialValueType>) -> Color {
    match value_type {
        Some(MaterialValueType::Float) => Color::srgb(0.45, 0.78, 1.0),
        Some(MaterialValueType::Vec2) => Color::srgb(0.30, 0.86, 0.72),
        Some(MaterialValueType::Vec3) => Color::srgb(0.48, 0.86, 0.38),
        Some(MaterialValueType::Vec4) => Color::srgb(0.81, 0.55, 1.0),
        Some(MaterialValueType::Color) => Color::srgb(1.0, 0.60, 0.34),
        Some(MaterialValueType::Texture2D(_)) => Color::srgb(0.92, 0.42, 0.77),
        Some(MaterialValueType::Bool) => Color::srgb(0.96, 0.38, 0.42),
        None => theme::TEXT_FAINT,
    }
}

fn wire_color(value_type: Option<MaterialValueType>) -> Vec4 {
    match value_type {
        Some(MaterialValueType::Float) => Vec4::new(0.45, 0.78, 1.0, 0.82),
        Some(MaterialValueType::Vec2) => Vec4::new(0.30, 0.86, 0.72, 0.82),
        Some(MaterialValueType::Vec3) => Vec4::new(0.48, 0.86, 0.38, 0.82),
        Some(MaterialValueType::Vec4) => Vec4::new(0.81, 0.55, 1.0, 0.82),
        Some(MaterialValueType::Color) => Vec4::new(1.0, 0.60, 0.34, 0.82),
        Some(MaterialValueType::Texture2D(_)) => Vec4::new(0.92, 0.42, 0.77, 0.82),
        Some(MaterialValueType::Bool) => Vec4::new(0.96, 0.38, 0.42, 0.82),
        None => Vec4::new(0.59, 0.62, 0.70, 0.68),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_graph_canvas_bundle_spawns_without_duplicate_components() {
        let mut world = World::new();
        let viewport = world.spawn_empty().id();
        let entity = world
            .spawn(crate::feathers::node_graph::graph_canvas_bundle(
                viewport,
                Vec2::new(720.0, 420.0),
                MaterialGraphCanvas,
            ))
            .id();

        assert!(world.get::<Node>(entity).is_some());
        assert!(world.get::<MaterialGraphCanvas>(entity).is_some());
    }

    #[test]
    fn graph_input_names_cover_every_current_semantic_socket() {
        let expression = MaterialExpressionId::new();
        for name in [
            "left",
            "right",
            "start",
            "end",
            "factor",
            "value",
            "min",
            "max",
            "input_min",
            "input_max",
            "output_min",
            "output_max",
            "edge_min",
            "edge_max",
            "normal",
            "view",
            "power",
            "radius",
            "softness",
            "threshold",
            "edge_width",
            "scene_depth",
            "pixel_depth",
            "fade_distance",
            "invert",
            "speed",
            "time",
            "center",
            "angle",
            "scale",
            "texture",
            "uv",
            "source",
            "alpha",
        ] {
            assert!(input_target(expression, name).is_some(), "missing {name}");
        }
        assert!(input_target(expression, "unknown").is_none());
    }

    #[test]
    fn graph_depth_places_consumers_after_their_sources() {
        let source = MaterialExpressionId::new();
        let middle = MaterialExpressionId::new();
        let output = MaterialExpressionId::new();
        let inputs = BTreeMap::from([
            (source, vec![]),
            (middle, vec![source]),
            (output, vec![middle]),
        ]);
        let mut memo = BTreeMap::new();
        assert_eq!(
            expression_depth(output, &inputs, &mut memo, &mut BTreeSet::new()),
            2
        );
        assert_eq!(memo[&source], 0);
        assert_eq!(memo[&middle], 1);
    }

    #[test]
    fn add_node_palette_offers_semantic_wraps_for_an_advanced_graph() {
        let effect = aestra_core::EffectAsset::from_ron(crate::MATERIAL_GRAPH_LAB_EFFECT_SOURCE)
            .expect("material graph lab effect should parse");
        let program = aestra_core::material::MaterialProgram::from_ron(
            crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE,
        )
        .expect("material graph lab program should parse");
        let document = MaterialAuthoringDocument::new(effect, vec![program.clone()]);
        let compiler = MaterialCompiler;
        let ir = compiler.compile(&program).unwrap();
        let projection = compiler.project_graph(&program, Some(&ir));
        let layout = layout_graph(&projection);

        let options = select_palette_operations(
            &document,
            program.id,
            &projection,
            &[],
            &layout,
            layout.output.x,
        );

        assert!(!options.is_empty());
        assert!(
            options
                .iter()
                .all(|option| matches!(option.edit, MaterialGraphPaletteEdit::Wrap(_)))
        );
        for option in options {
            let MaterialGraphPaletteEdit::Wrap(target) = option.edit else {
                unreachable!();
            };
            assert!(
                MaterialToolPlanner::plan(
                    &document,
                    MaterialToolCommand::WrapMaterialExpression {
                        program: program.id,
                        target,
                        kind: option.kind,
                    }
                )
                .is_ok()
            );
        }
    }
}
