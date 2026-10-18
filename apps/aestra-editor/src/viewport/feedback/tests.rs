//! Actual mock/input/output/drop consumers; native GPU producers remain renderer-qualified.
use super::super::runtime_preview::{
    EditorPreviewProject, configured_preview_player, sync_project_preview, sync_rendered_preview,
};
use super::*;
use crate::{
    ProjectEffectCatalog, preview_mocks::PreviewMock, test_support, timeline::TimelineState,
};
use aestra_authoring::{EffectCommand, SemanticTarget};
use aestra_bevy_render::gpu::{AestraOutputEvent, GpuEventLinkStatistics};
use aestra_core::{BindingId, BindingUpdateMode, EffectBinding, EffectClipId, EventTrigger};
use aestra_runtime::{EffectOutputEvent, EventOrigin, HostInputEvent};

struct Fixture {
    app: App,
    root: Entity,
    nested: Entity,
    binding: BindingId,
}
impl Fixture {
    fn new() -> Self {
        let mut session = test_support::session_with_timing_slack();
        let binding = EffectBinding::spatial("Target", BindingUpdateMode::Live);
        let id = binding.id;
        assert!(session.execute(
            "Add target",
            EffectCommand::AddBinding { binding, index: 0 },
            false
        ));
        session.preview_mocks.insert(id, PreviewMock::Orbit);
        session.seek_time(1.0);
        let player = configured_preview_player(&session).unwrap();
        let mut app = App::new();
        app.insert_resource(session)
            .add_message::<AestraOutputEvent>()
            .add_systems(
                Update,
                (
                    apply_preview_mocks,
                    record_preview_outputs,
                    track_event_link_drops,
                )
                    .chain(),
            );
        let root = app
            .world_mut()
            .spawn((
                PreviewPresentedEffect,
                PreviewEffectInstancePath::default(),
                player.clone(),
            ))
            .id();
        let nested = app
            .world_mut()
            .spawn((
                PreviewPresentedEffect,
                PreviewEffectInstancePath(vec![EffectClipId::new()]),
                player,
            ))
            .id();
        Self {
            app,
            root,
            nested,
            binding: id,
        }
    }
    fn tick(&mut self) {
        self.app.update();
    }
    fn player(&self, entity: Entity) -> &PresentedEffect {
        self.app.world().get::<PresentedEffect>(entity).unwrap()
    }
    fn trace(&self, entity: Entity) -> Option<u64> {
        self.player(entity)
            .instance
            .binding_trace()
            .map(|trace| trace.identity())
    }
    fn send(&mut self, output: AestraOutputEvent) {
        self.app.world_mut().write_message(output);
    }
    fn event(tick: u64) -> EffectOutputEvent {
        EffectOutputEvent::new(
            "impact",
            EventOrigin::Effect,
            "",
            vec![1.0, 2.0, 3.0],
            4.0,
            tick,
        )
    }
}

#[test]
fn root_and_session_share_one_trace_but_nested_players_are_untouched() {
    let mut f = Fixture::new();
    f.tick();
    let identity = f.trace(f.root).unwrap();
    let session = f.app.world().resource::<EditorSession>();
    assert_eq!(
        session
            .preview()
            .unwrap()
            .binding_trace()
            .unwrap()
            .identity(),
        identity
    );
    assert!(f.trace(f.nested).is_none());
    let field = aestra_core::BindingFieldId::new(aestra_core::AESTRA_FIELD_POSITION);
    let root = &f.player(f.root).instance;
    let position = root
        .binding_field(aestra_runtime::BindingSlot(0), &field)
        .unwrap();
    let time = aestra_runtime::trace_tick(root.time()) as f32
        * aestra_runtime::StatefulSimulation::TICK_DT;
    assert_eq!(position, PreviewMock::Orbit.pose(time).0);
}

#[test]
fn unchanged_mock_and_inputs_keep_history_epochs_and_trace_identity() {
    let mut f = Fixture::new();
    f.tick();
    let trace = f.trace(f.root);
    let epoch = f.player(f.root).instance.history_epoch();
    let session_epoch = f
        .app
        .world()
        .resource::<EditorSession>()
        .preview()
        .unwrap()
        .history_epoch();
    for _ in 0..3 {
        f.tick();
    }
    assert_eq!(f.trace(f.root), trace);
    assert_eq!(f.player(f.root).instance.history_epoch(), epoch);
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .preview()
            .unwrap()
            .history_epoch(),
        session_epoch
    );
}

