//! Actual persistent lifecycle and independent tick scheduler -> alpha producer -> draw queues.
//! The controller supplies time and compiled inputs; coupled/host scheduling is not emulated.
use crate::{
    alpha_sort_native as observer, draw_resources::*, effect_inputs::*, queue_native::asset,
    stateful_simulation::*,
};
use aestra_gpu::{GpuEffectArtifact, GpuRenderParams};
use bevy::{
    asset::{RenderAssetUsages, io::embedded::EmbeddedAssetRegistry},
    camera::{RenderTarget, ShadowLodOrigin, primitives::Aabb},
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderStartup, RenderSystems,
        render_asset::RenderAssets,
        render_resource::*,
        renderer::{
            RenderAdapterInfo, RenderContext, RenderDevice, RenderGraph, RenderGraphSystems,
        },
        storage::GpuShaderBuffer,
    },
};
use std::sync::Arc;

#[derive(Resource, Default)]
struct Dispatch {
    recorded_time: Option<f32>,
    pending_frames: u32,
    asynchronous_creation: bool,
}

fn simulate(
    mut context: RenderContext,
    cache: Res<PipelineCache>,
    pipeline: Option<Res<StatefulSimulationPipeline>>,
    effects: Query<(Entity, &GpuEffectBuffers)>,
    (buffers, device): (Res<RenderAssets<GpuShaderBuffer>>, Res<RenderDevice>),
    mut states: ResMut<StatefulStates>,
    mut recorded: ResMut<Dispatch>,
) {
    recorded.recorded_time = None;
    let Some(pipeline) = pipeline else { return };
    let (Some(death), Some(spawn), Some(present), Some(order)) = (
        cache.get_compute_pipeline(pipeline.death_integrate),
        cache.get_compute_pipeline(pipeline.spawn),
        cache.get_compute_pipeline(pipeline.present),
        cache.get_compute_pipeline(pipeline.order_present),
    ) else {
        recorded.pending_frames += 1;
        recorded.asynchronous_creation |= [
            pipeline.death_integrate,
            pipeline.spawn,
            pipeline.present,
            pipeline.order_present,
        ]
        .iter()
        .any(|id| {
            matches!(
                cache.get_compute_pipeline_state(*id),
                CachedPipelineState::Creating(_)
            )
        });
        return;
    };
    let layout = cache.get_bind_group_layout(&pipeline.layout);
    let absent = physics_scene_buffer(&device, &aestra_gpu::GpuWorldSdf::absent().words);
    for (entity, effect) in &effects {
        assert!(effect.stateful_only && !effect.has_trails && effect.event_links.is_empty());
        assert_eq!(
            effect.history_policy,
            aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly
        );
        let Some(persistent) = states.0.get_mut(&entity) else {
            continue;
        };
        let resolve = |handle| buffers.get(handle).map(|gpu| &gpu.buffer);
        let (Some(particles), Some(alive), Some(indirect), Some(counters)) = (
            resolve(&effect.particles),
            resolve(&effect.alive),
            resolve(&effect.indirect),
            resolve(&effect.counters),
        ) else {
            continue;
        };
        let render = StatefulRenderBuffers {
            particles,
            alive,
            indirect,
            counters,
            world: &absent,
            physics: &absent,
        };
        let encoder = context.command_encoder();
        encoder.clear_buffer(counters, 0, Some(4));
        for (persistent, dispatch) in persistent.iter_mut().zip(&effect.stateful_dispatch) {
            // Playback-only backward seeks reset to zero, then obey the same
            // bounded reconstruction budget as a fresh allocation.
            let before = if aestra_runtime::trace_tick(effect.simulation_time)
                < u64::from(persistent.last_tick)
            {
                0
            } else {
                persistent.last_tick
            };
            assert!(
                !persistent.restore_at(encoder, u32::MAX),
                "playback-only has no snapshots"
            );
            dispatch_stateful_effect(
                &device,
                encoder,
                death,
                spawn,
                present,
                Some(order),
                &layout,
                persistent,
                dispatch,
                &render,
                effect.simulation_time,
                effect.seek_quality,
            );
            assert!(
                persistent.last_tick - before
                    <= crate::catchup_pacing::stateful_catchup_budget(effect.seek_quality)
            );
            assert!(persistent.checkpoints.is_empty());
        }
        recorded.recorded_time = Some(effect.simulation_time);
    }
}

