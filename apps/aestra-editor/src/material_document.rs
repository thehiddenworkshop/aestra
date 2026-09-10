//! Shared-source graph context, independent from the active effect and its selection.

use crate::{ProjectEffectCatalog, session::EditorSession};
use aestra_authoring::MaterialAuthoringDocument;
use aestra_core::{MaterialProgramId, material::MaterialProgram};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum MaterialEditingTarget {
    #[default]
    EffectInstance,
    Program {
        root: PathBuf,
        id: MaterialProgramId,
    },
    Function {
        root: PathBuf,
        id: aestra_core::MaterialFunctionId,
    },
}

impl MaterialEditingTarget {
    /// The shared material program this target edits, if it is a program target.
    pub(crate) fn program(&self) -> Option<MaterialProgramId> {
        match self {
            Self::Program { id, .. } => Some(*id),
            _ => None,
        }
    }

    /// The material function this target edits, if it is a function target.
    pub(crate) fn function(&self) -> Option<aestra_core::MaterialFunctionId> {
        match self {
            Self::Function { id, .. } => Some(*id),
            _ => None,
        }
    }
}

impl EditorSession {
    pub(crate) fn standalone_material(&self) -> Option<MaterialProgramId> {
        self.material_target.program()
    }

    pub(crate) fn standalone_function(&self) -> Option<aestra_core::MaterialFunctionId> {
        self.material_target.function()
    }

    pub(crate) fn open_material_function(
        &mut self,
        catalog: &ProjectEffectCatalog,
        id: aestra_core::MaterialFunctionId,
    ) -> Result<(), String> {
        catalog
            .content()
            .cached_material_function(aestra_core::material::MaterialFunctionRef::Project(id))
            .map_err(|error| error.to_string())?;
        let target = MaterialEditingTarget::Function {
            root: catalog.root().to_owned(),
            id,
        };
        if self.material_target != target {
            self.material_target = target;
            self.ui_revision += 1;
        }
        self.material_history_active = true;
        Ok(())
    }

    pub(crate) fn graph_function(
        &self,
        catalog: &ProjectEffectCatalog,
    ) -> Result<aestra_core::material::MaterialFunction, String> {
        self.graph_function_for(&self.material_target, catalog)
    }

    /// Resolves the function for an explicit target, independent of the session's current target.
    /// Per-view rendering (M4c) passes the view's target here; `graph_function` delegates with the
    /// session target.
    pub(crate) fn graph_function_for(
        &self,
        target: &MaterialEditingTarget,
        catalog: &ProjectEffectCatalog,
    ) -> Result<aestra_core::material::MaterialFunction, String> {
        let MaterialEditingTarget::Function { root, id } = target else {
            return Err("No function selected".into());
        };
        if root != catalog.root() {
            return Err("Function belongs to another project; reopen it from Assets".into());
        }
        // Resolve current source identity before consulting drafts.
        catalog
            .content()
            .cached_material_function(aestra_core::material::MaterialFunctionRef::Project(*id))
            .map_err(|error| error.to_string())?;
        catalog
            .material_functions()?
            .into_iter()
            .find(|function| function.id == *id)
            .ok_or_else(|| "Function unavailable".into())
    }

    pub(crate) fn open_material_program(
        &mut self,
        catalog: &ProjectEffectCatalog,
        id: MaterialProgramId,
    ) -> Result<(), String> {
        // Resolve identity against the published snapshot even if a draft exists. A duplicate
        // or removed source must not silently win by reusing an old cached draft.
        catalog.material_program(id)?;
        let target = MaterialEditingTarget::Program {
            root: catalog.root().to_owned(),
            id,
        };
        if self.material_target != target {
            self.material_target = target;
            self.ui_revision += 1;
        }
        self.material_history_active = true;
        Ok(())
    }

    pub(crate) fn return_to_effect_material(&mut self) {
        if self.material_target != MaterialEditingTarget::EffectInstance {
            self.material_target = MaterialEditingTarget::EffectInstance;
            self.material_history_active = false;
            self.ui_revision += 1;
        }
    }

