//! Canonical host clock/presentation and production stage preparation on Bevy 0.20.
use crate::{
    AestraOutputEvent, EffectPlayer, PresentedEffect,
    catchup_pacing::CatchupPacer,
    host_playback::{advance_players, prepare_player_presentations, sync_player_presentations},
    stage_inputs::ExtractedStages,
    stage_output_delivery::{
        AestraEffectOutputs, OutputEvents, StageOutputIdentity, StageOutputMailbox,
        StageOutputStamp, encode_stage_outputs, receive_stage_outputs,
    },
    stage_runtimes::{StageRuntimes, prepare_stage_runtimes},
};
use bevy::{
    extract::{extract_component::ExtractComponentPlugin, sync_world::MainEntity},
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        renderer::{
            RenderAdapterInfo, RenderContext, RenderDevice, RenderGraph, RenderGraphSystems,
        },
    },
    time::TimeUpdateStrategy,
};
use std::{collections::BTreeMap, time::Duration};

#[derive(Resource, Default)]
struct Observed {
    events: Vec<AestraOutputEvent>,
}
#[derive(Resource, Default)]
struct Progress(BTreeMap<Entity, u32>);

#[cfg(feature = "async-qualification")]
#[derive(Resource, Default, Clone)]
struct ThreadProgress(std::sync::Arc<std::sync::Mutex<ThreadSnapshot>>);
#[cfg(feature = "async-qualification")]
#[derive(Default)]
struct ThreadSnapshot {
    thread: Option<std::thread::ThreadId>,
    owners: BTreeMap<Entity, (u32, StageOutputIdentity)>,
}

fn inputs(mut commands: Commands, effects: Query<(Entity, &PresentedEffect)>) {
    for (owner, presented) in &effects {
        commands
            .entity(owner)
            .insert(ExtractedStages::from_presented(
                presented,
                aestra_runtime::IDENTITY_AFFINE,
                None,
                Vec::new(),
                None,
            ));
    }
}
fn collect(mut messages: MessageReader<AestraOutputEvent>, mut observed: ResMut<Observed>) {
    observed.events.extend(messages.read().cloned());
}
// A narrow graph driver: the clock, extraction, allocation/reset logic, timeline,
// output encoding and delivery are production code. Volumes/profiling are not installed.
fn simulate(
    mut context: RenderContext,
    device: Res<RenderDevice>,
    effects: Query<(Entity, &MainEntity, &ExtractedStages)>,
    mut runtimes: ResMut<StageRuntimes>,
    mailbox: Res<StageOutputMailbox>,
    mut progress: ResMut<Progress>,
    #[cfg(feature = "async-qualification")] threaded: Res<ThreadProgress>,
) {
    progress.0.clear();
    #[cfg(feature = "async-qualification")]
    let mut threaded = threaded.0.lock().unwrap();
    #[cfg(feature = "async-qualification")]
    {
        threaded.thread = Some(std::thread::current().id());
        threaded.owners.clear();
    }
    for (entity, owner, extracted) in &effects {
        let runtime = runtimes.0.get_mut(&entity).unwrap();
        for (index, stage) in runtime.timelines.iter_mut().enumerate() {
            let stage = stage.as_mut().expect("production stage preparation failed");
            let target = stage.tick_for_time(extracted.time);
            let report = stage
                .advance_to(
                    device.wgpu_device(),
                    context.command_encoder(),
                    target,
                    4,
                    extracted.inputs(),
                    None,
                )
                .unwrap();
            assert!(report.ticks <= 4);
            assert_eq!(
                stage.checkpoint_bytes(),
                0,
                "playback-only retains no snapshots"
            );
            let tick = stage.last_tick();
            progress.0.insert(owner.id(), tick);
            #[cfg(feature = "async-qualification")]
            threaded.owners.insert(
                owner.id(),
                (tick, StageOutputIdentity::of_extracted(extracted)),
            );
            let stamp = StageOutputStamp {
                owner: owner.id(),
                stage: index,
                tick: u64::from(tick),
                identity: StageOutputIdentity::of_extracted(extracted),
            };
            if runtime.read_ticks[index].as_ref() != Some(&stamp)
                && encode_stage_outputs(
                    stage.executor(),
                    device.wgpu_device(),
                    context.command_encoder(),
                    &mailbox,
                    stamp.clone(),
                )
            {
                runtime.read_ticks[index] = Some(stamp);
            }
        }
    }
}
fn tick(app: &App, owner: Entity) -> u32 {
    app.sub_app(RenderApp).world().resource::<Progress>().0[&owner]
}
fn target(app: &App, owner: Entity) -> u32 {
    aestra_runtime::trace_tick(
        app.world()
            .get::<PresentedEffect>(owner)
            .unwrap()
            .instance
            .time(),
    ) as u32
}
fn advance(app: &mut App, owner: Entity, frames: u32) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        f64::from(frames) / 60.,
    )));
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .playing = true;
    app.update();
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .playing = false;
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
}
fn settle(app: &mut App, owner: Entity) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        app.update();
        if tick(app, owner) == target(app, owner)
            && app
                .world()
                .get::<OutputEvents>(owner)
                .and_then(|v| v.accepted_tick(0))
                == Some(u64::from(target(app, owner)))
        {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "host stage schedule timed out"
        );
        std::thread::yield_now();
    }
}

