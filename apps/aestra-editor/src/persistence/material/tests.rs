use super::*;

#[test]
fn function_inspection_save_does_not_save_the_effect() {
    let directory = tempfile::tempdir().unwrap();
    let function = aestra_core::material::MaterialFunction::from_ron(include_str!(
        "../../../../../assets/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    function
        .save_ron(
            directory
                .path()
                .join("function.aestra.material-function.ron"),
        )
        .unwrap();
    let (mut app, _, _) = setup(directory.path());
    let catalog = app.world().resource::<ProjectEffectCatalog>().clone();
    let effect = {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        session.adjust_effect_duration(0.25);
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        session.effect.clone()
    };
    for action in [DocumentAction::Save, DocumentAction::SaveAs] {
        app.world_mut().trigger(action);
        app.world_mut().flush();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect, effect);
        assert!(session.effect_is_dirty());
        assert!(session.source_path.is_none());
        assert!(session.status.contains("Function inspection is read-only"));
    }
}

#[test]
fn material_save_does_not_write_a_dirty_named_effect_or_launch_save_as() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, _) = setup(directory.path());
    let path = directory.path().join("effect.aestra.ron");
    app.world_mut()
        .resource_mut::<EditorSession>()
        .save_as(&path)
        .unwrap();
    let bytes = fs::read(&path).unwrap();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.25);
    let effect = app.world().resource::<EditorSession>().effect.clone();
    edit(&mut app, &first, "Material change");
    app.world_mut().trigger(DocumentAction::Save);
    io::drain(app.world_mut());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(app.world().resource::<EditorSession>().effect, effect);
    assert!(app.world().resource::<EditorSession>().dirty);
    app.world_mut().trigger(DocumentAction::SaveAs);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("not available")
    );
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn save_then_reload_requires_confirmation_again_for_concurrent_edits() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, _) = setup(directory.path());
    let saved = edit(&mut app, &first, "Written");
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    respond(&mut app, DocumentProtectionAction::Save);
    let mut completion = io::prepared_completion(app.world_mut());
    let newer = edit(&mut app, &saved, "Keep newer edit");
    completion.apply(app.world_mut());
    app.world_mut().flush();
    assert!(app.world().resource::<DocumentProtectionState>().is_open());
    assert_eq!(current(&app, first.id), newer);
    assert_eq!(
        MaterialProgram::load_ron(directory.path().join("first.aestra.material.ron")).unwrap(),
        saved
    );
    respond(&mut app, DocumentProtectionAction::Cancel);
    assert_eq!(current(&app, first.id), newer);
}

#[test]
fn material_save_retains_undo_and_reload_clears_only_that_program_history() {
    use crate::history::{EditorHistoryPlugin, HistoryAction, MaterialProgramEditHistory};
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, _) = setup(directory.path());
    app.add_plugins(EditorHistoryPlugin);
    app.world_mut().trigger(HistoryAction::Undo); // Initialize the effect-generation baseline.
    let mut session = app.world_mut().remove_resource::<EditorSession>().unwrap();
    let mut catalog = app
        .world_mut()
        .remove_resource::<ProjectEffectCatalog>()
        .unwrap();
    let mut changed = first.clone();
    changed.name = "Undoable".into();
    app.world_mut()
        .resource_mut::<MaterialProgramEditHistory>()
        .execute_replacement(
            &mut session,
            &mut catalog,
            "Rename",
            first.clone(),
            changed.clone(),
        )
        .unwrap();
    app.insert_resource(session).insert_resource(catalog);
    app.world_mut().trigger(DocumentAction::Save);
    io::drain(app.world_mut());
    app.world_mut().trigger(HistoryAction::Undo);
    assert_eq!(current(&app, first.id), first);
    assert_eq!(
        draft_count(&app),
        1,
        "Undo after Save is an unsaved inverse, not a disk write"
    );
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    respond(&mut app, DocumentProtectionAction::Discard);
    io::drain(app.world_mut());
    assert_eq!(current(&app, first.id), changed);
    app.world_mut().trigger(HistoryAction::Redo);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("No material history")
    );
}

