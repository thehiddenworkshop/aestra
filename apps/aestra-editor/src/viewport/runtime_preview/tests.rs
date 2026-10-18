//! Production project resolution and reconciliation with real session, catalog and player payloads.
use super::*;
use crate::test_support;
use aestra_authoring::{EffectCommand, EffectTransaction};
use aestra_core::{
    EffectAsset, EffectClip, EffectClipSeed, EffectPlaybackMode, Emitter, EmitterTransform, Value,
};

struct Fixture {
    app: App,
    directory: tempfile::TempDir,
    child: EffectAsset,
    clip: EffectClipId,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut child = EffectAsset::new("Child", 2.0);
        child
            .emitters
            .push(Emitter::basic_sprite("Child emitter", 2.0));
        child
            .save_ron(directory.path().join("child.aestra.ron"))
            .unwrap();
        let mut session = test_support::session_with_timing_slack();
        let mut clip = EffectClip::new(child.id, 0.5, 1.5);
        clip.source_offset = 0.1;
        clip.seed = EffectClipSeed::Fixed(77);
        clip.transform.translation = [5.0, 2.0, -1.0];
        let id = clip.id;
        assert!(session.execute(
            "Add child",
            EffectCommand::AddEffectClip { clip, index: 0 },
            false
        ));
        session.seek_time(0.75);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(ProjectEffectCatalog::scan(directory.path()))
            .init_resource::<EditorPreviewProject>()
            .init_resource::<TimelineState>()
            .add_systems(
                Update,
                (sync_project_preview, sync_rendered_preview).chain(),
            );
        Self {
            app,
            directory,
            child,
            clip: id,
        }
    }
    fn tick(&mut self) {
        self.app.update();
    }
    fn entity(&mut self, path: &[EffectClipId]) -> Entity {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &PreviewEffectInstancePath)>()
            .iter(world)
            .find(|(_, candidate)| candidate.0 == path)
            .map(|(entity, _)| entity)
            .expect("player path must exist")
    }
    fn player(&self, entity: Entity) -> &PresentedEffect {
        self.app.world().get::<PresentedEffect>(entity).unwrap()
    }
    fn seek(&mut self, time: f32) {
        self.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .seek_time(time);
        self.tick();
    }
}

#[test]
fn idle_resolution_retains_project_and_session_artifact_identity() {
    let mut fixture = Fixture::new();
    fixture.tick();
    let project = fixture
        .app
        .world()
        .resource::<EditorPreviewProject>()
        .project
        .clone()
        .unwrap();
    let source = fixture
        .app
        .world()
        .resource::<EditorPreviewProject>()
        .source_root
        .clone()
        .unwrap();
    let root = fixture.entity(&[]);
    let child = fixture.entity(&[fixture.clip]);
    for _ in 0..3 {
        fixture.tick();
    }
    let preview = fixture.app.world().resource::<EditorPreviewProject>();
    assert!(Arc::ptr_eq(preview.project.as_ref().unwrap(), &project));
    assert!(Arc::ptr_eq(preview.source_root.as_ref().unwrap(), &source));
    assert!(Arc::ptr_eq(fixture.player(root).effect(), &source));
    assert_eq!(fixture.entity(&[]), root);
    assert_eq!(fixture.entity(&[fixture.clip]), child);
}

#[test]
fn players_keep_required_components_layers_and_unowned_presentations() {
    let mut fixture = Fixture::new();
    let player =
        configured_preview_player(fixture.app.world().resource::<EditorSession>()).unwrap();
    let unrelated = fixture.app.world_mut().spawn(player).id();
    fixture.tick();
    for path in [vec![], vec![fixture.clip]] {
        let entity = fixture.entity(&path);
        assert_eq!(
            *fixture.app.world().get::<Transform>(entity).unwrap(),
            Transform::IDENTITY
        );
        assert!(fixture.app.world().get::<Visibility>(entity).is_some());
        assert_eq!(
            *fixture.app.world().get::<RenderLayers>(entity).unwrap(),
            RenderLayers::layer(0)
        );
    }
    fixture.seek(0.0);
    assert!(
        fixture
            .app
            .world()
            .get::<PresentedEffect>(unrelated)
            .is_some()
    );
    assert!(
        fixture
            .app
            .world()
            .get::<PreviewPresentedEffect>(unrelated)
            .is_none()
    );
}