fn instance(capacity: u32, seed: u64, burst: bool) -> aestra_runtime::EffectInstance {
    use aestra_core::*;
    let mut effect = EffectAsset::new("Stateful integration", 10.);
    effect.playback_mode = EffectPlaybackMode::Once;
    let mut emitter = Emitter::basic_sprite("Persistent shell", 10.);
    emitter.max_particles = capacity;
    emitter.modules[0] =
        ModuleInstance::emission(if burst { 0. } else { 120. }, if burst { 5 } else { 0 });
    emitter.modules[1] = ModuleInstance::shape(EmitterShape::Sphere { radius: 0.2 });
    emitter.modules[2] = ModuleInstance::initialize(
        ScalarRange::new(0.4, 0.65),
        ScalarRange::new(0.4, 0.9),
        [0., 0., 1.],
        30.,
        ScalarRange::new(0., 0.),
    );
    emitter.modules[3] = ModuleInstance::motion([0., -0.4, 0.], 0.1, 0.02);
    emitter.modules.push(ModuleInstance::persistent());
    effect.materials[0].blend = BlendMode::Alpha;
    effect.emitters.push(emitter);
    let compiled = aestra_compiler::EffectCompiler::default()
        .compile(&effect)
        .unwrap();
    aestra_runtime::EffectInstance::with_seed(Arc::new(compiled), seed)
}

fn inputs(world: &mut World, instance: &aestra_runtime::EffectInstance) -> GpuEffectBuffers {
    let artifact = GpuEffectArtifact::from_instance(instance).unwrap();
    let emitter = &artifact.emitters[0];
    let compiled = &instance.effect().emitters[0];
    assert_eq!(artifact.simulation_state.records, emitter.max_particles);
    assert_eq!(emitter.velocity_distribution, 0);
    assert_eq!(emitter.shape_kind, 3);
    assert!(
        compiled.colliders.is_empty()
            && compiled.homing.is_none()
            && compiled.attachment.is_none()
            && compiled.field_follow.is_none()
            && compiled.domain_spawn.is_none()
    );
    let mut effect = crate::simulation_native::inputs(world, instance, 0.);
    effect.stateful_only = true;
    effect.seek_quality = aestra_runtime::SeekQuality::Preview;
    effect.host_events = Arc::new(HostEventHistory::of(instance));
    effect.stateful_dispatch = vec![StatefulDispatch {
        capacity: emitter.max_particles,
        slot_offset: emitter.slot_offset,
        emitter_index: 0,
        emitter_count: 1,
        spawn_rate: (emitter.spawn_rate.x + emitter.spawn_rate.y) * 0.5,
        burst_count: emitter.burst_count,
        burst_tick: (emitter.start_time / STATEFUL_TICK_DT).ceil().max(1.) as u32 - 1,
        speed: (emitter.speed.x, emitter.speed.y),
        lifetime: (emitter.lifetime.x, emitter.lifetime.y),
        direction: emitter.direction.to_array(),
        velocity_distribution: emitter.velocity_distribution,
        spread: emitter.spread_radians / std::f32::consts::FRAC_PI_2,
        drag: (emitter.drag.x + emitter.drag.y) * 0.5,
        turbulence: (emitter.turbulence.x + emitter.turbulence.y) * 0.5,
        shape_kind: 1,
        shape_radius: emitter.shape_radius,
        shape_half_extents: [0.; 3],
        gravity: emitter.gravity.to_array(),
        seed: instance.seed(),
        colliders: vec![],
        field_follow: None,
        domain_spawn: None,
        homing: None,
        homing_target: None,
        homing_tracker: default(),
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
        cutoffs: aestra_runtime::EmissionCutoffs {
            stop_tick: Some(70),
            kill_tick: None,
        },
        placement: aestra_runtime::SpawnPlacement::IDENTITY,
        appearance: StatefulAppearance {
            size: emitter.size,
            opacity: emitter.opacity,
            color: emitter.color,
            max_scale: emitter.max_scale,
        },
    }];
    effect
}

