use super::*;
use crate::history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory};

fn fixture() -> (tempfile::TempDir, App, ProjectSourceId) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("folder/empty")).unwrap();
    std::fs::write(root.path().join("folder/image.png"), b"texture").unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let source = catalog
        .content()
        .source_tree()
        .at_relative_path("folder")
        .unwrap()
        .id;
    let mut app = App::new();
    app.insert_resource(catalog)
        .insert_resource(crate::test_support::session_with_timing_slack())
        .insert_resource(Localizer::new("en-US").unwrap())
        .init_resource::<super::super::AssetBrowserState>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<crate::material_function_editor::FunctionEditor>()
        .add_observer(crate::history::execute_history_action);
    register(&mut app);
    (root, app, source)
}
fn choose(app: &mut App, choice: Choice) {
    let entity = app.world_mut().spawn(choice).id();
    app.world_mut().trigger(Activate { entity });
    app.world_mut().flush();
}
fn history(app: &mut App, undo: bool) {
    app.world_mut().trigger(if undo {
        HistoryAction::Undo
    } else {
        HistoryAction::Redo
    });
    app.world_mut().flush();
    if !io::idle_world(app.world()) {
        io::drain(app.world_mut());
    }
}
fn delete(app: &mut App, source: ProjectSourceId) {
    app.world_mut().trigger(Open(Some(source)));
    io::drain(app.world_mut());
}

#[test]
fn delete_is_immediate_after_preflight_and_undo_redo_restore_exact_folder_contents() {
    let (root, mut app, source) = fixture();
    let effect = app.world().resource::<EditorSession>().effect.clone();
    app.world_mut().trigger(Open(Some(source)));
    assert!(!app.world().resource::<State>().open);
    assert!(!app.world().resource::<DocumentProtectionState>().is_open());
    assert!(root.path().join("folder").exists());
    io::drain(app.world_mut());
    assert!(!root.path().join("folder").exists());
    for _ in 0..3 {
        history(&mut app, true);
        assert!(root.path().join("folder/empty").is_dir());
        assert_eq!(
            std::fs::read(root.path().join("folder/image.png")).unwrap(),
            b"texture"
        );
        history(&mut app, false);
        assert!(!root.path().join("folder").exists());
    }
    assert_eq!(app.world().resource::<EditorSession>().effect, effect);
    assert!(!app.world().resource::<State>().open);
}

#[test]
fn document_edits_and_delete_follow_action_order_and_new_edits_discard_redo() {
    let (root, mut app, source) = fixture();
    let original = app.world().resource::<EditorSession>().effect.name.clone();
    delete(&mut app, source);
    app.world_mut()
        .resource_mut::<EditorSession>()
        .set_effect_name("After delete");
    history(&mut app, true);
    assert_eq!(
        app.world().resource::<EditorSession>().effect.name,
        original
    );
    assert!(!root.path().join("folder").exists());
    history(&mut app, true);
    assert!(root.path().join("folder").exists());
    history(&mut app, false);
    assert!(!root.path().join("folder").exists());
    history(&mut app, false);
    assert_eq!(
        app.world().resource::<EditorSession>().effect.name,
        "After delete"
    );
    history(&mut app, true);
    history(&mut app, true);
    app.world_mut()
        .resource_mut::<EditorSession>()
        .set_effect_name("New branch");
    history(&mut app, false);
    assert!(root.path().join("folder").exists());
    assert_eq!(
        app.world().resource::<EditorSession>().effect.name,
        "New branch"
    );
}

#[test]
fn no_op_effect_transactions_do_not_hide_deletion_or_invalidate_redo() {
    let (root, mut app, source) = fixture();
    let no_op = |app: &mut App| {
        app.world_mut()
            .resource_mut::<EditorSession>()
            .execute_transaction(
                aestra_authoring::EffectTransaction::new("No change", vec![]),
                false,
            );
    };
    delete(&mut app, source);
    no_op(&mut app);
    history(&mut app, true);
    assert!(root.path().join("folder/image.png").exists());
    no_op(&mut app);
    history(&mut app, false);
    assert!(!root.path().join("folder").exists());
}

