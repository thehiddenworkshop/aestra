//! Production material selection edits, semantic preparation and target activation.
use crate::feathers::node_graph::{FeathersGraphNode, GraphViewportMemory};
use crate::history::{EditorHistoryLedger, MaterialProgramEditHistory};
use crate::material_graph::{
    MaterialGraphAction, MaterialGraphPreviewState, MaterialGraphSelectionState,
    MaterialSelectionScope, clipboard,
    keys::{material_graph_expression_node_key, material_graph_view_key},
    presentation,
};
use crate::properties::MaterialStackInspectorState;
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_core::{MaterialExpressionId, MaterialFunctionId, MaterialProgramId};
use bevy::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

use crate::material_graph::command_preparation;
use aestra_authoring::{MaterialToolCommand, MaterialToolPlan};

#[derive(Debug, Clone, Copy)]
pub(crate) enum MaterialGraphSelectionEdit {
    ExtractFunction,
    Duplicate,
    Delete,
    Disconnect,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_material_graph_selection_edit(
    edit: MaterialGraphSelectionEdit,
    scope: MaterialSelectionScope,
    program: MaterialProgramId,
    target: &crate::material_document::MaterialEditingTarget,
    graph_nodes: &Query<(&MaterialGraphAction, &FeathersGraphNode)>,
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    material_history: &mut MaterialProgramEditHistory,
    history_ledger: &mut EditorHistoryLedger,
    graph_memory: &mut GraphViewportMemory,
    inspector: &mut MaterialStackInspectorState,
    selection: &mut MaterialGraphSelectionState,
    previews: &mut MaterialGraphPreviewState,
    duplicate_offset: Option<Vec2>,
) {
    if selection.program(scope) != Some(program) {
        return;
    }
    if let Err(error) = activate_material_graph_target(session, catalog, target, program) {
        session.status = format!("Could not edit material nodes: {error}");
        return;
    }
    let selected = selection.get(scope);
    let selected_expressions: BTreeSet<MaterialExpressionId> = selected
        .map(|selection| selection.expressions.clone())
        .unwrap_or_default();
    let expressions = selected_expressions.iter().copied().collect::<Vec<_>>();
    let connection = selected.and_then(|selection| selection.connection);
    let positions = graph_nodes
        .iter()
        .filter(|(action, _)| {
            action.program == program && selected_expressions.contains(&action.expression)
        })
        .map(|(action, node)| (action.expression, node.position()))
        .collect::<BTreeMap<_, _>>();
    let extraction_center = (!positions.is_empty())
        .then(|| positions.values().copied().sum::<Vec2>() / positions.len() as f32);
    if positions.values().any(|position| !position.is_finite())
        || extraction_center.is_some_and(|position| !position.is_finite())
    {
        session.status = "Cannot edit nodes with invalid graph positions".into();
        return;
    }
    if matches!(edit, MaterialGraphSelectionEdit::Duplicate) {
        let result = (|| {
            let source = session
                .graph_material_programs(catalog)?
                .into_iter()
                .find(|candidate| candidate.id == program)
                .ok_or("Material is unavailable")?;
            let key = material_graph_view_key(program);
            let positions = selected_expressions
                .iter()
                .map(|id| {
                    (
                        *id,
                        positions
                            .get(id)
                            .copied()
                            .or_else(|| {
                                graph_memory
                                    .node_position(&key, &material_graph_expression_node_key(*id))
                            })
                            .unwrap_or(Vec2::ZERO),
                    )
                })
                .collect();
            let fragment = clipboard::Fragment::capture(
                &source.expressions,
                &selected_expressions,
                positions,
                &source.disabled_expressions,
                &source.node_constants,
            )?
            .with_inline_defaults(&source);
            clipboard::insert_material(
                &fragment,
                duplicate_offset.unwrap_or(Vec2::splat(24.0)),
                "Duplicate material graph nodes",
                program,
                scope,
                target,
                session,
                catalog,
                material_history,
                history_ledger,
                graph_memory,
                inspector,
                selection,
            )
        })();
        session.status = match result {
            Ok(count) => format!("Duplicated {count} material node(s)"),
            Err(error) => format!("Could not duplicate material nodes: {error}"),
        };
        return;
    }
    let (label, command) = match edit {
        MaterialGraphSelectionEdit::ExtractFunction => (
            "Extract material function",
            MaterialToolCommand::ExtractMaterialFunction {
                program,
                function: MaterialFunctionId::new(),
                name: catalog.next_material_function_name("Extracted Function"),
                expressions: expressions.clone(),
            },
        ),
        MaterialGraphSelectionEdit::Duplicate => {
            unreachable!("duplication uses the shared clipboard path")
        }
        MaterialGraphSelectionEdit::Delete => (
            "Delete material graph nodes",
            MaterialToolCommand::DeleteMaterialExpressions {
                program,
                expressions: expressions.clone(),
            },
        ),
        MaterialGraphSelectionEdit::Disconnect => {
            let Some(connection) = connection else {
                return;
            };
            (
                "Reset material graph connection",
                MaterialToolCommand::DisconnectMaterialConnection {
                    program,
                    target: connection.target,
                },
            )
        }
    };
    let layout_before = presentation::Snapshot::capture(
        &material_graph_view_key(program),
        catalog,
        session,
        graph_memory,
    );
    match apply_material_tool_command(session, catalog, material_history, program, label, command) {
        Ok(plan) => {
            history_ledger.record_material_edit(session);
            let graph_key = material_graph_view_key(program);
            let selected = selection.entry(scope);
            match edit {
                MaterialGraphSelectionEdit::ExtractFunction => {
                    for expression in &expressions {
                        graph_memory.remove_node(
                            &graph_key,
                            &material_graph_expression_node_key(*expression),
                        );
                    }
                    selected.expressions.clear();
                    let center = extraction_center.unwrap_or(Vec2::ZERO);
                    let call_count = plan.created_expressions.len();
                    for (index, call) in plan.created_expressions.iter().copied().enumerate() {
                        let vertical_offset =
                            (index as f32 - (call_count.saturating_sub(1) as f32 * 0.5)) * 72.0;
                        graph_memory.place_node(
                            graph_key.clone(),
                            material_graph_expression_node_key(call),
                            center + Vec2::new(0.0, vertical_offset),
                        );
                        selected.expressions.insert(call);
                    }
                    selected.connection = None;
                    inspector.selected = plan
                        .created_expressions
                        .last()
                        .copied()
                        .map(|expression| (program, expression));
                    let function = plan
                        .created_function()
                        .expect("a successful extraction plan creates a function");
                    session.status = format!(
                        "Extracted {} node(s) as {}",
                        expressions.len(),
                        function.name
                    );
                }
                MaterialGraphSelectionEdit::Duplicate => {
                    unreachable!("duplication uses the shared clipboard path");
                }
                MaterialGraphSelectionEdit::Delete => {
                    for expression in &expressions {
                        graph_memory.remove_node(
                            &graph_key,
                            &material_graph_expression_node_key(*expression),
                        );
                    }
                    selected.expressions.clear();
                    selected.connection = None;
                    inspector.selected = None;
                    session.status = format!("Deleted {} material node(s)", expressions.len());
                }
                MaterialGraphSelectionEdit::Disconnect => {
                    selected.connection = None;
                    inspector.selected = None;
                    session.status = "Reset material connection to its typed default".into();
                }
            }
            if let Some(before) = layout_before {
                before.attach_with_previews(catalog, session, graph_memory, previews, program);
            }
        }
        Err(error) => {
            session.status = match edit {
                MaterialGraphSelectionEdit::ExtractFunction => {
                    format!("Could not extract material function: {error}")
                }
                MaterialGraphSelectionEdit::Duplicate => {
                    format!("Could not duplicate material nodes: {error}")
                }
                MaterialGraphSelectionEdit::Delete => {
                    format!("Could not delete material nodes: {error}")
                }
                MaterialGraphSelectionEdit::Disconnect => {
                    format!("Could not reset material connection: {error}")
                }
            };
        }
    }
}

pub(crate) fn apply_material_tool_command(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    material_history: &mut MaterialProgramEditHistory,
    program: MaterialProgramId,
    label: &str,
    command: MaterialToolCommand,
) -> Result<MaterialToolPlan, String> {
    if let Some(active) = session.standalone_material() {
        if active != program {
            return Err("The material editing target changed".into());
        }
        session.material_history_active = true;
    }
    let document = session.graph_authoring_document(catalog)?;
    let command_preparation::PreparedEdit {
        plan,
        before: current,
        after: replacement,
    } = command_preparation::prepare(document, program, command)?;
    if let Some(function) = plan.created_function().cloned() {
        material_history.execute_extraction(
            session,
            catalog,
            label,
            current,
            replacement,
            function,
        )?;
    } else {
        material_history.execute_replacement(session, catalog, label, current, replacement)?;
    }
    Ok(plan)
}

pub(crate) fn activate_material_graph_target(
    session: &mut EditorSession,
    catalog: &ProjectEffectCatalog,
    target: &crate::material_document::MaterialEditingTarget,
    program: MaterialProgramId,
) -> Result<(), String> {
    match target {
        crate::material_document::MaterialEditingTarget::EffectInstance => {
            session.return_to_effect_material();
            Ok(())
        }
        crate::material_document::MaterialEditingTarget::Program { id, .. } if *id == program => {
            session.open_material_program(catalog, *id)
        }
        crate::material_document::MaterialEditingTarget::Program { .. } => {
            Err("The material graph target no longer matches this tab".into())
        }
        crate::material_document::MaterialEditingTarget::Function { .. } => {
            Err("A material node cannot be added to a function target".into())
        }
    }
}
