//! Projectional material graph workspace backed by semantic material commands.

use crate::{
    feathers::{
        node_graph::{
            GraphNodeProps, GraphPortProps, GraphSocketSide, GraphWireMaterial, NODE_WIDTH,
            PORT_ROW_HEIGHT, graph_canvas, spawn_graph_node, spawn_graph_port,
        },
        panel::spawn_panel_empty_state,
        scroll::spawn_bidirectional_scroll_area,
    },
    *,
};
use aestra_authoring::{
    MaterialAuthoringDocument, MaterialConnectionTarget, MaterialExpressionInput,
    MaterialOutputSocket, MaterialToolCommand, MaterialToolPlanner,
};
use aestra_compiler::{
    MaterialCompiler, MaterialGraphEdgeTarget, MaterialGraphNode, MaterialGraphOutput,
    MaterialGraphOutputKind, MaterialGraphProjection,
};
use aestra_core::{
    MaterialExpressionId, MaterialProgramId,
    material::{MaterialExpressionDomain, MaterialValueType},
};
use bevy::ui_render::ui_material::MaterialNode;
use std::collections::{BTreeMap, BTreeSet};

const COLUMN_WIDTH: f32 = 282.0;
const CANVAS_PADDING: f32 = 34.0;
const NODE_GAP: f32 = 22.0;
const SNAP_RADIUS: f32 = 38.0;

pub(crate) struct EditorMaterialGraphPlugin;

impl Plugin for EditorMaterialGraphPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MaterialGraphGesture>()
            .add_observer(begin_material_connection_drag)
            .add_observer(update_material_connection_drag)
            .add_observer(finish_material_connection_drag)
            .add_observer(stop_material_socket_click)
            .add_systems(
                Update,
                (
                    handle_material_graph_actions,
                    attach_material_graph_wire_materials,
                ),
            )
            .add_systems(
                PostUpdate,
                update_material_graph_wires.after(bevy::ui::UiSystems::Layout),
            );
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct MaterialGraphAction {
    program: MaterialProgramId,
    expression: MaterialExpressionId,
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphCanvas {
    program: MaterialProgramId,
}

fn material_graph_canvas_bundle(program: MaterialProgramId, size: Vec2) -> impl Bundle {
    (graph_canvas(size), MaterialGraphCanvas { program })
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
struct MaterialGraphWire {
    program: MaterialProgramId,
    source: MaterialExpressionId,
    target: MaterialConnectionTarget,
    color: Vec4,
}

#[derive(Component, Debug, Clone, Copy)]
struct MaterialGraphGhostWire;

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
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut session: ResMut<EditorSession>,
    mut layout: ResMut<WorkspaceLayout>,
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
                if inspector.selected != Some((action.program, action.expression)) {
                    inspector.selected = Some((action.program, action.expression));
                    session.ui_revision += 1;
                }
                reveal_dock_panel(&mut layout, &mut session, DockPanel::Properties);
            }
        }
    }
}