#[test]
fn failed_undo_keeps_its_entry_and_never_undoes_an_unrelated_document_edit() {
    let (root, mut app, source) = fixture();
    delete(&mut app, source);
    std::fs::create_dir(root.path().join("folder")).unwrap();
    let effect = app.world().resource::<EditorSession>().effect.clone();
    history(&mut app, true);
    assert_eq!(app.world().resource::<EditorSession>().effect, effect);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("blocked")
    );
    assert!(!root.path().join("folder/image.png").exists());
    std::fs::remove_dir(root.path().join("folder")).unwrap();
    history(&mut app, true);
    assert!(root.path().join("folder/image.png").exists());
    std::fs::write(root.path().join("folder/image.png"), b"external edit").unwrap();
    history(&mut app, false);
    assert_eq!(
        std::fs::read(root.path().join("folder/image.png")).unwrap(),
        b"external edit"
    );
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("blocked")
    );
}

#[test]
fn unrelated_draft_allows_deletion_but_changes_during_io_cancel() {
    for during in [false, true] {
        let (root, mut app, source) = fixture();
        let mut completion = if during {
            app.world_mut().trigger(Open(Some(source)));
            Some(io::prepared_completion(app.world_mut()))
        } else {
            None
        };
        app.world_mut()
            .resource_mut::<EditorSession>()
            .set_effect_name("Unsaved");
        if let Some(completion) = completion.as_mut() {
            completion.apply(app.world_mut());
        } else {
            delete(&mut app, source);
        }
        if during {
            assert!(root.path().join("folder/image.png").exists());
            assert!(!root.path().join(".aestra/deleted").exists());
            assert!(app.world().resource::<State>().open);
        } else {
            assert!(!root.path().join("folder").exists());
            assert!(!app.world().resource::<State>().open);
            history(&mut app, true);
            assert!(root.path().join("folder/image.png").exists());
            history(&mut app, false);
            assert!(!root.path().join("folder").exists());
            assert_eq!(
                app.world().resource::<EditorSession>().effect.name,
                "Unsaved"
            );
            assert!(app.world().resource::<EditorSession>().effect_is_dirty());
        }
    }
}

#[test]
fn queued_undo_cancels_if_document_changes_then_can_be_retried_in_order() {
    let (root, mut app, source) = fixture();
    delete(&mut app, source);
    app.world_mut().trigger(HistoryAction::Undo);
    let mut completion = io::prepared_completion(app.world_mut());
    app.world_mut()
        .resource_mut::<EditorSession>()
        .set_effect_name("Newer edit");
    completion.apply(app.world_mut());
    assert!(!root.path().join("folder").exists());
    history(&mut app, true);
    assert!(!root.path().join("folder").exists());
    history(&mut app, true);
    assert!(root.path().join("folder").exists());
}

#[test]
fn unrelated_pending_proposal_is_preserved_but_proposed_references_block_delete() {
    use aestra_authoring::{EffectCommand, EffectTransaction};
    for referenced in [false, true] {
        let (root, mut app, source) = fixture();
        let command = if referenced {
            EffectCommand::AddAsset {
                asset: aestra_core::AssetDefinition::texture(
                    "Proposed usage",
                    "folder/image.png#Layer0",
                ),
                index: 0,
            }
        } else {
            EffectCommand::SetEffectName {
                name: "Proposed name".into(),
            }
        };
        app.world_mut()
            .resource_mut::<EditorSession>()
            .preview_transaction(EffectTransaction::single("Pending proposal", command));
        let candidate = app
            .world()
            .resource::<EditorSession>()
            .pending_change
            .as_ref()
            .unwrap()
            .preview
            .candidate()
            .clone();
        delete(&mut app, source);
        if referenced {
            assert!(root.path().join("folder/image.png").exists());
            assert!(
                app.world()
                    .resource::<State>()
                    .message
                    .contains("active document")
            );
        } else {
            assert!(!root.path().join("folder").exists());
            history(&mut app, true);
            assert!(root.path().join("folder/image.png").exists());
        }
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .pending_change
                .as_ref()
                .unwrap()
                .preview
                .candidate(),
            &candidate
        );
    }
}

#[test]
fn manual_restore_removes_matching_delete_history_and_restart_keeps_recovery() {
    let (root, mut app, source) = fixture();
    delete(&mut app, source);
    app.world_mut().trigger(Open(None));
    io::drain(app.world_mut());
    assert_eq!(app.world().resource::<State>().entries.len(), 1);
    choose(&mut app, Choice::Restore(0));
    io::drain(app.world_mut());
    history(&mut app, true);
    assert!(root.path().join("folder/empty").is_dir());
    assert!(
        !app.world()
            .resource::<EditorSession>()
            .status
            .contains("blocked")
    );
    delete(&mut app, source);
    aestra_project::ProjectContent::scan(root.path())
        .deleted_sources()
        .unwrap()
        .pop()
        .unwrap()
        .restore(&[], true)
        .unwrap();
    assert!(root.path().join("folder/empty").exists());
}

