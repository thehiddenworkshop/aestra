use super::*;
use crate::test_support;
use aestra_authoring::SemanticTarget;
use bevy::ecs::system::RunSystemOnce;

fn world(root: &std::path::Path) -> World {
    let mut session = test_support::session_with_timing_slack();
    session.save_as(root.join("current.aestra.ron")).unwrap();
    let mut world = World::new();
    world.insert_resource(session);
    world.insert_resource(ProjectEffectCatalog::scan(root));
    world.init_resource::<LibraryAssetOperationState>();
    world.insert_resource(Localizer::new("en-US").unwrap());
    world
}

fn start(world: &mut World, action: SourceAction) {
    let mut action = Some(action);
    world
        .run_system_once(
            move |mut commands: Commands,
                  catalog: Res<ProjectEffectCatalog>,
                  session: Res<EditorSession>,
                  locale: Res<Localizer>| {
                queue(
                    &mut commands,
                    action.take().unwrap(),
                    &catalog,
                    &session,
                    &locale,
                );
            },
        )
        .unwrap();
}

#[test]
fn background_rename_keeps_properties_changed_before_publication() {
    let directory = tempfile::tempdir().unwrap();
    let mut world = world(directory.path());
    let source = world.resource::<ProjectEffectCatalog>().entries()[0].id;
    start(
        &mut world,
        SourceAction::Rename {
            rename: LibraryRenameState {
                source,
                draft: "Renamed".into(),
                error: None,
            },
            current: true,
        },
    );
    let mut completion = io::prepared_completion(&mut world);
    world
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    let edited_duration = world.resource::<EditorSession>().effect.duration;
    completion.apply(&mut world);
    let session = world.resource::<EditorSession>();
    assert_eq!(session.effect.name, "Renamed");
    assert_eq!(session.effect.duration, edited_duration);
    assert!(session.dirty);
    assert_eq!(
        session.source_path.as_deref(),
        Some(directory.path().join("renamed.aestra.ron").as_path())
    );
    world.resource_mut::<EditorSession>().save().unwrap();
    assert_eq!(
        EffectAsset::load_ron(directory.path().join("renamed.aestra.ron"))
            .unwrap()
            .duration,
        edited_duration
    );
}

#[test]
fn background_move_updates_location_but_keeps_dirty_edits_and_preview() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("nested");
    fs::create_dir(&destination).unwrap();
    let mut world = world(directory.path());
    let source = world.resource::<ProjectEffectCatalog>().entries()[0].id;
    world
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    let effect = world.resource::<EditorSession>().effect.clone();
    start(
        &mut world,
        SourceAction::Move {
            source,
            destination: destination.clone(),
            current: true,
        },
    );
    io::drain(&mut world);
    let session = world.resource::<EditorSession>();
    assert_eq!(session.effect, effect);
    assert!(session.dirty);
    assert!(session.preview.is_some());
    assert_eq!(
        session.source_path.as_deref(),
        Some(destination.join("current.aestra.ron").as_path())
    );
    world.resource_mut::<EditorSession>().save().unwrap();
}

fn extract(world: &mut World, replace: bool) {
    let extraction = ReusableEffectExtractionState {
        emitters: world
            .resource::<EditorSession>()
            .effect
            .emitters
            .iter()
            .map(|emitter| emitter.id)
            .collect(),
        draft: "Extracted".into(),
        replace_selection: replace,
        error: None,
    };
    world
        .resource_mut::<LibraryAssetOperationState>()
        .extraction = Some(extraction.clone());
    start(world, SourceAction::Extract(extraction));
}

#[test]
fn extraction_publishes_a_compiled_preview_and_keeps_existing_undo_history() {
    let directory = tempfile::tempdir().unwrap();
    let mut world = world(directory.path());
    let original = world.resource::<EditorSession>().effect.clone();
    world
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    let edited = world.resource::<EditorSession>().effect.clone();
    extract(&mut world, true);
    io::drain(&mut world);
    let session = world.resource::<EditorSession>();
    assert!(matches!(
        session.selection.primary,
        SemanticTarget::EffectClip(_)
    ));
    assert!(session.effect.emitters.is_empty());
    assert!(session.preview.is_some());
    assert!(directory.path().join("extracted.aestra.ron").exists());
    world.resource_mut::<EditorSession>().undo();
    assert_eq!(world.resource::<EditorSession>().effect, edited);
    world.resource_mut::<EditorSession>().undo();
    assert_eq!(world.resource::<EditorSession>().effect, original);
}

#[test]
fn extraction_never_replaces_a_newer_owner_edit() {
    let directory = tempfile::tempdir().unwrap();
    let mut world = world(directory.path());
    extract(&mut world, true);
    let mut completion = io::prepared_completion(&mut world);
    world
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    let edited = world.resource::<EditorSession>().effect.clone();
    completion.apply(&mut world);
    assert_eq!(world.resource::<EditorSession>().effect, edited);
    assert!(world.resource::<EditorSession>().preview.is_some());
    assert!(directory.path().join("extracted.aestra.ron").exists());
    assert!(
        world
            .resource::<LibraryAssetOperationState>()
            .extraction
            .is_none()
    );
}

#[test]
fn copying_a_reusable_source_does_not_replace_the_live_session() {
    let directory = tempfile::tempdir().unwrap();
    let mut world = world(directory.path());
    extract(&mut world, false);
    let mut completion = io::prepared_completion(&mut world);
    world
        .resource_mut::<EditorSession>()
        .adjust_effect_duration(0.5);
    let edited = world.resource::<EditorSession>().effect.clone();
    completion.apply(&mut world);
    assert_eq!(world.resource::<EditorSession>().effect, edited);
    assert!(world.resource::<EditorSession>().preview.is_some());
    assert!(directory.path().join("extracted.aestra.ron").exists());
}

#[test]
fn deletion_rechecks_current_usages_before_removing_the_source() {
    let directory = tempfile::tempdir().unwrap();
    let child = EffectAsset::new("Child", 1.0);
    let path = directory.path().join("child.aestra.ron");
    child.save_ron(&path).unwrap();
    let mut world = world(directory.path());
    let catalog = world.resource::<ProjectEffectCatalog>();
    let source = catalog
        .entries()
        .iter()
        .find(|entry| entry.reference == Some(child.id.into()))
        .unwrap()
        .id;
    let graph = catalog.effect_usage_graph(child.id.into()).unwrap();
    let mut owner = EffectAsset::new("New owner", 1.0);
    owner.effect_clips.push(EffectClip::new(child.id, 0.0, 1.0));
    owner
        .save_ron(directory.path().join("new_owner.aestra.ron"))
        .unwrap();
    let deletion = LibraryEffectDeletionState {
        source,
        graph,
        error: None,
    };
    world.resource_mut::<LibraryAssetOperationState>().deletion = Some(deletion.clone());
    start(&mut world, SourceAction::Delete(deletion));
    io::drain(&mut world);
    assert!(path.exists());
    assert!(
        world
            .resource::<LibraryAssetOperationState>()
            .deletion
            .as_ref()
            .unwrap()
            .error
            .is_some()
    );
}