#[test]
fn clip_exit_retires_only_child_and_reentry_spawns_a_new_player() {
    let mut fixture = Fixture::new();
    fixture.tick();
    let root = fixture.entity(&[]);
    let child = fixture.entity(&[fixture.clip]);
    fixture.seek(2.5);
    assert!(fixture.app.world().get_entity(child).is_err());
    assert_eq!(fixture.entity(&[]), root);
    fixture.seek(0.75);
    let replacement = fixture.entity(&[fixture.clip]);
    assert_ne!(replacement, child);
    assert_eq!(fixture.player(replacement).instance.seed(), 77);
    assert!((fixture.player(replacement).simulation_time() - 0.35).abs() < 0.0001);
}

#[test]
fn seek_discontinuities_request_preview_then_settle_to_exact_without_replacing_entities() {
    let mut fixture = Fixture::new();
    fixture.tick();
    let root = fixture.entity(&[]);
    fixture.tick();
    assert_eq!(fixture.player(root).seek_quality(), SeekQuality::Exact);
    let epoch = fixture.player(root).instance.history_epoch();
    fixture.seek(1.25);
    assert_eq!(fixture.entity(&[]), root);
    assert_eq!(fixture.player(root).seek_quality(), SeekQuality::Preview);
    assert_ne!(fixture.player(root).instance.history_epoch(), epoch);
    assert_eq!(fixture.player(root).simulation_time(), 1.25);
    let epoch = fixture.player(root).instance.history_epoch();
    fixture.tick();
    assert_eq!(fixture.player(root).seek_quality(), SeekQuality::Exact);
    assert_eq!(fixture.player(root).instance.history_epoch(), epoch);
}

#[test]
fn steady_playback_corrects_only_errors_larger_than_half_a_tick() {
    let mut fixture = Fixture::new();
    fixture.tick();
    fixture.tick();
    let root = fixture.entity(&[]);
    let time = fixture.player(root).simulation_time();
    let tolerance = 0.5 / DEFAULT_PLAYBACK_TICK_RATE as f32;
    fixture
        .app
        .world_mut()
        .get_mut::<PresentedEffect>(root)
        .unwrap()
        .instance
        .set_playback_time(time + tolerance * 0.5);
    let epoch = fixture.player(root).instance.history_epoch();
    fixture.tick();
    assert!((fixture.player(root).simulation_time() - (time + tolerance * 0.5)).abs() < 0.0001);
    assert_eq!(fixture.player(root).instance.history_epoch(), epoch);
    fixture
        .app
        .world_mut()
        .get_mut::<PresentedEffect>(root)
        .unwrap()
        .instance
        .set_playback_time(time + tolerance * 2.0);
    fixture.tick();
    assert_eq!(fixture.player(root).simulation_time(), time);
    assert_eq!(fixture.player(root).seek_quality(), SeekQuality::Exact);
}

#[test]
fn changed_root_seed_updates_in_place_and_fixed_child_seed_stays_fixed() {
    let mut fixture = Fixture::new();
    fixture.tick();
    let root = fixture.entity(&[]);
    let child = fixture.entity(&[fixture.clip]);
    fixture
        .app
        .world_mut()
        .resource_mut::<EditorSession>()
        .preview_seed = 1234;
    fixture.tick();
    assert_eq!(fixture.entity(&[]), root);
    assert_eq!(fixture.entity(&[fixture.clip]), child);
    assert_eq!(fixture.player(root).instance.seed(), 1234);
    assert_eq!(fixture.player(child).instance.seed(), 77);
}