#[test]
fn changed_and_removed_mocks_rebuild_then_clear_both_traces() {
    let mut f = Fixture::new();
    f.tick();
    let before = f.trace(f.root);
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .preview_mocks
        .insert(f.binding, PreviewMock::Still);
    f.tick();
    assert_ne!(f.trace(f.root), before);
    let session = f.app.world().resource::<EditorSession>();
    assert_eq!(
        f.trace(f.root),
        Some(
            session
                .preview()
                .unwrap()
                .binding_trace()
                .unwrap()
                .identity()
        )
    );
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .preview_mocks
        .clear();
    f.tick();
    assert!(f.trace(f.root).is_none());
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .preview()
            .unwrap()
            .binding_trace()
            .is_none()
    );
}

#[test]
fn recorded_inputs_reach_roots_and_session_exactly_but_not_children() {
    let mut f = Fixture::new();
    let events = vec![HostInputEvent {
        input: aestra_runtime::INPUT_STOP_EMITTING.into(),
        tick: 30,
        payload: vec![],
    }];
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .preview_inputs = events.clone();
    f.tick();
    assert_eq!(f.player(f.root).instance.received_events(), events);
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .preview()
            .unwrap()
            .received_events(),
        events
    );
    assert!(f.player(f.nested).instance.received_events().is_empty());
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .preview_inputs
        .clear();
    f.tick();
    assert!(f.player(f.root).instance.received_events().is_empty());
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .preview()
            .unwrap()
            .received_events()
            .is_empty()
    );
}

#[test]
fn session_binding_trace_updates_without_any_presentation_entities() {
    let mut f = Fixture::new();
    f.app.world_mut().despawn(f.root);
    f.app.world_mut().despawn(f.nested);
    f.tick();
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .preview()
            .unwrap()
            .binding_trace()
            .is_some()
    );
}

#[test]
fn nested_and_unowned_traces_and_inputs_survive_root_mock_edits() {
    let mut f = Fixture::new();
    let player = f.player(f.root).clone();
    let unowned = f.app.world_mut().spawn(player).id();
    let inputs = vec![HostInputEvent {
        input: aestra_runtime::INPUT_KILL.into(),
        tick: 9,
        payload: vec![],
    }];
    for entity in [f.nested, unowned] {
        f.app
            .world_mut()
            .get_mut::<PresentedEffect>(entity)
            .unwrap()
            .instance
            .set_received_events(inputs.clone());
    }
    f.tick();
    for entity in [f.nested, unowned] {
        assert_eq!(f.player(entity).instance.received_events(), inputs);
        assert!(f.trace(entity).is_none());
    }
}

#[test]
fn root_output_filter_rejects_nested_paths_unowned_stale_and_incomplete_entities() {
    let mut f = Fixture::new();
    let unowned = f
        .app
        .world_mut()
        .spawn(PreviewEffectInstancePath::default())
        .id();
    let incomplete = f.app.world_mut().spawn(PreviewPresentedEffect).id();
    let stale = f
        .app
        .world_mut()
        .spawn((PreviewPresentedEffect, PreviewEffectInstancePath::default()))
        .id();
    f.app.world_mut().despawn(stale);
    for entity in [f.nested, unowned, incomplete, stale] {
        f.send(AestraOutputEvent::root(entity, Fixture::event(99)));
    }
    let mut child_output = AestraOutputEvent::root(f.root, Fixture::event(88));
    child_output.clip_path.push(EffectClipId::new());
    f.send(child_output);
    f.send(AestraOutputEvent::root(f.root, Fixture::event(7)));
    f.tick();
    let session = f.app.world().resource::<EditorSession>();
    assert_eq!(session.preview_outputs_heard, 1);
    assert_eq!(session.preview_outputs.front().unwrap().tick, 7);
}

