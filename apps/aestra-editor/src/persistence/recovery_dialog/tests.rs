use super::*;
use aestra_core::material::MaterialProgram;
use bevy::ecs::system::RunSystemOnce;

fn fixture() -> (tempfile::TempDir, App, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let program = MaterialProgram::additive_sprite("Recovered material").normalized();
    program
        .save_ron(root.join("program.aestra.material.ron"))
        .unwrap();
    let mut catalog = ProjectEffectCatalog::scan(root.canonicalize().unwrap());
    let mut recovered = crate::test_support::session_with_timing_slack();
    recovered.effect.name = "Recovered effect".into();
    recovered
        .open_material_program(&catalog, program.id)
        .unwrap();
    let mut edit = program.clone();
    edit.name = "Unsaved material".into();
    catalog.replace_material_program(&program, &edit).unwrap();
    recovered.set_material_drafts(catalog.material_drafts.clone());
    let recovery_root = directory.path().join("recovery");
    let mut persistence = RecoveryPersistence::for_test(recovery_root.clone(), None);
    let path = persistence
        .persist_document(
            &recovered.effect,
            None,
            &recovered.material_drafts,
            &recovered.material_target,
        )
        .unwrap();
    let (persistence, candidate, _) = RecoveryPersistence::discover_in(recovery_root);
    let mut session = crate::test_support::session_with_timing_slack();
    session.effect.name = "Current effect".into();
    let mut autosave = AutosaveState::new(&session, true);
    autosave.suspended = true;
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(autosave)
        .insert_resource(persistence)
        .insert_resource(RecoveryDialogState {
            candidate,
            error: None,
        })
        .insert_resource(DocumentProtectionState {
            recovery_open: true,
            ..default()
        })
        .insert_resource(ProjectEffectCatalog::scan(directory.path()))
        .insert_resource(EditorSettings::default())
        .insert_resource(Localizer::new("en-US").unwrap())
        .init_resource::<WorkspaceLayout>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_observer(activate);
    (directory, app, path)
}

fn choose(app: &mut App, choice: Choice) {
    let entity = app.world_mut().spawn(choice).id();
    app.world_mut().trigger(Activate { entity });
    app.world_mut().flush();
}

#[test]
fn later_close_and_escape_keep_snapshot_and_current_session() {
    // Header close and Decide Later share the same action; Escape uses the same finish path.
    for escape_key in [false, true] {
        let (_directory, mut app, path) = fixture();
        let original = fs::read(&path).unwrap();
        if escape_key {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
            app.world_mut().run_system_once(escape).unwrap();
        } else {
            choose(&mut app, Choice::Later);
        }
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!app.world().resource::<RecoveryPersistence>().has_active());
        assert!(!app.world().resource::<DocumentProtectionState>().is_open());
        assert!(!app.world().resource::<AutosaveState>().suspended);
        assert_eq!(
            app.world().resource::<EditorSession>().effect.name,
            "Current effect"
        );
        // Closing repeatedly cannot turn a postponed snapshot into a discard or restore.
        choose(&mut app, Choice::Discard);
        assert!(path.exists());
    }
}

#[test]
fn discard_deletes_only_the_candidate_and_does_not_restore_it() {
    let (directory, mut app, path) = fixture();
    let other = directory.path().join("recovery/other.recovery.ron");
    fs::write(&other, "protected").unwrap();
    let source = directory.path().join("project/program.aestra.material.ron");
    let original = fs::read(&source).unwrap();
    choose(&mut app, Choice::Discard);
    assert!(!path.exists());
    assert_eq!(fs::read_to_string(other).unwrap(), "protected");
    assert_eq!(fs::read(source).unwrap(), original);
    assert_eq!(
        app.world().resource::<EditorSession>().effect.name,
        "Current effect"
    );
    assert!(!app.world().resource::<DocumentProtectionState>().is_open());
}

#[test]
fn pending_recovery_blocks_shortcuts_and_autosave_and_window_close_keeps_snapshot() {
    let (_directory, mut app, path) = fixture();
    let bytes = fs::read(&path).unwrap();
    assert!(
        app.world_mut()
            .run_system_once(|context: crate::input::ShortcutContext| context.blocked())
            .unwrap()
    );
    app.world_mut().run_system_once(autosave_recovery).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(!app.world().resource::<RecoveryPersistence>().has_active());
    app.add_message::<WindowCloseRequested>()
        .add_message::<AppExit>();
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    app.world_mut()
        .write_message(WindowCloseRequested { window });
    app.world_mut()
        .run_system_once(handle_window_close_requests)
        .unwrap();
    app.world_mut().flush();
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert!(!app.world().resource::<Messages<AppExit>>().is_empty());
    assert!(
        app.world()
            .resource::<DocumentProtectionState>()
            .pending
            .is_none()
    );
}