#[test]
fn live_transform_reuses_entity_but_structural_edits_replace_the_player() {
    let mut fixture = Fixture::new();
    fixture.tick();
    let root = fixture.entity(&[]);
    let emitter = fixture
        .app
        .world()
        .resource::<EditorSession>()
        .effect
        .emitters[0]
        .id;
    let transform = EmitterTransform {
        translation: [9.0, -2.0, 4.0],
        ..default()
    };
    assert!(
        fixture
            .app
            .world_mut()
            .resource_mut::<EditorSession>()
            .preview_interaction(EffectTransaction::single(
                "Drag",
                EffectCommand::SetEmitterTransform {
                    id: emitter,
                    transform
                }
            ))
    );
    fixture.tick();
    assert_eq!(fixture.entity(&[]), root);
    assert_eq!(
        fixture.player(root).effect().emitters[0].transform,
        transform
    );
    fixture
        .app
        .world_mut()
        .resource_mut::<EditorSession>()
        .restore_interaction_preview();
    fixture
        .app
        .world_mut()
        .resource_mut::<EditorSession>()
        .duplicate_selected_layer();
    fixture.tick();
    assert!(fixture.app.world().get_entity(root).is_err());
    assert_ne!(fixture.entity(&[]), root);
}

#[test]
fn missing_dependency_falls_back_to_root_then_catalog_refresh_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let mut child = EffectAsset::new("Missing then found", 2.0);
    child.emitters.push(Emitter::basic_sprite("Child", 2.0));
    let mut session = test_support::session_with_timing_slack();
    let clip = EffectClip::new(child.id, 0.0, 1.0);
    let clip_id = clip.id;
    session.effect.effect_clips.push(clip);
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(ProjectEffectCatalog::scan(directory.path()))
        .init_resource::<EditorPreviewProject>()
        .init_resource::<TimelineState>()
        .add_systems(
            Update,
            (sync_project_preview, sync_rendered_preview).chain(),
        );
    app.update();
    let preview = app.world().resource::<EditorPreviewProject>();
    assert!(
        preview
            .error
            .as_ref()
            .unwrap()
            .contains(&child.id.to_string())
    );
    assert_eq!(
        preview.failed_source_revision,
        Some(app.world().resource::<EditorSession>().document_revision())
    );
    assert!(preview.project.as_ref().unwrap().dependencies.is_empty());
    child
        .save_ron(directory.path().join("child.aestra.ron"))
        .unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    app.update();
    let preview = app.world().resource::<EditorPreviewProject>();
    assert!(preview.error.is_none());
    assert!(preview.failed_source_revision.is_none());
    assert!(preview.project.as_ref().unwrap().effect(child.id).is_some());
    assert!(
        app.world_mut()
            .query::<&PreviewEffectInstancePath>()
            .iter(app.world())
            .any(|path| path.0 == [clip_id])
    );
}

#[test]
fn invalid_root_retires_players_and_failed_revision_is_not_retried_until_inputs_change() {
    let mut fixture = Fixture::new();
    fixture.tick();
    let root = fixture.entity(&[]);
    let child = fixture.entity(&[fixture.clip]);
    let (emitter, module) = {
        let session = fixture.app.world().resource::<EditorSession>();
        (
            session.effect.emitters[0].id,
            session.effect.emitters[0].modules[0].id,
        )
    };
    assert!(
        fixture
            .app
            .world_mut()
            .resource_mut::<EditorSession>()
            .execute(
                "Remove required module",
                EffectCommand::RemoveModule { emitter, module },
                true
            )
    );
    assert!(
        fixture
            .app
            .world()
            .resource::<EditorSession>()
            .preview()
            .is_none()
    );
    assert!(configured_preview_player(fixture.app.world().resource::<EditorSession>()).is_none());
    fixture.tick();
    assert!(fixture.app.world().get_entity(root).is_err());
    assert!(fixture.app.world().get_entity(child).is_err());
    assert!(
        fixture
            .app
            .world()
            .resource::<EditorPreviewProject>()
            .project
            .is_none()
    );
    fixture
        .app
        .world_mut()
        .resource_mut::<EditorPreviewProject>()
        .error = Some("cached failure".into());
    fixture.tick();
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<EditorPreviewProject>()
            .error
            .as_deref(),
        Some("cached failure")
    );
    fixture
        .app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    fixture.tick();
    assert_ne!(
        fixture
            .app
            .world()
            .resource::<EditorPreviewProject>()
            .error
            .as_deref(),
        Some("cached failure")
    );
    fixture
        .app
        .world_mut()
        .resource_mut::<EditorSession>()
        .undo();
    fixture.tick();
    assert!(
        fixture
            .app
            .world()
            .resource::<EditorPreviewProject>()
            .error
            .is_none()
    );
    assert_ne!(fixture.entity(&[]), root);
    let replacement = fixture.entity(&[fixture.clip]);
    assert_eq!(
        fixture.player(replacement).effect().source,
        fixture.child.id
    );
}

