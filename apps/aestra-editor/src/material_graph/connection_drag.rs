//! Original connection pointer lifecycle, real planner snapping and wire asset updates.
use crate::{
    EditorSession, ProjectEffectCatalog,
    feathers::{
        context_menu::pointer_position_in_node,
        node_graph::{
            FeathersGraphNode, FeathersGraphViewport, FeathersGraphWireLayer, GraphWireMaterial,
        },
    },
    history::{EditorHistoryLedger, MaterialProgramEditHistory},
    material_graph::{
        MaterialGraphSelectionState, MaterialGraphViewport,
        click_input::PointerClick,
        connection_state::{MaterialGraphGesture, MaterialGraphGhostWire},
        connections::edge_target,
        drag_input::{GraphDrag, PointerDrag, PointerDragEnd, PointerDragStart},
        execution::{activate_material_graph_target, apply_material_tool_command},
        keys::material_graph_view_key,
        palette_state::{
            MaterialGraphConnection, MaterialGraphPaletteConnection, MaterialGraphPaletteOpen,
            MaterialGraphPaletteState,
        },
        sockets::*,
    },
};
use aestra_authoring::{MaterialConnectionTarget, MaterialToolCommand, MaterialToolPlanner};
use aestra_compiler::MaterialCompiler;
use aestra_core::{MaterialExpressionId, MaterialProgramId};
use bevy::{picking::pointer::PointerButton, prelude::*, ui_render::ui_material::MaterialNode};

const SNAP_RADIUS: f32 = 38.0;

pub(super) fn begin_material_connection_drag(
    mut event: On<PointerDragStart>,
    sockets: Query<(&MaterialGraphSocket, &ComputedNode)>,
    viewports: Query<(Entity, &MaterialGraphViewport)>,
    wire_layers: Query<(Entity, &FeathersGraphWireLayer)>,
    parents: Query<&ChildOf>,
    mut materials: ResMut<Assets<GraphWireMaterial>>,
    mut gesture: ResMut<MaterialGraphGesture>,
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if event.button != PointerButton::Primary || keys.pressed(KeyCode::Space) {
        return;
    }
    let socket_entity = event.event_target();
    let Ok((socket, computed)) = sockets.get(socket_entity) else {
        return;
    };
    // The drag belongs to the viewport that contains its socket, so a connection started in one of
    // two views of a program tracks and snaps in that view rather than the first one.
    let Some(viewport) = std::iter::once(socket_entity)
        .chain(parents.iter_ancestors(socket_entity))
        .find(|entity| viewports.contains(*entity))
    else {
        return;
    };
    let cursor = event.graph_drag_position() / computed.inverse_scale_factor;
    *gesture = MaterialGraphGesture::Connecting {
        program: socket.program,
        viewport,
        origin: socket.kind,
        cursor,
        snap: None,
    };
    if let Some((wire_layer, _)) = wire_layers
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

pub(super) fn update_material_connection_drag(
    mut event: On<PointerDrag>,
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
        *cursor = event.graph_drag_position() / computed.inverse_scale_factor;
        event.propagate(false);
    }
}

pub(super) fn finish_material_connection_drag(
    mut event: On<PointerDragEnd>,
    sockets: Query<(), With<MaterialGraphSocket>>,
    ghosts: Query<Entity, With<MaterialGraphGhostWire>>,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    mut gesture: ResMut<MaterialGraphGesture>,
    mut palette: ResMut<MaterialGraphPaletteState>,
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
        viewport: origin_viewport,
        origin,
        snap,
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
    if let Some((source, target)) = snap.and_then(|snap| connection_endpoints(origin, snap)) {
        if let Ok((marker, _, _, _)) = viewports.get(origin_viewport) {
            apply_material_connection(
                &mut session,
                &mut catalog,
                &mut material_history,
                &mut history_ledger,
                &marker.editing_target,
                program,
                source,
                target,
            );
        }
    } else if let Ok((marker, viewport, computed, transform)) = viewports.get(origin_viewport) {
        let menu_position =
            pointer_position_in_node(event.graph_drag_position(), computed, transform);
        palette.open = Some(MaterialGraphPaletteOpen {
            program,
            editing_target: marker.editing_target.clone(),
            scope: marker.scope,
            menu_position,
            graph_position: viewport.unproject_viewport_point(menu_position),
            graph_key: material_graph_view_key(program),
            connection: Some(match origin {
                MaterialGraphSocketKind::ExpressionOutput(source) => {
                    MaterialGraphPaletteConnection::FromOutput(source)
                }
                MaterialGraphSocketKind::ConnectionInput(target) => {
                    MaterialGraphPaletteConnection::FromInput(target)
                }
            }),
        });
        palette.node_menu = None;
        palette.connection_menu = None;
        palette.query.clear();
        session.ui_revision += 1;
    }
    event.propagate(false);
}