#[test]
fn active_resource_blocks_and_escape_closes_the_error_panel() {
    let (root, mut app, source) = fixture();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .effect
        .assets
        .push(aestra_core::AssetDefinition::texture(
            "Image",
            "folder/image.png#Layer0",
        ));
    app.world_mut()
        .resource_mut::<EditorSession>()
        .set_effect_name("Unsaved owner");
    delete(&mut app, source);
    assert!(
        app.world()
            .resource::<State>()
            .message
            .contains("active document")
    );
    assert!(root.path().join("folder/image.png").exists());
    app.init_resource::<ButtonInput<KeyCode>>();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert!(!app.world().resource::<State>().open);
    assert!(!app.world().resource::<DocumentProtectionState>().is_open());
}

#[test]
fn native_blocker_scrolls_long_content_without_a_force_delete_button() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("folder")).unwrap();
    let mut app = super::super::tests::browser_layout_app(root.path(), UVec2::new(800, 500), 1.0);
    let host = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, (With<Node>, Without<ChildOf>)>()
            .single(world)
            .unwrap()
    };
    app.world_mut().get_mut::<Node>(host).unwrap().height = Val::Px(500.0);
    app.world_mut().commands().entity(host).with_children(spawn);
    app.world_mut().flush();
    let source = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .content()
        .source_tree()
        .at_relative_path("folder")
        .unwrap()
        .id;
    app.world_mut().resource_mut::<State>().source = Some(source);
    show_error(app.world_mut(), "A long dependency blocker\n".repeat(90));
    for _ in 0..4 {
        app.update();
    }
    let world = app.world_mut();
    let (node, scroll) = world
        .query_filtered::<(&ComputedNode, &Node), With<Body>>()
        .single(world)
        .unwrap();
    assert!(node.size().y > 40.0 && node.size().y <= 400.0);
    assert_eq!(scroll.overflow, Overflow::scroll_y());
    let focused = world.resource::<InputFocus>().get().unwrap();
    assert!(matches!(world.get::<Choice>(focused), Some(Choice::Cancel)));
    assert_eq!(world.query::<&Choice>().iter(world).count(), 2);
    assert!(root.path().join("folder").exists());
}

#[test]
fn deletion_orders_material_edits_before_and_after_it_even_after_focus_changes() {
    use aestra_core::material::MaterialProgram;
    let (root, mut app, source) = fixture();
    let original = MaterialProgram::additive_sprite("Original").normalized();
    original
        .save_ron(root.path().join("program.aestra.material.ron"))
        .unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    let edit = |app: &mut App, before: &MaterialProgram, name: &str| {
        let mut session = app.world_mut().remove_resource::<EditorSession>().unwrap();
        let mut catalog = app
            .world_mut()
            .remove_resource::<ProjectEffectCatalog>()
            .unwrap();
        session.open_material_program(&catalog, before.id).unwrap();
        let mut after = before.clone();
        after.name = name.into();
        app.world_mut()
            .resource_mut::<MaterialProgramEditHistory>()
            .execute_replacement(
                &mut session,
                &mut catalog,
                "Rename program",
                before.clone(),
                after,
            )
            .unwrap();
        app.world_mut()
            .resource_mut::<EditorHistoryLedger>()
            .record_material_edit(&mut session);
        app.insert_resource(session).insert_resource(catalog);
    };
    edit(&mut app, &original, "Before delete");
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .save_material_drafts()
        .unwrap();
    let drafts = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .material_drafts
        .clone();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .set_material_drafts(drafts);
    // A previously undone/redone edit must still precede the new filesystem entry.
    history(&mut app, true);
    history(&mut app, false);
    delete(&mut app, source);
    assert!(!root.path().join("folder").exists());
    let saved = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .material_program(original.id)
        .unwrap();
    edit(&mut app, &saved, "After delete");
    // Pointer focus alone must not bypass the newer material edit or deletion.
    app.world_mut()
        .resource_mut::<EditorSession>()
        .material_history_active = false;
    history(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(original.id)
            .unwrap()
            .name,
        "Before delete"
    );
    assert!(!root.path().join("folder").exists());
    history(&mut app, true);
    assert!(root.path().join("folder").exists());
    history(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(original.id)
            .unwrap(),
        original
    );
}

