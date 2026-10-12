use super::*;
use crate::{localization::Localizer, material_document::MaterialEditingTarget};
use aestra_core::{MaterialProgramId, material::MaterialProgram};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn fixture(root: &std::path::Path) -> World {
    let mut world = World::new();
    world.insert_resource(EditorProjectContent::scan(root));
    world.insert_resource(crate::test_support::session_with_timing_slack());
    world.insert_resource(Localizer::new("en-US").unwrap());
    world
}

fn guard(world: &World) -> IoGuard {
    IoGuard::capture(
        world.resource::<EditorProjectContent>(),
        world.resource::<EditorSession>(),
    )
}

#[derive(Resource, Default)]
struct Rejected(usize);

#[test]
fn worker_disk_write_is_drained_and_busy_lasts_until_application() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("written.txt");
    let mut world = fixture(directory.path());
    assert!(idle_world(&world));
    let captured = guard(&world);
    let output = path.clone();
    enqueue(&mut world.commands(), captured, move || {
        std::fs::write(output, "worker result").unwrap();
        completion(|world| world.insert_resource(Rejected(1)))
    });
    world.flush();
    assert!(!idle_world(&world));
    let mut queue = prepared_completion(&mut world);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "worker result");
    assert!(!idle_world(&world));
    assert!(!world.contains_resource::<Rejected>());
    queue.apply(&mut world);
    assert!(idle_world(&world));
    assert_eq!(world.resource::<Rejected>().0, 1);
}

#[test]
fn busy_rejection_releases_modal_without_running_or_losing_either_worker() {
    let directory = tempfile::tempdir().unwrap();
    let mut world = fixture(directory.path());
    world.init_resource::<Rejected>();
    let count = Arc::new(AtomicUsize::new(0));
    let worker = count.clone();
    let captured = guard(&world);
    enqueue(&mut world.commands(), captured, move || {
        worker.fetch_add(1, Ordering::SeqCst);
        completion(|_| {})
    });
    let mut queue = prepared_completion(&mut world);
    let worker = count.clone();
    let captured = guard(&world);
    enqueue_with_rejection(
        &mut world.commands(),
        captured,
        move || {
            worker.fetch_add(100, Ordering::SeqCst);
            completion(|_| {})
        },
        |world| world.resource_mut::<Rejected>().0 += 1,
    );
    world.flush();
    assert_eq!(world.resource::<Rejected>().0, 1);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(!idle_world(&world));
    queue.apply(&mut world);
    let worker = count.clone();
    let captured = guard(&world);
    enqueue(&mut world.commands(), captured, move || {
        worker.fetch_add(1, Ordering::SeqCst);
        completion(|_| {})
    });
    drain(&mut world);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert!(idle_world(&world));
}

#[test]
fn deferred_enqueue_rechecks_document_and_releases_rejected_modal() {
    let directory = tempfile::tempdir().unwrap();
    let mut world = fixture(directory.path());
    world.init_resource::<Rejected>();
    let count = Arc::new(AtomicUsize::new(0));
    let worker = count.clone();
    let captured = guard(&world);
    enqueue_with_rejection(
        &mut world.commands(),
        captured,
        move || {
            worker.fetch_add(1, Ordering::SeqCst);
            completion(|_| {})
        },
        |world| world.resource_mut::<Rejected>().0 += 1,
    );
    world.resource_mut::<EditorSession>().effect.name = "Newer edit".into();
    world.flush();
    assert_eq!(world.resource::<Rejected>().0, 1);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(idle_world(&world));
    assert_eq!(
        world.resource::<EditorSession>().status,
        world
            .resource::<Localizer>()
            .text("project-operation-queued-cancelled")
    );
}

