//! Function-native adapter for the shared material graph clipboard.
use super::*;
use crate::material_graph::clipboard::{self as shared, Fragment, GraphClipboard, Shortcut};

#[cfg(test)]
mod tests;

#[allow(clippy::too_many_arguments)]
pub(super) fn execute(
    action: Option<Shortcut>,
    delete: bool,
    owner: MaterialFunctionId,
    scope: crate::material_graph::MaterialSelectionScope,
    anchor: Option<Vec2>,
    nodes: &Query<(&FunctionGraphNodeAction, &FeathersGraphNode)>,
    clipboard: &mut GraphClipboard,
    selection: &mut crate::material_graph::MaterialGraphSelectionState,
    editor: &mut FunctionEditor,
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    memory: &mut GraphViewportMemory,
) -> Result<String, String> {
    if !activate_function_graph_target(session, catalog, owner) {
        return Err("The function is unavailable".into());
    }
    let selected = selection
        .function_arrange_seeds(scope, owner)
        .into_iter()
        .filter_map(|node| {
            if let GraphNodeKey::Expression(id) = node {
                Some(id)
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    if selected.is_empty() && action != Some(Shortcut::Paste) {
        return Err("Select nodes to copy or cut".into());
    }

    let function = session.graph_function(catalog)?;
    let graph_key = crate::material_graph::function_graph_memory_key(catalog.root(), owner);
    if matches!(
        action,
        Some(Shortcut::Copy | Shortcut::Cut | Shortcut::Duplicate)
    ) {
        let positions = nodes
            .iter()
            .filter(|(node, _)| node.owner == owner && node.scope == scope)
            .map(|(node, graph)| (node.expression, graph.position()))
            .collect::<BTreeMap<_, _>>();
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
        let fragment = Fragment::capture(&function.expressions, &selected, positions, &[], &[])?;
        if action == Some(Shortcut::Duplicate) {
            let count = insert(
                &fragment,
                Vec2::splat(24.0),
                owner,
                scope,
                editor,
                session,
                catalog,
                memory,
                selection,
            )?;
            return Ok(format!("Duplicated {count} function node(s)"));
        }
        let count = fragment.len();
        clipboard.fragment = Some(fragment);
        if action == Some(Shortcut::Copy) {
            return Ok(format!("Copied {count} node(s)"));
        }
    }
    if action == Some(Shortcut::Paste) {
        let Some(fragment) = &clipboard.fragment else {
            return Ok("No graph nodes to paste".into());
        };
        let offset = anchor.map_or(Vec2::splat(24.0), |position| fragment.offset_to(position));
        let count = insert(
            fragment, offset, owner, scope, editor, session, catalog, memory, selection,
        )?;
        return Ok(format!("Pasted {count} function node(s)"));
    }
    // The function transaction validates the complete selection atomically; a rejected cut
    // leaves the source intact and still copied, rather than partially deleting nodes.
    let before = crate::material_graph::presentation::Snapshot::capture(
        &graph_key, catalog, session, memory,
    );
    editor.edit_body(
        session,
        catalog,
        selected
            .iter()
            .rev()
            .map(|id| Edit::Remove { expression: *id })
            .collect(),
    )?;
    for id in &selected {
        memory.remove_node(&graph_key, &id.to_string());
    }
    if let Some(before) = before {
        before.attach(catalog, session, memory);
    }
    selection.clear_function_selection(scope, owner);
    Ok(format!(
        "{} {} function node(s)",
        if delete { "Deleted" } else { "Cut" },
        selected.len()
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn insert(
    fragment: &Fragment,
    offset: Vec2,
    owner: MaterialFunctionId,
    scope: crate::material_graph::MaterialSelectionScope,
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
    let insert = fragment.instantiate(
        &function.expressions.iter().map(|node| node.id).collect(),
        offset,
    )?;
    let graph_key = crate::material_graph::function_graph_memory_key(catalog.root(), owner);
    let before = crate::material_graph::presentation::Snapshot::capture(
        &graph_key, catalog, session, memory,
    );
    editor.edit_body(
        session,
        catalog,
        insert
            .expressions
            .iter()
            .enumerate()
            .map(|(index, expression)| Edit::Add {
                expression: expression.clone(),
                index: function.expressions.len() + index,
            })
            .collect(),
    )?;
    for (id, position) in &insert.positions {
        memory.place_node(&graph_key, id.to_string(), *position);
    }
    if let Some(before) = before {
        before.attach(catalog, session, memory);
    }
    selection.select_function_expressions(
        scope,
        owner,
        &insert.positions.keys().copied().collect(),
        GraphSelectionMode::Replace,
    );
    Ok(fragment.len())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn keyboard(
    input: crate::input::ShortcutKeys,
    shortcuts: crate::input::ShortcutContext,
    views: Query<(
        &View,
        &ViewScope,
        &RelativeCursorPosition,
        &FeathersGraphViewport,
        &ComputedNode,
    )>,
    nodes: Query<(&FunctionGraphNodeAction, &FeathersGraphNode)>,
    mut clipboard: ResMut<GraphClipboard>,
    mut selection: ResMut<crate::material_graph::MaterialGraphSelectionState>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut memory: ResMut<GraphViewportMemory>,
) {
    if shortcuts.blocked() {
        return;
    }
    let Some((owner, scope, anchor)) =
        views
            .iter()
            .find_map(|(view, scope, cursor, graph, computed)| {
                cursor.cursor_over().then_some((
                    view.0,
                    scope.0,
                    cursor.normalized.map(|point| {
                        graph.unproject_viewport_point((point + Vec2::splat(0.5)) * computed.size())
                    }),
                ))
            })
    else {
        return;
    };
    for keys in input.iter() {
        let action = shared::shortcut(keys);
        let delete = keys.just_pressed(KeyCode::Delete);
        if action.is_none() && !delete {
            continue;
        }
        let selected = selection
            .function_arrange_seeds(scope, owner)
            .into_iter()
            .filter_map(|node| {
                if let GraphNodeKey::Expression(id) = node {
                    Some(id)
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        if selected.is_empty() && action != Some(Shortcut::Paste) {
            continue;
        }
        if !activate_function_graph_target(&mut session, &catalog, owner) {
            return;
        }
        let result = execute(
            action,
            delete,
            owner,
            scope,
            anchor,
            &nodes,
            &mut clipboard,
            &mut selection,
            &mut editor,
            &mut session,
            &mut catalog,
            &mut memory,
        );
        session.status = result
            .unwrap_or_else(|error: String| format!("Could not edit function nodes: {error}"));
        session.ui_revision += 1;
    }
}