fn reference(dispatch: &StatefulDispatch) -> aestra_runtime::StatefulSimulation {
    use aestra_runtime::*;
    let mut cpu = StatefulSimulation::new(
        StatefulConfig {
            gravity: dispatch.gravity,
            spawn_per_tick: 2,
            speed: dispatch.speed,
            lifetime: dispatch.lifetime,
            direction: dispatch.direction,
            velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
            spread: dispatch.spread,
            drag: dispatch.drag,
            shape: SpawnShape::Sphere {
                radius: dispatch.shape_radius,
            },
            placement: dispatch.placement,
            turbulence: dispatch.turbulence,
            colliders: [aestra_core::Collider::NONE; MAX_COLLIDERS],
            collider_count: 0,
            capacity: dispatch.capacity,
            homing: None,
        },
        dispatch.seed,
    );
    cpu.set_cutoffs(dispatch.cutoffs);
    cpu
}

fn step(app: &mut App, owner: Entity, effect: &mut GpuEffectBuffers, tick: u32) {
    // A small positive subtick avoids f32 floor ambiguities while also exercising presentation.
    effect.simulation_time = tick as f32 * STATEFUL_TICK_DT + 0.0001;
    assert_eq!(
        aestra_runtime::trace_tick(effect.simulation_time),
        u64::from(tick)
    );
    app.world_mut().entity_mut(owner).insert(effect.clone());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        app.update();
        let world = app.sub_app(RenderApp).world();
        if world
            .resource::<StatefulStates>()
            .0
            .values()
            .next()
            .is_some_and(|states| states[0].last_tick == tick)
            && world.resource::<Dispatch>().recorded_time == Some(effect.simulation_time)
        {
            observer::settle(app, 2, true, 2);
            return;
        }
        std::thread::yield_now();
    }
    panic!("stateful scheduling did not converge to tick {tick}");
}

fn verify(
    app: &App,
    cameras: &[Entity; 2],
    effect: &GpuEffectBuffers,
    expected: &[(u64, [f32; 3])],
) -> Buffer {
    let world = app.sub_app(RenderApp).world();
    let buffers = world.resource::<RenderAssets<GpuShaderBuffer>>();
    let read =
        |handle, words| crate::trail_native::read(app, &buffers.get(handle).unwrap().buffer, words);
    assert_eq!(
        &read(&effect.indirect, 8)[..4],
        &[6, expected.len() as u32, 0, 0]
    );
    assert_eq!(read(&effect.counters, 2)[0], expected.len() as u32);
    let words = read(&effect.particles, effect.total_slots as usize * 12);
    let mut live = Vec::new();
    for (slot, particle) in words.as_chunks::<12>().0.iter().enumerate() {
        if particle[10] & 0xffff == 0 {
            continue;
        }
        let cpu = expected
            .iter()
            .find(|(ordinal, _)| *ordinal == u64::from(particle[11]))
            .unwrap();
        for (actual, expected) in particle[4..7].iter().map(|v| f32::from_bits(*v)).zip(cpu.1) {
            assert!(
                (actual - expected).abs() < 0.004,
                "CPU/GPU position {actual} != {expected}"
            );
        }
        live.push((slot as u32, f32::from_bits(particle[6])));
    }
    assert_eq!(live.len(), expected.len());
    let mut compacted = read(&effect.alive, effect.total_slots as usize)[..expected.len()].to_vec();
    compacted.sort_unstable();
    assert_eq!(
        compacted,
        live.iter().map(|(slot, _)| *slot).collect::<Vec<_>>()
    );
    for (index, camera) in cameras.iter().enumerate() {
        let (_, permutation) = observer::permutation(app, *camera);
        let mut order = live.clone();
        order.sort_by(|a, b| {
            let sign = if index == 0 { 1. } else { -1. };
            (a.1 * sign).total_cmp(&(b.1 * sign)).then(a.0.cmp(&b.0))
        });
        assert_eq!(
            &permutation[..order.len()],
            &order.iter().map(|v| v.0).collect::<Vec<_>>()
        );
        assert!(permutation[order.len()..].iter().all(|v| *v == u32::MAX));
    }
    let states = world
        .resource::<StatefulStates>()
        .0
        .values()
        .next()
        .unwrap();
    let state = &states[0];
    assert!(state.checkpoints.is_empty());
    assert_eq!(
        crate::trail_native::read(app, &state.free_count, 1)[0] as usize + live.len(),
        effect.total_slots as usize
    );
    state.state.clone()
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_stateful_simulation_feeds_alpha_and_installed_queues() {
    run_stateful(true);
}

#[cfg(feature = "async-qualification")]
#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_delayed_shader_recovers_stateful_simulation_and_draws() {
    run_stateful(false);
}

