//! Application undo/redo routing with the real ownership, registries and compound history.
//! The native recovery worker stays behind a deferred callback, with its guards unchanged.
use super::{compound, ownership};
use crate::{
    ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, asset_order},
    session::EditorSession,
};
use aestra_project::content::operations::DeletedSource;
use bevy::{ecs::system::SystemParam, prelude::*};

/// Invoked through Commands after observer borrows end; the native adapter rechecks its guards.
pub(crate) type DeleteHistoryStep = fn(&mut World, DeletedSource, bool, u64);

#[derive(SystemParam)]
pub(crate) struct Routing<'w, 's> {
    session: ResMut<'w, EditorSession>,
    catalog: ResMut<'w, ProjectEffectCatalog>,
    material_history: ResMut<'w, MaterialProgramEditHistory>,
    ledger: ResMut<'w, EditorHistoryLedger>,
    functions: ResMut<'w, crate::material_function_editor::FunctionEditor>,
    order: Option<ResMut<'w, asset_order::AssetOrder>>,
    ownership: ownership::Ownership<'w, 's>,
    active_document: Option<Res<'w, crate::editor_view_registry::ActiveEditorContext>>,
    editor_views: Option<Res<'w, crate::editor_view_registry::EditorViewManager>>,
    documents: Option<Res<'w, crate::document_registry::DocumentManager>>,
    presentation: crate::material_graph::presentation::State<'w>,
}

impl Routing<'_, '_> {
    pub(crate) fn execute(
        self,
        action: HistoryAction,
        commands: &mut Commands,
        delete_history: DeleteHistoryStep,
    ) {
        let Self {
            mut session,
            mut catalog,
            mut material_history,
            mut ledger,
            mut functions,
            mut order,
            mut ownership,
            active_document,
            editor_views,
            documents,
            mut presentation,
        } = self;
        match ownership.route(action) {
            ownership::Route::Blocked => return,
            ownership::Route::Code(status) => {
                session.status = status.into();
                return;
            }
            ownership::Route::Document => {}
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
        let undo = action == HistoryAction::Undo;
        let active = order.as_deref().is_some_and(|order| order.active());
        let next = order.as_deref().and_then(|order| order.top(undo)).cloned();
        if let Some(asset_order::Entry::Delete(item)) = next {
            let revision = order.as_ref().unwrap().revision;
            commands.queue(move |world: &mut World| {
                delete_history(world, *item, undo, revision);
            });
            return;
        }
        if active && next.is_none() {
            session.status = if undo {
                "Nothing to undo"
            } else {
                "Nothing to redo"
            }
            .into();
            return;
        }
        let previous_target = session.material_target.clone();
        let previous_active = session.material_history_active;
        if let Some(asset_order::Entry::Document(context)) = &next {
            context.select(&mut session);
        } else if !active && session.material_history_active {
            // Normal material Undo/Redo follows the focused editor view. A pointer focus on the
            // viewport/timeline turns material_history_active off and intentionally keeps Effect Undo.
            if let Some(target) = active_document
                .as_deref()
                .zip(editor_views.as_deref())
                .zip(documents.as_deref())
                .and_then(|((active, views), documents)| {
                    crate::editor_target::active_material_target(
                        active,
                        views,
                        documents,
                        catalog.root(),
                    )
                })
            {
                asset_order::Context::Material(target).select(&mut session);
            }
        }
        let context = asset_order::Context::current(&session);
        let succeeded = compound::execute(
            action,
            &mut session,
            &mut catalog,
            &mut material_history,
            &mut ledger,
            &mut functions,
            &mut presentation,
        );
        if succeeded && active {
            order.as_deref_mut().unwrap().finish_document(undo, context);
        } else if !succeeded {
            session.material_target = previous_target;
            session.material_history_active = previous_active;
        }
    }
}

#[cfg(test)]
#[path = "tests/routing.rs"]
mod tests;