#[test]
fn catalog_structural_child_refresh_replaces_child_but_preserves_root() {
    let mut fixture = Fixture::new();
    fixture.tick();
    let root = fixture.entity(&[]);
    let child = fixture.entity(&[fixture.clip]);
    fixture.child.name = "Changed child".into();
    fixture
        .child
        .save_ron(fixture.directory.path().join("child.aestra.ron"))
        .unwrap();
    fixture
        .app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    fixture.tick();
    assert_eq!(fixture.entity(&[]), root);
    assert!(fixture.app.world().get_entity(child).is_err());
    let replacement = fixture.entity(&[fixture.clip]);
    assert_eq!(fixture.player(replacement).effect().name, "Changed child");
}

#[test]
fn player_setup_preserves_explicit_transform_and_skips_invalid_preview() {
    let mut app = App::new();
    app.insert_resource(test_support::session_with_timing_slack())
        .add_systems(
            Update,
            |mut commands: Commands, session: Res<EditorSession>| {
                spawn_preview_effect_player(
                    &mut commands,
                    &session,
                    Transform::from_xyz(3.0, 4.0, 5.0),
                );
            },
        );
    app.update();
    let world = app.world_mut();
    let (path, transform, layers) = world
        .query::<(&PreviewEffectInstancePath, &Transform, &RenderLayers)>()
        .single(world)
        .unwrap();
    assert!(path.0.is_empty());
    assert_eq!(*transform, Transform::from_xyz(3.0, 4.0, 5.0));
    assert_eq!(*layers, RenderLayers::layer(0));
    let (emitter, module) = {
        let session = app.world().resource::<EditorSession>();
        (
            session.effect.emitters[0].id,
            session.effect.emitters[0].modules[0].id,
        )
    };
    app.world_mut().resource_mut::<EditorSession>().execute(
        "Invalid preview",
        EffectCommand::RemoveModule { emitter, module },
        true,
    );
    app.update();
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<&PreviewEffectInstancePath>()
            .iter(world)
            .count(),
        1
    );
}

#[test]
fn reconciliation_does_not_mutate_authored_data_or_history() {
    let mut fixture = Fixture::new();
    let session = fixture.app.world().resource::<EditorSession>();
    let effect = session.effect.clone();
    let revision_before_install = session.document_revision();
    let dirty = session.dirty;
    let undo = session.effect_undo_len();
    let redo = session.effect_redo_len();
    fixture.tick();
    // Initial catalog installation invalidates simulation checkpoints. This is an
    // existing runtime revision, not an authored edit or a semantic undo entry.
    let revision = fixture
        .app
        .world()
        .resource::<EditorSession>()
        .document_revision();
    assert_eq!(revision, revision_before_install.wrapping_add(1));
    for _ in 0..3 {
        fixture.tick();
    }
    fixture.seek(1.25);
    fixture.seek(0.25);
    let session = fixture.app.world().resource::<EditorSession>();
    assert_eq!(session.effect, effect);
    assert_eq!(session.document_revision(), revision);
    assert_eq!(session.dirty, dirty);
    assert_eq!(session.effect_undo_len(), undo);
    assert_eq!(session.effect_redo_len(), redo);
}

#[test]
fn sibling_clips_share_artifact_but_have_distinct_paths_players_and_seed() {
    let mut fixture = Fixture::new();
    let mut clip = EffectClip::new(fixture.child.id, 0.0, 2.0);
    clip.seed = EffectClipSeed::Fixed(99);
    let sibling = clip.id;
    fixture
        .app
        .world_mut()
        .resource_mut::<EditorSession>()
        .execute(
            "Add sibling",
            EffectCommand::AddEffectClip { clip, index: 1 },
            false,
        );
    fixture.tick();
    let first = fixture.entity(&[fixture.clip]);
    let second = fixture.entity(&[sibling]);
    assert_ne!(first, second);
    assert!(Arc::ptr_eq(
        fixture.player(first).effect(),
        fixture.player(second).effect()
    ));
    assert_eq!(fixture.player(first).instance.seed(), 77);
    assert_eq!(fixture.player(second).instance.seed(), 99);
    assert!((fixture.player(first).simulation_time() - 0.35).abs() < 0.0001);
    assert_eq!(fixture.player(second).simulation_time(), 0.75);
}

