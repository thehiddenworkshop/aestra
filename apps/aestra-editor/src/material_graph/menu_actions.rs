//! Material palette and context-menu semantic dispatch shared by both engine gates.
use crate::{
    EditorSession, ProjectEffectCatalog,
    feathers::{
        action_activation::{FeathersActionButton, PendingFeathersActivation},
        node_graph::{
            FeathersGraphNode, FeathersGraphViewport, GraphViewportMemory, NODE_HEADER_HEIGHT,
            NODE_WIDTH,
            geometry::{GraphDocumentKey, GraphNodeKey, GraphViewKey},
        },
    },
    history::{EditorHistoryLedger, MaterialProgramEditHistory},
    material_graph::{
        MaterialGraphAction, MaterialGraphPreviewState, MaterialGraphPreviewTarget,
        MaterialGraphSelectionState, MaterialGraphViewport, MaterialSelectionScope, arrange,
        clipboard, control_input,
        execution::{
            MaterialGraphSelectionEdit, activate_material_graph_target,
            apply_material_graph_selection_edit, apply_material_tool_command,
        },
        keys::material_graph_expression_node_key,
        node_metrics::{material_graph_node_row_count, node_height},
        palette::{MaterialGraphNodeMenu, MaterialGraphPaletteAction},
        palette_state::{MaterialGraphConnection, MaterialGraphPaletteState},
        presentation,
    },
    properties::MaterialStackInspectorState,
};
use aestra_authoring::{MaterialConnectionTarget, MaterialToolCommand};
use aestra_compiler::MaterialCompiler;
use aestra_core::MaterialProgramId;
use bevy::{prelude::*, ui_widgets::Activate};
use std::collections::BTreeSet;

