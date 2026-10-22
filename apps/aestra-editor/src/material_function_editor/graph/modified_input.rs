//! Production function modified-drag and duplication consumers.
use crate::feathers::node_graph::{geometry::GraphNodeKey, *};
use crate::material_function_editor::{
    FunctionEditor,
    graph::{
        clipboard,
        clipboard_context::{FunctionGraphNodeAction, activate_function_graph_target},
        composition::target,
        menu_state::Target,
    },
};
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_compiler::{MaterialCompiler, MaterialFunctionBodyProjection};
use aestra_core::{MaterialExpressionId, MaterialFunctionId};
use bevy::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn register(app: &mut App) {
    app.add_observer(handle_modified_function_node_drag);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn duplicate_function_selection(
    owner: MaterialFunctionId,
    scope: crate::material_graph::MaterialSelectionScope,
    anchor: MaterialExpressionId,
    offset: Vec2,
    graph_nodes: &Query<(&FunctionGraphNodeAction, &FeathersGraphNode)>,
    editor: &mut FunctionEditor,
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    memory: &mut GraphViewportMemory,
    selection: &mut crate::material_graph::MaterialGraphSelectionState,
) -> Result<usize, String> {
    let function = session.graph_function(catalog)?;
    if function.id != owner {
        return Err("The function editing target changed".into());
    }
    let mut selected = selection
        .function_arrange_seeds(scope, owner)
        .into_iter()
        .filter_map(|key| match key {
            GraphNodeKey::Expression(id) => Some(id),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if selected.is_empty() {
        selected.insert(anchor);
    }
    let positions = graph_nodes
        .iter()
        .filter(|(node, _)| {
            node.owner == owner && node.scope == scope && selected.contains(&node.expression)
        })
        .map(|(node, graph)| (node.expression, graph.position()))
        .collect::<BTreeMap<_, _>>();
    let graph_key = crate::material_graph::function_graph_memory_key(catalog.root(), owner);
    let positions = selected
        .iter()
        .map(|id| {
            (
                *id,
                positions
                    .get(id)
                    .copied()
                    .or_else(|| memory.node_position(&graph_key, &id.to_string()))
                    .unwrap_or(Vec2::ZERO),
            )
        })
        .collect();
    let fragment = crate::material_graph::clipboard::Fragment::capture(
        &function.expressions,
        &selected,
        positions,
        &[],
        &[],
    )?;
    clipboard::insert(
        &fragment, offset, owner, scope, editor, session, catalog, memory, selection,
    )
}

#[allow(clippy::too_many_arguments)]
fn handle_modified_function_node_drag(
    event: On<GraphModifiedNodeDrag>,
    graph_nodes: Query<(&FunctionGraphNodeAction, &FeathersGraphNode)>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut memory: ResMut<GraphViewportMemory>,
    mut selection: ResMut<crate::material_graph::MaterialGraphSelectionState>,
    mut commands: Commands,
) {
    let Some((action, _)) = graph_nodes
        .iter()
        .find(|(_, node)| node.graph_key() == event.graph && node.node_key() == event.node)
    else {
        return;
    };
    if !activate_function_graph_target(&mut session, &catalog, action.owner) {
        return;
    }
    if !selection.is_function_expression_selected(action.scope, action.owner, action.expression) {
        selection.select_function_expression(
            action.scope,
            action.owner,
            action.expression,
            false,
            false,
        );
    }
    let delta = event.after.0 - event.before.0;
    match event.modifier {
        GraphNodeDragModifier::Duplicate => {
            match duplicate_function_selection(
                action.owner,
                action.scope,
                action.expression,
                delta,
                &graph_nodes,
                &mut editor,
                &mut session,
                &mut catalog,
                &mut memory,
                &mut selection,
            ) {
                Ok(count) => session.status = format!("Duplicated {count} function node(s)"),
                Err(error) => {
                    session.status = format!("Could not duplicate function nodes: {error}")
                }
            }
        }
        GraphNodeDragModifier::Upstream | GraphNodeDragModifier::Downstream => {
            let Ok(function) = session.graph_function(&catalog) else {
                return;
            };
            if function.id != action.owner {
                return;
            }
            let Ok(library) = catalog.material_function_library() else {
                return;
            };
            let projection = MaterialCompiler.project_function_graph(&function, &library);
            let keys = function_branch_nodes(
                &projection.body,
                GraphNodeKey::Expression(action.expression),
                event.modifier == GraphNodeDragModifier::Upstream,
            );
            let mut before = BTreeMap::new();
            for key in &keys {
                let node = match key {
                    GraphNodeKey::Expression(id) => id.to_string(),
                    GraphNodeKey::FunctionOutputs => "outputs".into(),
                    GraphNodeKey::MaterialOutputs => continue,
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
            selection.select_function_expressions(
                action.scope,
                action.owner,
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

pub(super) fn function_branch_nodes(
    projection: &MaterialFunctionBodyProjection,
    seed: GraphNodeKey,
    upstream: bool,
) -> BTreeSet<GraphNodeKey> {
    let MaterialFunctionBodyProjection::Graph { edges, .. } = projection else {
        return BTreeSet::from([seed]);
    };
    let mut result = BTreeSet::from([seed]);
    let mut frontier = vec![seed];
    while let Some(current) = frontier.pop() {
        for edge in edges {
            let Some(target) = target(&edge.target).map(|target| match target {
                Target::Input(expression, _) => GraphNodeKey::Expression(expression),
                Target::Output(_) => GraphNodeKey::FunctionOutputs,
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