pub(super) fn connection_endpoints(
    first: MaterialGraphSocketKind,
    second: MaterialGraphSocketKind,
) -> Option<(MaterialExpressionId, MaterialConnectionTarget)> {
    match (first, second) {
        (
            MaterialGraphSocketKind::ExpressionOutput(source),
            MaterialGraphSocketKind::ConnectionInput(target),
        )
        | (
            MaterialGraphSocketKind::ConnectionInput(target),
            MaterialGraphSocketKind::ExpressionOutput(source),
        ) => Some((source, target)),
        _ => None,
    }
}

pub(super) fn stop_material_socket_click(
    mut event: On<PointerClick>,
    sockets: Query<(), With<MaterialGraphSocket>>,
) {
    if event.button == PointerButton::Primary && sockets.get(event.event_target()).is_ok() {
        event.propagate(false);
    }
}

pub(super) fn apply_material_connection(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    material_history: &mut MaterialProgramEditHistory,
    history_ledger: &mut EditorHistoryLedger,
    editing_target: &crate::material_document::MaterialEditingTarget,
    program: MaterialProgramId,
    source: MaterialExpressionId,
    target: MaterialConnectionTarget,
) {
    if let Err(error) = activate_material_graph_target(session, catalog, editing_target, program) {
        session.status = format!("Material connection failed: {error}");
        session.ui_revision += 1;
        return;
    }
    if current_material_connection_source(session, catalog, program, target) == Some(source) {
        session.status = "Material connection unchanged".into();
        session.ui_revision += 1;
        return;
    }
    let result = apply_material_tool_command(
        session,
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

pub(super) fn current_material_connection_source(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    program: MaterialProgramId,
    target: MaterialConnectionTarget,
) -> Option<MaterialExpressionId> {
    let programs = session.graph_material_programs(catalog).ok()?;
    let program = programs.iter().find(|candidate| candidate.id == program)?;
    let functions = catalog.material_function_library().ok()?;
    MaterialCompiler
        .project_graph_with_functions(program, None, &functions)
        .edges
        .into_iter()
        .find(|edge| edge_target(&edge.target) == Some(target))
        .map(|edge| edge.source)
}

pub(super) fn attach_material_graph_wire_materials(
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

pub(super) fn update_material_graph_wires(
    mut materials: ResMut<Assets<GraphWireMaterial>>,
    wires: Query<(Entity, &MaterialGraphWire, &MaterialNode<GraphWireMaterial>)>,
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
    // A wire lives in its viewport's own wire layer; both are resolved so each wire projects through
    // the viewport that owns it, keeping two views of one program independent.
    parents: Query<&ChildOf>,
    wire_layers: Query<&FeathersGraphWireLayer>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    selection: Res<MaterialGraphSelectionState>,
    mut gesture: ResMut<MaterialGraphGesture>,
    insertion: Option<Res<crate::feathers::node_graph::insertion::State>>,
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
        let compact_offset = crate::feathers::node_graph::compact_graph_socket_offset(
            node,
            computed,
            socket.kind.side(),
        );
        let offset = compact_offset.unwrap_or_else(|| {
            *anchor
                .offset
                .get_or_insert_with(|| viewport_local_position(computed, node_transform, world))
        });
        socket_positions.push(MaterialGraphSocketPosition {
            program: socket.program,
            kind: socket.kind,
            graph: node.position() + offset,
            world,
            interactive: compact_offset.is_none(),
        });
    }

    let detached_target = match &*gesture {
        MaterialGraphGesture::Connecting {
            origin: MaterialGraphSocketKind::ConnectionInput(target),
            ..
        } => Some(*target),
        _ => None,
    };
    for (entity, wire, material) in &wires {
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
        // Project through the viewport that owns this wire, not the first one for the program, so
        // both views of a program place their wires with their own pan/zoom.
        let Some((marker, viewport, computed, _)) = wire_viewport(entity, &parents, &wire_layers)
            .and_then(|viewport_entity| viewports.get(viewport_entity).ok())
        else {
            continue;
        };
        let start = viewport.project_graph_point(start_graph);
        let end = viewport.project_graph_point(end_graph);
        let selected = selection.selected_connection(marker.scope)
            == Some(MaterialGraphConnection {
                program: wire.program,
                source: wire.source,
                target: wire.target,
            });
        let detached = detached_target == Some(wire.target);
        update_wire_material(
            &mut materials,
            &material.0,
            start,
            end,
            if detached {
                Vec4::ZERO
            } else if let Some(color) = insertion.as_ref().and_then(|state| state.color(entity)) {
                color
            } else if selected {
                Vec4::new(0.70, 0.50, 1.0, 1.0)
            } else {
                wire.color
            },
            if detached {
                0.0
            } else if selected
                || insertion
                    .as_ref()
                    .and_then(|state| state.color(entity))
                    .is_some()
            {
                4.0
            } else {
                2.0
            },
            computed.inverse_scale_factor,
        );
    }

    let MaterialGraphGesture::Connecting {
        program,
        viewport: origin_viewport,
        origin,
        cursor,
        snap,
    } = &mut *gesture
    else {
        return;
    };
    let Some(origin_graph) = socket_graph_position(&socket_positions, *program, *origin) else {
        return;
    };

    let document = viewports
        .get(*origin_viewport)
        .ok()
        .and_then(|(marker, _, _, _)| {
            session
                .graph_authoring_document_for(&marker.editing_target, &catalog)
                .ok()
        });
    let mut nearest: Option<(f32, MaterialGraphSocketKind, Vec2)> = None;
    for socket in &socket_positions {
        if socket.program != *program || !socket.interactive {
            continue;
        }
        let Some((source, target)) = connection_endpoints(*origin, socket.kind) else {
            continue;
        };
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
                    source,
                    target,
                },
            )
            .is_ok()
        });
        if valid {
            nearest = Some((distance, socket.kind, socket.graph));
        }
    }
    *snap = nearest.map(|(_, endpoint, _)| endpoint);
    // Project through the view the drag started in, so the ghost tracks that pane even when a
    // second view of the same program is open.
    let Ok((_, viewport, computed, transform)) = viewports.get(*origin_viewport) else {
        return;
    };
    let origin_position = viewport.project_graph_point(origin_graph);
    let cursor = viewport_local_position(computed, transform, *cursor);
    let snapped = nearest.map(|(_, _, graph)| viewport.project_graph_point(graph));
    let (start, end) = match *origin {
        MaterialGraphSocketKind::ExpressionOutput(_) => {
            (origin_position, snapped.unwrap_or(cursor))
        }
        MaterialGraphSocketKind::ConnectionInput(_) => (snapped.unwrap_or(cursor), origin_position),
    };
    for (ghost, material) in &ghosts {
        if ghost.program != *program {
            continue;
        }
        let color = if snap.is_some() {
            Vec4::new(0.45, 1.0, 0.72, 1.0)
        } else {
            Vec4::new(0.76, 0.70, 0.92, 0.72)
        };
        update_wire_material(
            &mut materials,
            &material.0,
            start,
            end,
            color,
            2.5,
            computed.inverse_scale_factor,
        );
    }
}

