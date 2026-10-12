//! Actual session-backed function publication and scoped history execution.
use crate::material_function_editor::{FunctionEditor, history::Key};
use crate::material_target::MaterialEditingTarget;
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_core::material::MaterialFunction;

pub(super) fn key(session: &EditorSession) -> Result<Key, String> {
    match &session.material_target {
        MaterialEditingTarget::Function { root, id } => Ok((root.clone(), *id)),
        _ => Err("No function selected".into()),
    }
}

impl FunctionEditor {
    pub(crate) fn edit_body(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        edits: Vec<aestra_authoring::MaterialFunctionBodyCommand>,
    ) -> Result<(), String> {
        let id = session
            .standalone_function()
            .ok_or("No function selected")?;
        let mut document = session.graph_authoring_document(catalog)?;
        aestra_authoring::MaterialCommandExecutor::execute(
            &mut document,
            &aestra_authoring::MaterialTransaction::new(
                "Edit function body",
                edits
                    .into_iter()
                    .map(
                        |edit| aestra_authoring::MaterialCommand::EditMaterialFunctionBody {
                            function: id,
                            edit,
                        },
                    )
                    .collect(),
            ),
        )
        .map_err(|error| error.to_string())?;
        let function = document
            .material_functions
            .into_iter()
            .find(|function| function.id == id)
            .ok_or("Function disappeared")?;
        self.edit(session, catalog, function)
    }
    fn apply(
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        before: &MaterialFunction,
        after: &MaterialFunction,
    ) -> Result<String, String> {
        let current = session.graph_function(catalog)?;
        let report =
            crate::history::publication::replace_function(catalog, &current, before, after)?;
        // Catalog change detection invalidates dependent viewport/graph previews. The effect
        // and its history are never replaced by a function signature edit.
        session.set_material_drafts(catalog.material_drafts.clone());
        session.material_history_active = true;
        Ok(report)
    }

    pub(crate) fn edit(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        after: MaterialFunction,
    ) -> Result<(), String> {
        let key = key(session)?;
        let before = session.graph_function(catalog)?;
        if let Some(report) = self.edit_applied(key, before, after, |before, after| {
            Self::apply(session, catalog, before, after)
        })? {
            session.status = format!("Function updated (unsaved draft).\n{report}");
            session
                .operation_order
                .record(crate::history::asset_order::Context::current(session));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn available(&self, session: &EditorSession, undo: bool) -> bool {
        key(session)
            .ok()
            .is_some_and(|(root, id)| self.available_for_function(&root, id, undo))
    }

    pub(crate) fn step(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        undo: bool,
    ) -> Result<(), String> {
        let key = key(session)?;
        self.step_applied(&key, undo, |before, after| {
            Self::apply(session, catalog, before, after)
        })?;
        session
            .operation_order
            .step(crate::history::asset_order::Context::current(session), undo);
        Ok(())
    }
}