#[test]
fn save_includes_transitive_function_drafts_but_not_unrelated_functions() {
    use aestra_core::{MaterialExpressionId, MaterialFunctionOutputId};
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, _) = setup(directory.path());
    let make_function = |value: u128, dependency: Option<&MaterialFunction>| MaterialFunction {
        id: MaterialFunctionId::from_u128(value),
        schema_version: MaterialSchemaVersion::CURRENT,
        name: format!("Function {value}"),
        inputs: vec![],
        outputs: vec![MaterialFunctionOutput {
            id: MaterialFunctionOutputId::from_u128(value + 1),
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            expression: MaterialExpressionId::from_u128(value + 2),
        }],
        expressions: vec![MaterialExpression {
            id: MaterialExpressionId::from_u128(value + 2),
            kind: dependency.map_or(
                MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
                |function| MaterialExpressionKind::FunctionCall {
                    function: MaterialFunctionRef::Project(function.id),
                    arguments: default(),
                    output: function.outputs[0].id,
                },
            ),
        }],
        custom_wesl: None,
    };
    let inner = make_function(100, None);
    let outer = make_function(200, Some(&inner));
    let unrelated = make_function(300, None);
    {
        let mut catalog = app.world_mut().resource_mut::<ProjectEffectCatalog>();
        for function in [&inner, &outer, &unrelated] {
            catalog.create_material_function(function).unwrap();
        }
        let mut changed = first.clone();
        let expression = MaterialExpressionId::from_u128(400);
        changed.expressions.push(MaterialExpression {
            id: expression,
            kind: MaterialExpressionKind::FunctionCall {
                function: MaterialFunctionRef::Project(outer.id),
                arguments: default(),
                output: outer.outputs[0].id,
            },
        });
        changed.outputs.alpha = expression;
        catalog.replace_material_program(&first, &changed).unwrap();
    }
    let drafts = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .material_drafts
        .clone();
    let inner_path = drafts.functions[&inner.id].path.clone();
    let outer_path = drafts.functions[&outer.id].path.clone();
    let unrelated_path = drafts.functions[&unrelated.id].path.clone();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .set_material_drafts(drafts);
    app.world_mut().trigger(DocumentAction::Save);
    io::drain(app.world_mut());
    assert_eq!(MaterialFunction::load_ron(inner_path).unwrap(), inner);
    assert_eq!(MaterialFunction::load_ron(outer_path).unwrap(), outer);
    assert!(!unrelated_path.exists());
    assert_eq!(draft_count(&app), 1);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .material_drafts
            .functions
            .contains_key(&unrelated.id)
    );
}

fn setup(root: &Path) -> (App, MaterialProgram, MaterialProgram) {
    let first = MaterialProgram::additive_sprite("First").normalized();
    let second = MaterialProgram::additive_sprite("Second").normalized();
    first
        .save_ron(root.join("first.aestra.material.ron"))
        .unwrap();
    second
        .save_ron(root.join("second.aestra.material.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root);
    let mut session = crate::test_support::session_with_timing_slack();
    session.source_path = None;
    session.open_material_program(&catalog, first.id).unwrap();
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(catalog)
        .insert_resource(Localizer::new("en-US").unwrap());
    install_document_open_test_runtime(&mut app, root.join("recovery"));
    app.add_observer(resolve_document_protection)
        .init_resource::<crate::history::MaterialProgramEditHistory>();
    (app, first, second)
}
fn edit(app: &mut App, original: &MaterialProgram, name: &str) -> MaterialProgram {
    let mut changed = original.clone();
    changed.name = name.into();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(original, &changed)
        .unwrap();
    let drafts = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .material_drafts
        .clone();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .set_material_drafts(drafts);
    changed
}
fn respond(app: &mut App, action: DocumentProtectionAction) {
    let entity = app.world_mut().spawn(action).id();
    app.world_mut().trigger(Activate { entity });
    app.world_mut().flush();
}
fn current(app: &App, id: MaterialProgramId) -> MaterialProgram {
    app.world()
        .resource::<ProjectEffectCatalog>()
        .material_program(id)
        .unwrap()
}
fn draft_count(app: &App) -> usize {
    app.world()
        .resource::<EditorSession>()
        .material_drafts
        .count()
}

#[test]
fn save_keeps_untitled_effect_and_unrelated_drafts_unsaved() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, second) = setup(directory.path());
    let effect = app.world().resource::<EditorSession>().effect.clone();
    let selection = app.world().resource::<EditorSession>().selection;
    let generation = app.world().resource::<EditorSession>().history_generation();
    let changed = edit(&mut app, &first, "Saved material");
    edit(&mut app, &second, "Unrelated draft");
    app.world_mut().trigger(DocumentAction::Save);
    io::drain(app.world_mut());
    assert_eq!(
        MaterialProgram::load_ron(directory.path().join("first.aestra.material.ron")).unwrap(),
        changed
    );
    assert_eq!(
        MaterialProgram::load_ron(directory.path().join("second.aestra.material.ron")).unwrap(),
        second
    );
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect, effect);
    assert_eq!(session.selection, selection);
    assert!(session.source_path.is_none());
    assert_eq!(session.history_generation(), generation);
    assert_eq!(draft_count(&app), 1);
    assert!(session.material_drafts.programs.contains_key(&second.id));
    assert!(session.status.contains("effect was not saved"));
}

#[test]
fn external_conflict_and_duplicate_identity_saves_preserve_disk_and_drafts() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, _) = setup(directory.path());
    edit(&mut app, &first, "Draft");
    let path = directory.path().join("first.aestra.material.ron");
    let external = format!("// External edit\n{}", first.to_pretty_ron().unwrap());
    fs::write(&path, &external).unwrap();
    app.world_mut().trigger(DocumentAction::Save);
    io::drain(app.world_mut());
    assert_eq!(fs::read_to_string(&path).unwrap(), external);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("changed outside")
    );
    assert_eq!(draft_count(&app), 1);
    first.save_ron(&path).unwrap();
    first
        .save_ron(directory.path().join("duplicate.aestra.material.ron"))
        .unwrap();
    app.world_mut().trigger(DocumentAction::Save);
    io::drain(app.world_mut());
    assert_eq!(MaterialProgram::load_ron(&path).unwrap(), first);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("failed")
    );
    assert_eq!(draft_count(&app), 1);
}

