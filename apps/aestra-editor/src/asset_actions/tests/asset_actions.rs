use super::*;
use crate::{session::blank_effect, test_support};
use aestra_core::{AssetDefinition, ChoreographyTrackId, EffectAssetRef, Emitter, Value};
use bevy::{asset::AssetPlugin, scene::ScenePlugin, text::TextPlugin};

#[derive(Resource, Default)]
struct CapturedAssetAction(Option<AssetAction>);

fn capture_asset_action(action: On<AssetAction>, mut captured: ResMut<CapturedAssetAction>) {
    captured.0 = Some(*action);
}

fn asset_action_test_app(session: EditorSession, catalog: ProjectEffectCatalog) -> App {
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(catalog)
        .init_resource::<CurvesState>()
        .init_resource::<WorkspaceLayout>()
        .init_resource::<MenuState>()
        .init_resource::<ModulePaletteState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_plugins((EditorProjectContentPlugin, EditorAssetActionsPlugin));
    app
}

fn spawn_test_asset_operation_overlay(
    mut commands: Commands,
    state: Res<AssetOperationState>,
    localizer: Res<Localizer>,
) {
    commands.spawn(Node::default()).with_children(|parent| {
        spawn_asset_operation_overlay(parent, &state, &localizer);
    });
}

#[test]
fn extraction_action_confirmation_and_undo_work_without_library() {
    let root = tempfile::tempdir().unwrap();
    let mut effect = EffectAsset::new("Owner", 2.0);
    effect.emitters.push(Emitter::basic_sprite("Selected", 1.0));
    let emitter = effect.emitters[0].id;
    let mut session = test_support::session_from_effect_with_source_path(
        effect,
        root.path().join("owner.aestra.ron"),
    );
    session.select_emitter(emitter);
    let before = session.effect.clone();
    let mut app = asset_action_test_app(session, ProjectEffectCatalog::scan(root.path()));
    app.world_mut()
        .trigger(AssetAction::CreateReusableEffectFromSelection);
    {
        let mut state = app.world_mut().resource_mut::<AssetOperationState>();
        let extraction = state.extraction.as_mut().expect("shared extraction dialog");
        assert_eq!(extraction.emitters, vec![emitter]);
        extraction.draft = "Reusable Burst".into();
    }
    let confirm = app
        .world_mut()
        .spawn(AssetOperationAction::ConfirmReusableEffectExtraction)
        .id();
    app.world_mut().trigger(Activate { entity: confirm });
    crate::project_content::io::drain(app.world_mut());
    assert!(root.path().join("reusable_burst.aestra.ron").exists());
    assert!(!app.world().resource::<AssetOperationState>().is_open());
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .effect
            .effect_clips
            .len(),
        1
    );
    app.world_mut().resource_mut::<EditorSession>().undo();
    assert_eq!(app.world().resource::<EditorSession>().effect, before);
    assert!(
        root.path().join("reusable_burst.aestra.ron").exists(),
        "Undo restores the document, preserving the reusable source"
    );
}