fn host_app() -> (App, RenderDevice) {
    configured_host_app(false)
}

fn configured_host_app(pipelined: bool) -> (App, RenderDevice) {
    let mut app = App::new();
    let plugins = DefaultPlugins
        .set(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        .set(RenderPlugin {
            synchronous_pipeline_compilation: !pipelined,
            ..default()
        });
    #[cfg(feature = "async-qualification")]
    let plugins = if pipelined {
        plugins
    } else {
        plugins.disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
    };
    app.add_plugins(plugins);
    let mailbox = StageOutputMailbox::default();
    app.insert_resource(mailbox.clone())
        .init_resource::<Observed>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
        .add_message::<AestraOutputEvent>()
        .add_plugins(ExtractComponentPlugin::<ExtractedStages, RenderApp>::default())
        .add_systems(PreUpdate, receive_stage_outputs)
        .configure_sets(
            Update,
            crate::AestraSet::ResolveHostInputs.before(crate::AestraSet::Playback),
        )
        .add_systems(
            Update,
            (
                crate::bindings::apply_binding_traces,
                crate::bindings::resolve_host_bindings,
            )
                .chain()
                .in_set(crate::AestraSet::ResolveHostInputs),
        )
        .add_systems(
            Update,
            (
                prepare_player_presentations,
                advance_players,
                crate::project::sync_project_instances,
                crate::bindings::forward_project_bindings,
                sync_player_presentations,
            )
                .chain()
                .in_set(crate::AestraSet::Playback),
        )
        .add_systems(Update, inputs.after(crate::AestraSet::Playback))
        .add_systems(Last, collect);
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(Duration::from_secs(2));
    #[cfg(feature = "async-qualification")]
    let threaded = ThreadProgress::default();
    #[cfg(feature = "async-qualification")]
    app.insert_resource(threaded.clone());
    let render = app.sub_app_mut(RenderApp);
    #[cfg(feature = "async-qualification")]
    render.insert_resource(threaded);
    render
        .insert_resource(mailbox)
        .init_resource::<StageRuntimes>()
        .init_resource::<CatchupPacer>()
        .init_resource::<Progress>()
        .add_systems(
            Render,
            prepare_stage_runtimes.in_set(RenderSystems::PrepareResources),
        )
        .add_systems(
            RenderGraph,
            simulate
                .after(RenderGraphSystems::Begin)
                .before(RenderGraphSystems::Render),
        );
    app.finish();
    let device = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderDevice>()
        .clone();
    let info = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderAdapterInfo>();
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    assert_ne!(info.device_type, wgpu::DeviceType::Cpu);
    println!(
        "Native host stage schedule: {} / {:?}",
        info.name, info.backend
    );
    // PipelinedRenderingPlugin moves RenderApp to its real render thread here.
    app.cleanup();
    (app, device)
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_host_clock_drives_stage_reset_and_suppressed_seek_outputs() {
    aestra_fluid::link();
    let registry = aestra_compiler::ExtensionRegistry::linked();
    let effect = super::stage_outputs_native::plate(&registry, 5., false, true);
    let (mut app, device) = host_app();
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let mut player = EffectPlayer::from_compiled(effect.clone())
        .with_history_policy(aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly);
    player.playing = false;
    let owner = app.world_mut().spawn(player).id();
    app.update();
    assert_eq!(target(&app, owner), 0);
    advance(&mut app, owner, 20);
    assert_eq!(
        target(&app, owner),
        20,
        "canonical host clock advanced exactly once"
    );
    settle(&mut app, owner);
    assert_eq!(tick(&app, owner), 20);

    // Restart followed by a large first live frame must reset even though the
    // requested tick is greater than the preceding simulation tick.
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .restart();
    advance(&mut app, owner, 30);
    assert_eq!(target(&app, owner), 30);
    assert_eq!(
        tick(&app, owner),
        4,
        "restart rebuilds before bounded catch-up"
    );
    settle(&mut app, owner);

    // Forward seek crosses several real render submissions. Reconstructed force
    // values remain available, but no impact may escape to gameplay.
    app.world_mut().resource_mut::<Observed>().events.clear();
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .seek(1.5);
    settle(&mut app, owner);
    assert_eq!(target(&app, owner), 90);
    assert!(app.world().resource::<Observed>().events.is_empty());
    assert!(
        app.world()
            .get::<AestraEffectOutputs>(owner)
            .unwrap()
            .values()
            .iter()
            .any(|v| v.value.iter().any(|x| x.abs() > 0.00001))
    );
    advance(&mut app, owner, 1);
    settle(&mut app, owner);
    assert_eq!(
        app.world().resource::<Observed>().events.len(),
        1,
        "live impact resumes after seek"
    );

    app.world_mut().resource_mut::<Observed>().events.clear();
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .seek(0.5);
    settle(&mut app, owner);
    assert_eq!(tick(&app, owner), 30);
    assert!(app.world().resource::<Observed>().events.is_empty());

    // An explicit context edit and a new presentation reset compatible blocks.
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .instance_mut()
        .invalidate_history();
    app.update();
    assert_eq!(tick(&app, owner), 4);
    settle(&mut app, owner);
    app.world_mut()
        .entity_mut(owner)
        .insert(PresentedEffect::new(effect));
    app.update();
    assert_eq!(tick(&app, owner), 4);
    settle(&mut app, owner);
    app.world_mut().despawn(owner);
    app.update();
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<StageRuntimes>()
            .0
            .is_empty()
    );
    device
        .wgpu_device()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert!(pollster::block_on(scope.pop()).is_none());
    println!(
        "host clock, bounded catch-up, restart, forward/backward seek suppression, live resume, context/presentation resets and teardown passed"
    );
}

fn bound_project(
    registry: &aestra_compiler::ExtensionRegistry,
) -> std::sync::Arc<aestra_runtime::CompiledEffectProject> {
    use aestra_core::*;
    let mut leaf = super::stage_outputs_native::plate_asset(registry, 5., false, true);
    leaf.duration = 4.;
    leaf.bindings = vec![EffectBinding::spatial("Source", BindingUpdateMode::Live)];
    let source = leaf.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|m| m.module_type.0 == aestra_fluid::MODULE_DENSITY_SOURCE)
        .unwrap();
    source
        .property_sources
        .insert("position".into(), PropertySource::HostBinding);
    source.host_bindings.insert(
        "position".into(),
        HostFieldRef::new(leaf.bindings[0].id, AESTRA_FIELD_POSITION),
    );
    leaf.choreography_events.push(ChoreographyEvent::new(
        "Nested cue",
        1.,
        ChoreographyEventPayload::PlaySound {
            cue: "burst".into(),
        },
    ));
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: "Rate".into(),
        default: Value::Scalar(5.),
        exposed: true,
    };
    let parameter_id = parameter.id;
    leaf.parameters.push(parameter);
    let mut emitter = Emitter::basic_sprite("Parameter probe", 4.);
    emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_EMISSION)
        .unwrap()
        .bindings
        .insert("spawn_rate".into(), parameter_id);
    leaf.emitters.push(emitter);
    let mut parent = EffectAsset::new("Parent", 4.);
    parent.bindings = vec![EffectBinding::spatial("Origin", BindingUpdateMode::Live)];
    let mut clip = EffectClip::new(leaf.id, 0., 3.5);
    clip.source_offset = 0.25;
    clip.seed = EffectClipSeed::Fixed(47);
    clip.parameter_overrides
        .insert(parameter_id, Value::Scalar(7.));
    clip.binding_forwards
        .insert(leaf.bindings[0].id, parent.bindings[0].id);
    parent.effect_clips.push(clip);
    let mut root = EffectAsset::new("Root", 4.);
    root.bindings = vec![EffectBinding::spatial("Emitter", BindingUpdateMode::Live)];
    let mut clip = EffectClip::new(parent.id, 0., 3.5);
    clip.source_offset = 0.25;
    clip.binding_forwards
        .insert(parent.bindings[0].id, root.bindings[0].id);
    root.effect_clips.push(clip);
    std::sync::Arc::new(
        crate::EffectCompiler::with_extensions(registry.clone())
            .compile_resolved_project(&aestra_project::ResolvedEffectProject {
                root,
                dependencies: BTreeMap::from([(parent.id, parent), (leaf.id, leaf)]),
                material_programs: BTreeMap::new(),
                material_functions: BTreeMap::new(),
            })
            .unwrap(),
    )
}