#[test]
fn root_switch_does_not_replay_a_deletion_in_the_previous_project() {
    let (root, mut app, source) = fixture();
    delete(&mut app, source);
    let other = tempfile::tempdir().unwrap();
    app.insert_resource(ProjectEffectCatalog::scan(other.path()));
    history(&mut app, true);
    assert!(!root.path().join("folder").exists());
    assert_eq!(
        aestra_project::ProjectContent::scan(root.path())
            .deleted_sources()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn deleting_open_dirty_material_closes_graph_and_recovers_draft_history_and_disk() {
    use aestra_core::material::MaterialProgram;
    for folder in [false, true] {
        let (root, mut app, source) = fixture();
        let original = MaterialProgram::additive_sprite("Original").normalized();
        let path = root.path().join("folder/program.aestra.material.ron");
        original.save_ron(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut catalog = app
            .world_mut()
            .remove_resource::<ProjectEffectCatalog>()
            .unwrap();
        catalog.refresh();
        let source = if folder {
            source
        } else {
            catalog
                .content()
                .source_tree()
                .at_relative_path("folder/program.aestra.material.ron")
                .unwrap()
                .id
        };
        let mut session = app.world_mut().remove_resource::<EditorSession>().unwrap();
        session
            .open_material_program(&catalog, original.id)
            .unwrap();
        let mut edited = original.clone();
        edited.name = "Unsaved graph".into();
        app.world_mut()
            .resource_mut::<MaterialProgramEditHistory>()
            .execute_replacement(
                &mut session,
                &mut catalog,
                "Edit graph",
                original.clone(),
                edited.clone(),
            )
            .unwrap();
        app.world_mut()
            .resource_mut::<EditorHistoryLedger>()
            .record_material_edit(&mut session);
        let drafts = catalog.material_drafts.clone();
        app.insert_resource(session).insert_resource(catalog);
        delete(&mut app, source);
        assert!(
            !path.exists(),
            "{}",
            app.world().resource::<EditorSession>().status
        );
        assert!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_drafts
                .is_empty()
        );
        assert!(
            app.world()
                .resource::<EditorSession>()
                .standalone_material()
                .is_none()
        );
        history(&mut app, true);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_drafts,
            drafts
        );
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .standalone_material(),
            Some(original.id)
        );
        history(&mut app, true);
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_program(original.id)
                .unwrap(),
            original
        );
        history(&mut app, false);
        history(&mut app, false);
        assert!(!path.exists());
        // Restart-like state: recovery must not depend on the in-memory Undo stack.
        app.insert_resource(crate::test_support::session_with_timing_slack());
        app.insert_resource(crate::history::asset_order::AssetOrder::default());
        app.world_mut().trigger(Open(None));
        io::drain(app.world_mut());
        choose(&mut app, Choice::Restore(0));
        io::drain(app.world_mut());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_drafts,
            drafts
        );
    }
}

#[test]
fn deleting_open_effect_detaches_source_without_losing_unsaved_work() {
    let (root, mut app, _) = fixture();
    let path = root.path().join("open.aestra.ron");
    let original = app.world().resource::<EditorSession>().effect.clone();
    original.save_ron(&path).unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .accept_external_source_path(path.clone());
    app.world_mut()
        .resource_mut::<EditorSession>()
        .set_effect_name("Unsaved open effect");
    let source = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .content()
        .source_tree()
        .at_relative_path("open.aestra.ron")
        .unwrap()
        .id;
    delete(&mut app, source);
    assert!(!path.exists());
    assert!(
        app.world()
            .resource::<EditorSession>()
            .source_path
            .is_none()
    );
    assert_eq!(
        app.world().resource::<EditorSession>().effect.name,
        "Unsaved open effect"
    );
    history(&mut app, true);
    assert_eq!(
        app.world().resource::<EditorSession>().source_path.as_ref(),
        Some(&path)
    );
    assert_eq!(aestra_core::EffectAsset::load_ron(&path).unwrap(), original);
    assert_eq!(
        app.world().resource::<EditorSession>().effect.name,
        "Unsaved open effect"
    );
}

