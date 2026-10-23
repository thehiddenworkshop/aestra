//! Actual history controls, availability and classification audit.
use super::control_input::AuditedButton;
use super::ownership;
use crate::{
    ProjectEffectCatalog,
    feathers::action_activation::{EditorNativeControl, FeathersActionButton, dispatch_action},
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, asset_order},
    menus::{MenuState, RedoMenuItem, UndoMenuItem},
    session::EditorSession,
    theme,
};
use bevy::{prelude::*, ui::InteractionDisabled};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    None,
    Hovered,
    Pressed,
}

pub(super) struct Control<'a> {
    pub(super) entity: Entity,
    pub(super) phase: Phase,
    pub(super) action: HistoryAction,
    pub(super) feathers: bool,
    pub(super) pending: bool,
    pub(super) disabled: bool,
    pub(super) background: Mut<'a, BackgroundColor>,
}

pub(super) fn handle_history_buttons(
    mut commands: Commands,
    mut buttons: super::control_input::Controls,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for Control {
        entity,
        phase,
        action,
        feathers,
        pending,
        disabled,
        mut background,
    } in buttons.iter()
    {
        if disabled {
            if !feathers {
                background.0 = theme::PANEL_DARK;
            }
            continue;
        }
        match phase {
            Phase::Hovered if !feathers => background.0 = theme::BUTTON_HOVER,
            Phase::None if !feathers => background.0 = theme::BUTTON,
            Phase::Pressed => {
                if !dispatch_action(&mut commands, entity, action, feathers, pending, disabled) {
                    continue;
                }
                if !feathers {
                    background.0 = theme::ACCENT_DIM;
                }
                menu.open = None;
                menu.panels_open = false;
                if menu.tab_context.take().is_some() {
                    session.ui_revision += 1;
                }
            }
            _ => {}
        }
    }
}

type HistoryMenuFilter = Or<(With<UndoMenuItem>, With<RedoMenuItem>)>;

#[allow(clippy::too_many_arguments)]
pub(super) fn update_history_availability(
    session: Res<EditorSession>,
    catalog: Option<Res<ProjectEffectCatalog>>,
    ledger: Res<EditorHistoryLedger>,
    material_history: Res<MaterialProgramEditHistory>,
    functions: Res<crate::material_function_editor::FunctionEditor>,
    order: Option<Res<asset_order::AssetOrder>>,
    gate: ownership::Gate,
    active_editor: Option<Res<crate::feathers::code_editor::ActiveCodeEditor>>,
    active_document: Option<Res<crate::editor_view_registry::ActiveEditorContext>>,
    editor_views: Option<Res<crate::editor_view_registry::EditorViewManager>>,
    documents: Option<Res<crate::document_registry::DocumentManager>>,
    code_editors: Query<&crate::feathers::code_editor::CodeEditor>,
    mut commands: Commands,
    items: Query<(Entity, Has<UndoMenuItem>, Has<RedoMenuItem>), HistoryMenuFilter>,
) {
    let blocked = gate.blocked();
    // While a code editor is the active editing context, the Edit menu reflects its own history.
    let code_editor = active_editor
        .as_deref()
        .and_then(|active| active.0)
        .and_then(|entity| code_editors.get(entity).ok());
    let active_target = session
        .material_history_active
        .then(|| {
            active_document
                .as_deref()
                .zip(editor_views.as_deref())
                .zip(documents.as_deref())
                .zip(catalog.as_deref())
                .and_then(|(((active, views), documents), catalog)| {
                    crate::editor_target::active_material_target(
                        active,
                        views,
                        documents,
                        catalog.root(),
                    )
                })
        })
        .flatten();
    let context = active_target
        .map(asset_order::Context::Material)
        .unwrap_or_else(|| asset_order::Context::current(&session));
    for (entity, undo, redo) in &items {
        let ordered = order.as_ref().filter(|order| order.active());
        let enabled = if blocked {
            false
        } else if let Some(editor) = code_editor {
            (undo && editor.can_undo()) || (redo && editor.can_redo())
        } else if ordered.is_some_and(|order| order.top(undo).is_some()) {
            true
        } else if ordered.is_some() {
            false
        } else if session.operation_order.layout(&context, undo).is_some() {
            true
        } else if let asset_order::Context::Material(
            crate::material_document::MaterialEditingTarget::Function { root, id },
        ) = &context
        {
            functions.available_for_function(root, *id, undo)
        } else if let asset_order::Context::Material(
            crate::material_document::MaterialEditingTarget::Program { root, id },
        ) = &context
        {
            material_history.available_for_program(root, *id, undo)
        } else {
            (undo && (ledger.can_undo() || session.can_undo()))
                || (redo && (ledger.can_redo() || session.can_redo()))
        };
        if enabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}

type UnclassifiedHistoryControl = (
    Added<HistoryAction>,
    With<AuditedButton>,
    Without<FeathersActionButton>,
    Without<EditorNativeControl>,
);

pub(super) fn audit_history_controls(controls: Query<Entity, UnclassifiedHistoryControl>) {
    if cfg!(debug_assertions)
        && let Some(entity) = controls.iter().next()
    {
        panic!(
            "history control {entity:?} must use FeathersActionButton or be explicitly marked \
             EditorNativeControl"
        );
    }
}

#[cfg(test)]
#[path = "tests/controls.rs"]
mod tests;