#[test]
fn shared_dialog_blocks_shortcuts_and_escape_cancels_without_library() {
    use bevy::ecs::system::RunSystemOnce;
    let root = tempfile::tempdir().unwrap();
    let mut session = test_support::session_with_timing_slack();
    let emitter = session.effect.emitters[0].id;
    session.select_emitter(emitter);
    let before = session.effect.clone();
    let mut app = asset_action_test_app(session, ProjectEffectCatalog::scan(root.path()));
    app.world_mut()
        .trigger(AssetAction::CreateReusableEffectFromSelection);
    assert!(
        app.world_mut()
            .run_system_once(|context: crate::input::ShortcutContext| context.blocked())
            .unwrap()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert!(!app.world().resource::<AssetOperationState>().is_open());
    assert!(
        !app.world_mut()
            .run_system_once(|context: crate::input::ShortcutContext| context.blocked())
            .unwrap()
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, before);
}

#[test]
fn asset_actions_work_without_library() {
    let session = test_support::session_with_timing_slack();
    let root = tempfile::tempdir().unwrap();
    let mut app = asset_action_test_app(session, ProjectEffectCatalog::scan(root.path()));
    let control = app
        .world_mut()
        .spawn((
            Button,
            FeathersActionButton,
            Interaction::None,
            AssetAction::CreateReusableEffectFromSelection,
            BackgroundColor::default(),
        ))
        .id();

    app.world_mut().trigger(Activate { entity: control });
    app.update();

    assert!(app.world().contains_resource::<ProjectEffectCatalog>());
    assert!(
        !app.world()
            .entity(control)
            .contains::<PendingFeathersActivation>()
    );
    assert!(app.world().resource::<AssetOperationState>().is_open());
}

#[test]
fn context_menu_asset_actions_dispatch_without_a_background_component() {
    let clip = EffectClipId::new();
    let mut app = App::new();
    app.insert_resource(test_support::session_with_timing_slack())
        .init_resource::<MenuState>()
        .init_resource::<CapturedAssetAction>()
        .add_observer(queue_asset_action_activation)
        .add_observer(capture_asset_action)
        .add_systems(Update, handle_asset_action_buttons);
    let item = app
        .world_mut()
        .spawn((
            Interaction::None,
            AssetAction::ExplodeEffectClip(clip),
            FeathersActionButton,
        ))
        .id();

    app.world_mut().trigger(Activate { entity: item });
    app.update();

    assert_eq!(
        app.world().resource::<CapturedAssetAction>().0,
        Some(AssetAction::ExplodeEffectClip(clip))
    );
}

#[test]
fn reusable_effect_extraction_overlay_tracks_modal_state_without_query_conflicts() {
    let state = AssetOperationState {
        extraction: Some(ReusableEffectExtractionState {
            emitters: vec![EmitterId::new()],
            draft: "Reusable Burst".into(),
            replace_selection: true,
            error: Some("Choose another name".into()),
        }),
    };
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        ScenePlugin,
        TextPlugin,
    ))
    .insert_resource(state)
    .insert_resource(ProjectEffectCatalog::from_entries(Vec::new()))
    .insert_resource(Localizer::new("en-US").unwrap())
    .add_systems(Startup, spawn_test_asset_operation_overlay)
    .add_systems(Update, sync_asset_operation_overlay);

    app.update();

    let world = app.world_mut();
    let overlay = world
        .query_filtered::<&Node, With<AssetOperationOverlay>>()
        .single(world)
        .unwrap();
    assert_eq!(overlay.display, Display::Flex);
    let extraction = world
        .query_filtered::<&Node, With<ReusableEffectExtractionDialog>>()
        .single(world)
        .unwrap();
    assert_eq!(extraction.display, Display::Flex);
    let (error, error_node) = world
        .query_filtered::<(&Text, &Node), With<ReusableEffectExtractionError>>()
        .single(world)
        .unwrap();
    assert_eq!(error.0, "Choose another name");
    assert_eq!(error_node.display, Display::Flex);

    world.resource_mut::<AssetOperationState>().extraction = None;
    app.update();

    let world = app.world_mut();
    let overlay = world
        .query_filtered::<&Node, With<AssetOperationOverlay>>()
        .single(world)
        .unwrap();
    assert_eq!(overlay.display, Display::None);
    let extraction = world
        .query_filtered::<&Node, With<ReusableEffectExtractionDialog>>()
        .single(world)
        .unwrap();
    assert_eq!(extraction.display, Display::None);
}

