use super::*;
use crate::test_support;
use bevy::ecs::system::RunSystemOnce;

fn open_folder(app: &mut App, path: &Path) {
    let mut plan = Some(OpenPlan {
        target: OpenTarget::Folder(path.to_owned()),
        navigation: default(),
        restore: None,
        clip: None,
        emitter: None,
    });
    app.world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  session: Res<EditorSession>,
                  settings: Res<EditorSettings>,
                  catalog: Res<ProjectEffectCatalog>,
                  locale: Res<Localizer>| {
                queue_plan(
                    &mut commands,
                    &session,
                    &settings,
                    &catalog,
                    &locale,
                    plan.take().unwrap(),
                );
            },
        )
        .unwrap();
}

#[test]
fn explicit_folder_open_publishes_root_and_document_together_and_recovers_from_error() {
    let directory = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let effects = destination.path().join("assets/effects/nested");
    fs::create_dir_all(&effects).unwrap();
    let expected = target(&effects, "target");
    let mut app = app(directory.path());
    let original = app.world().resource::<EditorSession>().effect.clone();
    open_folder(&mut app, &destination.path().join("missing"));
    io::drain(app.world_mut());
    assert_eq!(app.world().resource::<EditorSession>().effect, original);
    open_folder(&mut app, destination.path());
    let mut completion = io::prepared_completion(app.world_mut());
    assert_eq!(app.world().resource::<EditorSession>().effect, original);
    completion.apply(app.world_mut());
    let session = app.world().resource::<EditorSession>();
    assert_ne!(session.effect.id, original.id);
    assert!(session.source_path.is_none());
    assert!(session.dirty);
    let catalog = app.world().resource::<ProjectEffectCatalog>();
    assert_eq!(
        catalog.root(),
        destination.path().join("assets").canonicalize().unwrap()
    );
    assert_eq!(catalog.cached_effect(expected.id.into()).unwrap(), expected);
}

#[test]
fn partial_background_save_updates_the_effect_baseline_but_retains_material_failures() {
    use aestra_core::material::MaterialProgram;
    let directory = tempfile::tempdir().unwrap();
    let program = MaterialProgram::additive_sprite("Original").normalized();
    let path = directory.path().join("program.aestra.material.ron");
    program.save_ron(&path).unwrap();
    let original_bytes = fs::read(&path).unwrap();
    let mut app = app(directory.path());
    let mut replacement = program.clone();
    replacement.name = "Edited".into();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&program, &replacement)
        .unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .material_drafts
        .programs
        .get_mut(&program.id)
        .unwrap()
        .current
        .as_mut()
        .unwrap()
        .id = aestra_core::MaterialProgramId::from_u128(0);
    let drafts = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .material_drafts
        .clone();
    {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        session.set_material_drafts(drafts);
        session.adjust_effect_duration(0.5);
    }
    app.world_mut().trigger(DocumentAction::Save);
    io::drain(app.world_mut());
    let session = app.world().resource::<EditorSession>();
    assert!(session.dirty);
    assert!(!session.effect_is_dirty());
    assert_eq!(
        EffectAsset::load_ron(directory.path().join("current.aestra.ron")).unwrap(),
        session.effect
    );
    assert_eq!(session.material_drafts.count(), 1);
    assert_eq!(
        session.material_drafts,
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_drafts
    );
    assert_eq!(fs::read(path).unwrap(), original_bytes);
}

#[test]
fn production_poller_applies_background_completion() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    app.world_mut().trigger(DocumentAction::Save);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    app.world_mut().flush();
    while app.world().resource::<EditorSession>().dirty {
        app.world_mut().run_system_once(io::poll).unwrap();
        assert!(
            std::time::Instant::now() < deadline,
            "completion was not published"
        );
        std::thread::yield_now();
    }
    assert_eq!(
        EffectAsset::load_ron(directory.path().join("current.aestra.ron")).unwrap(),
        app.world().resource::<EditorSession>().effect
    );
}

