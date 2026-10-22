use super::*;

#[test]
fn effect_identity_is_guarded_but_saved_program_identity_survives_effect_replacement() {
    let mut session = crate::test_support::session_with_timing_slack();
    let id = MaterialProgramId::new();
    let effect = GraphDropTarget::program(&session, id);
    assert!(effect.check(&session).is_ok());
    let target = MaterialEditingTarget::Program {
        root: "project".into(),
        id,
    };
    let saved = GraphDropTarget::program_for(&session, id, &target);
    session.effect.id = aestra_core::EffectId::new();
    assert!(
        effect
            .check(&session)
            .unwrap_err()
            .contains("Graph changed")
    );
    assert!(saved.check(&session).is_ok());
    assert_eq!(saved.editing_target(), &target);
    assert!(matches!(saved.owner, Owner::Program(actual) if actual == id));
}

#[test]
fn function_targets_retain_their_identity_and_project_namespace() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut session = crate::test_support::session_with_timing_slack();
    let id = MaterialFunctionId::new();
    session.material_target = MaterialEditingTarget::Function {
        root: first.path().to_owned(),
        id,
    };
    let target = GraphDropTarget::function(&session, id);
    assert!(target.check(&session).is_ok());
    assert!(matches!(target.owner, Owner::Function(actual) if actual == id));
    let a = ProjectEffectCatalog::scan(first.path());
    let b = ProjectEffectCatalog::scan(second.path());
    assert_ne!(target.key(&a), target.key(&b));
    assert_eq!(target.editing_target(), &session.material_target);
    let program =
        GraphDropTarget::program_for(&session, MaterialProgramId::new(), &session.material_target);
    assert!(
        program
            .check(&session)
            .unwrap_err()
            .contains("target changed")
    );
}

#[test]
fn material_keys_preserve_the_document_identity_independent_of_project_camera_namespace() {
    let root = tempfile::tempdir().unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let session = crate::test_support::session_with_timing_slack();
    let id = MaterialProgramId::new();
    assert_eq!(
        GraphDropTarget::program(&session, id).key(&catalog),
        material_graph_view_key(id)
    );
}
