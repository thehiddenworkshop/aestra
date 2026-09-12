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
    world.init_resource::<AssetOperationState>();
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
    world.resource_mut::<AssetOperationState>().extraction = Some(extraction.clone());
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
    assert!(world.resource::<AssetOperationState>().extraction.is_none());
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
