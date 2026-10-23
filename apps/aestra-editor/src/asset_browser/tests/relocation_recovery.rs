use super::*;
use aestra_project::content::operations::OperationRequest;
use bevy::ecs::system::RunSystemOnce;
use std::{fs, path::PathBuf};

/// Use the real journal writer and publication sequence. Reinstating the completed
/// journal simulates interruption immediately before its final archival rename.
fn fixture() -> (tempfile::TempDir, App, PathBuf, Vec<u8>) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("pack/empty")).unwrap();
    fs::create_dir(root.path().join("destination")).unwrap();
    let mut effect = crate::test_support::effect_with_timing_slack();
    effect.assets.push(aestra_core::AssetDefinition {
        id: aestra_core::AssetId::from_u128(12345),
        name: "Texture".into(),
        kind: aestra_core::AssetKind::Texture,
        path: "pack/texture.png".into(),
    });
    fs::write(root.path().join("pack/texture.png"), b"test image bytes").unwrap();
    let source = root.path().join("pack/effect.aestra.ron");
    let bytes = format!(
        "// keep authored formatting\n{}\n",
        effect.to_pretty_ron().unwrap()
    )
    .into_bytes();
    fs::write(&source, &bytes).unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let content = catalog.content();
    let plan = content
        .plan_content_relocations(
            vec![OperationRequest::Move {
                source: content.source_tree().at_relative_path("pack").unwrap().id,
                parent: content
                    .source_tree()
                    .at_relative_path("destination")
                    .unwrap()
                    .id,
            }],
            &[],
            true,
        )
        .unwrap();
    let result = plan.apply().unwrap();
    let pending = result.journal.parent().unwrap().join("active.pending");
    fs::rename(result.journal, &pending).unwrap();
    let mut session = crate::test_support::session_with_timing_slack();
    session
        .open(root.path().join("destination/pack/effect.aestra.ron"))
        .unwrap();
    let mut app = App::new();
    app.insert_resource(ProjectEffectCatalog::scan(root.path()))
        .insert_resource(session)
        .insert_resource(Localizer::new("en-US").unwrap())
        .init_resource::<DocumentProtectionState>()
        .init_resource::<super::super::AssetBrowserState>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>();
    register(&mut app);
    app.update();
    io::drain(app.world_mut());
    (root, app, pending, bytes)
}

fn choose(app: &mut App, choice: Choice) {
    let entity = app.world_mut().spawn(choice).id();
    app.world_mut().trigger(Activate { entity });
    app.world_mut().flush();
}

#[test]
fn inspection_and_later_are_read_only_and_escape_allows_reopening() {
    let (root, mut app, pending, _) = fixture();
    let journal = fs::read(&pending).unwrap();
    let state = app.world().resource::<RecoveryState>();
    assert!(state.open && state.available);
    assert_eq!(state.pending.as_ref().unwrap().moved_folders, 1);
    assert!(app.world().resource::<DocumentProtectionState>().is_open());
    choose(&mut app, Choice::Later);
    assert!(!app.world().resource::<DocumentProtectionState>().is_open());
    choose(&mut app, Choice::Open);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.world_mut().run_system_once(ui::sync).unwrap();
    assert!(!app.world().resource::<RecoveryState>().open);
    assert_eq!(fs::read(&pending).unwrap(), journal);
    assert!(!root.path().join("pack").exists());
    assert!(root.path().join("destination/pack/empty").is_dir());
}

