//! Real payload ownership and lifecycle contracts, shared by the 0.19/0.20 adapters.
use super::super::{
    effect_inputs::{
        GpuEffectBuffers, HostEventHistory, StatefulAppearance, StatefulDispatch, TickSchedule,
        TrailContext,
    },
    particle_light_inputs::{
        AestraParticleLightSettings, Input, Inputs, ParticleLightArtifact, ParticleLightMode,
        ParticleLightSource,
    },
    particle_light_transport::{ParticleLightReadbackFrame, ParticleLightReadbackSettings},
    stage_inputs::{ExtractedStages, FieldViewTarget, VolumeFieldTarget},
};
use super::{MainEntity, render_entity, test_app};
use aestra_runtime::{PlaybackHistoryPolicy, SeekQuality};
use bevy::{
    asset::{Asset, uuid::Uuid},
    prelude::*,
    render::RenderApp,
};
use std::{sync::Arc, time::Duration};

macro_rules! equal_fields {
    ($actual:expr, $expected:expr; $($field:ident),+ $(,)?) => {
        $(assert_eq!($actual.$field, $expected.$field, stringify!($field));)+
    };
}
fn handle<A: Asset>(id: u128) -> Handle<A> {
    Handle::Uuid(Uuid::from_u128(id), Default::default())
}
fn effect() -> Arc<aestra_runtime::CompiledEffect> {
    Arc::new(
        aestra_compiler::EffectCompiler::default()
            .compile(&aestra_core::EffectAsset::new("Extraction state", 3.0))
            .unwrap(),
    )
}
fn dispatch() -> StatefulDispatch {
    let mut value = StatefulDispatch {
        capacity: 128,
        slot_offset: 0,
        emitter_index: 0,
        emitter_count: 1,
        spawn_rate: 24.0,
        burst_count: 0,
        burst_tick: 0,
        speed: (10.0, 14.0),
        lifetime: (1.0, 1.5),
        direction: [0.0, 1.0, 0.0],
        spread: 0.4,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone as u32,
        drag: 0.5,
        turbulence: 4.0,
        shape_kind: 1,
        shape_radius: 3.0,
        shape_half_extents: [0.0; 3],
        gravity: [0.0, -9.81, 0.0],
        seed: 42,
        colliders: Vec::new(),
        field_follow: None,
        domain_spawn: None,
        homing: None,
        homing_target: None,
        homing_tracker: aestra_runtime::HomingTracker::default(),
        attachment: None,
        arrival_word: None,
        homing_world_target: None,
        event_mask: 0,
        distance_emission: None,
        event_signature: 0,
        overflow_word: None,
        schedule: None,
        world_from_effect: aestra_runtime::IDENTITY_AFFINE,
        world_revision: 0,
        cutoffs: aestra_runtime::EmissionCutoffs::NONE,
        placement: aestra_runtime::SpawnPlacement::IDENTITY,
        appearance: StatefulAppearance {
            size: Default::default(),
            opacity: Default::default(),
            color: Default::default(),
            max_scale: 1.0,
        },
    };
    value.schedule = Some(Arc::new(TickSchedule {
        key: 92,
        homing: vec![None],
        placement: vec![aestra_runtime::SpawnPlacement::IDENTITY],
    }));
    value.distance_emission = Some((0.75, 8));
    value.arrival_word = Some(24);
    value.homing_world_target = Some([1.0, 2.0, 3.0]);
    value.overflow_word = Some(25);
    value.event_mask = 3;
    value.event_signature = 45;
    value.appearance.size.count = 1;
    value.appearance.opacity.count = 2;
    value.appearance.color.count = 3;
    value
}
fn buffers(policy: PlaybackHistoryPolicy, id: u128) -> GpuEffectBuffers {
    GpuEffectBuffers {
        emitters: handle(id),
        renderers: handle(id + 1),
        particles: handle(id + 2),
        alive: handle(id + 3),
        dead: handle(id + 4),
        counters: handle(id + 5),
        indirect: handle(id + 6),
        globals: handle(id + 7),
        aux: handle(id + 8),
        render_globals: handle(id + 9),
        workgroups: 2,
        has_ribbons: true,
        has_trails: true,
        ribbon_workgroups: 3,
        trail_workgroups: 4,
        trail_plan: aestra_gpu::TrailScratchPlan {
            aux_words: 64,
            max_heads: 8,
            max_owners: 4,
        },
        total_slots: 128,
        simulation_time: 1.5,
        seek_quality: SeekQuality::Exact,
        history_policy: policy,
        history_epoch: 7,
        statistics_token: 99,
        checkpoint_context: Arc::new(TrailContext {
            emitters: vec![1, 2, 3],
            key: [42; 22],
            motion: None,
        }),
        trail_roots: vec![(1, 2)],
        simulation_state: aestra_gpu::GpuSimulationState {
            stride: 16,
            records: 128,
        },
        stateful_dispatch: vec![dispatch()],
        event_links: vec![aestra_runtime::CompiledEventLink {
            source: 0,
            trigger: aestra_core::EventTrigger::OnDeath,
            target: 1,
            count: 3,
            inherit: 0.25,
        }],
        routed: true,
        particle_outputs: vec![(
            aestra_runtime::CompiledParticleOutput {
                output: "boom".into(),
                source: 0,
                trigger: aestra_core::EventTrigger::OnSpawn,
                aggregation: aestra_core::EventAggregation::FirstPerTick,
            },
            32,
        )],
        output_suppress_through: 17,
        host_events: Arc::new(HostEventHistory {
            bursts: vec![aestra_runtime::InputSpawnBurst {
                tick: 12,
                route: 0,
                target: 1,
                count: 3,
                events: vec![],
            }],
            events: vec![(12, 55)],
        }),
        physics: Arc::from([2, 4, 6]),
        stateful_only: true,
    }
}
fn assert_buffers(actual: &GpuEffectBuffers, expected: &GpuEffectBuffers) {
    equal_fields!(actual, expected; emitters, renderers, particles, alive, dead, counters, indirect, globals, aux, render_globals,
        workgroups, has_ribbons, has_trails, ribbon_workgroups, trail_workgroups, trail_plan, total_slots, simulation_time,
        seek_quality, history_policy, history_epoch, statistics_token, trail_roots, simulation_state, event_links, routed,
        particle_outputs, output_suppress_through, host_events, physics, stateful_only);
    assert!(Arc::ptr_eq(
        &actual.checkpoint_context,
        &expected.checkpoint_context
    ));
    assert!(actual.checkpoint_context == expected.checkpoint_context);
    assert!(Arc::ptr_eq(&actual.host_events, &expected.host_events));
    assert!(Arc::ptr_eq(&actual.physics, &expected.physics));
    assert_eq!(
        actual.stateful_dispatch.len(),
        expected.stateful_dispatch.len()
    );
    for (actual, expected) in actual
        .stateful_dispatch
        .iter()
        .zip(&expected.stateful_dispatch)
    {
        equal_fields!(actual, expected; capacity, slot_offset, emitter_index, emitter_count, spawn_rate, burst_count, burst_tick,
            speed, lifetime, direction, velocity_distribution, spread, drag, turbulence, shape_kind, shape_radius,
            shape_half_extents, gravity, seed, colliders, field_follow, domain_spawn, homing, homing_target, homing_tracker,
            attachment, arrival_word, homing_world_target, event_mask, distance_emission, event_signature, overflow_word,
            schedule, world_from_effect, world_revision, cutoffs, placement);
        assert_eq!(actual.appearance.max_scale, expected.appearance.max_scale);
        assert_eq!(
            format!("{:?}", actual.appearance.size),
            format!("{:?}", expected.appearance.size)
        );
        assert_eq!(
            format!("{:?}", actual.appearance.opacity),
            format!("{:?}", expected.appearance.opacity)
        );
        assert_eq!(
            format!("{:?}", actual.appearance.color),
            format!("{:?}", expected.appearance.color)
        );
        if let (Some(actual), Some(expected)) = (&actual.schedule, &expected.schedule) {
            assert!(Arc::ptr_eq(actual, expected));
        }
    }
}
fn stages(policy: PlaybackHistoryPolicy, id: u128) -> ExtractedStages {
    let layout = aestra_runtime::FieldLayout {
        resource: aestra_core::ResourceTypeId::new("org.aestra::field/density"),
        dims: [16, 24, 32],
        components: 1,
        origin: [-4.0, 2.0, -6.0],
        cell_size: 0.5,
        staggered: false,
        bricks: Some(aestra_runtime::BrickLayout {
            edge: 8,
            slots: 16,
            table: aestra_core::ResourceTypeId::new("org.aestra::field/table"),
            table_word: 2,
            slot_bricks_word: 12,
        }),
    };
    ExtractedStages {
        effect: effect(),
        output_identity: Arc::new(()),
        time: 2.5,
        quality: SeekQuality::Preview,
        history_policy: policy,
        host: aestra_gpu::GpuHostBindings {
            words: vec![5, 6, 7],
        },
        seed: 78,
        history_epoch: 4,
        history_epoch_start_time: 0.5,
        history_revision: 9,
        host_epoch: 6,
        world_to_effect: aestra_runtime::IDENTITY_AFFINE,
        coupled: true,
        view: Some(FieldViewTarget {
            image: handle::<Image>(id).id(),
            stage: 1,
            layout: layout.clone(),
            slice: 3,
            gain: 2.0,
        }),
        volumes: vec![VolumeFieldTarget {
            stage: 2,
            layout,
            image: handle::<Image>(id + 1).id(),
            table: Some(handle::<Image>(id + 2).id()),
        }],
        world: Some(aestra_gpu::GpuWorldSdf {
            revision: 84,
            words: Arc::from([3, 5, 7]),
        }),
    }
}
fn assert_stages(actual: &ExtractedStages, expected: &ExtractedStages) {
    assert!(Arc::ptr_eq(&actual.effect, &expected.effect));
    assert!(Arc::ptr_eq(
        &actual.output_identity,
        &expected.output_identity
    ));
    equal_fields!(actual, expected; time, quality, history_policy, host, seed, history_epoch, history_epoch_start_time, history_revision, host_epoch, world_to_effect, coupled);
    assert_eq!(actual.view.is_some(), expected.view.is_some());
    if let (Some(actual), Some(expected)) = (&actual.view, &expected.view) {
        equal_fields!(actual, expected; image, stage, layout, slice, gain);
    }
    assert_eq!(actual.volumes.len(), expected.volumes.len());
    for (actual, expected) in actual.volumes.iter().zip(&expected.volumes) {
        equal_fields!(actual, expected; stage, layout, image, table);
    }
    assert_eq!(actual.world.is_some(), expected.world.is_some());
    if let (Some(actual), Some(expected)) = (&actual.world, &expected.world) {
        equal_fields!(actual, expected; revision, words);
        assert!(Arc::ptr_eq(&actual.words, &expected.words));
    }
}
fn lights(owner: Entity, root: Entity) -> Inputs {
    let curve = aestra_runtime::CompiledCurve::compile(&aestra_core::Curve::new(vec![
        aestra_core::CurveKey::new(0.0, 12.0),
    ]));
    Inputs(vec![Input {
        source: ParticleLightSource {
            root,
            root_epoch: 2,
            clip_path: vec![aestra_core::EffectClipId::from_u128(123)],
            owner,
            owner_epoch: 3,
            revision: 4,
            effect: aestra_core::EffectId::from_u128(5),
            seed: 6,
            emitter: aestra_core::EmitterId::from_u128(7),
            region: aestra_core::EmitterRegionId::from_u128(8),
            output: aestra_core::SceneOutputId::from_u128(9),
            artifact: ParticleLightArtifact(effect()),
        },
        emitter_index: 2,
        offset: 64,
        count: 128,
        plan: aestra_runtime::ParticlePointLightPlan {
            color: aestra_runtime::ParticleLightColorPlan::Constant([0.1, 0.2, 0.3]),
            intensity: curve.clone(),
            range: curve,
            radius: 0.2,
            selection_policy: aestra_core::ParticleLightSelectionPolicy::Brightest,
            max_lights: 8,
            priority: 3,
        },
        parameters: Arc::from([aestra_runtime::RuntimeValue::Scalar(3.0)]),
    }])
}
fn assert_lights(actual: &Inputs, expected: &Inputs) {
    assert_eq!(actual.0.len(), expected.0.len());
    for (actual, expected) in actual.0.iter().zip(&expected.0) {
        equal_fields!(actual, expected; source, emitter_index, offset, count, plan, parameters);
        assert!(actual.source.artifact.matches(&expected.source.artifact.0));
        assert!(Arc::ptr_eq(&actual.parameters, &expected.parameters));
    }
}
fn assert_owner(app: &App, owner: Entity) {
    let render = app.sub_app(RenderApp).world();
    let entity = render_entity(app, owner);
    assert_eq!(render.get::<MainEntity>(entity).unwrap().id(), owner);
    assert_buffers(
        render.get::<GpuEffectBuffers>(entity).unwrap(),
        app.world().get::<GpuEffectBuffers>(owner).unwrap(),
    );
    assert_stages(
        render.get::<ExtractedStages>(entity).unwrap(),
        app.world().get::<ExtractedStages>(owner).unwrap(),
    );
    assert_lights(
        render.get::<Inputs>(entity).unwrap(),
        app.world().get::<Inputs>(owner).unwrap(),
    );
}
fn spawn(app: &mut App, root: Entity, policy: PlaybackHistoryPolicy, id: u128) -> Entity {
    let owner = app
        .world_mut()
        .spawn((buffers(policy, id), stages(policy, id + 100)))
        .id();
    app.world_mut()
        .entity_mut(owner)
        .insert(lights(owner, root));
    owner
}

