//! Actual atomic semantic/presentation undo-redo execution, shared by the action observer.
use crate::history::{
    HistoryAction, MaterialProgramEditHistory, asset_order,
    document_execution::{EditorHistoryLedger, execute_document_history},
};
use crate::material_function_editor::FunctionEditor;
use crate::material_graph::presentation::State;
use crate::{ProjectEffectCatalog, session::EditorSession};

#[allow(
    clippy::too_many_arguments,
    reason = "The coupled history boundary shares the observer's actual state"
)]
pub(crate) fn execute(
    action: HistoryAction,
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    material_history: &mut MaterialProgramEditHistory,
    ledger: &mut EditorHistoryLedger,
    functions: &mut FunctionEditor,
    presentation: &mut State,
) -> bool {
    let undo = action == HistoryAction::Undo;
    let context = asset_order::Context::current(session);
    let layout = session
        .operation_order
        .layout(&context, undo)
        .map(|(entry, semantic)| (entry.clone(), semantic));
    if let Some((layout, _)) = &layout
        && let Err(error) = presentation.validate(layout, undo, catalog, session)
    {
        session.status = error;
        return false;
    }
    let succeeded = if layout.as_ref().is_some_and(|(_, semantic)| !semantic) {
        session.operation_order.step(context.clone(), undo);
        true
    } else {
        execute_document_history(
            action,
            session,
            catalog,
            material_history,
            ledger,
            functions,
        )
    };
    if succeeded && let Some((layout, _)) = &layout {
        presentation.apply(layout, undo);
        session.ui_revision += 1;
        session.status = if undo {
            "Undid graph edit"
        } else {
            "Redid graph edit"
        }
        .into();
    }
    succeeded
}

#[cfg(test)]
#[path = "tests/compound.rs"]
mod tests;