/// The viewport entity that owns a wire (or ghost wire): the wire is a child of a graph wire layer,
/// which names its viewport. Lets each wire project through its own view's pan/zoom.
pub(super) fn wire_viewport(
    wire: Entity,
    parents: &Query<&ChildOf>,
    wire_layers: &Query<&FeathersGraphWireLayer>,
) -> Option<Entity> {
    let layer = parents.get(wire).ok()?.parent();
    Some(wire_layers.get(layer).ok()?.viewport)
}

pub(super) fn set_wire_points(material: &mut GraphWireMaterial, start: Vec2, end: Vec2) {
    let control = (end.x - start.x).abs().mul_add(0.5, 54.0).min(220.0);
    material.start = start;
    material.control_start = start + Vec2::new(control, 0.0);
    material.control_end = end - Vec2::new(control, 0.0);
    material.end = end;
}

pub(crate) fn update_wire_material(
    materials: &mut Assets<GraphWireMaterial>,
    handle: &Handle<GraphWireMaterial>,
    start: Vec2,
    end: Vec2,
    color: Vec4,
    width: f32,
    inverse_scale: f32,
) {
    const POSITION_EPSILON_SQUARED: f32 = 0.0001;
    let unchanged = materials.get(handle).is_some_and(|material| {
        material.start.distance_squared(start) <= POSITION_EPSILON_SQUARED
            && material.end.distance_squared(end) <= POSITION_EPSILON_SQUARED
            && material.color == color
            && material.width == width
            && material.inverse_scale == inverse_scale
    });
    if unchanged {
        return;
    }
    if let Some(mut material) = materials.get_mut(handle) {
        set_wire_points(&mut material, start, end);
        material.color = color;
        material.width = width;
        material.inverse_scale = inverse_scale;
    }
}

#[cfg(test)]
#[path = "connection_drag/tests.rs"]
mod tests;