#[test]
fn real_state_payloads_preserve_per_owner_data_for_both_history_policies() {
    let mut app = test_app();
    let root = app.world_mut().spawn_empty().id();
    let live = spawn(&mut app, root, PlaybackHistoryPolicy::PlaybackOnly, 1000);
    let replay = spawn(&mut app, root, PlaybackHistoryPolicy::ReplayEnabled, 2000);
    app.update();
    assert_owner(&app, live);
    assert_owner(&app, replay);
    let replay_before = app.world().get::<GpuEffectBuffers>(replay).unwrap().clone();
    {
        let mut value = app.world_mut().get_mut::<GpuEffectBuffers>(live).unwrap();
        *value = buffers(PlaybackHistoryPolicy::ReplayEnabled, 3000);
        value.history_epoch = 9;
        value.simulation_time = 0.25;
        value.seek_quality = SeekQuality::Preview;
        value.stateful_dispatch[0].world_revision = 98;
        value.stateful_dispatch[0].placement.translation = [4.0, 5.0, 6.0];
    }
    {
        let mut value = app.world_mut().get_mut::<ExtractedStages>(live).unwrap();
        *value = stages(PlaybackHistoryPolicy::ReplayEnabled, 4000);
        value.view = None;
        value.volumes.clear();
        value.world = None;
        value.coupled = false;
        value.history_epoch = 10;
        value.history_revision = 11;
        value.host_epoch = 12;
        value.world_to_effect[0][3] = -4.0;
    }
    {
        let mut value = app.world_mut().get_mut::<Inputs>(live).unwrap();
        value.0[0].source.owner_epoch += 1;
        value.0[0].source.root_epoch += 1;
        value.0[0].source.artifact = ParticleLightArtifact(effect());
        value.0[0].parameters = Arc::from([aestra_runtime::RuntimeValue::Scalar(9.0)]);
        value.0[0].count = 3;
    }
    app.update();
    assert_owner(&app, live);
    assert_owner(&app, replay);
    assert_buffers(
        app.sub_app(RenderApp)
            .world()
            .get::<GpuEffectBuffers>(render_entity(&app, replay))
            .unwrap(),
        &replay_before,
    );
}