#[test]
fn nested_players_keep_historical_ancestry_out_of_entity_placement() {
    let mut fixture = Fixture::new();
    let mut grandchild = EffectAsset::new("Grandchild", 2.0);
    grandchild.emitters.push(Emitter::basic_sprite("Leaf", 2.0));
    grandchild
        .save_ron(fixture.directory.path().join("grandchild.aestra.ron"))
        .unwrap();
    let mut nested = EffectClip::new(grandchild.id, 0.0, 2.0);
    nested.source_offset = 0.2;
    nested.transform.translation = [1.0, -4.0, 3.0];
    let nested_id = nested.id;
    fixture.child.effect_clips.push(nested);
    fixture
        .child
        .save_ron(fixture.directory.path().join("child.aestra.ron"))
        .unwrap();
    fixture
        .app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    fixture.tick();
    let leaf = fixture.entity(&[fixture.clip, nested_id]);
    let player = fixture.player(leaf);
    assert_eq!(
        *fixture.app.world().get::<Transform>(leaf).unwrap(),
        Transform::IDENTITY
    );
    assert!((player.simulation_time() - 0.55).abs() < 0.0001);
    let context = player.instance.host_transform_context();
    assert_eq!(
        Mat4::from_cols_array(&context.matrix_at(player.simulation_time()))
            .w_axis
            .truncate(),
        Vec3::new(6.0, -2.0, 2.0)
    );
    let project = fixture
        .app
        .world()
        .resource::<EditorPreviewProject>()
        .project
        .as_ref()
        .unwrap();
    let mut reference = Vec::new();
    project.evaluate(
        0.75,
        fixture.app.world().resource::<EditorSession>().preview_seed,
        &mut reference,
    );
    let samples: Vec<_> = reference
        .iter()
        .filter(|sample| sample.instance_path == [fixture.clip, nested_id])
        .collect();
    assert!(!samples.is_empty());
    assert!(
        samples
            .iter()
            .all(|sample| sample.world_from_effect == context.matrix_at(player.simulation_time()))
    );
}

#[test]
fn live_edit_classifier_accepts_clip_pose_but_rejects_timing_and_topology() {
    let mut fixture = Fixture::new();
    fixture.tick();
    let root = fixture.entity(&[]);
    let original = fixture.player(root).effect();
    let mut edited = original.as_ref().clone();
    edited.effect_clips[0].transform.translation = [3.0, 4.0, 5.0];
    assert!(compiled_effects_differ_only_in_live_edits(
        original, &edited
    ));
    edited.effect_clips[0].source_offset += 0.5;
    assert!(!compiled_effects_differ_only_in_live_edits(
        original, &edited
    ));
    let mut edited = original.as_ref().clone();
    edited.effect_clips.clear();
    assert!(!compiled_effects_differ_only_in_live_edits(
        original, &edited
    ));
    let mut edited = original.as_ref().clone();
    edited.emitters.pop();
    assert!(!compiled_effects_differ_only_in_live_edits(
        original, &edited
    ));
}

fn presented_playhead_time(player: &PresentedEffect) -> f32 {
    let effect = player.effect();
    if effect.playback_mode.is_looping() && effect.duration > 0.0 {
        player.simulation_time().rem_euclid(effect.duration)
    } else {
        player
            .simulation_time()
            .clamp(0.0, effect.duration.max(0.0))
    }
}

#[test]
fn editor_preview_player_uses_the_compiled_effect_timeline_and_seed() {
    let mut session = test_support::session_with_timing_slack();
    session.preview_seed = 42;
    session
        .driver
        .clock
        .seek_frame(37, session.playback_duration());

    let player = configured_preview_player(&session).unwrap();

    assert!(std::sync::Arc::ptr_eq(
        player.effect(),
        session.preview().unwrap().effect()
    ));
    assert_eq!(player.simulation_time(), session.simulation_time());
    assert_eq!(player.instance.seed(), 42);
}