#[test]
fn open_function_draft_survives_failed_delete_and_restores_after_success() {
    use crate::material_function_editor::FunctionEditor;
    use aestra_core::material::{MaterialExpression, MaterialExpressionKind, MaterialFunction};
    let (root, mut app, source) = fixture();
    let mut original = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    original.custom_wesl = None;
    original.expressions.push(MaterialExpression {
        id: original.outputs[0].expression,
        kind: MaterialExpressionKind::FunctionInput(original.inputs[0].id),
    });
    let original = original.normalized();
    let path = root
        .path()
        .join("folder/function.aestra.material-function.ron");
    original.save_ron(&path).unwrap();
    let mut catalog = app
        .world_mut()
        .remove_resource::<ProjectEffectCatalog>()
        .unwrap();
    catalog.refresh();
    let mut session = app.world_mut().remove_resource::<EditorSession>().unwrap();
    session
        .open_material_function(&catalog, original.id)
        .unwrap();
    let mut edited = original.clone();
    edited.name = "Unsaved function".into();
    app.world_mut()
        .resource_mut::<FunctionEditor>()
        .edit(&mut session, &mut catalog, edited.clone())
        .unwrap();
    let drafts = catalog.material_drafts.clone();
    app.insert_resource(session).insert_resource(catalog);
    app.world_mut().trigger(Open(Some(source)));
    let mut completion = io::prepared_completion(app.world_mut());
    std::fs::write(root.path().join("external.png"), b"changed inventory").unwrap();
    completion.apply(app.world_mut());
    assert!(path.exists());
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_drafts,
        drafts
    );
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .standalone_function(),
        Some(original.id)
    );
    choose(&mut app, Choice::Cancel);
    delete(&mut app, source);
    assert!(!path.exists());
    assert!(
        app.world()
            .resource::<EditorSession>()
            .standalone_function()
            .is_none()
    );
    history(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_drafts,
        drafts
    );
    assert_eq!(MaterialFunction::load_ron(&path).unwrap(), original);
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .standalone_function(),
        Some(original.id)
    );
}

#[test]
fn unrelated_material_draft_is_preserved_by_delete_undo_redo_and_bin_restore() {
    use aestra_core::material::MaterialProgram;
    let (root, mut app, source) = fixture();
    let original = MaterialProgram::additive_sprite("Saved").normalized();
    original
        .save_ron(root.path().join("program.aestra.material.ron"))
        .unwrap();
    let mut edited = original.clone();
    edited.name = "Unsaved material".into();
    let mut catalog = app.world_mut().resource_mut::<ProjectEffectCatalog>();
    catalog.refresh();
    catalog
        .replace_material_program(&original, &edited)
        .unwrap();
    let drafts = catalog.material_drafts.clone();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .set_material_drafts(drafts.clone());
    delete(&mut app, source);
    assert!(!root.path().join("folder").exists());
    history(&mut app, true);
    assert!(root.path().join("folder/image.png").exists());
    history(&mut app, false);
    assert!(!root.path().join("folder").exists());
    app.world_mut().trigger(Open(None));
    io::drain(app.world_mut());
    choose(&mut app, Choice::Restore(0));
    io::drain(app.world_mut());
    assert!(root.path().join("folder/image.png").exists());
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_drafts,
        drafts
    );
    assert_eq!(
        MaterialProgram::load_ron(root.path().join("program.aestra.material.ron")).unwrap(),
        original
    );
}

#[test]
fn deletion_orders_function_edits_even_after_returning_to_the_effect() {
    use crate::material_function_editor::FunctionEditor;
    use aestra_core::material::{MaterialExpression, MaterialExpressionKind, MaterialFunction};
    let (root, mut app, source) = fixture();
    let mut function = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    function.custom_wesl = None;
    function.expressions.push(MaterialExpression {
        id: function.outputs[0].expression,
        kind: MaterialExpressionKind::FunctionInput(function.inputs[0].id),
    });
    let function = function.normalized();
    function
        .save_ron(root.path().join("function.aestra.material-function.ron"))
        .unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    delete(&mut app, source);
    let mut session = app.world_mut().remove_resource::<EditorSession>().unwrap();
    let mut catalog = app
        .world_mut()
        .remove_resource::<ProjectEffectCatalog>()
        .unwrap();
    session
        .open_material_function(&catalog, function.id)
        .unwrap();
    let mut edited = function.clone();
    edited.name = "Changed function".into();
    app.world_mut()
        .resource_mut::<FunctionEditor>()
        .edit(&mut session, &mut catalog, edited)
        .unwrap();
    session.return_to_effect_material();
    app.insert_resource(session).insert_resource(catalog);
    history(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap(),
        function
    );
    assert!(!root.path().join("folder").exists());
    history(&mut app, true);
    assert!(root.path().join("folder/image.png").exists());
}
