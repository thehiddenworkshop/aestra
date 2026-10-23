//! Shared semantic/presentation redo branches and filesystem chronology synchronization.
use crate::history::{
    MaterialProgramEditHistory, asset_order,
    document_execution::{EditorHistoryLedger, EffectHistoryChange},
};
use crate::{ProjectEffectCatalog, session::EditorSession};
use bevy::prelude::*;

/// Fork just the editing context's semantic redo branch for a presentation edit.
pub(crate) fn clear_presentation_redo(world: &mut World, context: &asset_order::Context) {
    match context {
        asset_order::Context::Effect => {
            world.resource_mut::<EditorSession>().clear_effect_redo();
            if let Some(mut ledger) = world.get_resource_mut::<EditorHistoryLedger>() {
                ledger.redo.clear();
            }
            if let Some(mut history) = world.get_resource_mut::<MaterialProgramEditHistory>() {
                history.redo.clear();
            }
        }
        asset_order::Context::Material(
            crate::material_document::MaterialEditingTarget::Program { root, id },
        ) => {
            if let Some(mut history) = world.get_resource_mut::<MaterialProgramEditHistory>()
                && let Some(history) = history.standalone.get_mut(&(root.clone(), *id))
            {
                history.redo.clear();
            }
        }
        asset_order::Context::Material(
            crate::material_document::MaterialEditingTarget::Function { root, id },
        ) => {
            if let Some(mut history) =
                world.get_resource_mut::<crate::material_function_editor::FunctionEditor>()
            {
                history.clear_function_redo(root, *id);
            }
        }
        _ => {}
    }
}

pub(crate) fn clear_document_order(
    world: &mut World,
    target: crate::material_document::MaterialEditingTarget,
) {
    let context = asset_order::Context::Material(target);
    world
        .resource_mut::<EditorSession>()
        .operation_order
        .clear_context(&context);
    if let Some(mut order) = world.get_resource_mut::<asset_order::AssetOrder>() {
        order.clear_context(&context);
    }
}

pub(super) fn sync_effect_history_ledger(
    mut session: ResMut<EditorSession>,
    mut ledger: ResMut<EditorHistoryLedger>,
    mut material_history: ResMut<MaterialProgramEditHistory>,
    mut order: Option<ResMut<asset_order::AssetOrder>>,
    catalog: Res<ProjectEffectCatalog>,
    mut functions: ResMut<crate::material_function_editor::FunctionEditor>,
) {
    match ledger.capture_effect_changes(&session) {
        EffectHistoryChange::Reset => material_history.clear(),
        EffectHistoryChange::Edited => material_history.clear_redo(),
        EffectHistoryChange::None => {}
    }
    if let Some(order) = order.as_deref_mut()
        && order.sync(&session, &catalog)
    {
        session.clear_effect_redo();
        session.operation_order.clear_redo();
        ledger.redo.clear();
        asset_order::clear_material_redo(&mut material_history);
        functions.clear_redo();
    }
}

#[cfg(test)]
#[path = "tests/synchronization.rs"]
mod tests;