#[test]
fn restore_installs_target_drafts_and_resumes_autosave_without_writing_sources() {
    let (directory, mut app, path) = fixture();
    let source = directory.path().join("project/program.aestra.material.ron");
    let original = fs::read(&source).unwrap();
    choose(&mut app, Choice::Restore);
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect.name, "Recovered effect");
    assert!(session.standalone_material().is_some());
    assert_eq!(session.material_drafts.count(), 1);
    assert!(app.world().resource::<RecoveryPersistence>().has_active());
    assert!(!app.world().resource::<DocumentProtectionState>().is_open());
    assert!(!app.world().resource::<AutosaveState>().suspended);
    assert!(path.exists());
    assert_eq!(fs::read(source).unwrap(), original);
}

#[test]
fn failed_restore_and_discard_keep_dialog_and_snapshot_for_retry() {
    let (directory, mut app, path) = fixture();
    // An unavailable recorded project must not replace the current session.
    fs::rename(
        directory.path().join("project"),
        directory.path().join("moved-project"),
    )
    .unwrap();
    choose(&mut app, Choice::Restore);
    assert!(
        app.world()
            .resource::<RecoveryDialogState>()
            .error
            .is_some()
    );
    assert!(app.world().resource::<DocumentProtectionState>().is_open());
    assert!(app.world().resource::<AutosaveState>().suspended);
    assert_eq!(
        app.world().resource::<EditorSession>().effect.name,
        "Current effect"
    );
    assert!(path.exists());
    // A deletion error stays visible and does not count as a successful discard.
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("retained"), "protected").unwrap();
    choose(&mut app, Choice::Discard);
    assert!(
        app.world()
            .resource::<RecoveryDialogState>()
            .error
            .is_some()
    );
    assert!(app.world().resource::<DocumentProtectionState>().is_open());
    assert!(path.join("retained").exists());
}

#[test]
fn modal_is_retained_localized_and_focus_defaults_to_a_non_destructive_action() {
    let (_directory, mut app, _path) = fixture();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::scene::ScenePlugin,
    ))
    .init_asset::<Font>();
    for locale in ["en-US", "fr-FR"] {
        let localizer = Localizer::new(locale).unwrap();
        let state = app.world().resource::<RecoveryDialogState>();
        let summary = description(
            state.candidate.as_ref().unwrap(),
            &localizer,
            SystemTime::now(),
        );
        assert!(summary.contains("Unsaved material"));
        assert!(summary.contains("Recovered effect"));
        for key in [
            "persistence-recovery-restore",
            "persistence-recovery-discard",
            "persistence-recovery-later",
            "persistence-recovery-age-minutes",
            "persistence-recovery-age-hours",
            "persistence-recovery-age-days",
        ] {
            let mut args = FluentArgs::new();
            args.set("count", "1");
            assert_ne!(localizer.text_with(key, &args), key);
        }
    }
    let localizer = Localizer::new("en-US").unwrap();
    let protection = app.world().resource::<DocumentProtectionState>().clone();
    app.world_mut()
        .commands()
        .spawn_empty()
        .with_children(|root| spawn(root, &protection, &localizer));
    app.world_mut().flush();
    app.add_systems(Update, sync);
    app.update();
    let focused = app.world().resource::<InputFocus>().get().unwrap();
    assert_eq!(app.world().get::<Choice>(focused), Some(&Choice::Later));
    let overlay = app
        .world_mut()
        .query_filtered::<Entity, With<Overlay>>()
        .single(app.world())
        .unwrap();
    assert!(app.world().get::<TabGroup>(overlay).unwrap().modal);
    let revision = app.world().resource::<EditorSession>().ui_revision;
    app.update();
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision
    );
    choose(&mut app, Choice::Later);
    app.update();
    assert_eq!(
        app.world().get::<Node>(overlay).unwrap().display,
        Display::None
    );
}