#[test]
fn window_close_waits_for_an_unapplied_write() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    app.init_resource::<WorkspaceLayout>()
        .add_message::<WindowCloseRequested>()
        .add_message::<AppExit>();
    let primary = app.world_mut().spawn(PrimaryWindow).id();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    app.world_mut().trigger(DocumentAction::Save);
    let mut completion = io::prepared_completion(app.world_mut());
    app.world_mut()
        .write_message(WindowCloseRequested { window: primary });
    app.world_mut()
        .run_system_once(handle_window_close_requests)
        .unwrap();
    assert!(app.world().resource::<Messages<AppExit>>().is_empty());
    assert!(
        app.world()
            .resource::<DocumentProtectionState>()
            .pending
            .is_none()
    );
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("still running")
    );
    completion.apply(app.world_mut());
    assert!(!app.world().resource::<EditorSession>().dirty);
}

fn app(root: &Path) -> App {
    let path = root.join("current.aestra.ron");
    let mut session = test_support::session_with_timing_slack();
    session.save_as(&path).unwrap();
    let autosave = AutosaveState::new(&session, true);
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(ProjectEffectCatalog::scan(root))
        .init_resource::<crate::project_content::ProjectEffectWatchState>()
        .insert_resource(EditorSettings::default())
        .insert_resource(Localizer::new("en-US").unwrap())
        .insert_resource(RecoveryPersistence::for_test(root.join("recovery"), None))
        .insert_resource(autosave)
        .init_resource::<CurvesState>()
        .init_resource::<SourceNavigationState>()
        .init_resource::<DocumentProtectionState>()
        .insert_resource(TimelineState::framed(5.0))
        .add_observer(execute_document_action)
        .add_observer(resolve_document_protection);
    app
}

fn target(root: &Path, name: &str) -> EffectAsset {
    let effect = EffectAsset::new(name, 2.0);
    effect
        .save_ron(root.join(format!("{name}.aestra.ron")))
        .unwrap();
    effect
}

fn confirm(app: &mut App, action: DocumentProtectionAction) {
    let entity = app.world_mut().spawn(action).id();
    app.world_mut().trigger(Activate { entity });
    app.world_mut().flush();
}

#[test]
fn prepared_open_is_atomic_and_publication_never_reopens_the_disk() {
    let directory = tempfile::tempdir().unwrap();
    let target = target(directory.path(), "target");
    let mut app = app(directory.path());
    let original = app.world().resource::<EditorSession>().effect.clone();
    app.world_mut()
        .trigger(DocumentAction::OpenCatalog(target.id.into()));
    let mut completion = io::prepared_completion(app.world_mut());
    assert_eq!(app.world().resource::<EditorSession>().effect, original);
    // A prepared result is a coherent observation, not authorization for a later write.
    fs::remove_file(directory.path().join("target.aestra.ron")).unwrap();
    completion.apply(app.world_mut());
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect, target);
    assert!(session.preview.is_some());
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .cached_effect(target.id.into())
            .unwrap(),
        target
    );
    assert!(
        app.world_mut()
            .resource_mut::<EditorSession>()
            .save()
            .is_err()
    );
}

#[test]
fn discard_then_open_survives_viewport_sync_without_false_catalog_changes() {
    let directory = tempfile::tempdir().unwrap();
    let target = target(directory.path(), "target");
    let mut app = app(directory.path());
    crate::viewport::install_project_preview_test_runtime(&mut app);
    app.update();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    app.update();
    app.world_mut()
        .trigger(DocumentAction::OpenCatalog(target.id.into()));
    assert!(app.world().resource::<DocumentProtectionState>().is_open());
    confirm(&mut app, DocumentProtectionAction::Discard);
    let mut completion = io::prepared_completion(app.world_mut());
    // Production updates the viewport while the background worker is running.
    app.update();
    completion.apply(app.world_mut());
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect, target, "{}", session.status);
    assert!(!session.dirty);
}