#[test]
fn state_component_removal_reinsertion_and_despawn_do_not_leak_between_owners() {
    let mut app = test_app();
    let root = app.world_mut().spawn_empty().id();
    let removed = spawn(&mut app, root, PlaybackHistoryPolicy::PlaybackOnly, 1000);
    let retained = spawn(&mut app, root, PlaybackHistoryPolicy::ReplayEnabled, 2000);
    app.update();
    let mapped = render_entity(&app, removed);
    app.world_mut()
        .entity_mut(removed)
        .remove::<GpuEffectBuffers>();
    app.update();
    assert!(
        app.sub_app(RenderApp)
            .world()
            .get::<GpuEffectBuffers>(mapped)
            .is_none()
    );
    assert!(
        app.sub_app(RenderApp)
            .world()
            .get::<ExtractedStages>(mapped)
            .is_some()
    );
    assert!(
        app.sub_app(RenderApp)
            .world()
            .get::<Inputs>(mapped)
            .is_some()
    );
    assert_owner(&app, retained);
    app.world_mut()
        .entity_mut(removed)
        .remove::<(ExtractedStages, Inputs)>();
    app.update();
    assert!(
        app.sub_app(RenderApp)
            .world()
            .get::<ExtractedStages>(mapped)
            .is_none()
    );
    assert!(
        app.sub_app(RenderApp)
            .world()
            .get::<Inputs>(mapped)
            .is_none()
    );
    app.world_mut().entity_mut(removed).insert((
        buffers(PlaybackHistoryPolicy::ReplayEnabled, 3000),
        stages(PlaybackHistoryPolicy::ReplayEnabled, 4000),
        lights(removed, root),
    ));
    app.update();
    assert_owner(&app, removed);
    assert_owner(&app, retained);
    app.world_mut().despawn(removed);
    app.update();
    assert!(app.sub_app(RenderApp).world().get_entity(mapped).is_err());
    assert_owner(&app, retained);
}