#[test]
fn restore_reloads_paths_bindings_and_exact_save_baseline_and_keeps_graph_target() {
    let (root, mut app, pending, bytes) = fixture();
    let program = aestra_core::material::MaterialProgram::additive_sprite("Test").normalized();
    program
        .save_ron(root.path().join("program.aestra.material.ron"))
        .unwrap();
    // Prepare a valid shared graph target without touching the journaled paths.
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    let catalog = app.world().resource::<ProjectEffectCatalog>().clone();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .open_material_program(&catalog, program.id)
        .unwrap();
    let target = app
        .world()
        .resource::<EditorSession>()
        .material_target
        .clone();
    choose(&mut app, Choice::Restore);
    io::drain(app.world_mut());
    assert!(!pending.exists());
    assert!(root.path().join("pack/empty").is_dir());
    assert!(!root.path().join("destination/pack").exists());
    let original = root.path().join("pack/effect.aestra.ron");
    assert_eq!(fs::read(&original).unwrap(), bytes);
    let session = app.world().resource::<EditorSession>();
    assert_eq!(
        session
            .source_path
            .as_ref()
            .unwrap()
            .canonicalize()
            .unwrap(),
        original.canonicalize().unwrap()
    );
    assert_eq!(session.effect.assets[0].path, "pack/texture.png");
    assert_eq!(session.material_target, target);
    assert!(session.material_history_active);
    assert!(!session.effect_is_dirty());
    assert!(session.status.contains("Backup retained"));
    // Saving must target the restored file and use its exact restored-byte baseline.
    app.world_mut()
        .resource_mut::<EditorSession>()
        .save()
        .unwrap();
    assert!(!app.world().resource::<DocumentProtectionState>().is_open());
}

#[test]
fn dirty_documents_disable_restore_and_edits_during_preparation_cancel_it() {
    for during in [false, true] {
        let (root, mut app, pending, _) = fixture();
        let before = fs::read(&pending).unwrap();
        let edit = |app: &mut App| {
            app.world_mut().resource_mut::<EditorSession>().execute(
                "Edit",
                aestra_authoring::EffectCommand::SetEffectName {
                    name: "Unsaved".into(),
                },
                false,
            );
        };
        if during {
            choose(&mut app, Choice::Restore);
            let mut completion = io::prepared_completion(app.world_mut());
            edit(&mut app);
            completion.apply(app.world_mut());
            assert!(app.world().resource::<RecoveryState>().error.is_some());
        } else {
            edit(&mut app);
            choose(&mut app, Choice::Restore);
        }
        assert_eq!(fs::read(pending).unwrap(), before);
        assert!(!root.path().join("pack").exists());
        assert_eq!(
            app.world().resource::<EditorSession>().effect.name,
            "Unsaved"
        );
    }
}

#[test]
fn journal_tampering_and_external_folder_additions_fail_without_overwriting() {
    for tamper in [false, true] {
        let (root, mut app, pending, _) = fixture();
        if tamper {
            fs::write(&pending, b"malformed journal").unwrap();
        } else {
            fs::write(root.path().join("destination/pack/new.txt"), b"user file").unwrap();
        }
        let before = fs::read(&pending).unwrap();
        choose(&mut app, Choice::Restore);
        io::drain(app.world_mut());
        assert_eq!(fs::read(&pending).unwrap(), before);
        assert!(app.world().resource::<RecoveryState>().error.is_some());
        assert!(app.world().resource::<RecoveryState>().pending.is_none());
        assert!(!root.path().join("pack").exists());
        if !tamper {
            assert_eq!(
                fs::read(root.path().join("destination/pack/new.txt")).unwrap(),
                b"user file"
            );
        }
    }
}

#[test]
fn project_switch_cancels_prepared_restore_and_malformed_journals_remain_inspectable() {
    let (root, mut app, pending, _) = fixture();
    choose(&mut app, Choice::Restore);
    let mut completion = io::prepared_completion(app.world_mut());
    let other = tempfile::tempdir().unwrap();
    app.world_mut()
        .insert_resource(ProjectEffectCatalog::scan(other.path()));
    completion.apply(app.world_mut());
    app.update();
    assert!(pending.exists());
    assert!(!app.world().resource::<RecoveryState>().available);
    fs::write(&pending, b"do not replay").unwrap();
    app.world_mut()
        .insert_resource(ProjectEffectCatalog::scan(root.path()));
    app.update();
    io::drain(app.world_mut());
    let state = app.world().resource::<RecoveryState>();
    assert!(state.open && state.available && state.pending.is_none());
    assert!(
        state
            .error
            .as_ref()
            .unwrap()
            .contains("Unreadable transaction journal")
    );
    assert_eq!(fs::read(pending).unwrap(), b"do not replay");
}