#[test]
fn transform_only_preview_updates_the_existing_player_in_place() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = test_support::session_with_timing_slack();
    let player = configured_preview_player(&session).unwrap();
    let emitter = session.selected_layer().unwrap().id;
    let transform = EmitterTransform {
        translation: [7.0, -3.0, 2.0],
        ..default()
    };
    assert!(session.preview_interaction(EffectTransaction::single(
        "Preview emitter transform",
        EffectCommand::SetEmitterTransform {
            id: emitter,
            transform,
        },
    )));

    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(ProjectEffectCatalog::scan(directory.path()))
        .init_resource::<EditorPreviewProject>()
        .init_resource::<TimelineState>()
        .add_systems(
            Update,
            (sync_project_preview, sync_rendered_preview).chain(),
        );
    let player_entity = app
        .world_mut()
        .spawn((
            PreviewPresentedEffect,
            PreviewEffectInstancePath::default(),
            player,
        ))
        .id();

    app.update();

    let world = app.world();
    let player = world.get::<PresentedEffect>(player_entity).unwrap();
    assert_eq!(player.effect().emitters[0].transform, transform);
    assert!(std::sync::Arc::ptr_eq(
        player.effect(),
        world
            .resource::<EditorSession>()
            .preview()
            .unwrap()
            .effect()
    ));
}

#[test]
fn live_player_replacement_rejects_structural_compiler_changes() {
    let session = test_support::session_with_timing_slack();
    let current = session.preview().unwrap().effect();
    let mut transformed = current.as_ref().clone();
    transformed.emitters[0].transform.translation[0] = 4.0;
    assert!(compiled_effects_differ_only_in_live_edits(
        current,
        &transformed
    ));

    transformed.emitters[0].duration += 0.25;
    assert!(!compiled_effects_differ_only_in_live_edits(
        current,
        &transformed
    ));
}

#[test]
fn project_preview_collects_active_effect_clips_with_local_time_and_seed() {
    let temporary = tempfile::tempdir().unwrap();
    let mut child = aestra_core::EffectAsset::new("Child", 1.0);
    let parameter = aestra_core::EffectParameter {
        id: aestra_core::ParameterId::new(),
        name: "Spawn Rate".into(),
        default: Value::Scalar(4.0),
        exposed: true,
    };
    let parameter_id = parameter.id;
    let mut emitter = aestra_core::Emitter::basic_sprite("Child emitter", 1.0);
    emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == aestra_core::MODULE_EMISSION)
        .unwrap()
        .bindings
        .insert("spawn_rate".into(), parameter_id);
    child.parameters.push(parameter);
    child.emitters.push(emitter);
    child
        .save_ron(temporary.path().join("child.aestra.ron"))
        .unwrap();

    let mut root = aestra_core::EffectAsset::new("Root", 2.0);
    root.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut clip = aestra_core::EffectClip::new(child.id, 0.5, 1.0);
    clip.source_offset = 0.1;
    clip.seed = aestra_core::EffectClipSeed::Fixed(77);
    clip.transform.translation = [5.0, 2.0, -1.0];
    clip.parameter_overrides
        .insert(parameter_id, Value::Scalar(20.0));
    let clip_id = clip.id;
    root.effect_clips.push(clip);
    let catalog = ProjectEffectCatalog::scan(temporary.path());
    let project = Arc::new(catalog.compile_project(&root).unwrap());
    let preview = EditorPreviewProject {
        source_root: Some(project.root.clone()),
        project: Some(project),
        ..default()
    };
    let timeline = TimelineState::framed(root.duration);

    let before = desired_preview_instances(&preview, &timeline, 0.25, 9);
    assert_eq!(before.len(), 1, "the child must not run before its clip");

    let active = desired_preview_instances(&preview, &timeline, 0.75, 9);
    assert_eq!(active.len(), 2);
    assert!(active[0].path.is_empty());
    assert_eq!(active[1].path, vec![clip_id]);
    assert_eq!(active[1].effect.source, child.id);
    assert!((active[1].time - 0.35).abs() < 0.000_1);
    assert_eq!(active[1].seed, 77);
    let context = aestra_runtime::HostTransformContext {
        motion: active[1].effect.host_transform_track.clone(),
        inherited: active[1].inherited.clone(),
    };
    assert_eq!(
        Mat4::from_cols_array(&context.matrix_at(active[1].time))
            .w_axis
            .truncate(),
        Vec3::new(5.0, 2.0, -1.0)
    );
    assert_eq!(active[1].parameter_overrides.len(), 1);
    let player = configured_preview_instance(
        active[1].effect.clone(),
        active[1].time,
        active[1].seed,
        &active[1].parameter_overrides,
    );
    assert_eq!(
        player.instance.parameter(parameter_id),
        Some(&aestra_runtime::RuntimeValue::Scalar(20.0))
    );

    let repeated = desired_preview_instances(&preview, &timeline, 2.75, 9);
    assert_eq!(repeated.len(), 2);
    assert_eq!(repeated[0].time, 2.75);
    assert!((repeated[1].time - 0.35).abs() < 0.000_1);
}

