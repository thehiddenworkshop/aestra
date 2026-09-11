use super::*;
use aestra_core::MaterialProgramId;

fn function_fixture(root: &Path) -> (App, MaterialFunction, PathBuf) {
    let function = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap();
    let path = root.join("function.aestra.material-function.ron");
    function.save_ron(&path).unwrap();
    let (mut app, program, _) = setup(root);
    edit(&mut app, &program, "Unrelated draft");
    let catalog = app.world().resource::<ProjectEffectCatalog>().clone();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .open_material_function(&catalog, function.id)
        .unwrap();
    app.init_resource::<crate::material_function_editor::FunctionEditor>();
    (app, function, path)
}

fn edit_function(app: &mut App, function: &MaterialFunction, name: &str) {
    let mut changed = function.clone();
    changed.name = name.into();
    app.world_mut().resource_scope(
        |world, mut editor: Mut<crate::material_function_editor::FunctionEditor>| {
            world.resource_scope(|world, mut session: Mut<EditorSession>| {
                editor
                    .edit(
                        &mut session,
                        &mut world.resource_mut::<ProjectEffectCatalog>(),
                        changed,
                    )
                    .unwrap();
            });
        },
    );
}

#[test]
fn function_reload_cancel_discard_and_save_are_scoped() {
    for action in [
        DocumentProtectionAction::Cancel,
        DocumentProtectionAction::Discard,
        DocumentProtectionAction::Save,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, function, path) = function_fixture(directory.path());
        let effect = app.world().resource::<EditorSession>().effect.clone();
        edit_function(&mut app, &function, "Draft");
        app.world_mut().trigger(DocumentAction::ReloadMaterial);
        assert!(app.world().resource::<DocumentProtectionState>().is_open());
        respond(&mut app, action);
        io::drain(app.world_mut());
        let session = app.world().resource::<EditorSession>();
        let catalog = app.world().resource::<ProjectEffectCatalog>();
        assert_eq!(session.effect, effect);
        assert_eq!(catalog.material_drafts.programs.len(), 1);
        let cancelled = action == DocumentProtectionAction::Cancel;
        assert_eq!(
            catalog.material_drafts.functions.contains_key(&function.id),
            cancelled
        );
        assert_eq!(
            app.world()
                .resource::<crate::material_function_editor::FunctionEditor>()
                .available(session, true),
            cancelled
        );
        let disk = MaterialFunction::from_ron(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            disk.name,
            if action == DocumentProtectionAction::Save {
                "Draft"
            } else {
                &function.name
            }
        );
    }
}

#[test]
fn function_reload_keeps_missing_draft_then_resolves_moved_source() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, function, path) = function_fixture(directory.path());
    edit_function(&mut app, &function, "Keep me");
    fs::remove_file(&path).unwrap();
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    respond(&mut app, DocumentProtectionAction::Discard);
    io::drain(app.world_mut());
    assert!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_drafts
            .functions
            .contains_key(&function.id)
    );
    assert!(
        app.world()
            .resource::<crate::material_function_editor::FunctionEditor>()
            .available(app.world().resource::<EditorSession>(), true)
    );
    function
        .save_ron(directory.path().join("moved.aestra.material-function.ron"))
        .unwrap();
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    respond(&mut app, DocumentProtectionAction::Discard);
    io::drain(app.world_mut());
    assert!(
        !app.world()
            .resource::<ProjectEffectCatalog>()
            .material_drafts
            .functions
            .contains_key(&function.id)
    );
}

#[test]
fn function_reload_rejects_concurrent_edits() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, function, _) = function_fixture(directory.path());
    app.world_mut().trigger(DocumentAction::ReloadMaterial);
    let mut completion = io::prepared_completion(app.world_mut());
    edit_function(&mut app, &function, "Newer draft");
    completion.apply(app.world_mut());
    assert!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_drafts
            .functions
            .contains_key(&function.id)
    );
    assert!(
        app.world()
            .resource::<crate::material_function_editor::FunctionEditor>()
            .available(app.world().resource::<EditorSession>(), true)
    );
}