#[test]
fn empty_light_selection_clears_previous_sources_and_preserves_root_identity() {
    let mut app = test_app();
    let root = app.world_mut().spawn_empty().id();
    let owner = spawn(&mut app, root, PlaybackHistoryPolicy::PlaybackOnly, 1000);
    app.update();
    let old = app.world().get::<Inputs>(owner).unwrap().0[0]
        .source
        .clone();
    let mut replacement = lights(owner, root);
    replacement.0[0]
        .source
        .clip_path
        .push(aestra_core::EffectClipId::from_u128(456));
    assert_ne!(old, replacement.0[0].source);
    assert_ne!(old.artifact, replacement.0[0].source.artifact);
    app.world_mut().entity_mut(owner).insert(replacement);
    app.update();
    assert_owner(&app, owner);
    app.world_mut().entity_mut(owner).insert(Inputs::default());
    app.update();
    assert_owner(&app, owner);
    assert!(
        app.sub_app(RenderApp)
            .world()
            .get::<Inputs>(render_entity(&app, owner))
            .unwrap()
            .0
            .is_empty()
    );
}

#[test]
fn light_selection_and_readback_resources_update_disable_and_wrap_frames() {
    let mut app = test_app();
    assert_eq!(AestraParticleLightSettings::default().max_lights, 0);
    assert_eq!(
        ParticleLightMode::default(),
        ParticleLightMode::PortableAsync
    );
    let mut settings = AestraParticleLightSettings {
        max_lights: 96,
        max_scratch_bytes: 8 * 1024 * 1024,
    };
    let mut transport = ParticleLightReadbackSettings::default();
    for (mode, frame, disable) in [
        (ParticleLightMode::PortableAsync, u64::MAX, false),
        (ParticleLightMode::SameFrameGpu, 0, true),
    ] {
        if disable {
            settings.max_lights = 0;
            settings.max_scratch_bytes = 0;
            transport = ParticleLightReadbackSettings {
                max_lights: 0,
                max_in_flight: 1,
                max_staging_bytes: 256,
                max_manifest_bytes: 512,
                max_age: Duration::from_millis(17),
                max_frame_lag: 2,
            };
        }
        app.insert_resource(settings)
            .insert_resource(mode)
            .insert_resource(transport.clone())
            .insert_resource(ParticleLightReadbackFrame(frame));
        app.update();
        let render = app.sub_app(RenderApp).world();
        assert_eq!(render.resource::<AestraParticleLightSettings>(), &settings);
        assert_eq!(render.resource::<ParticleLightMode>(), &mode);
        assert_eq!(
            render.resource::<ParticleLightReadbackSettings>(),
            &transport
        );
        assert_eq!(render.resource::<ParticleLightReadbackFrame>().0, frame);
    }
}