#[test]
fn guards_separate_project_document_and_exact_authored_state() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut world = fixture(first.path());
    let captured = guard(&world);
    let matches = |world: &World| {
        captured.matches(
            world.resource::<EditorProjectContent>(),
            world.resource::<EditorSession>(),
        )
    };
    assert!(matches(&world));
    world.resource_mut::<EditorSession>().material_target = MaterialEditingTarget::Program {
        root: first.path().into(),
        id: MaterialProgramId::new(),
    };
    assert!(!matches(&world));
    assert!(captured.same_document(
        world.resource::<EditorProjectContent>(),
        world.resource::<EditorSession>()
    ));
    world.resource_mut::<EditorSession>().material_target = MaterialEditingTarget::EffectInstance;
    let emitter = world.resource::<EditorSession>().effect.emitters[0].id;
    world
        .resource_mut::<EditorSession>()
        .locks
        .lock(aestra_authoring::SemanticTarget::Emitter(emitter));
    assert!(!matches(&world));
    world.resource_mut::<EditorSession>().locks = Default::default();
    assert!(matches(&world));
    world.resource_mut::<EditorProjectContent>().refresh();
    assert!(captured.same_project(world.resource::<EditorProjectContent>()));
    assert!(!matches(&world));
    world.insert_resource(EditorProjectContent::scan(second.path()));
    assert!(!captured.same_project(world.resource::<EditorProjectContent>()));
    assert!(!captured.same_document(
        world.resource::<EditorProjectContent>(),
        world.resource::<EditorSession>()
    ));
}

#[test]
fn material_reload_allows_preview_revision_but_not_pending_proposals_or_reopened_documents() {
    let directory = tempfile::tempdir().unwrap();
    let mut world = fixture(directory.path());
    let captured = guard(&world);
    let effect = world.resource::<EditorSession>().effect.clone();
    let compiled = world
        .resource::<EditorProjectContent>()
        .compile_project(&effect)
        .unwrap();
    world
        .resource_mut::<EditorSession>()
        .install_compiled_project_root(compiled.root)
        .unwrap();
    assert!(!captured.matches(
        world.resource::<EditorProjectContent>(),
        world.resource::<EditorSession>()
    ));
    assert!(captured.matches_material_reload(
        world.resource::<EditorProjectContent>(),
        world.resource::<EditorSession>()
    ));
    assert!(world.resource_mut::<EditorSession>().preview_transaction(
        aestra_authoring::EffectTransaction::single(
            "Candidate",
            aestra_authoring::EffectCommand::SetEffectName {
                name: "Candidate".into()
            },
        )
    ));
    assert!(!captured.matches_material_reload(
        world.resource::<EditorProjectContent>(),
        world.resource::<EditorSession>()
    ));
    world
        .resource_mut::<EditorSession>()
        .discard_pending_change();
    let path = directory.path().join("open.aestra.ron");
    world.resource_mut::<EditorSession>().source_path = Some(path.clone());
    let captured = guard(&world);
    let compiled = world
        .resource::<EditorProjectContent>()
        .compile_project(&effect)
        .unwrap();
    world
        .resource_mut::<EditorSession>()
        .open_compiled_effect(path, effect, compiled.root);
    assert!(!captured.same_document(
        world.resource::<EditorProjectContent>(),
        world.resource::<EditorSession>()
    ));
}

#[test]
fn catalog_publication_keeps_live_drafts_and_resets_watch_baseline() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("program.aestra.material.ron");
    let original = MaterialProgram::additive_sprite("Original").normalized();
    original.save_ron(&path).unwrap();
    let mut world = fixture(directory.path());
    world.init_resource::<ProjectEffectWatchState>();
    let mut prepared = world.resource::<EditorProjectContent>().clone();
    prepared.refresh();
    let mut draft = original.clone();
    draft.name = "Live draft while worker runs".into();
    world
        .resource_mut::<EditorProjectContent>()
        .replace_material_program(&original, &draft)
        .unwrap();
    let captured = guard(&world);
    let mut newer = draft.clone();
    newer.name = "Newer draft".into();
    world
        .resource_mut::<EditorProjectContent>()
        .replace_material_program(&draft, &newer)
        .unwrap();
    assert!(!captured.matches_material_reload(
        world.resource::<EditorProjectContent>(),
        world.resource::<EditorSession>()
    ));
    let live = world
        .resource::<EditorProjectContent>()
        .material_drafts
        .clone();
    publish_catalog(&mut world, prepared);
    assert_eq!(
        world.resource::<EditorProjectContent>().material_drafts,
        live
    );
    assert_eq!(
        world.resource::<ProjectEffectWatchState>().version(),
        world.resource::<EditorProjectContent>().content_revision()
    );
    assert_eq!(MaterialProgram::load_ron(path).unwrap(), original);
}