fn run_stateful(synchronous: bool) {
    let mut app = App::new();
    let plugins = DefaultPlugins
        .set(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        .set(RenderPlugin {
            synchronous_pipeline_compilation: synchronous,
            ..default()
        });
    // The asynchronous-cache gate retains local render-world inspection. The
    // separate host gate installs the actual pipelined renderer instead.
    #[cfg(feature = "async-qualification")]
    let plugins = plugins.disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>();
    app.add_plugins(plugins);
    crate::extraction::install(&mut app);
    app.init_resource::<crate::AestraRenderSettings>();
    let render = app.sub_app_mut(RenderApp);
    crate::render::install(render);
    crate::alpha_sort::install(render);
    render
        .init_resource::<crate::alpha_sort::GpuAlphaSortStatistics>()
        .init_resource::<TrailCompaction>()
        .init_resource::<TrailCulling>()
        .init_resource::<Submissions>()
        .init_resource::<Dispatch>()
        .init_resource::<StatefulStates>()
        .add_systems(RenderStartup, init_stateful_pipeline)
        .add_systems(
            Render,
            prepare_stateful_states.in_set(RenderSystems::PrepareBindGroups),
        )
        .add_systems(
            RenderGraph,
            simulate
                .in_set(SimulateEffects)
                .after(RenderGraphSystems::Begin)
                .before(RenderGraphSystems::Render),
        )
        .add_systems(
            RenderGraph,
            observer::observe
                .after(SortAlpha)
                .before(RenderGraphSystems::Render),
        )
        .add_systems(Render, observer::capture.in_set(RenderSystems::Cleanup));
    for (path, source) in [
        (
            "aestra_bevy_render/shaders/aestra_stateful_simulation.wgsl",
            aestra_gpu::stateful_simulation_wgsl(),
        ),
        (
            "aestra_bevy_render/shaders/alpha_sort.wgsl",
            include_str!("../../../bevy/aestra-bevy-render/src/gpu/alpha_sort.wgsl").to_owned(),
        ),
        (
            "aestra_bevy_render/shaders/aestra_sprite_render.wesl",
            aestra_gpu::shader::SPRITE_RENDER_WESL.to_owned(),
        ),
        (
            "aestra_bevy_render/shaders/aestra_mesh_wireframe.wesl",
            aestra_gpu::shader::mesh_wireframe_wesl(),
        ),
    ] {
        app.world()
            .resource::<EmbeddedAssetRegistry>()
            .insert_asset(
                std::path::PathBuf::from(path),
                std::path::Path::new(path),
                source.into_bytes(),
            );
    }
    app.finish();
    app.cleanup();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let info = world.resource::<RenderAdapterInfo>();
    assert_ne!(info.device_type, wgpu::DeviceType::Cpu);
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    println!(
        "Native stateful simulation integration: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    let device = world.resource::<RenderDevice>().clone();
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    observer::install_observer(world);
    app.world_mut()
        .resource_mut::<crate::AestraRenderSettings>()
        .transparent_order = crate::TransparentOrderMode::DepthBackToFront;
    let cameras = std::array::from_fn(|index| {
        let target =
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_target_texture(
                    64,
                    64,
                    TextureFormat::Bgra8UnormSrgb,
                    None,
                ));
        app.world_mut()
            .spawn((
                Camera3d::default(),
                ShadowLodOrigin,
                Camera {
                    order: index as isize,
                    ..default()
                },
                RenderTarget::Image(target.into()),
                Msaa::Off,
                Transform::from_xyz(0., 0., if index == 0 { 8. } else { -8. })
                    .looking_at(Vec3::ZERO, Vec3::Y),
            ))
            .id()
    });
    let world = app.world_mut();
    let texture = world.resource_mut::<Assets<Image>>().add(Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255; 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let material = crate::queue_native::material(
        world,
        &aestra_core::material::MaterialProgram::additive_sprite("Simulation"),
        "simulation",
    );
    let mut draw = crate::pipeline_native::draw(material);
    draw.semantic_material = None;
    draw.owner = world.spawn_empty().id();
    draw.texture = texture.clone();
    draw.fallback_texture = texture;
    draw.render_params = asset(world, GpuRenderParams::default());

    let mut effect = inputs(world, &instance(129, 7, false));
    let owner = draw.owner;
    world.entity_mut(owner).insert(effect.clone());
    let draw = world
        .spawn((draw, Aabb::from_min_max(Vec3::splat(-4.), Vec3::splat(4.))))
        .id();
    crate::simulation_native::bind_inputs(world, draw, &effect);
    #[cfg(feature = "async-qualification")]
    if !synchronous {
        delayed_shader(&mut app, owner, &mut effect);
        let mut cpu = reference(&effect.stateful_dispatch[0]);
        cpu.advance_to_tick(30);
        verify(&app, &cameras, &effect, &cpu.alive_particles());
        assert!(
            app.sub_app(RenderApp)
                .world()
                .resource::<Dispatch>()
                .asynchronous_creation
        );
    }
    let mut previous = None;
    for (capacity, seed) in [(129, 7), (129, 11), (257, 13)] {
        effect = inputs(app.world_mut(), &instance(capacity, seed, false));
        app.world_mut().entity_mut(owner).insert(effect.clone());
        crate::simulation_native::bind_inputs(app.world_mut(), draw, &effect);
        let mut cpu = reference(&effect.stateful_dispatch[0]);
        let mut allocation = None;
        for tick in [0, 3, 20, 44, 80, 120] {
            step(&mut app, owner, &mut effect, tick);
            cpu.advance_to_tick(u64::from(tick));
            let buffer = verify(&app, &cameras, &effect, &cpu.alive_particles());
            if let Some(existing) = allocation {
                assert_eq!(buffer.id(), existing, "persistent allocation reused");
            }
            if let Some(previous) = previous {
                assert_ne!(buffer.id(), previous, "new seed/capacity invalidates state");
            }
            allocation = Some(buffer.id());
            // Repeated-time presentation must not integrate or emit again.
            step(&mut app, owner, &mut effect, tick);
            verify(&app, &cameras, &effect, &cpu.alive_particles());
        }
        previous = allocation;
    }
    // A multi-tick batch must consume a one-shot burst exactly once.
    effect = inputs(app.world_mut(), &instance(129, 17, true));
    app.world_mut().entity_mut(owner).insert(effect.clone());
    crate::simulation_native::bind_inputs(app.world_mut(), draw, &effect);
    step(&mut app, owner, &mut effect, 3);
    let world = app.sub_app(RenderApp).world();
    let buffers = world.resource::<RenderAssets<GpuShaderBuffer>>();
    assert_eq!(
        crate::trail_native::read(&app, &buffers.get(&effect.indirect).unwrap().buffer, 4)[1],
        5
    );
    let state = &world
        .resource::<StatefulStates>()
        .0
        .values()
        .next()
        .unwrap()[0];
    assert_eq!(
        crate::trail_native::read(&app, &state.spawn_counter, 1)[0],
        5
    );
    app.world_mut().entity_mut(draw).despawn();
    app.world_mut().entity_mut(owner).despawn();
    observer::settle(&mut app, 0, false, 0);
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<StatefulStates>()
            .0
            .is_empty()
    );
    device
        .wgpu_device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    assert!(pollster::block_on(scope.pop()).is_none());
    println!(
        "stateful_integration ticks=0,3,20,44,80,120 capacities=129,257 seed_invalidation=true opposite_views=2 cpu_positions=true death_reuse=true stop_tick=70 bounded_preview=true burst_once=true repeated_time=true playback_only_no_checkpoints=true teardown=true test_only_readback=true coupled_host_scheduler_qualified=false"
    );
}

#[cfg(feature = "async-qualification")]
fn delayed_shader(app: &mut App, owner: Entity, effect: &mut GpuEffectBuffers) {
    use bevy::shader::Shader;
    // Process the canonical initializer's descriptors once, then retain their
    // actual layout/entries while withholding ONLY their shader dependency.
    app.update();
    let delayed = app.world().resource::<Assets<Shader>>().reserve_handle();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let pipeline = world.resource::<StatefulSimulationPipeline>();
    let cache = world.resource::<PipelineCache>();
    let ids = [
        pipeline.death_integrate,
        pipeline.spawn,
        pipeline.present,
        pipeline.order_present,
    ]
    .map(|id| {
        let mut descriptor = cache.get_compute_pipeline_descriptor(id).clone();
        descriptor.shader = delayed.clone();
        cache.queue_compute_pipeline(descriptor)
    });
    let mut pipeline = world.resource_mut::<StatefulSimulationPipeline>();
    [
        pipeline.death_integrate,
        pipeline.spawn,
        pipeline.present,
        pipeline.order_present,
    ] = ids;
    *world.resource_mut::<Dispatch>() = Dispatch::default();
    effect.simulation_time = 30. * STATEFUL_TICK_DT + 0.0001;
    app.world_mut().entity_mut(owner).insert(effect.clone());
    for _ in 0..8 {
        app.update();
        let world = app.sub_app(RenderApp).world();
        assert!(world.resource::<Dispatch>().recorded_time.is_none());
        for state in world.resource::<StatefulStates>().0.values().flatten() {
            assert_eq!(
                state.last_tick, 0,
                "missing shader must not consume simulation ticks"
            );
            assert_eq!(state.spawn_accumulator, 0.);
            assert!(state.checkpoints.is_empty());
        }
    }
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<Dispatch>()
            .pending_frames
            >= 8
    );
    let states = &app
        .sub_app(RenderApp)
        .world()
        .resource::<StatefulStates>()
        .0;
    assert_eq!(
        states.len(),
        1,
        "pending pipelines must retain their allocation"
    );
    let state = &states.values().next().unwrap()[0];
    assert_eq!(state.last_tick, 0);
    assert_eq!(
        crate::trail_native::read(app, &state.spawn_counter, 1)[0],
        0
    );
    app.world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(
            delayed.id(),
            Shader::from_wgsl(
                aestra_gpu::stateful_simulation_wgsl(),
                STATEFUL_SIMULATION_SHADER_PATH,
            ),
        )
        .unwrap();
    step(app, owner, effect, 30);
    println!(
        "delayed_shader frames=8 no_early_ticks=true async_creation=true resumed_tick=30 cpu_gpu_oracle=true"
    );
}