fn begin_material_connection_drag(
    mut event: On<Pointer<DragStart>>,
    sockets: Query<(&MaterialGraphSocket, &ComputedNode)>,
    canvases: Query<(Entity, &MaterialGraphCanvas)>,
    mut materials: ResMut<Assets<GraphWireMaterial>>,
    mut gesture: ResMut<MaterialGraphGesture>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary {
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
    if let Some((canvas, _)) = canvases
        .iter()
        .find(|(_, canvas)| canvas.program == socket.program)
    {
        let material = materials.add(GraphWireMaterial::default());
        commands.spawn((
            MaterialGraphGhostWire,
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
            ChildOf(canvas),
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
    let result = catalog
        .material_programs_for_effect(&session.effect)
        .and_then(|programs| {
            let current = programs
                .iter()
                .find(|candidate| candidate.id == program)
                .cloned()
                .ok_or_else(|| format!("Material program {program} is unavailable"))?;
            let document = MaterialAuthoringDocument::new(session.effect.clone(), programs);
            let plan = MaterialToolPlanner::plan(
                &document,
                MaterialToolCommand::ConnectMaterialExpression {
                    program,
                    source,
                    target,
                },
            )
            .map_err(|error| error.to_string())?;
            let replacement = plan
                .replacement_program(program)
                .cloned()
                .ok_or_else(|| "Connection plan did not replace its material program".to_owned())?;
            material_history.execute_replacement(
                &session.effect,
                catalog,
                "Connect material nodes",
                current,
                replacement,
            )
        });
    match result {
        Ok(()) => {
            history_ledger.record_material_edit(session);
            session.status = "Connected material nodes".into();
        }
        Err(error) => session.status = format!("Material connection failed: {error}"),
    }
    session.ui_revision += 1;
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
    ghosts: Query<&MaterialNode<GraphWireMaterial>, With<MaterialGraphGhostWire>>,
    sockets: Query<(&MaterialGraphSocket, &UiGlobalTransform)>,
    canvases: Query<(&MaterialGraphCanvas, &ComputedNode, &UiGlobalTransform)>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    mut gesture: ResMut<MaterialGraphGesture>,
) {
    for (wire, material) in &wires {
        let Some((origin, _)) = canvas_origin(&canvases, wire.program) else {
            continue;
        };
        let Some(start) = socket_position(
            &sockets,
            wire.program,
            MaterialGraphSocketKind::ExpressionOutput(wire.source),
        ) else {
            continue;
        };
        let Some(end) = socket_position(
            &sockets,
            wire.program,
            MaterialGraphSocketKind::ConnectionInput(wire.target),
        ) else {
            continue;
        };
        if let Some(mut material) = materials.get_mut(&material.0) {
            set_wire_points(&mut material, start - origin, end - origin);
            material.color = wire.color;
        }
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
    let Some((origin, _)) = canvas_origin(&canvases, *program) else {
        return;
    };
    let Some(start) = socket_position(
        &sockets,
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
    for (socket, transform) in &sockets {
        let MaterialGraphSocketKind::ConnectionInput(target) = socket.kind else {
            continue;
        };
        if socket.program != *program {
            continue;
        }
        let (_, _, position) = transform.to_scale_angle_translation();
        let distance = position.distance(*cursor);
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
            nearest = Some((distance, target, position));
        }
    }
    *snap_target = nearest.map(|(_, target, _)| target);
    let end = nearest.map_or(*cursor, |(_, _, position)| position);
    for material in &ghosts {
        if let Some(mut material) = materials.get_mut(&material.0) {
            set_wire_points(&mut material, start - origin, end - origin);
            material.color = if snap_target.is_some() {
                Vec4::new(0.45, 1.0, 0.72, 1.0)
            } else {
                Vec4::new(0.76, 0.70, 0.92, 0.72)
            };
            material.width = 2.5;
        }
    }
}

fn canvas_origin(
    canvases: &Query<(&MaterialGraphCanvas, &ComputedNode, &UiGlobalTransform)>,
    program: MaterialProgramId,
) -> Option<(Vec2, Vec2)> {
    canvases
        .iter()
        .find(|(canvas, _, _)| canvas.program == program)
        .map(|(_, computed, transform)| {
            let (_, _, center) = transform.to_scale_angle_translation();
            let size = computed.size();
            (center - size * 0.5, size)
        })
}

fn socket_position(
    sockets: &Query<(&MaterialGraphSocket, &UiGlobalTransform)>,
    program: MaterialProgramId,
    kind: MaterialGraphSocketKind,
) -> Option<Vec2> {
    sockets
        .iter()
        .find(|(socket, _)| socket.program == program && socket.kind == kind)
        .map(|(_, transform)| transform.to_scale_angle_translation().2)
}

fn set_wire_points(material: &mut GraphWireMaterial, start: Vec2, end: Vec2) {
    let control = (end.x - start.x).abs().mul_add(0.5, 54.0).min(220.0);
    material.start = start;
    material.control_start = start + Vec2::new(control, 0.0);
    material.control_end = end - Vec2::new(control, 0.0);
    material.end = end;
}

pub(crate) fn spawn_material_graph_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    inspector: &MaterialStackInspectorState,
    localizer: &Localizer,
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
            spawn_header(panel, projection.as_ref().ok(), localizer);
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
            spawn_bidirectional_scroll_area(
                panel,
                ScrollMemoryKey::MaterialGraph,
                Node {
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    ..default()
                },
                |viewport| {
                    viewport
                        .spawn(material_graph_canvas_bundle(
                            projection.program,
                            layout.size,
                        ))
                        .with_children(|canvas| {
                            spawn_graph_background(canvas);
                            spawn_graph_wires(canvas, &projection);
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
                                );
                            }
                            spawn_output_node(
                                canvas,
                                projection.program,
                                &projection.outputs,
                                layout.output,
                                localizer,
                            );
                        });
                },
            );
        });
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

fn spawn_graph_background(parent: &mut ChildSpawnerCommands) {
    const SPACING: f32 = 32.0;
    for index in 0..64 {
        let offset = index as f32 * SPACING;
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(offset),
                top: Val::Px(0.0),
                width: Val::Px(1.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.25, 0.28, 0.38, 0.10)),
            Pickable::IGNORE,
        ));
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(offset),
                width: Val::Percent(100.0),
                height: Val::Px(1.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.25, 0.28, 0.38, 0.10)),
            Pickable::IGNORE,
        ));
    }
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
) {
    let selected = inspector.selected == Some((program, node.expression));
    let subtitle = type_domain(node.value_type, node.evaluation_domain);
    spawn_graph_node(
        parent,
        GraphNodeProps {
            title: node.label.clone(),
            subtitle,
            position,
            selected,
            muted: !node.reachable || node.disabled,
        },
        (
            Button,
            MaterialGraphAction {
                program,
                expression: node.expression,
            },
        ),
        |body| {
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
                    MaterialGraphSocket {
                        program,
                        kind: MaterialGraphSocketKind::ConnectionInput(target),
                    },
                );
            }
            spawn_graph_port(
                body,
                GraphPortProps {
                    label: "result".into(),
                    side: GraphSocketSide::Output,
                    color: socket_color(node.value_type),
                },
                MaterialGraphSocket {
                    program,
                    kind: MaterialGraphSocketKind::ExpressionOutput(node.expression),
                },
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
) {
    spawn_graph_node(
        parent,
        GraphNodeProps {
            title: localizer.text("material-graph-outputs"),
            subtitle: "FRAGMENT".into(),
            position,
            selected: false,
            muted: false,
        },
        Pickable::IGNORE,
        |body| {
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
                    MaterialGraphSocket {
                        program,
                        kind: MaterialGraphSocketKind::ConnectionInput(target),
                    },
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
        let entity = world
            .spawn(material_graph_canvas_bundle(
                MaterialProgramId::new(),
                Vec2::new(720.0, 420.0),
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
}