#[test]
fn project_preview_error_identifies_a_missing_effect_reference() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = test_support::session_with_timing_slack();
    let missing = aestra_core::EffectId::from_u128(0xfeed_cafe);
    session
        .effect
        .effect_clips
        .push(aestra_core::EffectClip::new(missing, 0.0, 0.5));
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(ProjectEffectCatalog::scan(directory.path()))
        .init_resource::<EditorPreviewProject>()
        .add_systems(Update, sync_project_preview);

    app.update();

    let preview = app.world().resource::<EditorPreviewProject>();
    let error = preview.error.as_deref().expect("missing source must fail");
    assert!(error.contains(&missing.to_string()), "{error}");
    assert!(
        preview.project.is_some(),
        "the valid root remains previewable"
    );
}

#[test]
fn project_preview_recompiles_referenced_effects_when_the_catalog_refreshes() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("child.aestra.ron");
    let mut child = aestra_core::EffectAsset::new("Child A", 1.0);
    child
        .emitters
        .push(aestra_core::Emitter::basic_sprite("Child emitter", 1.0));
    child.save_ron(&path).unwrap();
    let child_id = child.id;
    let mut session = test_support::session_with_timing_slack();
    session.effect.effect_clips.clear();
    session
        .effect
        .effect_clips
        .push(aestra_core::EffectClip::new(child_id, 0.0, 0.5));
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(ProjectEffectCatalog::scan(temporary.path()))
        .init_resource::<EditorPreviewProject>()
        .add_systems(Update, sync_project_preview);

    app.update();
    assert_eq!(
        app.world()
            .resource::<EditorPreviewProject>()
            .project
            .as_ref()
            .unwrap()
            .effect(child_id)
            .unwrap()
            .name,
        "Child A"
    );

    child.name = "Child B".into();
    child.save_ron(&path).unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    app.update();

    assert_eq!(
        app.world()
            .resource::<EditorPreviewProject>()
            .project
            .as_ref()
            .unwrap()
            .effect(child_id)
            .unwrap()
            .name,
        "Child B"
    );
}

#[test]
fn project_preview_players_follow_seek_and_restart_deterministically() {
    let temporary = tempfile::tempdir().unwrap();
    let mut child = aestra_core::EffectAsset::new("Child", 1.0);
    child
        .emitters
        .push(aestra_core::Emitter::basic_sprite("Child emitter", 1.0));
    child
        .save_ron(temporary.path().join("child.aestra.ron"))
        .unwrap();

    let mut session = test_support::session_with_timing_slack();
    session.effect.effect_clips.clear();
    let mut clip = aestra_core::EffectClip::new(child.id, 0.5, 1.0);
    clip.source_offset = 0.1;
    clip.seed = aestra_core::EffectClipSeed::Fixed(77);
    let clip_id = clip.id;
    assert!(session.execute(
        "Add child",
        EffectCommand::AddEffectClip {
            clip,
            index: session.effect.effect_clips.len(),
        },
        false,
    ));
    session.seek_time(0.75);

    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(ProjectEffectCatalog::scan(temporary.path()))
        .init_resource::<EditorPreviewProject>()
        .init_resource::<TimelineState>()
        .add_systems(
            Update,
            (sync_project_preview, sync_rendered_preview).chain(),
        );
    app.update();

    let child_state = {
        let world = app.world_mut();
        let mut players = world.query::<(&PreviewEffectInstancePath, &PresentedEffect)>();
        let instances = players.iter(world).collect::<Vec<_>>();
        assert_eq!(instances.len(), 2);
        let (_, player) = instances
            .into_iter()
            .find(|(path, _)| path.0.as_slice() == [clip_id])
            .expect("active clip must have a player");
        (presented_playhead_time(player), player.instance.seed())
    };
    assert!((child_state.0 - 0.35).abs() <= 1.0 / 60.0);
    assert_eq!(child_state.1, 77);

    app.world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(0.25);
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&PreviewEffectInstancePath>()
            .iter(app.world())
            .count(),
        1,
        "seeking before the clip removes its player"
    );

    {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        session.restart();
        session.seek_time(0.75);
    }
    app.update();
    let replayed = {
        let world = app.world_mut();
        let mut players = world.query::<(&PreviewEffectInstancePath, &PresentedEffect)>();
        players
            .iter(world)
            .find(|(path, _)| path.0.as_slice() == [clip_id])
            .map(|(_, player)| (presented_playhead_time(player), player.instance.seed()))
            .expect("restarted clip must be active after seeking back into it")
    };
    assert_eq!(replayed, child_state);
}