#[test]
fn root_outputs_keep_payload_and_arrival_order_without_sorting_or_deduplication() {
    let mut f = Fixture::new();
    let mut payload = Fixture::event(9);
    payload.text = Some("sound/explosion.ogg".into());
    payload.output = "force".into();
    for tick in [9, 3, 9] {
        let mut event = payload.clone();
        event.tick = tick;
        f.send(AestraOutputEvent::root(f.root, event).in_epoch(123));
    }
    f.tick();
    let session = f.app.world().resource::<EditorSession>();
    assert_eq!(session.preview_outputs_heard, 3);
    assert_eq!(
        session
            .preview_outputs
            .iter()
            .map(|event| event.tick)
            .collect::<Vec<_>>(),
        [9, 3, 9]
    );
    assert_eq!(session.preview_outputs[0], payload);
    assert_eq!(session.preview_outputs[2], payload);
    // Epoch validation belongs to output producers/hosts; this editor consumer
    // deliberately retains its existing root/path-only filtering policy.
}

#[test]
fn output_reader_consumes_once_and_bounded_history_keeps_latest_arrivals() {
    let mut f = Fixture::new();
    for tick in 0..12 {
        f.send(AestraOutputEvent::root(f.root, Fixture::event(tick)));
    }
    f.tick();
    f.tick();
    let session = f.app.world().resource::<EditorSession>();
    assert_eq!(session.preview_outputs.len(), PREVIEW_OUTPUTS_KEPT);
    assert_eq!(session.preview_outputs_heard, 12);
    assert_eq!(session.preview_outputs.front().unwrap().tick, 4);
    assert_eq!(session.preview_outputs.back().unwrap().tick, 11);
}

fn drop_fixture() -> (App, Entity, aestra_core::EventId) {
    let mut session = test_support::session_with_timing_slack();
    let source = session.effect.emitters[0].id;
    let target = session.effect.emitters[1].id;
    session.selection.primary = SemanticTarget::Emitter(source);
    let id = session
        .add_event_link(EventTrigger::OnDeath, target)
        .unwrap();
    let player = configured_preview_player(&session).unwrap();
    let mut app = App::new();
    app.insert_resource(session)
        .add_systems(Update, track_event_link_drops);
    let entity = app
        .world_mut()
        .spawn((
            PreviewPresentedEffect,
            PreviewEffectInstancePath::default(),
            player,
            GpuEventLinkStatistics {
                dropped: vec![3],
                ..default()
            },
        ))
        .id();
    (app, entity, id)
}

#[test]
fn dropped_link_membership_changes_rebuild_ui_but_count_changes_do_not() {
    let (mut app, entity, id) = drop_fixture();
    let revision = app.world().resource::<EditorSession>().ui_revision;
    app.update();
    assert_eq!(
        app.world().resource::<EditorSession>().event_link_drops[&id],
        3
    );
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision + 1
    );
    app.world_mut()
        .get_mut::<GpuEventLinkStatistics>(entity)
        .unwrap()
        .dropped[0] = 8;
    app.update();
    assert_eq!(
        app.world().resource::<EditorSession>().event_link_drops[&id],
        8
    );
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision + 1
    );
    app.world_mut()
        .get_mut::<GpuEventLinkStatistics>(entity)
        .unwrap()
        .dropped[0] = 0;
    app.update();
    assert!(
        app.world()
            .resource::<EditorSession>()
            .event_link_drops
            .is_empty()
    );
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision + 2
    );
    app.update();
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision + 2
    );
}

#[test]
fn removing_root_statistics_clears_drops_without_reading_nested_observations() {
    let (mut app, entity, _) = drop_fixture();
    let player = app.world().get::<PresentedEffect>(entity).unwrap().clone();
    app.world_mut().spawn((
        PreviewPresentedEffect,
        PreviewEffectInstancePath(vec![EffectClipId::new()]),
        player,
        GpuEventLinkStatistics {
            dropped: vec![999],
            ..default()
        },
    ));
    app.update();
    app.world_mut()
        .entity_mut(entity)
        .remove::<GpuEventLinkStatistics>();
    app.update();
    assert!(
        app.world()
            .resource::<EditorSession>()
            .event_link_drops
            .is_empty()
    );
}

