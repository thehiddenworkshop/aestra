//! Shared-source graph context, independent from the active effect and its selection.

pub(crate) use crate::material_target::MaterialEditingTarget;
mod context;
#[cfg(test)]
use crate::ProjectEffectCatalog;
#[cfg(test)]
use aestra_core::{MaterialProgramId, material::MaterialProgram};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn missing_program_detection_distinguishes_absent_from_present() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Present").normalized();
        program
            .save_ron(root.path().join("present.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        // A present source is not reported missing; an unknown id is.
        assert!(!catalog.material_program_missing(program.id));
        assert!(catalog.material_program_missing(MaterialProgramId::from_u128(0xdead_beef)));
        assert!(catalog.material_function_missing(aestra_core::MaterialFunctionId::from_u128(0x1)));
    }

    #[test]
    fn function_target_rejects_ambiguous_and_wrong_root_sources() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let function = aestra_core::material::MaterialFunction::from_ron(include_str!(
            "../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
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
        let clock = session.driver.clock;
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
        assert_eq!(session.driver.clock, clock);
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