#[test]
fn function_inspection_save_does_not_save_the_effect() {
    let directory = tempfile::tempdir().unwrap();
    let function = aestra_core::material::MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
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
        io::drain(app.world_mut());
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect, effect);
        assert!(session.effect_is_dirty());
        assert!(session.source_path.is_none());
        if action == DocumentAction::SaveAs {
            assert!(session.status.contains("not available"));
        }
    }
}

#[test]
fn function_save_is_scoped_and_rejects_external_changes() {
    let directory = tempfile::tempdir().unwrap();
    let function = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap();
    let path = directory
        .path()
        .join("function.aestra.material-function.ron");
    function.save_ron(&path).unwrap();
    let (mut app, program, _) = setup(directory.path());
    edit(&mut app, &program, "Unrelated dirty program");
    let catalog = app.world().resource::<ProjectEffectCatalog>().clone();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .open_material_function(&catalog, function.id)
        .unwrap();
    let mut changed = function.clone();
    changed.name = "Saved function".into();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_function(&function, &changed)
        .unwrap();
    app.world_mut().trigger(DocumentAction::Save);
    io::drain(app.world_mut());
    assert_eq!(
        MaterialFunction::from_ron(&fs::read_to_string(&path).unwrap()).unwrap(),
        changed
    );
    let catalog = app.world().resource::<ProjectEffectCatalog>();
    assert!(!catalog.material_drafts.functions.contains_key(&function.id));
    assert!(catalog.material_drafts.programs.contains_key(&program.id));
    let mut newer = changed.clone();
    newer.name = "Keep draft".into();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_function(&changed, &newer)
        .unwrap();
    let mut external = changed.clone();
    external.name = "External change".into();
    external.save_ron(&path).unwrap();
    let bytes = fs::read(&path).unwrap();
    app.world_mut().trigger(DocumentAction::Save);
    io::drain(app.world_mut());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_drafts
            .functions
            .contains_key(&function.id)
    );
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

/// Registers `program_id` as an open editor view docked beside the material graph, and installs the
/// editor-view close observers, so the dirty-close prompt can be driven end to end.
fn install_editor_view_close(
    app: &mut App,
    program_id: MaterialProgramId,
) -> crate::docking::EditorViewId {
    use crate::docking::{ToolPanel, WorkspaceLayout};
    use crate::document::{DocumentKey, DocumentManager};
    use crate::editor_view::{
        ActiveEditorContext, EditorViewKind, EditorViewManager, close_editor_view,
        discard_and_close_editor_view, open_document_view, save_and_close_editor_view,
    };
    let mut documents = DocumentManager::default();
    let mut views = EditorViewManager::default();
    let mut active = ActiveEditorContext::default();
    let view = open_document_view(
        &mut documents,
        &mut views,
        &mut active,
        DocumentKey::MaterialProgram(program_id),
        EditorViewKind::MaterialGraph,
    );
    let mut layout = WorkspaceLayout::default();
    layout.show(ToolPanel::MaterialGraph);
    layout.show_editor(view);
    app.insert_resource(documents)
        .insert_resource(views)
        .insert_resource(active)
        .insert_resource(layout)
        .init_resource::<crate::wesl_document::WeslDocuments>()
        .add_observer(close_editor_view)
        .add_observer(save_and_close_editor_view)
        .add_observer(discard_and_close_editor_view);
    view
}

fn editor_tabs(app: &mut App) -> Vec<crate::docking::EditorViewId> {
    app.world()
        .resource::<crate::docking::WorkspaceLayout>()
        .editor_views()
}

