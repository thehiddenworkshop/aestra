//! Actual material node click/opening, marquee and modified-drag consumers.
use crate::{
    EditorSession, ProjectEffectCatalog,
    docking::{ToolPanel, WorkspaceLayout},
    feathers::node_graph::{
        FeathersGraphNode, GraphMarqueeSelection, GraphModifiedNodeDrag, GraphNodeDragModifier,
        GraphPresentationBatchEdit, GraphSelectionMode, GraphViewportMemory,
        geometry::GraphNodeKey,
    },
    history::{EditorHistoryLedger, MaterialProgramEditHistory},
    material_graph::{
        MaterialGraphAction, MaterialGraphPreviewState, MaterialGraphSelectionState,
        MaterialGraphViewport,
        click_input::PointerClick,
        connections::edge_target,
        execution::{MaterialGraphSelectionEdit, apply_material_graph_selection_edit},
        keys::{MATERIAL_GRAPH_OUTPUT_NODE_KEY, material_graph_expression_node_key},
        menu_actions::material_graph_target,
        node_controls::{
            MaterialGraphDefaultNumberControl, MaterialGraphDefaultToggleControl,
            MaterialGraphNodeScope,
        },
        sockets::MaterialGraphSocket,
    },
    properties::MaterialStackInspectorState,
    reveal_dock_panel,
};
use aestra_authoring::MaterialConnectionTarget;
use aestra_compiler::{MaterialCompiler, MaterialGraphProjection};
use aestra_core::material::MaterialExpressionKind;
use bevy::{picking::pointer::PointerButton, prelude::*};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn select_material_graph_node(
    mut click: On<PointerClick>,
    actions: Query<&MaterialGraphAction>,
    scopes: Query<&MaterialGraphNodeScope>,
    mut graph_nodes: Query<&mut FeathersGraphNode>,
    parents: Query<&ChildOf>,
    sockets: Query<(), With<MaterialGraphSocket>>,
    value_controls: Query<
        (),
        Or<(
            With<MaterialGraphDefaultNumberControl>,
            With<MaterialGraphDefaultToggleControl>,
        )>,
    >,
    keys: Res<ButtonInput<KeyCode>>,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut session: ResMut<EditorSession>,
    mut layout: ResMut<WorkspaceLayout>,
) {
    if click.button != PointerButton::Primary || keys.pressed(KeyCode::Space) {
        return;
    }
    let mut entity = click.event_target();
    let (action, action_entity) = loop {
        if sockets.contains(entity) || value_controls.contains(entity) {
            return;
        }
        if let Ok(action) = actions.get(entity) {
            break (action, entity);
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
    let scope = scopes
        .get(action_entity)
        .map(|scope| scope.0)
        .unwrap_or(None);
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    inspector.selected = selection
        .select_expression(scope, action.program, action.expression, control, shift)
        .map(|expression| (action.program, expression));
    session.ui_revision += 1;
    reveal_dock_panel(&mut layout, &mut session, ToolPanel::Properties);
    click.propagate(false);
}

pub(super) fn open_material_graph_function_call(
    mut click: On<PointerClick>,
    actions: Query<&MaterialGraphAction>,
    scopes: Query<&MaterialGraphNodeScope>,
    viewports: Query<&MaterialGraphViewport>,
    parents: Query<&ChildOf>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary || click.count < 2 {
        return;
    }
    let mut entity = click.event_target();
    let (action, scope) = loop {
        if let Ok(action) = actions.get(entity) {
            break (
                *action,
                scopes.get(entity).ok().map(|scope| scope.0).unwrap_or(None),
            );
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let Some(target) = material_graph_target(&viewports, action.program, scope) else {
        return;
    };
    let Ok(document) = session.graph_authoring_document_for(&target, &catalog) else {
        return;
    };
    let Some(expression) = document
        .programs
        .iter()
        .find(|program| program.id == action.program)
        .and_then(|program| {
            program
                .expressions
                .iter()
                .find(|expression| expression.id == action.expression)
        })
    else {
        return;
    };
    let MaterialExpressionKind::FunctionCall {
        function: aestra_core::material::MaterialFunctionRef::Project(function),
        ..
    } = &expression.kind
    else {
        return;
    };
    commands.trigger(crate::asset_browser::OpenFunction {
        function: *function,
        new_view: false,
    });
    click.propagate(false);
}

pub(super) fn select_material_graph_marquee(
    event: On<GraphMarqueeSelection>,
    viewports: Query<&MaterialGraphViewport>,
    graph_nodes: Query<(&MaterialGraphAction, &FeathersGraphNode)>,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut session: ResMut<EditorSession>,
) {
    let Ok(viewport) = viewports.get(event.viewport) else {
        return;
    };
    let expressions = graph_nodes
        .iter()
        .filter(|(action, node)| {
            action.program == viewport.program && event.nodes.contains(node.node_key())
        })
        .map(|(action, _)| action.expression)
        .collect::<BTreeSet<_>>();
    if selection.select_material_expressions(
        viewport.scope,
        viewport.program,
        &expressions,
        event.mode,
    ) {
        inspector.selected = (expressions.len() == 1)
            .then(|| (viewport.program, *expressions.iter().next().unwrap()));
        session.ui_revision += 1;
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_modified_material_node_drag(
    event: On<GraphModifiedNodeDrag>,
    node_entities: Query<(
        Entity,
        &MaterialGraphAction,
        &FeathersGraphNode,
        Option<&MaterialGraphNodeScope>,
    )>,
    graph_nodes: Query<(&MaterialGraphAction, &FeathersGraphNode)>,
    viewports: Query<&MaterialGraphViewport>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
    mut memory: ResMut<GraphViewportMemory>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut previews: ResMut<MaterialGraphPreviewState>,
    mut commands: Commands,
) {
    let Some((_, action, _, scope)) = node_entities
        .iter()
        .find(|(_, _, node, _)| node.graph_key() == event.graph && node.node_key() == event.node)
    else {
        return;
    };
    let scope = scope.map(|scope| scope.0).unwrap_or(None);
    let Some(target) = material_graph_target(&viewports, action.program, scope) else {
        return;
    };
    if !selection.is_expression_selected(scope, action.program, action.expression) {
        selection.select_single(scope, action.program, action.expression);
    }
    let delta = event.after.0 - event.before.0;
    match event.modifier {
        GraphNodeDragModifier::Duplicate => {
            apply_material_graph_selection_edit(
                MaterialGraphSelectionEdit::Duplicate,
                scope,
                action.program,
                &target,
                &graph_nodes,
                &mut session,
                &mut catalog,
                &mut material_history,
                &mut history_ledger,
                &mut memory,
                &mut inspector,
                &mut selection,
                &mut previews,
                Some(delta),
            );
        }
        GraphNodeDragModifier::Upstream | GraphNodeDragModifier::Downstream => {
            let Ok(document) = session.graph_authoring_document_for(&target, &catalog) else {
                return;
            };
            let Some(program) = document
                .programs
                .iter()
                .find(|program| program.id == action.program)
            else {
                return;
            };
            let functions = document.material_function_library();
            let projection =
                MaterialCompiler.project_graph_with_functions(program, None, &functions);
            let keys = graph_branch_nodes(
                &projection,
                GraphNodeKey::Expression(action.expression),
                event.modifier == GraphNodeDragModifier::Upstream,
            );
            let mut before = BTreeMap::new();
            for key in &keys {
                let node = match key {
                    GraphNodeKey::Expression(id) => material_graph_expression_node_key(*id),
                    GraphNodeKey::MaterialOutputs => MATERIAL_GRAPH_OUTPUT_NODE_KEY.into(),
                    GraphNodeKey::FunctionOutputs => continue,
                };
                let Some(state) = memory.node(&event.graph, &node) else {
                    continue;
                };
                let original = if node == event.node {
                    event.before
                } else {
                    state
                };
                before.insert(node.clone(), original);
                memory.set_node(&event.graph, node, original.0 + delta, original.1);
            }
            let expressions = keys
                .into_iter()
                .filter_map(|key| match key {
                    GraphNodeKey::Expression(id) => Some(id),
                    _ => None,
                })
                .collect();
            selection.select_material_expressions(
                scope,
                action.program,
                &expressions,
                GraphSelectionMode::Replace,
            );
            commands.trigger(GraphPresentationBatchEdit {
                graph: event.graph.clone(),
                before,
                origin: None,
            });
        }
    }
    session.ui_revision += 1;
}

pub(super) fn graph_branch_nodes(
    projection: &MaterialGraphProjection,
    seed: GraphNodeKey,
    upstream: bool,
) -> BTreeSet<GraphNodeKey> {
    let mut result = BTreeSet::from([seed]);
    let mut frontier = vec![seed];
    while let Some(current) = frontier.pop() {
        for edge in &projection.edges {
            let Some(target) = edge_target(&edge.target).map(|target| match target {
                MaterialConnectionTarget::ExpressionInput { expression, .. } => {
                    GraphNodeKey::Expression(expression)
                }
                MaterialConnectionTarget::ProgramOutput(_) => GraphNodeKey::MaterialOutputs,
            }) else {
                continue;
            };
            let source = GraphNodeKey::Expression(edge.source);
            let candidate = if upstream && target == current {
                Some(source)
            } else if !upstream && source == current {
                Some(target)
            } else {
                None
            };
            if let Some(candidate) = candidate
                && result.insert(candidate)
            {
                frontier.push(candidate);
            }
        }
    }
    result
}

#[cfg(test)]
#[path = "node_actions/tests.rs"]
mod tests;
