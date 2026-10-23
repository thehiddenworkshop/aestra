//! Complete production material graph keyboard consumer.
use crate::feathers::node_graph::{FeathersGraphNode, FeathersGraphViewport, GraphViewportMemory};
use crate::history::{EditorHistoryLedger, MaterialProgramEditHistory};
use crate::material_graph::{
    MaterialGraphAction, MaterialGraphPaletteState, MaterialGraphPreviewState,
    MaterialGraphSelectionState, MaterialGraphViewport, clipboard,
    execution::{MaterialGraphSelectionEdit, apply_material_graph_selection_edit},
};
use crate::properties::MaterialStackInspectorState;
use crate::{EditorSession, ProjectEffectCatalog};
use bevy::{input_focus::InputFocus, prelude::*, text::EditableText, ui::RelativeCursorPosition};

pub(crate) fn register(app: &mut App) {
    app.add_systems(Update, material_graph_keyboard_input);
}

#[cfg(test)]
#[path = "tests/keyboard.rs"]
mod tests;

#[allow(clippy::too_many_arguments)]
pub(crate) fn material_graph_keyboard_input(
    input: crate::input::ShortcutKeys,
    shortcuts: crate::input::ShortcutContext,
    viewports: Query<(
        &MaterialGraphViewport,
        &RelativeCursorPosition,
        &FeathersGraphViewport,
        &ComputedNode,
    )>,
    graph_nodes: Query<(&MaterialGraphAction, &FeathersGraphNode)>,
    focus: Option<Res<InputFocus>>,
    editable_text: Query<(), With<EditableText>>,
    palette: Res<MaterialGraphPaletteState>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut history_ledger: ResMut<EditorHistoryLedger>,
    mut graph_memory: ResMut<GraphViewportMemory>,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut previews: ResMut<MaterialGraphPreviewState>,
    mut clipboard: ResMut<clipboard::GraphClipboard>,
) {
    let editing_text = focus
        .as_ref()
        .and_then(|focus| focus.get())
        .is_some_and(|entity| editable_text.contains(entity));
    if editing_text || shortcuts.blocked() || palette.is_open() {
        return;
    }
    // Keyboard edits act on the graph under the cursor, in that viewport's own selection scope.
    let Some((program, scope, target, paste_position)) =
        viewports
            .iter()
            .find_map(|(viewport, cursor, graph, computed)| {
                cursor.cursor_over().then_some((
                    viewport.program,
                    viewport.scope,
                    viewport.editing_target.clone(),
                    cursor.normalized.map(|point| {
                        graph.unproject_viewport_point((point + Vec2::splat(0.5)) * computed.size())
                    }),
                ))
            })
    else {
        return;
    };
    for keys in input.iter() {
        let has_expressions = selection.get(scope).is_some_and(|selection| {
            selection.program == Some(program) && !selection.expressions.is_empty()
        });
        let has_connection = selection.program(scope) == Some(program)
            && selection.selected_connection(scope).is_some();
        if let Some(action) = clipboard::shortcut(keys) {
            match action {
                clipboard::Shortcut::Copy | clipboard::Shortcut::Cut if has_expressions => {
                    let result = clipboard::capture_material(
                        program,
                        scope,
                        &target,
                        &graph_nodes,
                        &session,
                        &catalog,
                        &graph_memory,
                        &selection,
                    );
                    match result {
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
                                session.ui_revision += 1;
                            }
                        }
                        Err(error) => session.status = format!("Could not copy nodes: {error}"),
                    }
                    continue;
                }
                clipboard::Shortcut::Paste => {
                    if let Some(fragment) = &clipboard.fragment {
                        let offset = paste_position
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
                        session.ui_revision += 1;
                    }
                    continue;
                }
                _ => {}
            }
        }
        let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        let edit = if control && shift && keys.just_pressed(KeyCode::KeyE) && has_expressions {
            Some(MaterialGraphSelectionEdit::ExtractFunction)
        } else if control && keys.just_pressed(KeyCode::KeyD) && has_expressions {
            Some(MaterialGraphSelectionEdit::Duplicate)
        } else if keys.just_pressed(KeyCode::Delete) {
            if has_expressions {
                Some(MaterialGraphSelectionEdit::Delete)
            } else if has_connection {
                Some(MaterialGraphSelectionEdit::Disconnect)
            } else {
                None
            }
        } else {
            None
        };
        let Some(edit) = edit else {
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
        session.ui_revision += 1;
    }
}