#[test]
fn prepared_open_rejects_newer_edits_and_preserves_preview_and_navigation() {
    let directory = tempfile::tempdir().unwrap();
    let target = target(directory.path(), "target");
    let mut app = app(directory.path());
    app.world_mut()
        .trigger(DocumentAction::OpenCatalog(target.id.into()));
    let mut completion = io::prepared_completion(app.world_mut());
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    let edited = app.world().resource::<EditorSession>().effect.clone();
    let version = app
        .world()
        .resource::<crate::project_content::ProjectEffectWatchState>()
        .version();
    completion.apply(app.world_mut());
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect, edited);
    assert!(session.dirty);
    assert!(session.preview.is_some());
    assert_eq!(
        app.world()
            .resource::<crate::project_content::ProjectEffectWatchState>()
            .version(),
        version
    );
    assert!(
        !app.world()
            .resource::<SourceNavigationState>()
            .can_go_back()
    );
    assert!(session.status.contains("Unsaved edits were kept"));
}

#[test]
fn prepared_open_cannot_restore_a_previous_project() {
    let directory = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let target = target(directory.path(), "target");
    let mut app = app(directory.path());
    app.world_mut()
        .trigger(DocumentAction::OpenCatalog(target.id.into()));
    let mut completion = io::prepared_completion(app.world_mut());
    app.insert_resource(ProjectEffectCatalog::scan(other.path()));
    let root = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .root()
        .to_owned();
    completion.apply(app.world_mut());
    assert_eq!(app.world().resource::<ProjectEffectCatalog>().root(), root);
    assert_ne!(app.world().resource::<EditorSession>().effect.id, target.id);
}

#[test]
fn failed_open_after_discard_keeps_the_original_dirty_document() {
    let directory = tempfile::tempdir().unwrap();
    let target = target(directory.path(), "target");
    let mut app = app(directory.path());
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    let original = app.world().resource::<EditorSession>().effect.clone();
    app.world_mut()
        .trigger(DocumentAction::OpenCatalog(target.id.into()));
    fs::remove_file(directory.path().join("target.aestra.ron")).unwrap();
    confirm(&mut app, DocumentProtectionAction::Discard);
    io::drain(app.world_mut());
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect, original);
    assert!(session.dirty);
    assert!(session.preview.is_some());
    assert!(session.status.contains("failed"));
}

#[test]
fn save_completion_keeps_newer_edits_undo_and_transport() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    let written = app.world().resource::<EditorSession>().effect.clone();
    app.world_mut().trigger(DocumentAction::Save);
    let mut completion = io::prepared_completion(app.world_mut());
    {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        session.adjust_effect_duration(0.75);
        session.seek_time(0.7);
        session.playing = false;
    }
    let edited = app.world().resource::<EditorSession>().effect.clone();
    completion.apply(app.world_mut());
    let mut session = app.world_mut().resource_mut::<EditorSession>();
    assert_eq!(session.effect, edited);
    assert!(session.dirty);
    assert_eq!(session.clock.time(session.playback_duration()), 0.7);
    assert!(!session.playing);
    assert_eq!(
        EffectAsset::load_ron(directory.path().join("current.aestra.ron")).unwrap(),
        written
    );
    session.undo();
    assert_eq!(session.effect, written);
    assert!(!session.dirty);
}

#[test]
fn save_receipt_does_not_authorize_an_external_comment_written_after_it() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    app.world_mut().trigger(DocumentAction::Save);
    let mut completion = io::prepared_completion(app.world_mut());
    let path = directory.path().join("current.aestra.ron");
    let external = format!("// external\n{}", fs::read_to_string(&path).unwrap());
    fs::write(&path, &external).unwrap();
    completion.apply(app.world_mut());
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    app.world_mut().trigger(DocumentAction::Save);
    io::drain(app.world_mut());
    assert_eq!(fs::read_to_string(&path).unwrap(), external);
    let session = app.world().resource::<EditorSession>();
    assert!(session.dirty);
    assert!(session.status.contains("changed outside"));
}