#[test]
fn continuous_project_preview_uses_unwrapped_simulation_time() {
    let temporary = tempfile::tempdir().unwrap();
    let mut session = test_support::session_with_playback_mode(EffectPlaybackMode::LoopRestart);
    session.effect.effect_clips.clear();
    assert!(session.set_effect_playback_mode(EffectPlaybackMode::LoopContinuous));
    session.restart();
    let duration = session.effect.duration;
    session.advance_playback(duration + 0.25);
    let expected_simulation_time = session.simulation_time();
    let expected_playhead = session.time();
    assert!(expected_simulation_time > duration);
    assert!(expected_playhead < duration);

    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(ProjectEffectCatalog::scan(temporary.path()))
        .init_resource::<EditorPreviewProject>()
        .init_resource::<TimelineState>()
        .add_systems(
            Update,
            (sync_project_preview, sync_rendered_preview).chain(),
        );
    app.update();

    let world = app.world_mut();
    let mut players = world.query::<(&PreviewEffectInstancePath, &PresentedEffect)>();
    let (_, player) = players
        .iter(world)
        .find(|(path, _)| path.0.is_empty())
        .expect("the root preview player must exist");
    assert!((player.simulation_time() - expected_simulation_time).abs() < 0.000_1);
    assert!((presented_playhead_time(player) - expected_playhead).abs() < 0.000_1);
}

#[test]
fn material_refresh_updates_root_without_losing_interaction_preview() {
    use aestra_core::material::{
        MaterialInstance, MaterialProgram, MaterialProgramRef, MaterialRenderState,
    };
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("material.aestra.material.ron");
    let mut program = MaterialProgram::additive_sprite("Before");
    program.save_ron(&path).unwrap();
    let mut session = test_support::session_with_timing_slack();
    session.effect.material_instances.push(MaterialInstance {
        id: aestra_core::MaterialId::new(),
        program: MaterialProgramRef::Project(program.id),
        values: default(),
        render_state: MaterialRenderState::additive_sprite(),
    });
    let catalog = ProjectEffectCatalog::scan(temporary.path());
    session
        .install_compiled_project_root(catalog.compile_project(&session.effect).unwrap().root)
        .unwrap();
    let emitter = session.selected_layer().unwrap().id;
    let transform = EmitterTransform {
        translation: [9.0, 0.0, 0.0],
        ..default()
    };
    assert!(session.preview_interaction(EffectTransaction::single(
        "Drag",
        EffectCommand::SetEmitterTransform {
            id: emitter,
            transform
        }
    )));
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(catalog)
        .init_resource::<EditorPreviewProject>()
        .add_systems(Update, sync_project_preview);
    app.update();
    program.name = "After external save".into();
    program.save_ron(&path).unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    app.update();
    let session = app.world().resource::<EditorSession>();
    let root = session.preview().unwrap().effect();
    assert_eq!(
        root.material_program(program.id).unwrap().name,
        program.name
    );
    assert_eq!(root.emitters[0].transform, transform);
    assert_ne!(session.effect.emitters[0].transform, transform);
}
