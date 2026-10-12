//! Session-backed material publication and root-scoped undo/redo.
use crate::history::{
    asset_order,
    material_program::{MaterialProgramEditHistory, MaterialProgramHistoryEntry},
    publication,
};
use crate::{ProjectEffectCatalog, session::EditorSession};
use aestra_core::material::{MaterialFunction, MaterialProgram};

impl MaterialProgramEditHistory {
    pub(super) fn for_target_mut(&mut self, session: &EditorSession) -> &mut Self {
        if !session.material_history_active {
            return self;
        }
        match &session.material_target {
            crate::material_document::MaterialEditingTarget::EffectInstance => self,
            crate::material_document::MaterialEditingTarget::Function { .. } => self,
            crate::material_document::MaterialEditingTarget::Program { root, id } => {
                self.standalone.entry((root.clone(), *id)).or_default()
            }
        }
    }

    #[cfg(test)]
    pub(super) fn for_target(&self, session: &EditorSession) -> Option<&Self> {
        if !session.material_history_active {
            return Some(self);
        }
        match &session.material_target {
            crate::material_document::MaterialEditingTarget::EffectInstance => Some(self),
            crate::material_document::MaterialEditingTarget::Function { .. } => None,
            crate::material_document::MaterialEditingTarget::Program { root, id } => {
                self.standalone.get(&(root.clone(), *id))
            }
        }
    }

    pub(crate) fn execute_replacement(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        label: impl Into<String>,
        before: MaterialProgram,
        after: MaterialProgram,
    ) -> Result<(), String> {
        let label = label.into();
        apply_material_program_replacement(session, catalog, &label, &before, &after)?;
        session.set_material_drafts(catalog.material_drafts.clone());
        let history = self.for_target_mut(session);
        history.record_applied(MaterialProgramHistoryEntry {
            label,
            before,
            after,
            created_function: None,
        });
        session
            .operation_order
            .record(asset_order::Context::current(session));
        Ok(())
    }

    pub(crate) fn execute_extraction(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        label: impl Into<String>,
        before: MaterialProgram,
        after: MaterialProgram,
        function: MaterialFunction,
    ) -> Result<(), String> {
        let label = label.into();
        apply_material_function_extraction(session, catalog, &label, &before, &after, &function)?;
        session.set_material_drafts(catalog.material_drafts.clone());
        let history = self.for_target_mut(session);
        history.record_applied(MaterialProgramHistoryEntry {
            label,
            before,
            after,
            created_function: Some(function),
        });
        session
            .operation_order
            .record(asset_order::Context::current(session));
        Ok(())
    }

    pub(super) fn undo(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
    ) -> Result<Option<String>, String> {
        self.step_applied(true, |entry| {
            if let Some(function) = &entry.created_function {
                undo_material_function_extraction(session, catalog, entry, function)?;
            } else {
                apply_material_program_replacement(
                    session,
                    catalog,
                    &format!("Undo {}", entry.label),
                    &entry.after,
                    &entry.before,
                )?;
            }
            session.set_material_drafts(catalog.material_drafts.clone());
            session
                .operation_order
                .step(asset_order::Context::current(session), true);
            Ok(())
        })
    }

    pub(super) fn redo(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
    ) -> Result<Option<String>, String> {
        self.step_applied(false, |entry| {
            if let Some(function) = &entry.created_function {
                apply_material_function_extraction(
                    session,
                    catalog,
                    &format!("Redo {}", entry.label),
                    &entry.before,
                    &entry.after,
                    function,
                )?;
            } else {
                apply_material_program_replacement(
                    session,
                    catalog,
                    &format!("Redo {}", entry.label),
                    &entry.before,
                    &entry.after,
                )?;
            }
            session.set_material_drafts(catalog.material_drafts.clone());
            session
                .operation_order
                .step(asset_order::Context::current(session), false);
            Ok(())
        })
    }
}

fn apply_material_function_extraction(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    label: &str,
    before: &MaterialProgram,
    after: &MaterialProgram,
    function: &MaterialFunction,
) -> Result<(), String> {
    publication::extract_function(catalog, function, |catalog| {
        apply_material_program_replacement(session, catalog, label, before, after)
    })
}

fn undo_material_function_extraction(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    entry: &MaterialProgramHistoryEntry,
    function: &MaterialFunction,
) -> Result<(), String> {
    publication::undo_extraction(
        catalog,
        &entry.label,
        &entry.before,
        &entry.after,
        function,
        |catalog, label, expected, replacement| {
            apply_material_program_replacement(session, catalog, label, expected, replacement)
        },
    )
}

fn apply_material_program_replacement(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    label: &str,
    expected: &MaterialProgram,
    replacement: &MaterialProgram,
) -> Result<(), String> {
    let standalone = session.standalone_material().is_some() && session.material_history_active;
    let document = if standalone {
        session.graph_authoring_document(catalog)?
    } else {
        aestra_authoring::MaterialAuthoringDocument::new(
            session.effect.clone(),
            catalog.material_programs_for_effect(&session.effect)?,
        )
        .with_material_functions(catalog.material_functions()?)
    };
    publication::replace_program(
        catalog,
        document,
        label,
        expected,
        replacement,
        |compiled| {
            session
                .install_compiled_project_root(compiled)
                .map_err(|error| error.to_string())
        },
    )
}
