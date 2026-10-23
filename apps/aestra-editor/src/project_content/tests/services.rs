use super::*;
use crate::{
    localization::Localizer, project_content::EditorProjectContent, session::EditorSession,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn fixture(root: &Path) -> App {
    let path = root.join("open.aestra.ron");
    let session = crate::test_support::session_with_source_path(&path);
    session.effect.save_ron(&path).unwrap();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(250));
    let mut app = App::new();
    app.add_plugins(bevy::app::TaskPoolPlugin::default())
        .insert_resource(time)
        .insert_resource(EditorProjectContent::scan(root))
        .insert_resource(session)
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_plugins(ProjectContentServicesPlugin);
    app
}

fn until(app: &mut App, done: impl Fn(&World) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        app.update();
        if done(app.world()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "background project service did not settle"
        );
        std::thread::yield_now();
    }
}

#[test]
fn background_services_publish_generic_files_without_invalidating_preview() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = fixture(directory.path());
    let original_revision = app.world().resource::<EditorSession>().document_revision();
    let original_ui = app.world().resource::<EditorSession>().ui_revision;
    let original = app
        .world()
        .resource::<EditorSession>()
        .preview()
        .unwrap()
        .effect()
        .clone();
    std::fs::write(directory.path().join("notes.txt"), "new generic file").unwrap();
    until(&mut app, |world| {
        world
            .resource::<EditorProjectContent>()
            .content()
            .source_tree()
            .at_relative_path("notes.txt")
            .is_some()
    });
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.document_revision(), original_revision);
    assert_eq!(session.ui_revision, original_ui);
    assert!(std::sync::Arc::ptr_eq(
        &original,
        session.preview().unwrap().effect()
    ));
}

#[test]
fn background_services_reload_clean_sources_from_real_worker_results() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = fixture(directory.path());
    let mut external = app.world().resource::<EditorSession>().effect.clone();
    external.name = "Externally edited on disk".into();
    external
        .save_ron(directory.path().join("open.aestra.ron"))
        .unwrap();
    until(&mut app, |world| {
        world.resource::<EditorSession>().effect == external
    });
    assert!(!app.world().resource::<EditorSession>().dirty);
    assert_eq!(
        app.world()
            .resource::<refresh::ProjectEffectWatchState>()
            .version(),
        app.world()
            .resource::<EditorProjectContent>()
            .content_revision()
    );
}

#[test]
fn explicit_io_completion_is_applied_before_watch_accepts_published_catalog() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = fixture(directory.path());
    let captured = io::IoGuard::capture(
        app.world().resource::<EditorProjectContent>(),
        app.world().resource::<EditorSession>(),
    );
    let root = directory.path().to_owned();
    io::enqueue(&mut app.world_mut().commands(), captured, move || {
        std::fs::write(root.join("worker.txt"), "completed write").unwrap();
        let prepared = EditorProjectContent::scan(&root);
        io::completion(move |world| {
            assert!(io::idle_world(world));
            io::publish_catalog(world, prepared);
        })
    });
    app.world_mut().flush();
    assert!(!io::idle_world(app.world()));
    until(&mut app, io::idle_world);
    let catalog = app.world().resource::<EditorProjectContent>();
    assert!(
        catalog
            .content()
            .source_tree()
            .at_relative_path("worker.txt")
            .is_some()
    );
    assert_eq!(
        app.world()
            .resource::<refresh::ProjectEffectWatchState>()
            .version(),
        catalog.content_revision()
    );
}

#[test]
fn project_switch_cancels_old_watch_and_adopts_new_root_without_reloading_document() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut app = fixture(first.path());
    let original = app.world().resource::<EditorSession>().effect.clone();
    std::fs::write(first.path().join("old.txt"), "old root").unwrap();
    // First update dispatches the old-root scan; it cannot publish until a later update.
    app.update();
    app.insert_resource(EditorProjectContent::scan(second.path()));
    app.update();
    assert_eq!(
        app.world().resource::<EditorProjectContent>().root(),
        second.path()
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, original);
    std::fs::write(second.path().join("new.txt"), "new root").unwrap();
    until(&mut app, |world| {
        world
            .resource::<EditorProjectContent>()
            .content()
            .source_tree()
            .at_relative_path("new.txt")
            .is_some()
    });
    assert!(
        app.world()
            .resource::<EditorProjectContent>()
            .content()
            .source_tree()
            .at_relative_path("old.txt")
            .is_none()
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, original);
}

#[test]
fn background_reload_preserves_dirty_document_and_reports_conflict() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = fixture(directory.path());
    let mut external = app.world().resource::<EditorSession>().effect.clone();
    external.name = "External edit".into();
    external
        .save_ron(directory.path().join("open.aestra.ron"))
        .unwrap();
    {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        session.effect.name = "Unsaved local edit".into();
        session.dirty = true;
    }
    let original = app
        .world()
        .resource::<EditorProjectContent>()
        .content_revision();
    until(&mut app, |world| {
        world.resource::<EditorProjectContent>().content_revision() != original
    });
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect.name, "Unsaved local edit");
    assert!(session.dirty);
    assert_eq!(
        session.status,
        app.world()
            .resource::<Localizer>()
            .text("library-status-source-conflict")
    );
}

#[test]
fn watch_waits_until_explicit_io_completion_is_applied() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = fixture(directory.path());
    let original = app.world().resource::<EditorSession>().effect.clone();
    let version = app
        .world()
        .resource::<EditorProjectContent>()
        .content_revision();
    let captured = io::IoGuard::capture(
        app.world().resource::<EditorProjectContent>(),
        app.world().resource::<EditorSession>(),
    );
    io::enqueue(&mut app.world_mut().commands(), captured, || {
        io::completion(|_| {})
    });
    let mut completion = io::prepared_completion(app.world_mut());
    let mut external = original.clone();
    external.name = "Must wait for I/O application".into();
    external
        .save_ron(directory.path().join("open.aestra.ron"))
        .unwrap();
    for _ in 0..20 {
        app.update();
    }
    assert!(!io::idle_world(app.world()));
    assert_eq!(app.world().resource::<EditorSession>().effect, original);
    assert_eq!(
        app.world()
            .resource::<EditorProjectContent>()
            .content_revision(),
        version
    );
    completion.apply(app.world_mut());
    until(&mut app, |world| {
        world.resource::<EditorSession>().effect == external
    });
}

#[test]
fn invalid_external_source_keeps_preview_and_recovers_on_requested_refresh() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = fixture(directory.path());
    let original = app.world().resource::<EditorSession>().effect.clone();
    let preview = app
        .world()
        .resource::<EditorSession>()
        .preview()
        .unwrap()
        .effect()
        .clone();
    let version = app
        .world()
        .resource::<EditorProjectContent>()
        .content_revision();
    std::fs::write(directory.path().join("open.aestra.ron"), "malformed effect").unwrap();
    until(&mut app, |world| {
        world.resource::<EditorProjectContent>().content_revision() != version
    });
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect, original);
    assert!(std::sync::Arc::ptr_eq(
        &preview,
        session.preview().unwrap().effect()
    ));
    assert!(session.status.contains("reload"), "{}", session.status);
    let mut repaired = original;
    repaired.name = "Repaired source".into();
    repaired
        .save_ron(directory.path().join("open.aestra.ron"))
        .unwrap();
    app.world_mut()
        .resource_mut::<refresh::ProjectEffectWatchState>()
        .request_refresh();
    until(&mut app, |world| {
        world.resource::<EditorSession>().effect == repaired
    });
    assert!(!app.world().resource::<EditorSession>().dirty);
}