    pub(crate) fn graph_material_programs(
        &self,
        catalog: &ProjectEffectCatalog,
    ) -> Result<Vec<MaterialProgram>, String> {
        self.graph_material_programs_for(&self.material_target, catalog)
    }

    pub(crate) fn graph_material_programs_for(
        &self,
        target: &MaterialEditingTarget,
        catalog: &ProjectEffectCatalog,
    ) -> Result<Vec<MaterialProgram>, String> {
        match target {
            MaterialEditingTarget::Function { .. } => Ok(Vec::new()),
            MaterialEditingTarget::EffectInstance => {
                catalog.material_programs_for_effect(&self.effect)
            }
            MaterialEditingTarget::Program { root, id } => {
                if root != catalog.root() {
                    return Err(
                        "The material belongs to another project; reopen it from Assets".into(),
                    );
                }
                Ok(vec![catalog.material_program(*id)?])
            }
        }
    }

    pub(crate) fn graph_authoring_document(
        &self,
        catalog: &ProjectEffectCatalog,
    ) -> Result<MaterialAuthoringDocument, String> {
        self.graph_authoring_document_for(&self.material_target, catalog)
    }

    /// Builds the authoring document for an explicit target, independent of the session's current
    /// target. Per-view rendering (M4c) uses this; `graph_authoring_document` delegates with the
    /// session target.
    pub(crate) fn graph_authoring_document_for(
        &self,
        target: &MaterialEditingTarget,
        catalog: &ProjectEffectCatalog,
    ) -> Result<MaterialAuthoringDocument, String> {
        if target.function().is_some() {
            self.graph_function_for(target, catalog)?;
        }
        let programs = self.graph_material_programs_for(target, catalog)?;
        let document = if target.program().is_some() || target.function().is_some() {
            MaterialAuthoringDocument::standalone(programs)
        } else {
            MaterialAuthoringDocument::new(self.effect.clone(), programs)
        };
        Ok(document.with_material_functions(catalog.material_functions()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn function_target_rejects_ambiguous_and_wrong_root_sources() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let function = aestra_core::material::MaterialFunction::from_ron(include_str!(
            "../../../assets/materials/pulse_wave.aestra.material-function.ron"
        ))
        .unwrap();
        function
            .save_ron(root.path().join("function.aestra.material-function.ron"))
            .unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = test_support::session_with_timing_slack();
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        let revision = session.ui_revision;
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        assert_eq!(session.ui_revision, revision);
        assert!(
            session
                .graph_authoring_document(&catalog)
                .unwrap()
                .effect
                .is_none()
        );
        assert!(
            session
                .graph_authoring_document(&catalog)
                .unwrap()
                .programs
                .is_empty()
        );
        let encoded = ron::to_string(&session.material_target).unwrap();
        assert_eq!(
            ron::from_str::<MaterialEditingTarget>(&encoded).unwrap(),
            session.material_target
        );
        assert!(
            session
                .graph_function(&ProjectEffectCatalog::scan(other.path()))
                .is_err()
        );
        function
            .save_ron(root.path().join("duplicate.aestra.material-function.ron"))
            .unwrap();
        catalog.refresh();
        let target = session.material_target.clone();
        assert!(
            session
                .open_material_function(&catalog, function.id)
                .is_err()
        );
        assert!(session.graph_function(&catalog).is_err());
        assert_eq!(session.material_target, target);
        session.return_to_effect_material();
        assert_eq!(
            session.material_target,
            MaterialEditingTarget::EffectInstance
        );
    }

    #[test]
    fn opening_unused_source_preserves_effect_and_uses_only_its_own_authoring_context() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Unused").normalized();
        program
            .save_ron(root.path().join("unused.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = test_support::session_with_timing_slack();
        session.adjust_effect_duration(0.25);
        let effect = session.effect.clone();
        let selection = session.selection;
        let path = session.source_path.clone();
        let revision = session.document_revision();
        let generation = session.history_generation();
        let guard = crate::project_content::io::IoGuard::capture(&catalog, &session);
        let history = session.effect_undo_len();
        let clock = session.clock;
        session.open_material_program(&catalog, program.id).unwrap();
        assert!(
            !guard.matches(&catalog, &session),
            "queued navigation must not overwrite a newer graph target"
        );
        assert!(
            guard.same_document(&catalog, &session),
            "non-destructive target changes must still allow save baseline merging"
        );
        let document = session.graph_authoring_document(&catalog).unwrap();
        assert!(document.effect.is_none());
        assert_eq!(document.programs.as_slice(), std::slice::from_ref(&program));
        document.validate().unwrap();
        assert_eq!(session.effect, effect);
        assert_eq!(session.selection, selection);
        assert_eq!(session.source_path, path);
        assert_eq!(session.clock, clock);
        assert_eq!(session.document_revision(), revision);
        assert_eq!(session.history_generation(), generation);
        assert_eq!(session.effect_undo_len(), history);
        assert!(session.effect_is_dirty());
        let ui = session.ui_revision;
        session.open_material_program(&catalog, program.id).unwrap();
        assert_eq!(
            session.ui_revision, ui,
            "same-target reopen must not rebuild the graph"
        );
        session.return_to_effect_material();
        assert!(session.standalone_material().is_none());
        assert_eq!(session.effect, effect);
        assert_eq!(session.selection, selection);
        assert_eq!(session.effect_undo_len(), history);
    }

    #[test]
    fn stale_or_duplicate_source_cannot_replace_the_current_target() {
        let root = tempfile::tempdir().unwrap();
        let other_root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Source");
        let path = root.path().join("source.aestra.material.ron");
        program.save_ron(&path).unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = test_support::session_with_timing_slack();
        session.open_material_program(&catalog, program.id).unwrap();
        let target = session.material_target.clone();
        program
            .save_ron(root.path().join("duplicate.aestra.material.ron"))
            .unwrap();
        catalog.refresh();
        assert!(session.open_material_program(&catalog, program.id).is_err());
        assert_eq!(session.material_target, target);
        assert!(session.graph_authoring_document(&catalog).is_err());
        program
            .save_ron(other_root.path().join("same-id.aestra.material.ron"))
            .unwrap();
        let other_catalog = ProjectEffectCatalog::scan(other_root.path());
        assert!(session.graph_authoring_document(&other_catalog).is_err());
        session
            .open_material_program(&other_catalog, program.id)
            .unwrap();
        assert_ne!(session.material_target, target);
        session.new_effect();
        assert_eq!(
            session.material_target,
            MaterialEditingTarget::EffectInstance
        );
    }

    #[test]
    fn target_switch_retains_drafts_and_unrelated_invalid_effect_does_not_block_source_editing() {
        let root = tempfile::tempdir().unwrap();
        let first = MaterialProgram::additive_sprite("First").normalized();
        let second = MaterialProgram::additive_sprite("Second").normalized();
        first
            .save_ron(root.path().join("first.aestra.material.ron"))
            .unwrap();
        second
            .save_ron(root.path().join("second.aestra.material.ron"))
            .unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = test_support::session_with_timing_slack();
        session.open_material_program(&catalog, first.id).unwrap();
        let mut edited = first.clone();
        edited.name = "Edited draft".into();
        catalog.replace_material_program(&first, &edited).unwrap();
        session.set_material_drafts(catalog.material_drafts.clone());
        session.open_material_program(&catalog, second.id).unwrap();
        assert_eq!(session.graph_material_programs(&catalog).unwrap(), [second]);
        session.open_material_program(&catalog, first.id).unwrap();
        session.effect.duration = -1.0;
        let document = session.graph_authoring_document(&catalog).unwrap();
        assert_eq!(document.programs, [edited]);
        assert!(document.validate().is_ok());
        assert!(!session.material_drafts.is_empty());
        assert_eq!(
            MaterialProgram::load_ron(root.path().join("first.aestra.material.ron")).unwrap(),
            first
        );
    }
}