#[test]
fn save_receipt_keeps_newer_edits_when_target_changes_during_io() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, second) = setup(directory.path());
    let saved = edit(&mut app, &first, "Written");
    app.world_mut().trigger(DocumentAction::Save);
    let mut completion = io::prepared_completion(app.world_mut());
    let newer = edit(&mut app, &saved, "Newer unsaved");
    let catalog = app.world().resource::<ProjectEffectCatalog>().clone();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .open_material_program(&catalog, second.id)
        .unwrap();
    completion.apply(app.world_mut());
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.standalone_material(), Some(second.id));
    assert_eq!(current(&app, first.id), newer);
    session.material_drafts.preflight().unwrap();
    assert_eq!(
        MaterialProgram::load_ron(directory.path().join("first.aestra.material.ron")).unwrap(),
        saved
    );
}

#[test]
fn reload_cancel_discard_and_save_are_scoped_to_selected_material() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, second) = setup(directory.path());
    let draft = edit(&mut app, &first, "Draft");
    edit(&mut app, &second, "Keep");
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    assert!(app.world().resource::<DocumentProtectionState>().is_open());
    respond(&mut app, DocumentProtectionAction::Cancel);
    assert_eq!(current(&app, first.id), draft);
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    respond(&mut app, DocumentProtectionAction::Discard);
    io::drain(app.world_mut());
    assert_eq!(current(&app, first.id), first);
    assert_eq!(draft_count(&app), 1);
    let saved = edit(&mut app, &first, "Save before reload");
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    respond(&mut app, DocumentProtectionAction::Save);
    io::drain(app.world_mut());
    assert!(!app.world().resource::<DocumentProtectionState>().is_open());
    assert_eq!(current(&app, first.id), saved);
    assert_eq!(draft_count(&app), 1);
    assert_eq!(
        app.world().resource::<EditorSession>().status,
        app.world()
            .resource::<Localizer>()
            .text("material-reload-complete")
    );
    assert!(
        app.world()
            .resource::<EditorSession>()
            .source_path
            .is_none()
    );
}

#[test]
fn reload_resolves_moved_source_and_retains_draft_on_missing_or_malformed_source() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, _) = setup(directory.path());
    let draft = edit(&mut app, &first, "Keep until successful");
    let path = directory.path().join("first.aestra.material.ron");
    fs::write(&path, "malformed").unwrap();
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    respond(&mut app, DocumentProtectionAction::Discard);
    io::drain(app.world_mut());
    assert_eq!(current(&app, first.id), draft);
    fs::remove_file(&path).unwrap();
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    respond(&mut app, DocumentProtectionAction::Discard);
    io::drain(app.world_mut());
    assert_eq!(draft_count(&app), 1);
    first
        .save_ron(directory.path().join("moved.aestra.material.ron"))
        .unwrap();
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    respond(&mut app, DocumentProtectionAction::Discard);
    io::drain(app.world_mut());
    assert_eq!(draft_count(&app), 0);
    assert_eq!(current(&app, first.id), first);
}

#[test]
fn reload_survives_preview_recompilation_without_authored_changes() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, _, _) = setup(directory.path());
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    let mut completion = io::prepared_completion(app.world_mut());
    let effect = app.world().resource::<EditorSession>().effect.clone();
    let compiled = aestra_compiler::EffectCompiler::default()
        .compile(&effect)
        .unwrap();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .install_compiled_project_root(std::sync::Arc::new(compiled))
        .unwrap();
    completion.apply(app.world_mut());
    assert_eq!(
        app.world().resource::<EditorSession>().status,
        app.world()
            .resource::<Localizer>()
            .text("material-reload-complete")
    );
}

#[test]
fn reload_completion_cannot_discard_edits_made_after_preparation() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, _) = setup(directory.path());
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    let mut completion = io::prepared_completion(app.world_mut());
    let changed = edit(&mut app, &first, "Newer");
    completion.apply(app.world_mut());
    assert_eq!(current(&app, first.id), changed);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("changed")
    );
}

#[test]
fn reload_confirmation_does_not_authorize_a_different_material() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, second) = setup(directory.path());
    edit(&mut app, &first, "First draft");
    edit(&mut app, &second, "Second draft");
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    let catalog = app.world().resource::<ProjectEffectCatalog>().clone();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .open_material_program(&catalog, second.id)
        .unwrap();
    respond(&mut app, DocumentProtectionAction::Discard);
    assert_eq!(draft_count(&app), 2);
    assert!(!app.world().resource::<DocumentProtectionState>().is_open());
}