#[derive(Component, Debug, Clone)]
pub(super) struct MaterialGraphPreviewToggle {
    pub(super) program: MaterialProgramId,
    pub(super) editing_target: crate::material_document::MaterialEditingTarget,
    pub(super) target: MaterialGraphPreviewTarget,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MaterialGraphToolbarAction {
    EffectContext,
    LocateSource(MaterialProgramId),
    AddNode(MaterialProgramId, MaterialSelectionScope),
    Arrange(
        MaterialProgramId,
        MaterialSelectionScope,
        arrange::ArrangeScope,
    ),
    ToggleAllPreviews(MaterialProgramId, MaterialSelectionScope),
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) enum MaterialGraphContextAction {
    Clipboard(MaterialProgramId, clipboard::Shortcut),
    Arrange(MaterialProgramId, arrange::ArrangeScope),
    ExtractFunction(MaterialProgramId),
    Duplicate(MaterialProgramId),
    Delete(MaterialProgramId),
    Disconnect(MaterialGraphConnection),
}

pub(super) fn queue_material_graph_menu_action_activation(
    activate: On<Activate>,
    actions: Query<
        (),
        (
            With<FeathersActionButton>,
            Or<(
                With<MaterialGraphPaletteAction>,
                With<MaterialGraphContextAction>,
                With<MaterialGraphPreviewToggle>,
                With<MaterialGraphToolbarAction>,
            )>,
        ),
    >,
    commands: Commands,
) {
    crate::feathers::action_activation::queue_activation(activate, actions, commands);
}

pub(super) fn material_graph_target(
    viewports: &Query<&MaterialGraphViewport>,
    program: MaterialProgramId,
    scope: MaterialSelectionScope,
) -> Option<crate::material_document::MaterialEditingTarget> {
    viewports
        .iter()
        .find(|viewport| viewport.program == program && viewport.scope == scope)
        .map(|viewport| viewport.editing_target.clone())
}

pub(super) fn arrange_seeds(
    selection: &MaterialGraphSelectionState,
    scope: MaterialSelectionScope,
    program: MaterialProgramId,
) -> BTreeSet<GraphNodeKey> {
    selection
        .get(scope)
        .filter(|selected| selected.program == Some(program))
        .map(|selected| {
            selected
                .expressions
                .iter()
                .copied()
                .map(GraphNodeKey::Expression)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn handle_material_graph_palette_actions(
    mut commands: Commands,
    actions: control_input::Controls<MaterialGraphPaletteAction>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
    mut graph_memory: ResMut<GraphViewportMemory>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut selection: ResMut<MaterialGraphSelectionState>,
    placement: crate::feathers::node_graph::placement::Context,
) {
    for (entity, pressed, action, pending) in actions.iter() {
        if !pressed || !pending {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>();
        control_input::clear_pressed(&mut commands, entity);
        let command = MaterialToolCommand::CreateMaterialGraphNode {
            program: action.program,
            kind: action.kind,
            source: action.source,
            target: action.target,
        };
        if !action.graph_position.is_finite() {
            session.status = "Cannot insert a node at an invalid graph position".into();
            continue;
        }
        if let Err(error) = activate_material_graph_target(
            &mut session,
            &catalog,
            &action.editing_target,
            action.program,
        ) {
            session.status = format!("Could not add material node: {error}");
            palette.open = None;
            palette.query.clear();
            continue;
        }
        let layout_before =
            presentation::Snapshot::capture(&action.graph_key, &catalog, &session, &graph_memory);
        let view_key = GraphViewKey {
            document: GraphDocumentKey {
                project: catalog.root().to_owned(),
                asset: crate::document::DocumentKey::MaterialProgram(action.program),
            },
            view: action.scope,
        };
        let mut area = placement.capture(&view_key, &graph_memory);
        let result = apply_material_tool_command(
            &mut session,
            &mut catalog,
            &mut material_history,
            action.program,
            "Add material graph node",
            command,
        );
        match result {
            Ok(plan) => {
                history_ledger.record_material_edit(&mut session);
                placement.preserve_existing(&view_key, &mut graph_memory);
                let position =
                    action.graph_position - Vec2::new(NODE_WIDTH * 0.5, NODE_HEADER_HEIGHT * 0.5);
                let projection = plan
                    .replacement_program(action.program)
                    .and_then(|program| {
                        catalog.material_function_library().ok().map(|functions| {
                            MaterialCompiler.project_graph_with_functions(program, None, &functions)
                        })
                    });
                use crate::feathers::node_graph::placement::Neighborhood;
                let neighborhood = match action.target {
                    Some(MaterialConnectionTarget::ExpressionInput { expression, .. }) => {
                        Neighborhood::Before(GraphNodeKey::Expression(expression))
                    }
                    Some(MaterialConnectionTarget::ProgramOutput(_)) => {
                        Neighborhood::Before(GraphNodeKey::MaterialOutputs)
                    }
                    None => action.source.map_or(Neighborhood::Cursor, |id| {
                        Neighborhood::After(GraphNodeKey::Expression(id))
                    }),
                };
                let mut unassisted = false;
                for expression in &plan.created_expressions {
                    let height = projection
                        .as_ref()
                        .and_then(|projection| {
                            projection
                                .nodes
                                .iter()
                                .find(|node| node.expression == *expression)
                        })
                        .map_or(160.0, |node| {
                            node_height(
                                material_graph_node_row_count(node),
                                node.disabled || !node.reachable,
                                false,
                            )
                        });
                    let placed = area.place(position, Vec2::new(NODE_WIDTH, height), neighborhood);
                    unassisted |= !placed.assisted;
                    graph_memory.place_node(
                        action.graph_key.clone(),
                        material_graph_expression_node_key(*expression),
                        placed.position,
                    );
                }
                if let Some(before) = layout_before {
                    before.attach(&catalog, &mut session, &mut graph_memory);
                }
                if let Some(expression) = plan.created_expressions.last().copied() {
                    inspector.selected = Some((action.program, expression));
                    selection.select_single(action.scope, action.program, expression);
                }
                session.status = format!("Added {} node", action.label);
                if unassisted {
                    session
                        .status
                        .push_str(&format!(" · {}", placement.notice()));
                }
            }
            Err(error) => session.status = format!("Could not add material node: {error}"),
        }
        palette.open = None;
        palette.query.clear();
        session.ui_revision += 1;
    }
}

pub(super) fn handle_material_graph_context_actions(
    mut commands: Commands,
    actions: control_input::Controls<MaterialGraphContextAction>,
    graph_nodes: Query<(&MaterialGraphAction, &FeathersGraphNode)>,
    viewports: Query<&MaterialGraphViewport>,
    graph_views: Query<(&MaterialGraphViewport, &FeathersGraphViewport)>,
    menu_surfaces: Query<&ChildOf, With<MaterialGraphNodeMenu>>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
    mut graph_memory: ResMut<GraphViewportMemory>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut previews: ResMut<MaterialGraphPreviewState>,
    mut clipboard: ResMut<clipboard::GraphClipboard>,
) {
    for (entity, pressed, action, pending) in actions.iter() {
        if !pressed || !pending {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>();
        control_input::clear_pressed(&mut commands, entity);
        let (program, edit) = match *action {
            MaterialGraphContextAction::Clipboard(program, action) => {
                let Some(open) = palette
                    .node_menu
                    .as_ref()
                    .filter(|open| open.program == program)
                else {
                    continue;
                };
                let scope = open.scope;
                let Some(target) = material_graph_target(&viewports, program, scope) else {
                    continue;
                };
                if action == clipboard::Shortcut::Paste {
                    if let Some(fragment) = &clipboard.fragment {
                        let position = graph_views
                            .iter()
                            .find(|(view, _)| view.program == program && view.scope == scope)
                            .map(|(_, graph)| graph.unproject_viewport_point(open.menu_position));
                        let offset = position
                            .map_or(Vec2::splat(24.0), |position| fragment.offset_to(position));
                        session.status = match clipboard::insert_material(
                            fragment,
                            offset,
                            "Paste graph nodes",
                            program,
                            scope,
                            &target,
                            &mut session,
                            &mut catalog,
                            &mut material_history,
                            &mut history_ledger,
                            &mut graph_memory,
                            &mut inspector,
                            &mut selection,
                        ) {
                            Ok(count) => format!("Pasted {count} node(s)"),
                            Err(error) => format!("Could not paste nodes: {error}"),
                        };
                    } else {
                        session.status = "No graph nodes to paste".into();
                    }
                } else {
                    match clipboard::capture_material(
                        program,
                        scope,
                        &target,
                        &graph_nodes,
                        &session,
                        &catalog,
                        &graph_memory,
                        &selection,
                    ) {
                        Ok(fragment) => {
                            session.status = format!("Copied {} node(s)", fragment.len());
                            clipboard.fragment = Some(fragment);
                            if action == clipboard::Shortcut::Cut {
                                apply_material_graph_selection_edit(
                                    MaterialGraphSelectionEdit::Delete,
                                    scope,
                                    program,
                                    &target,
                                    &graph_nodes,
                                    &mut session,
                                    &mut catalog,
                                    &mut material_history,
                                    &mut history_ledger,
                                    &mut graph_memory,
                                    &mut inspector,
                                    &mut selection,
                                    &mut previews,
                                    None,
                                );
                            }
                        }
                        Err(error) => session.status = format!("Could not copy nodes: {error}"),
                    }
                }
                palette.node_menu = None;
                session.ui_revision += 1;
                continue;
            }
            MaterialGraphContextAction::Arrange(program, arrange_scope) => {
                let Some(open) = palette
                    .node_menu
                    .as_ref()
                    .filter(|menu| menu.program == program)
                else {
                    continue;
                };
                let scope = open.scope;
                let Some(target) = material_graph_target(&viewports, program, scope) else {
                    session.status = "The material graph is no longer available".into();
                    continue;
                };
                commands.trigger(arrange::ArrangeGraph {
                    view: GraphViewKey {
                        document: GraphDocumentKey {
                            project: catalog.root().to_owned(),
                            asset: crate::document::DocumentKey::MaterialProgram(program),
                        },
                        view: scope,
                    },
                    editing_target: target,
                    scope: arrange_scope,
                    seeds: arrange_seeds(&selection, scope, program),
                });
                palette.node_menu = None;
                // Remove only the popup. Rebuilding the graph here invalidates its async layout.
                for parent in &menu_surfaces {
                    commands.entity(parent.parent()).despawn();
                }
                continue;
            }
            MaterialGraphContextAction::ExtractFunction(program) => {
                (program, MaterialGraphSelectionEdit::ExtractFunction)
            }
            MaterialGraphContextAction::Duplicate(program) => {
                (program, MaterialGraphSelectionEdit::Duplicate)
            }
            MaterialGraphContextAction::Delete(program) => {
                (program, MaterialGraphSelectionEdit::Delete)
            }
            MaterialGraphContextAction::Disconnect(connection) => {
                let scope = palette.connection_menu.as_ref().and_then(|menu| menu.scope);
                let selected = selection.entry(scope);
                selected.program = Some(connection.program);
                selected.expressions.clear();
                selected.connection = Some(connection);
                (connection.program, MaterialGraphSelectionEdit::Disconnect)
            }
        };
        let scope = palette
            .node_menu
            .as_ref()
            .map(|menu| menu.scope)
            .or_else(|| palette.connection_menu.as_ref().map(|menu| menu.scope))
            .unwrap_or(None);
        let Some(target) = material_graph_target(&viewports, program, scope) else {
            session.status = "The material graph is no longer available".into();
            continue;
        };
        apply_material_graph_selection_edit(
            edit,
            scope,
            program,
            &target,
            &graph_nodes,
            &mut session,
            &mut catalog,
            &mut material_history,
            &mut history_ledger,
            &mut graph_memory,
            &mut inspector,
            &mut selection,
            &mut previews,
            None,
        );
        palette.node_menu = None;
        palette.connection_menu = None;
        session.ui_revision += 1;
    }
}

#[cfg(test)]
#[path = "menu_actions/tests.rs"]
mod tests;