#[test]
fn drop_mapping_handles_short_extra_zero_and_invalid_compiled_indices() {
    let (app, entity, id) = drop_fixture();
    let player = app.world().get::<PresentedEffect>(entity).unwrap();
    let authored = &app.world().resource::<EditorSession>().effect;
    for (counts, expected) in [(vec![], None), (vec![0], None), (vec![7, 999], Some(7))] {
        let mapped = event_link_drops(
            authored,
            player.effect(),
            &GpuEventLinkStatistics {
                dropped: counts,
                ..default()
            },
        );
        assert_eq!(mapped.get(&id).copied(), expected);
    }
    let mut compiled = player.effect().as_ref().clone();
    compiled.event_links[0].source = usize::MAX;
    assert!(
        event_link_drops(
            authored,
            &compiled,
            &GpuEventLinkStatistics {
                dropped: vec![1],
                ..default()
            }
        )
        .is_empty()
    );
    compiled.event_links[0].source = 0;
    compiled.event_links[0].target = usize::MAX;
    assert!(
        event_link_drops(
            authored,
            &compiled,
            &GpuEventLinkStatistics {
                dropped: vec![1],
                ..default()
            }
        )
        .is_empty()
    );
}

#[test]
fn authored_identity_and_trigger_must_match_compiled_drop_observations() {
    let (app, entity, _) = drop_fixture();
    let player = app.world().get::<PresentedEffect>(entity).unwrap();
    let mut authored = app.world().resource::<EditorSession>().effect.clone();
    authored.events[0].trigger = EventTrigger::OnSpawn;
    assert!(
        event_link_drops(
            &authored,
            player.effect(),
            &GpuEventLinkStatistics {
                dropped: vec![2],
                ..default()
            }
        )
        .is_empty()
    );
    authored.events.clear();
    assert!(
        event_link_drops(
            &authored,
            player.effect(),
            &GpuEventLinkStatistics {
                dropped: vec![2],
                ..default()
            }
        )
        .is_empty()
    );
}

#[test]
fn feedback_does_not_edit_authored_data_or_semantic_history() {
    let mut f = Fixture::new();
    let session = f.app.world().resource::<EditorSession>();
    let before = (
        session.effect.clone(),
        session.document_revision(),
        session.effect_undo_len(),
        session.effect_redo_len(),
        session.dirty,
    );
    f.send(AestraOutputEvent::root(f.root, Fixture::event(1)));
    f.tick();
    let session = f.app.world().resource::<EditorSession>();
    assert_eq!(
        (
            session.effect.clone(),
            session.document_revision(),
            session.effect_undo_len(),
            session.effect_redo_len(),
            session.dirty
        ),
        before
    );
    assert!(!session.effect.to_pretty_ron().unwrap().contains("Orbit"));
}

#[test]
fn seeking_replays_the_recording_without_allocating_a_different_trace() {
    let mut f = Fixture::new();
    f.tick();
    let trace = f.trace(f.root);
    let field = aestra_core::BindingFieldId::new(aestra_core::AESTRA_FIELD_POSITION);
    for time in [2.0, 0.5, 2.0] {
        f.app
            .world_mut()
            .get_mut::<PresentedEffect>(f.root)
            .unwrap()
            .instance
            .seek(time);
        f.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .seek_time(time);
        f.tick();
        let tick_time =
            aestra_runtime::trace_tick(time) as f32 * aestra_runtime::StatefulSimulation::TICK_DT;
        let expected = PreviewMock::Orbit.pose(tick_time).0;
        assert_eq!(f.trace(f.root), trace);
        assert_eq!(
            f.player(f.root)
                .instance
                .binding_field(aestra_runtime::BindingSlot(0), &field)
                .unwrap(),
            expected
        );
        assert_eq!(
            f.app
                .world()
                .resource::<EditorSession>()
                .preview()
                .unwrap()
                .binding_field(aestra_runtime::BindingSlot(0), &field)
                .unwrap(),
            expected
        );
    }
}

#[test]
fn replacement_root_receives_the_existing_recording() {
    let mut f = Fixture::new();
    f.tick();
    let trace = f.trace(f.root);
    f.app.world_mut().despawn(f.root);
    let player = configured_preview_player(f.app.world().resource::<EditorSession>()).unwrap();
    let replacement = f
        .app
        .world_mut()
        .spawn((
            PreviewPresentedEffect,
            PreviewEffectInstancePath::default(),
            player,
        ))
        .id();
    f.tick();
    assert_eq!(f.trace(replacement), trace);
    assert!(f.trace(f.nested).is_none());
}