#[test]
fn save_then_open_reenters_protection_for_newer_edits() {
    let directory = tempfile::tempdir().unwrap();
    let target = target(directory.path(), "target");
    let mut app = app(directory.path());
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    let action = DocumentAction::OpenCatalog(target.id.into());
    app.world_mut().trigger(action);
    confirm(&mut app, DocumentProtectionAction::Save);
    let mut completion = io::prepared_completion(app.world_mut());
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    completion.apply(app.world_mut());
    app.world_mut().flush();
    assert_eq!(
        app.world().resource::<DocumentProtectionState>().pending,
        Some(action)
    );
    assert_ne!(app.world().resource::<EditorSession>().effect.id, target.id);
    confirm(&mut app, DocumentProtectionAction::Discard);
    io::drain(app.world_mut());
    assert_eq!(app.world().resource::<EditorSession>().effect.id, target.id);
}

#[test]
fn cancelling_pending_navigation_does_not_cancel_its_in_flight_save() {
    let directory = tempfile::tempdir().unwrap();
    let target = target(directory.path(), "target");
    let mut app = app(directory.path());
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    let original = app.world().resource::<EditorSession>().effect.clone();
    app.world_mut()
        .trigger(DocumentAction::OpenCatalog(target.id.into()));
    confirm(&mut app, DocumentProtectionAction::Save);
    let mut completion = io::prepared_completion(app.world_mut());
    confirm(&mut app, DocumentProtectionAction::Cancel);
    completion.apply(app.world_mut());
    app.world_mut().flush();
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect, original);
    assert!(!session.dirty);
    assert!(
        app.world()
            .resource::<DocumentProtectionState>()
            .pending
            .is_none()
    );
}

#[test]
fn background_navigation_restores_parent_playhead_selection_and_forward_history() {
    let directory = tempfile::tempdir().unwrap();
    let target = target(directory.path(), "target");
    let mut app = app(directory.path());
    let id = app.world().resource::<EditorSession>().effect.emitters[1].id;
    {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        session.select_emitter(id);
        session.seek_time(0.6);
        session.playing = false;
    }
    app.world_mut()
        .trigger(DocumentAction::OpenSource(target.id.into()));
    io::drain(app.world_mut());
    assert_eq!(app.world().resource::<EditorSession>().effect.id, target.id);
    app.world_mut().trigger(DocumentAction::BackToSource);
    io::drain(app.world_mut());
    let session = app.world().resource::<EditorSession>();
    assert_eq!(
        session.selection.primary,
        aestra_authoring::SemanticTarget::Emitter(id)
    );
    assert_eq!(session.clock.time(session.playback_duration()), 0.6);
    assert!(!session.playing);
    assert!(
        app.world()
            .resource::<SourceNavigationState>()
            .can_go_forward()
    );
    app.world_mut().trigger(DocumentAction::ForwardToSource);
    io::drain(app.world_mut());
    assert_eq!(app.world().resource::<EditorSession>().effect.id, target.id);
}

#[test]
fn a_second_explicit_operation_cannot_replace_a_completed_unapplied_write() {
    let directory = tempfile::tempdir().unwrap();
    let target = target(directory.path(), "target");
    let mut app = app(directory.path());
    app.world_mut()
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    let id = app.world().resource::<EditorSession>().effect.id;
    app.world_mut().trigger(DocumentAction::Save);
    let mut completion = io::prepared_completion(app.world_mut());
    app.world_mut()
        .trigger(DocumentAction::OpenCatalog(target.id.into()));
    completion.apply(app.world_mut());
    app.world_mut().flush();
    assert_eq!(app.world().resource::<EditorSession>().effect.id, id);
    assert!(!app.world().resource::<EditorSession>().dirty);
    assert!(
        app.world()
            .resource::<DocumentProtectionState>()
            .pending
            .is_none()
    );
}