fn leaf(app: &mut App, root: Entity) -> Entity {
    app.world_mut()
        .query::<(Entity, &crate::EffectClipInstance)>()
        .iter(app.world())
        .find(|(_, clip)| clip.root == root && clip.path.len() == 2)
        .unwrap()
        .0
}
fn force(app: &App, owner: Entity) -> f32 {
    app.world()
        .get::<AestraEffectOutputs>(owner)
        .unwrap()
        .values()
        .iter()
        .flat_map(|v| v.value.iter())
        .map(|v| v.abs())
        .sum()
}
fn assert_binding_upload(app: &mut App, owner: Entity, expected: [f32; 3]) {
    let presented = app.world().get::<PresentedEffect>(owner).unwrap();
    assert_eq!(
        presented.instance.binding_field(
            aestra_runtime::BindingSlot(0),
            &aestra_core::BindingFieldId::new(aestra_core::AESTRA_FIELD_POSITION)
        ),
        Some(expected.as_slice())
    );
    let host = aestra_gpu::GpuHostBindings::from_instance(&presented.instance);
    let world = app.sub_app_mut(RenderApp).world_mut();
    let extracted = world
        .query::<(&MainEntity, &ExtractedStages)>()
        .iter(world)
        .find(|(main, _)| main.id() == owner)
        .unwrap()
        .1;
    assert_eq!(
        extracted.host, host,
        "actual forwarded bindings reach render extraction"
    );
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_nested_project_bindings_drive_independent_stage_timelines() {
    use bevy::camera::visibility::RenderLayers;
    aestra_fluid::link();
    let registry = aestra_compiler::ExtensionRegistry::linked();
    let project = bound_project(&registry);
    let (mut app, device) = host_app();
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let inside = [0., 0.5, 0.];
    let outside = [100., 100., 100.];
    let first = app
        .world_mut()
        .spawn(GlobalTransform::from_translation(Vec3::from_array(inside)))
        .id();
    let second = app
        .world_mut()
        .spawn(GlobalTransform::from_translation(Vec3::from_array(outside)))
        .id();
    let spawn = |app: &mut App, object, layer| {
        let mut player = EffectPlayer::from_project(project.clone())
            .with_history_policy(aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly);
        player.playing = false;
        app.world_mut()
            .spawn((
                player,
                crate::AestraBindings::new().bind("Emitter", object),
                RenderLayers::layer(layer),
            ))
            .id()
    };
    let a = spawn(&mut app, first, 7);
    let b = spawn(&mut app, second, 8);
    app.update();
    let a_leaf = leaf(&mut app, a);
    let b_leaf = leaf(&mut app, b);
    assert_ne!(a_leaf, b_leaf);
    settle(&mut app, a_leaf);
    settle(&mut app, b_leaf);
    assert_binding_upload(&mut app, a_leaf, inside);
    assert_binding_upload(&mut app, b_leaf, outside);
    assert!(
        force(&app, a_leaf) > 0.00001,
        "bound source inside the grid produces real GPU force"
    );
    assert_eq!(
        force(&app, b_leaf),
        0.,
        "same artifact, other host binding: empty fluid grid"
    );
    let instance = &app.world().get::<PresentedEffect>(a_leaf).unwrap().instance;
    assert_eq!(instance.seed(), 47);
    assert_eq!(
        instance.parameter(instance.effect().parameters[0].source),
        Some(&aestra_runtime::RuntimeValue::Scalar(7.))
    );
    assert_eq!(
        app.world().get::<RenderLayers>(a_leaf),
        Some(&RenderLayers::layer(7))
    );
    assert_eq!(target(&app, a_leaf), 30, "two source offsets are composed");
    advance(&mut app, a, 20);
    settle(&mut app, a_leaf);
    assert_eq!(
        target(&app, a_leaf),
        50,
        "children follow one root clock, not their own clocks"
    );
    assert_eq!(tick(&app, b_leaf), 30, "other paused root is isolated");

    app.world_mut()
        .get_mut::<EffectPlayer>(a)
        .unwrap()
        .restart();
    advance(&mut app, a, 30);
    assert_eq!(
        leaf(&mut app, a),
        a_leaf,
        "restart retains compatible child entities"
    );
    assert_eq!(target(&app, a_leaf), 60);
    assert_eq!(
        tick(&app, a_leaf),
        4,
        "positive-offset child resets before bounded reconstruction"
    );
    settle(&mut app, a_leaf);
    let cues: Vec<_> = app
        .world()
        .resource::<Observed>()
        .events
        .iter()
        .filter(|event| event.effect == a)
        .collect();
    assert_eq!(cues.len(), 1, "nested authored cue reaches the root once");
    assert_eq!(cues[0].clip_path.len(), 2);
    assert!(cues[0].playback_epoch.is_some());

    app.world_mut().resource_mut::<Observed>().events.clear();
    app.world_mut()
        .get_mut::<EffectPlayer>(a)
        .unwrap()
        .seek(1.5);
    settle(&mut app, a_leaf);
    assert_eq!(tick(&app, a_leaf), 120);
    assert!(
        app.world().resource::<Observed>().events.is_empty(),
        "seek reconstruction must not replay cues or impacts"
    );
    advance(&mut app, a, 1);
    settle(&mut app, a_leaf);
    assert!(
        app.world()
            .resource::<Observed>()
            .events
            .iter()
            .any(|event| event.effect == a_leaf),
        "child-local stage impacts resume after the seek"
    );

    let epoch = app
        .world()
        .get::<PresentedEffect>(a_leaf)
        .unwrap()
        .instance
        .host_input_epoch();
    let replacement = app
        .world_mut()
        .spawn(GlobalTransform::from_translation(Vec3::from_array(inside)))
        .id();
    app.world_mut()
        .get_mut::<crate::AestraBindings>(a)
        .unwrap()
        .set("Emitter", replacement);
    app.update();
    assert_ne!(
        app.world()
            .get::<PresentedEffect>(a_leaf)
            .unwrap()
            .instance
            .host_input_epoch(),
        epoch,
        "entity identity is forwarded even with equal values"
    );
    settle(&mut app, a_leaf);
    assert_binding_upload(&mut app, a_leaf, inside);
    *app.world_mut()
        .get_mut::<GlobalTransform>(replacement)
        .unwrap() = GlobalTransform::from_translation(Vec3::from_array(outside));
    app.update();
    assert_binding_upload(&mut app, a_leaf, outside);
    app.world_mut()
        .get_mut::<EffectPlayer>(a)
        .unwrap()
        .restart();
    advance(&mut app, a, 30);
    settle(&mut app, a_leaf);
    assert_eq!(
        force(&app, a_leaf),
        0.,
        "restart consumes the changed live source, not the previous plume"
    );
    assert_eq!(tick(&app, b_leaf), 30);

    app.world_mut()
        .get_mut::<EffectPlayer>(a)
        .unwrap()
        .seek(0.25);
    settle(&mut app, a_leaf);
    assert_eq!(tick(&app, a_leaf), 45);
    app.world_mut()
        .get_mut::<EffectPlayer>(a)
        .unwrap()
        .seek(3.75);
    app.update();
    assert!(
        app.world().get_entity(a_leaf).is_err(),
        "expired clips are retired"
    );
    assert!(app.world().get_entity(b_leaf).is_ok());
    app.world_mut().despawn(a);
    app.world_mut().despawn(b);
    app.update();
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<StageRuntimes>()
            .0
            .is_empty()
    );
    device
        .wgpu_device()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert!(pollster::block_on(scope.pop()).is_none());
    println!(
        "two roots, nested source offsets, host capture/forwarding, GPU binding consumption, seed/parameter/layers, restart/seek, cue routing, retarget/motion and retirement passed"
    );
}

