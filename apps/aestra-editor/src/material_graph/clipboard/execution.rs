//! Actual material clipboard capture and insertion through semantic/history publication.
use crate::feathers::node_graph::{FeathersGraphNode, GraphSelectionMode, GraphViewportMemory};
use crate::history::{EditorHistoryLedger, MaterialProgramEditHistory};
use crate::material_graph::{
    MaterialGraphAction, MaterialGraphSelectionState, MaterialSelectionScope,
    keys::{material_graph_expression_node_key, material_graph_view_key},
    presentation,
};
use crate::properties::MaterialStackInspectorState;
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_core::MaterialProgramId;
use bevy::prelude::*;
use std::collections::BTreeMap;

use crate::material_graph::clipboard::Fragment;
use crate::material_graph::execution::activate_material_graph_target;

/// Capture the same multi-selection for menu actions and keyboard shortcuts.
#[allow(clippy::too_many_arguments)]
pub(crate) fn capture_material(
    program: MaterialProgramId,
    scope: MaterialSelectionScope,
    target: &crate::material_document::MaterialEditingTarget,
    nodes: &Query<(&MaterialGraphAction, &FeathersGraphNode)>,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    memory: &GraphViewportMemory,
    selection: &MaterialGraphSelectionState,
) -> Result<Fragment, String> {
    let source = session
        .graph_material_programs_for(target, catalog)?
        .into_iter()
        .find(|candidate| candidate.id == program)
        .ok_or("Material is unavailable")?;
    let selected = &selection
        .get(scope)
        .filter(|entry| entry.program == Some(program))
        .ok_or("Select nodes to copy")?
        .expressions;
    let key = material_graph_view_key(program);
    let positions = nodes
        .iter()
        .filter(|(action, node)| action.program == program && node.graph_key() == key)
        .map(|(action, node)| (action.expression, node.position()))
        .collect::<BTreeMap<_, _>>();
    let positions = selected
        .iter()
        .map(|id| {
            (
                *id,
                positions
                    .get(id)
                    .copied()
                    .or_else(|| {
                        memory.node_position(&key, &material_graph_expression_node_key(*id))
                    })
                    .unwrap_or(Vec2::ZERO),
            )
        })
        .collect();
    Fragment::capture(
        &source.expressions,
        selected,
        positions,
        &source.disabled_expressions,
        &source.node_constants,
    )
    .map(|fragment| fragment.with_inline_defaults(&source))
}

/// Uses the normal material validation/history path; a rejected paste changes nothing.
#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_material(
    fragment: &Fragment,
    offset: Vec2,
    label: &str,
    program: MaterialProgramId,
    scope: MaterialSelectionScope,
    target: &crate::material_document::MaterialEditingTarget,
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    history: &mut MaterialProgramEditHistory,
    ledger: &mut EditorHistoryLedger,
    memory: &mut GraphViewportMemory,
    inspector: &mut MaterialStackInspectorState,
    selection: &mut MaterialGraphSelectionState,
) -> Result<usize, String> {
    activate_material_graph_target(session, catalog, target, program)?;
    let before = session
        .graph_material_programs(catalog)?
        .into_iter()
        .find(|candidate| candidate.id == program)
        .ok_or("Material is unavailable")?;
    let insert = fragment.instantiate(
        &before.expressions.iter().map(|node| node.id).collect(),
        offset,
    )?;
    let mut after = before.clone();
    after.expressions.extend(insert.expressions.iter().cloned());
    after.disabled_expressions.extend(&insert.disabled);
    after.node_constants.extend(&insert.constants);
    let key = material_graph_view_key(program);
    let layout = presentation::Snapshot::capture(&key, catalog, session, memory);
    history.execute_replacement(session, catalog, label, before, after)?;
    ledger.record_material_edit(session);
    for (id, position) in &insert.positions {
        memory.place_node(&key, material_graph_expression_node_key(*id), *position);
    }
    let created = insert.positions.keys().copied().collect();
    selection.select_material_expressions(scope, program, &created, GraphSelectionMode::Replace);
    inspector.selected = insert.expressions.last().map(|node| (program, node.id));
    if let Some(layout) = layout {
        layout.attach(catalog, session, memory);
    }
    Ok(fragment.len())
}