#[test]
fn explode_replaces_the_clip_with_editable_emitters_and_is_undoable() {
    let temporary = tempfile::tempdir().unwrap();
    let child_path = temporary.path().join("child.aestra.ron");
    let mut child = EffectAsset::new("Child", 2.0);
    let texture = AssetDefinition::texture("Child Texture", "textures/child.png");
    let texture_id = texture.id;
    child.assets.push(texture);
    let MaterialProperties::Sprite { texture, .. } = &mut child.materials[0].properties;
    *texture = Some(texture_id);
    child
        .emitters
        .push(Emitter::basic_sprite("First", child.duration));
    let parameter = aestra_core::ParameterId::new();
    child.parameters.push(aestra_core::EffectParameter {
        id: parameter,
        name: "Intensity".into(),
        default: Value::Scalar(1.0),
        exposed: true,
    });
    child.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == aestra_core::MODULE_EMISSION)
        .unwrap()
        .bindings
        .insert("spawn_rate".into(), parameter);
    let mut second = child.emitters[0].clone();
    second.regenerate_ids();
    second.name = "Second".into();
    second.transform.translation = [1.0, 0.0, 0.0];
    child.emitters.push(second);
    child.save_ron(&child_path).unwrap();
    let child_reference = EffectAssetRef::new(child.id);

    let owner = blank_effect();
    let mut session = test_support::session_from_effect_with_source_path(
        owner,
        temporary.path().join("owner.aestra.ron"),
    );
    let mut clip = aestra_core::EffectClip::new(child_reference, 0.25, 1.0);
    let clip_id = clip.id;
    clip.transform.translation = [2.0, 0.0, 0.0];
    clip.parameter_overrides
        .insert(parameter, Value::Scalar(3.5));
    session.effect.effect_clips.push(clip.clone());
    session.effect.choreography_order = vec![ChoreographyTrackId::EffectClip(clip_id)];
    let original_emitter_count = session.effect.emitters.len();
    let original_asset_count = session.effect.assets.len();
    let catalog = ProjectEffectCatalog::scan(temporary.path());
    let catalog_entries = catalog.entries().len();
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(catalog)
        .init_resource::<CurvesState>()
        .init_resource::<WorkspaceLayout>()
        .init_resource::<MenuState>()
        .init_resource::<ModulePaletteState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_plugins((EditorProjectContentPlugin, EditorAssetActionsPlugin));

    app.world_mut()
        .trigger(AssetAction::ExplodeEffectClip(clip_id));

    let session = app.world().resource::<EditorSession>();
    assert!(
        !session
            .effect
            .effect_clips
            .iter()
            .any(|candidate| candidate.id == clip_id),
        "{}",
        session.status
    );
    let local_emitters = &session.effect.emitters[original_emitter_count..];
    assert_eq!(local_emitters.len(), 2);
    assert_eq!(local_emitters[0].name, "First");
    assert_eq!(local_emitters[1].name, "Second");
    assert_eq!(local_emitters[0].start_time, clip.start_time);
    assert_eq!(local_emitters[0].duration, clip.duration);
    assert_eq!(local_emitters[0].transform.translation, [2.0, 0.0, 0.0]);
    assert_eq!(local_emitters[1].transform.translation, [3.0, 0.0, 0.0]);
    assert_eq!(session.effect.assets.len(), original_asset_count + 1);
    assert_ne!(session.effect.assets.last().unwrap().id, texture_id);
    let local_parameter = session.effect.parameters.last().unwrap();
    assert_ne!(local_parameter.id, parameter);
    assert_eq!(local_parameter.default, Value::Scalar(3.5));
    assert!(!local_parameter.exposed);
    assert_eq!(
        local_emitters[0]
            .modules
            .iter()
            .find(|module| module.module_type.0 == aestra_core::MODULE_EMISSION)
            .unwrap()
            .bindings["spawn_rate"],
        local_parameter.id
    );
    assert_eq!(
        session.effect.choreography_order[0],
        ChoreographyTrackId::Emitter(local_emitters[0].id)
    );
    assert!(
        session.status.contains("editable emitters"),
        "{}",
        session.status
    );
    assert!(!app.world().resource::<AssetOperationState>().is_open());
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .entries()
            .len(),
        catalog_entries
    );
    assert!(child_path.exists());

    assert!(app.world().resource::<EditorSession>().can_undo());
    app.world_mut().resource_mut::<EditorSession>().undo();
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .starts_with("Undid"),
        "{}",
        app.world().resource::<EditorSession>().status
    );
    let session = app.world().resource::<EditorSession>();
    let restored = session
        .effect
        .effect_clips
        .iter()
        .find(|candidate| candidate.id == clip_id)
        .unwrap();
    assert_eq!(restored.source, child_reference);
    assert_eq!(restored.parameter_overrides[&parameter], Value::Scalar(3.5));
    assert_eq!(session.effect.emitters.len(), original_emitter_count);
    assert_eq!(session.effect.assets.len(), original_asset_count);
    assert_eq!(
        session.effect.choreography_order,
        vec![ChoreographyTrackId::EffectClip(clip_id)]
    );
}