#[test]
fn the_preview_keeps_its_latest_root_outputs() {
    use aestra_bevy_render::gpu::AestraOutputEvent;
    let mut app = App::new();
    app.insert_resource(test_support::session_with_timing_slack())
        .add_message::<AestraOutputEvent>()
        .add_systems(Update, record_preview_outputs);
    let root = app
        .world_mut()
        .spawn((PreviewPresentedEffect, PreviewEffectInstancePath::default()))
        .id();
    let nested = app
        .world_mut()
        .spawn((
            PreviewPresentedEffect,
            PreviewEffectInstancePath(vec![aestra_core::EffectClipId::new()]),
        ))
        .id();
    let event = |tick| {
        aestra_runtime::EffectOutputEvent::new(
            aestra_runtime::EVENT_IMPACT,
            aestra_runtime::EventOrigin::Effect,
            "",
            Vec::new(),
            1.0,
            tick,
        )
    };
    for tick in 0..10 {
        app.world_mut()
            .write_message(AestraOutputEvent::root(root, event(tick)));
    }
    app.world_mut()
        .write_message(AestraOutputEvent::root(nested, event(99)));
    app.update();
    let session = app.world().resource::<EditorSession>();
    let ticks: Vec<u64> = session
        .preview_outputs
        .iter()
        .map(|event| event.tick)
        .collect();
    assert_eq!(
        ticks,
        (2..10).collect::<Vec<_>>(),
        "the latest eight, root only"
    );
    assert_eq!(session.preview_outputs_heard, 10);
}

#[test]
fn gpu_link_drops_map_back_to_authored_links() {
    let mut session = test_support::session_with_timing_slack();
    session.duplicate_selected_layer();
    let (rocket, stars) = (session.effect.emitters[0].id, session.effect.emitters[1].id);
    session.selection.primary = SemanticTarget::Emitter(rocket);
    let id = session
        .add_event_link(aestra_core::EventTrigger::OnDeath, stars)
        .unwrap();
    let compiled = session.preview().unwrap().effect().clone();
    let statistics = |dropped: u64| aestra_bevy_render::gpu::GpuEventLinkStatistics {
        dropped: vec![dropped],
        ..default()
    };
    assert_eq!(
        event_link_drops(&session.effect, &compiled, &statistics(3072)),
        std::collections::BTreeMap::from([(id, 3072)])
    );
    assert!(event_link_drops(&session.effect, &compiled, &statistics(0)).is_empty());
}

#[test]
fn preview_stand_ins_drive_the_edited_effect_and_are_never_saved() {
    let directory = tempfile::tempdir().unwrap();
    let mut session = test_support::session_with_timing_slack();
    let target =
        aestra_core::EffectBinding::spatial("Target", aestra_core::BindingUpdateMode::Live);
    let id = target.id;
    assert!(session.execute(
        "Add binding",
        EffectCommand::AddBinding {
            binding: target,
            index: 0,
        },
        false,
    ));
    session
        .preview_mocks
        .insert(id, crate::preview_mocks::PreviewMock::Orbit);
    let player = configured_preview_player(&session).unwrap();

    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(ProjectEffectCatalog::scan(directory.path()))
        .init_resource::<EditorPreviewProject>()
        .init_resource::<TimelineState>()
        .add_systems(
            Update,
            (
                sync_project_preview,
                sync_rendered_preview,
                apply_preview_mocks,
            )
                .chain(),
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

    let traced = |app: &App| {
        let world = app.world();
        let player = world.get::<PresentedEffect>(player_entity).unwrap();
        let session = world.resource::<EditorSession>();
        (
            player
                .instance
                .binding_trace()
                .map(|trace| trace.identity()),
            session
                .preview()
                .unwrap()
                .binding_trace()
                .map(|trace| trace.identity()),
        )
    };
    let (presented, edited) = traced(&app);
    assert!(
        presented.is_some(),
        "the rendered preview reads the stand-in"
    );
    assert_eq!(presented, edited, "both previews read the same recording");
    let saved = app
        .world()
        .resource::<EditorSession>()
        .effect
        .to_pretty_ron()
        .unwrap();
    assert!(!saved.contains("Orbit"), "a stand-in is never saved");

    app.world_mut()
        .resource_mut::<EditorSession>()
        .preview_mocks
        .clear();
    app.update();
    assert_eq!(traced(&app), (None, None));
}