#[cfg(feature = "async-qualification")]
fn settle_pipelined(app: &mut App, owner: Entity) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        app.update();
        let presented = app.world().get::<PresentedEffect>(owner).unwrap();
        let identity = StageOutputIdentity::of_presented(presented);
        let tick = target(app, owner);
        let rendered = app.world().resource::<ThreadProgress>().0.lock().unwrap();
        if rendered
            .owners
            .get(&owner)
            .is_some_and(|(actual, context)| *actual == tick && *context == identity)
            && app
                .world()
                .get::<OutputEvents>(owner)
                .and_then(|v| v.accepted_tick(0))
                == Some(u64::from(tick))
        {
            assert_ne!(rendered.thread, Some(std::thread::current().id()));
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "pipelined host stage schedule timed out"
        );
        drop(rendered);
        std::thread::yield_now();
    }
}

#[cfg(feature = "async-qualification")]
#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_pipelined_project_outputs_survive_restart_seek_and_teardown() {
    aestra_fluid::link();
    let registry = aestra_compiler::ExtensionRegistry::linked();
    let project = bound_project(&registry);
    let (mut app, device) = configured_host_app(true);
    assert!(
        app.get_sub_app(RenderApp).is_none(),
        "RenderApp must actually move off-thread"
    );
    assert!(
        app.world()
            .contains_resource::<bevy::render::pipelined_rendering::RenderAppChannels>()
    );
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let spawn = |app: &mut App, position| {
        let source = app
            .world_mut()
            .spawn(GlobalTransform::from_translation(position))
            .id();
        let mut player = EffectPlayer::from_project(project.clone())
            .with_history_policy(aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly);
        player.playing = false;
        app.world_mut()
            .spawn((player, crate::AestraBindings::new().bind("Emitter", source)))
            .id()
    };
    let a = spawn(&mut app, Vec3::new(0., 0.5, 0.));
    let b = spawn(&mut app, Vec3::splat(100.));
    app.update();
    let a_leaf = leaf(&mut app, a);
    let b_leaf = leaf(&mut app, b);
    settle_pipelined(&mut app, a_leaf);
    settle_pipelined(&mut app, b_leaf);
    assert!(force(&app, a_leaf) > 0.00001);
    assert_eq!(force(&app, b_leaf), 0.);
    advance(&mut app, a, 20);
    settle_pipelined(&mut app, a_leaf);
    assert_eq!(target(&app, a_leaf), 50);
    assert_eq!(target(&app, b_leaf), 30);

    // Keep rendering in flight across transitions. Acceptance must use the
    // submission's identity, not whichever main-world presentation is current.
    app.world_mut()
        .get_mut::<EffectPlayer>(a)
        .unwrap()
        .restart();
    advance(&mut app, a, 30);
    settle_pipelined(&mut app, a_leaf);
    assert_eq!(target(&app, a_leaf), 60);
    let cues: Vec<_> = app
        .world()
        .resource::<Observed>()
        .events
        .iter()
        .filter(|event| event.effect == a)
        .collect();
    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].clip_path.len(), 2);
    app.world_mut().resource_mut::<Observed>().events.clear();
    app.world_mut()
        .get_mut::<EffectPlayer>(a)
        .unwrap()
        .seek(1.5);
    settle_pipelined(&mut app, a_leaf);
    assert_eq!(target(&app, a_leaf), 120);
    assert!(app.world().resource::<Observed>().events.is_empty());
    advance(&mut app, a, 1);
    settle_pipelined(&mut app, a_leaf);
    assert_eq!(
        app.world()
            .resource::<Observed>()
            .events
            .iter()
            .filter(|event| event.effect == a_leaf)
            .count(),
        1
    );
    let count = app.world().resource::<Observed>().events.len();
    for _ in 0..12 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<Observed>().events.len(),
        count,
        "paused reads must not duplicate outputs"
    );
    app.world_mut().resource_mut::<Observed>().events.clear();
    app.world_mut()
        .get_mut::<EffectPlayer>(a)
        .unwrap()
        .seek(0.25);
    settle_pipelined(&mut app, a_leaf);
    assert_eq!(target(&app, a_leaf), 45);
    assert!(app.world().resource::<Observed>().events.is_empty());
    app.world_mut()
        .get_mut::<EffectPlayer>(a)
        .unwrap()
        .seek(3.75);
    app.update();
    assert!(app.world().get_entity(a_leaf).is_err());
    settle_pipelined(&mut app, b_leaf);
    assert_eq!(force(&app, b_leaf), 0.);
    app.world_mut().despawn(a);
    app.world_mut().despawn(b);
    for _ in 0..8 {
        app.update();
    }
    assert!(
        app.world()
            .resource::<ThreadProgress>()
            .0
            .lock()
            .unwrap()
            .owners
            .is_empty()
    );
    assert!(app.world().resource::<Observed>().events.is_empty());
    // Drop uses Bevy's executor-pumping channel shutdown, before device checks.
    drop(app);
    device
        .wgpu_device()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert!(pollster::block_on(scope.pop()).is_none());
    println!(
        "pipelined_rendering actual_channels=true separate_thread=true nested_roots=2 live_gpu_forces=true restart=true seek_suppression=true live_resume_once=true paused_dedup=true teardown=true"
    );
}
