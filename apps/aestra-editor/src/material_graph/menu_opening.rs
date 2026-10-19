//! Actual material canvas/node/pin menu opening and canvas wire selection.
use crate::{
    EditorSession,
    feathers::{
        context_menu::{PointerContextSubmenuSurface, pointer_position_in_node},
        node_graph::{FeathersGraphNode, FeathersGraphViewport},
    },
    material_graph::{
        MaterialGraphAction, MaterialGraphSelectionState, MaterialGraphViewport,
        click_input::{GraphClick, PointerClick},
        keys::material_graph_view_key,
        palette::{MaterialGraphConnectionMenu, MaterialGraphNodeMenu, MaterialGraphPalette},
        palette_state::{
            MaterialGraphConnection, MaterialGraphConnectionMenuOpen, MaterialGraphNodeMenuOpen,
            MaterialGraphPaletteOpen, MaterialGraphPaletteState,
        },
        sockets::*,
    },
    properties::MaterialStackInspectorState,
};
use bevy::{picking::pointer::PointerButton, prelude::*};

pub(super) fn open_material_graph_palette(
    mut click: On<PointerClick>,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    graph_nodes: Query<(&FeathersGraphNode, &ComputedNode, &UiGlobalTransform)>,
    sockets: Query<(
        &MaterialGraphSocket,
        &UiGlobalTransform,
        &MaterialGraphSocketAnchor,
    )>,
    wires: Query<&MaterialGraphWire>,
    palette_surfaces: Query<
        (),
        Or<(
            With<MaterialGraphPalette>,
            With<MaterialGraphNodeMenu>,
            With<MaterialGraphConnectionMenu>,
            With<PointerContextSubmenuSurface>,
        )>,
    >,
    parents: Query<&ChildOf>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut inspector: ResMut<MaterialStackInspectorState>,
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
                pointer_position_in_node(click.graph_click_position(), computed, transform);
            let socket_positions = collect_socket_positions(&sockets, &graph_nodes);
            if let Some(connection) = wires
                .iter()
                .filter(|wire| wire.program == marker.program)
                .filter_map(|wire| {
                    let start = socket_graph_position(
                        &socket_positions,
                        wire.program,
                        MaterialGraphSocketKind::ExpressionOutput(wire.source),
                    )?;
                    let end = socket_graph_position(
                        &socket_positions,
                        wire.program,
                        MaterialGraphSocketKind::ConnectionInput(wire.target),
                    )?;
                    let distance = distance_to_graph_wire(
                        menu_position,
                        viewport.project_graph_point(start),
                        viewport.project_graph_point(end),
                    );
                    (distance <= 9.0).then_some((distance, *wire))
                })
                .min_by(|left, right| left.0.total_cmp(&right.0))
                .map(|(_, wire)| MaterialGraphConnection {
                    program: wire.program,
                    source: wire.source,
                    target: wire.target,
                })
            {
                let selected = selection.entry(marker.scope);
                selected.program = Some(marker.program);
                selected.expressions.clear();
                selected.connection = Some(connection);
                inspector.selected = None;
                palette.open = None;
                palette.node_menu = None;
                palette.connection_menu = Some(MaterialGraphConnectionMenuOpen {
                    program: marker.program,
                    scope: marker.scope,
                    menu_position,
                    connections: vec![connection],
                });
                palette.query.clear();
                session.ui_revision += 1;
                click.propagate(false);
                return;
            }
            palette.open = Some(MaterialGraphPaletteOpen {
                program: marker.program,
                editing_target: marker.editing_target.clone(),
                scope: marker.scope,
                menu_position,
                graph_position: viewport.unproject_viewport_point(menu_position),
                graph_key: material_graph_view_key(marker.program),
                connection: None,
            });
            palette.node_menu = None;
            palette.connection_menu = None;
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

pub(super) fn open_material_graph_pin_menu(
    mut click: On<PointerClick>,
    sockets: Query<&MaterialGraphSocket>,
    viewports: Query<(&MaterialGraphViewport, &ComputedNode, &UiGlobalTransform)>,
    parents: Query<&ChildOf>,
    wires: Query<&MaterialGraphWire>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let mut entity = click.event_target();
    let socket = loop {
        if let Ok(socket) = sockets.get(entity) {
            break *socket;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let mut ancestor = entity;
    let (viewport, computed, transform) = loop {
        if let Ok(viewport) = viewports.get(ancestor) {
            break viewport;
        }
        let Ok(parent) = parents.get(ancestor) else {
            return;
        };
        ancestor = parent.parent();
    };
    if viewport.program != socket.program {
        return;
    }
    let connections = wires
        .iter()
        .filter(|wire| {
            wire.program == socket.program
                && match socket.kind {
                    MaterialGraphSocketKind::ConnectionInput(target) => wire.target == target,
                    MaterialGraphSocketKind::ExpressionOutput(source) => wire.source == source,
                }
        })
        .map(|wire| MaterialGraphConnection {
            program: wire.program,
            source: wire.source,
            target: wire.target,
        })
        .collect::<Vec<_>>();
    let Some(connection) = connections.first().copied() else {
        return;
    };
    let selected = selection.entry(viewport.scope);
    selected.program = Some(socket.program);
    selected.expressions.clear();
    selected.connection = Some(connection);
    inspector.selected = None;
    palette.open = None;
    palette.node_menu = None;
    palette.connection_menu = Some(MaterialGraphConnectionMenuOpen {
        program: socket.program,
        scope: viewport.scope,
        menu_position: pointer_position_in_node(click.graph_click_position(), computed, transform),
        connections,
    });
    palette.query.clear();
    session.ui_revision += 1;
    click.propagate(false);
}

pub(super) fn open_material_graph_node_menu(
    mut click: On<PointerClick>,
    actions: Query<&MaterialGraphAction>,
    sockets: Query<(), With<MaterialGraphSocket>>,
    viewports: Query<(&MaterialGraphViewport, &ComputedNode, &UiGlobalTransform)>,
    parents: Query<&ChildOf>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let mut entity = click.event_target();
    let action = loop {
        if sockets.contains(entity) {
            return;
        }
        if let Ok(action) = actions.get(entity) {
            break *action;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let mut ancestor = entity;
    let (viewport, computed, transform) = loop {
        if let Ok(viewport) = viewports.get(ancestor) {
            break (viewport.0, viewport.1, viewport.2);
        }
        let Ok(parent) = parents.get(ancestor) else {
            return;
        };
        ancestor = parent.parent();
    };
    if viewport.program != action.program {
        return;
    }
    let scope = viewport.scope;
    if !selection.is_expression_selected(scope, action.program, action.expression) {
        selection.select_single(scope, action.program, action.expression);
    }
    inspector.selected = Some((action.program, action.expression));
    palette.open = None;
    palette.connection_menu = None;
    palette.query.clear();
    palette.node_menu = Some(MaterialGraphNodeMenuOpen {
        program: action.program,
        scope,
        menu_position: pointer_position_in_node(click.graph_click_position(), computed, transform),
    });
    session.ui_revision += 1;
    click.propagate(false);
}

pub(super) fn select_material_graph_canvas(
    mut click: On<PointerClick>,
    viewports: Query<(
        &MaterialGraphViewport,
        &FeathersGraphViewport,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    graph_nodes: Query<(&FeathersGraphNode, &ComputedNode, &UiGlobalTransform)>,
    sockets: Query<(
        &MaterialGraphSocket,
        &UiGlobalTransform,
        &MaterialGraphSocketAnchor,
    )>,
    wires: Query<&MaterialGraphWire>,
    parents: Query<&ChildOf>,
    controls: Query<
        (),
        Or<(
            With<FeathersGraphNode>,
            With<MaterialGraphPalette>,
            With<MaterialGraphNodeMenu>,
            With<MaterialGraphConnectionMenu>,
        )>,
    >,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let mut entity = click.event_target();
    let (marker, viewport, computed, transform) = loop {
        if controls.contains(entity) {
            return;
        }
        if let Ok(viewport) = viewports.get(entity) {
            break viewport;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let cursor = pointer_position_in_node(click.graph_click_position(), computed, transform);
    let socket_positions = collect_socket_positions(&sockets, &graph_nodes);
    let selected_wire = wires
        .iter()
        .filter(|wire| wire.program == marker.program)
        .filter_map(|wire| {
            let start = socket_graph_position(
                &socket_positions,
                wire.program,
                MaterialGraphSocketKind::ExpressionOutput(wire.source),
            )?;
            let end = socket_graph_position(
                &socket_positions,
                wire.program,
                MaterialGraphSocketKind::ConnectionInput(wire.target),
            )?;
            let start = viewport.project_graph_point(start);
            let end = viewport.project_graph_point(end);
            let distance = distance_to_graph_wire(cursor, start, end);
            (distance <= 9.0).then_some((
                distance,
                MaterialGraphConnection {
                    program: wire.program,
                    source: wire.source,
                    target: wire.target,
                },
            ))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, connection)| connection);
    let scope = marker.scope;
    let current = selection.get(scope);
    let changed = current.map(|selection| selection.program) != Some(Some(marker.program))
        || current.is_some_and(|selection| !selection.expressions.is_empty())
        || selection.selected_connection(scope) != selected_wire
        || inspector.selected.is_some();
    let selected = selection.entry(scope);
    selected.program = Some(marker.program);
    selected.expressions.clear();
    selected.connection = selected_wire;
    inspector.selected = None;
    if changed {
        session.ui_revision += 1;
    }
    click.propagate(false);
}

#[cfg(test)]
#[path = "menu_opening/tests.rs"]
mod tests;