#[test]
fn shared_drafts_block_restore_and_retry_rechecks_external_conflicts() {
    let (root, mut app, pending, _) = fixture();
    let program = aestra_core::material::MaterialProgram::additive_sprite("Draft").normalized();
    program
        .save_ron(root.path().join("draft.aestra.material.ron"))
        .unwrap();
    let mut changed = program.clone();
    changed.name = "Unsaved program".into();
    let mut catalog = app.world_mut().resource_mut::<ProjectEffectCatalog>();
    catalog.refresh();
    catalog
        .replace_material_program(&program, &changed)
        .unwrap();
    choose(&mut app, Choice::Restore);
    assert!(pending.exists());
    assert!(!app.world().resource::<RecoveryState>().busy);
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .material_drafts = default();
    let extra = root.path().join("destination/pack/unexpected");
    fs::create_dir(&extra).unwrap();
    choose(&mut app, Choice::Retry);
    app.update();
    io::drain(app.world_mut());
    assert!(app.world().resource::<RecoveryState>().pending.is_none());
    // Remove only our empty test directory, then explicitly re-inspect.
    fs::remove_dir(extra).unwrap();
    choose(&mut app, Choice::Retry);
    app.update();
    io::drain(app.world_mut());
    assert!(app.world().resource::<RecoveryState>().pending.is_some());
    assert!(pending.exists());
}

#[test]
fn unchanged_effect_reconciliation_preserves_history_and_does_not_hijack_outside_sources() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("effect.aestra.ron");
    let mut session = crate::test_support::session_with_timing_slack();
    session.effect.save_ron(&original).unwrap();
    session.open(&original).unwrap();
    session.execute(
        "Name",
        aestra_authoring::EffectCommand::SetEffectName {
            name: "Temporary".into(),
        },
        false,
    );
    session.undo();
    let generation = session.history_generation();
    let moved = root.path().join("renamed.aestra.ron");
    fs::rename(&original, &moved).unwrap();
    let bytes = format!(
        "// new exact-byte baseline\n{}",
        session.effect.to_pretty_ron().unwrap()
    );
    fs::write(&moved, bytes).unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    reconcile_document(&catalog, &mut session).unwrap();
    assert!(session.can_redo());
    assert_eq!(session.history_generation(), generation);
    session.save().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let external = outside.path().join("same-id.aestra.ron");
    session.effect.save_ron(&external).unwrap();
    session.open(&external).unwrap();
    reconcile_document(&catalog, &mut session).unwrap();
    assert_eq!(session.source_path.as_ref().unwrap(), &external);
}

#[test]
fn modal_buttons_explain_dirty_state_and_focus_later_by_default() {
    let (_root, mut app, _, _) = fixture();
    app.add_plugins((
        bevy::asset::AssetPlugin::default(),
        bevy::scene::ScenePlugin,
    ))
    .init_asset::<Font>();
    let localizer = Localizer::new("en-US").unwrap();
    let previous = app.world_mut().spawn(Node::default()).id();
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(previous, bevy::input_focus::FocusCause::Navigated);
    app.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|parent| {
            spawn(parent, &localizer);
            spawn_reopen_button(parent, &localizer);
        });
    app.world_mut().flush();
    app.world_mut().run_system_once(ui::sync).unwrap();
    let later = app
        .world_mut()
        .query::<(Entity, &Choice)>()
        .iter(app.world())
        .find(|(_, choice)| **choice == Choice::Later)
        .unwrap()
        .0;
    assert_eq!(
        app.world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(later)
    );
    app.world_mut().resource_mut::<EditorSession>().effect.name = "Dirty".into();
    app.world_mut().run_system_once(ui::sync).unwrap();
    app.world_mut().flush();
    let restore = app
        .world_mut()
        .query::<(Entity, &Choice)>()
        .iter(app.world())
        .find(|(_, choice)| **choice == Choice::Restore)
        .unwrap()
        .0;
    assert!(
        app.world()
            .entity(restore)
            .contains::<InteractionDisabled>()
    );
    assert!(
        app.world_mut()
            .query_filtered::<&Text, With<ui::Description>>()
            .iter(app.world())
            .any(|text| text.0.contains("Save or discard"))
    );
}