#[test]
fn closing_a_dirty_material_tab_prompts_then_saves_and_closes() {
    use crate::editor_view::CloseEditorView;
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, _second) = setup(directory.path());
    let view = install_editor_view_close(&mut app, first.id);
    let edited = edit(&mut app, &first, "Edited then closed");

    app.world_mut().trigger(CloseEditorView::requested(view));
    app.world_mut().flush();
    // A dirty document defers to the prompt rather than closing.
    assert_eq!(
        app.world()
            .resource::<DocumentProtectionState>()
            .pending_editor_close,
        Some(view)
    );
    assert_eq!(editor_tabs(&mut app), vec![view]);

    respond(&mut app, DocumentProtectionAction::Save);
    io::drain(app.world_mut());
    // Saved to disk, draft cleared, tab closed, prompt dismissed.
    assert_eq!(
        MaterialProgram::load_ron(directory.path().join("first.aestra.material.ron")).unwrap(),
        edited
    );
    assert!(
        !app.world()
            .resource::<ProjectEffectCatalog>()
            .material_drafts
            .programs
            .contains_key(&first.id)
    );
    assert!(editor_tabs(&mut app).is_empty());
    assert_eq!(
        app.world()
            .resource::<DocumentProtectionState>()
            .pending_editor_close,
        None
    );
}

#[test]
fn cancelling_a_dirty_close_keeps_the_tab_and_its_draft() {
    use crate::editor_view::CloseEditorView;
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, _second) = setup(directory.path());
    let view = install_editor_view_close(&mut app, first.id);
    edit(&mut app, &first, "Kept draft");

    app.world_mut().trigger(CloseEditorView::requested(view));
    app.world_mut().flush();
    respond(&mut app, DocumentProtectionAction::Cancel);

    assert_eq!(
        app.world()
            .resource::<DocumentProtectionState>()
            .pending_editor_close,
        None
    );
    assert_eq!(editor_tabs(&mut app), vec![view]);
    assert!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_drafts
            .programs
            .contains_key(&first.id)
    );
}

#[test]
fn discarding_a_dirty_close_drops_the_draft_and_closes_without_writing() {
    use crate::editor_view::CloseEditorView;
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, _second) = setup(directory.path());
    let path = directory.path().join("first.aestra.material.ron");
    let view = install_editor_view_close(&mut app, first.id);
    edit(&mut app, &first, "Discarded draft");

    app.world_mut().trigger(CloseEditorView::requested(view));
    app.world_mut().flush();
    respond(&mut app, DocumentProtectionAction::Discard);
    app.world_mut().flush();

    // The draft was dropped, the tab closed, and the file on disk was never rewritten.
    assert!(editor_tabs(&mut app).is_empty());
    assert!(
        !app.world()
            .resource::<ProjectEffectCatalog>()
            .material_drafts
            .programs
            .contains_key(&first.id)
    );
    assert_eq!(MaterialProgram::load_ron(&path).unwrap(), first);
}

#[test]
fn save_all_writes_every_dirty_open_material_document() {
    use crate::document::{DocumentKey, DocumentManager};
    let directory = tempfile::tempdir().unwrap();
    let (mut app, first, second) = setup(directory.path());
    // Both materials are open documents; scoped Save would only write the active one.
    {
        let mut documents = app.world_mut().resource_mut::<DocumentManager>();
        documents.open(DocumentKey::MaterialProgram(first.id));
        documents.open(DocumentKey::MaterialProgram(second.id));
    }
    let first_edited = edit(&mut app, &first, "First edited");
    let second_edited = edit(&mut app, &second, "Second edited");
    assert!(
        !app.world().resource::<EditorSession>().effect_is_dirty(),
        "the effect stays clean, so Save All takes the material-only path"
    );

    app.world_mut().trigger(DocumentAction::SaveAll);
    io::drain(app.world_mut());

    // Both documents were written to disk, and both drafts were cleared.
    assert_eq!(
        MaterialProgram::load_ron(directory.path().join("first.aestra.material.ron")).unwrap(),
        first_edited
    );
    assert_eq!(
        MaterialProgram::load_ron(directory.path().join("second.aestra.material.ron")).unwrap(),
        second_edited
    );
    let catalog = app.world().resource::<ProjectEffectCatalog>();
    assert!(!catalog.material_drafts.programs.contains_key(&first.id));
    assert!(!catalog.material_drafts.programs.contains_key(&second.id));
}

#[test]
fn save_all_reports_nothing_to_save_when_no_document_is_dirty() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, _first, _second) = setup(directory.path());
    app.world_mut().trigger(DocumentAction::SaveAll);
    io::drain(app.world_mut());
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("Nothing to save")
    );
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
