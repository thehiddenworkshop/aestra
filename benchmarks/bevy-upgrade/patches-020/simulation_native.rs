//! Actual analytic setup/dispatch -> installed alpha producer -> installed draw queues.
//! The fixture supplies compiled inputs and time; it does not qualify the host scheduler.
use crate::{
    alpha_sort_native as observer,
    draw_resources::*,
    effect_inputs::*,
    queue_native::{asset, indirect_asset, update},
    simulation_pipeline::*,
};
use aestra_gpu::{GpuEffectArtifact, GpuGlobals, GpuRenderGlobals, GpuRenderParams};
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
    enabled: bool,
    recorded_time: Option<f32>,
}

// Test controller only: the production pipeline/layout/preparation and dispatch
// are path-imported. Stateful ticks, replay observations and lighting are not emulated.
fn simulate(
    mut context: RenderContext,
    cache: Res<PipelineCache>,
    pipeline: Option<Res<SimulationPipeline>>,
    effects: Query<(&GpuEffectBuffers, &GpuBindGroup)>,
    mut dispatch: ResMut<Dispatch>,
) {
    dispatch.recorded_time = None;
    if !dispatch.enabled {
        return;
    }
    let Some(pipeline) = pipeline else {
        return;
    };
    let (Some(reset), Some(particles)) = (
        cache.get_compute_pipeline(pipeline.reset),
        cache.get_compute_pipeline(pipeline.simulate),
    ) else {
        return;
    };
    // Qualify all actual analytic descriptors, including the deferred trail entries.
    if [pipeline.link_ribbons, pipeline.update_trails]
        .into_iter()
        .chain(pipeline.paged_trails)
        .any(|id| cache.get_compute_pipeline(id).is_none())
    {
        return;
    }
    for (effect, group) in &effects {
        assert!(!effect.has_trails && !effect.has_ribbons && !effect.stateful_only);
        assert!(effect.stateful_dispatch.is_empty());
        assert_eq!(effect.simulation_state.records, 0);
        assert_eq!(
            effect.history_policy,
            aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly
        );
        record_particles(
            context.command_encoder(),
            group,
            effect,
            reset,
            particles,
            None,
            None,
        );
        dispatch.recorded_time = Some(effect.simulation_time);
    }
}

fn instance(capacity: u32, seed: u64) -> aestra_runtime::EffectInstance {
    use aestra_core::*;
    let mut effect = EffectAsset::new("Analytic integration", 6.);
    effect.playback_mode = EffectPlaybackMode::Once;
    let mut emitter = Emitter::basic_sprite("Shell", 6.);
    emitter.max_particles = capacity;
    emitter.regions = vec![EmitterRegion::new(0.25, 0., 2.)];
    emitter.modules[0] = ModuleInstance::emission(80., 9);
    emitter.modules[1] = ModuleInstance::shape(EmitterShape::Sphere { radius: 0.5 });
    emitter.modules[2] = ModuleInstance::initialize(
        ScalarRange::new(1.2, 1.8),
        ScalarRange::new(0.4, 1.2),
        [0., 1., 0.],
        120.,
        ScalarRange::new(-0.2, 0.2),
    );
    emitter.modules[3] = ModuleInstance::motion([0., -0.2, 0.], 0.2, 0.);
    effect.materials[0].blend = BlendMode::Alpha;
    effect.emitters.push(emitter);
    let compiled = Arc::new(
        aestra_compiler::EffectCompiler::default()
            .compile(&effect)
            .unwrap(),
    );
    aestra_runtime::EffectInstance::with_seed(compiled, seed)
}

fn globals(instance: &aestra_runtime::EffectInstance, capacity: u32, time: f32) -> GpuGlobals {
    GpuGlobals {
        time,
        total_slots: capacity,
        seed: aestra_gpu::fold_seed(instance.seed()),
        emitter_count: instance.effect().emitters.len() as u32,
        duration: instance.effect().duration,
        ..default()
    }
}

pub(super) fn inputs(
    world: &mut World,
    instance: &aestra_runtime::EffectInstance,
    time: f32,
) -> GpuEffectBuffers {
    let mut artifact = GpuEffectArtifact::from_instance(instance).unwrap();
    assert!(
        artifact
            .particles
            .iter()
            .all(|p| p.packed_emitter_alive == 0)
    );
    let slots = artifact.total_slots;
    let has_trails = artifact.renderers.iter().any(|r| r.renderer_kind == 4);
    let has_ribbons = artifact.renderers.iter().any(|r| r.renderer_kind == 3);
    let emitters = artifact.emitters.len() as u32;
    let trail_roots = artifact
        .emitters
        .iter()
        .enumerate()
        .filter(|(_, e)| e.trail_points >= 2)
        .map(|(i, e)| (i as u32, e.trail_offset))
        .collect();
    let trail_plan = aestra_gpu::TrailScratchPlan::configure(
        &mut artifact.emitters,
        artifact.particles.len() as u32,
    )
    .unwrap();
    let commands = aestra_gpu::indirect_draw_commands_with_statistics(&artifact.emitters);
    GpuEffectBuffers {
        emitters: asset(world, artifact.emitters),
        renderers: asset(world, artifact.renderers),
        particles: asset(world, artifact.particles),
        alive: asset(world, vec![0u32; slots as usize]),
        dead: asset(world, vec![0u32; slots as usize]),
        counters: asset(
            world,
            vec![0u32; (2 + if has_trails { 6 * emitters } else { 0 }) as usize],
        ),
        indirect: indirect_asset(world, commands),
        globals: asset(world, globals(instance, slots, time)),
        aux: asset(
            world,
            vec![
                0u32;
                if has_trails || has_ribbons {
                    trail_plan.aux_words as usize
                } else {
                    1
                }
            ],
        ),
        render_globals: asset(
            world,
            GpuRenderGlobals {
                time,
                seed: aestra_gpu::fold_seed(instance.seed()),
                ..default()
            },
        ),
        workgroups: slots.div_ceil(aestra_gpu::WORKGROUP_SIZE),
        has_ribbons,
        has_trails,
        ribbon_workgroups: emitters.div_ceil(aestra_gpu::WORKGROUP_SIZE),
        trail_workgroups: emitters,
        trail_plan,
        total_slots: slots,
        simulation_time: time,
        seek_quality: aestra_runtime::SeekQuality::Exact,
        history_policy: aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly,
        history_epoch: 0,
        statistics_token: 0,
        checkpoint_context: default(),
        trail_roots,
        simulation_state: artifact.simulation_state,
        stateful_dispatch: vec![],
        event_links: vec![],
        routed: false,
        particle_outputs: vec![],
        output_suppress_through: 0,
        host_events: default(),
        physics: Arc::from([]),
        stateful_only: false,
    }
}

pub(super) fn bind_inputs(world: &mut World, entity: Entity, effect: &GpuEffectBuffers) {
    let mut draw = world.get_mut::<crate::GpuDrawInstance>(entity).unwrap();
    draw.particles = effect.particles.clone();
    draw.renderers = effect.renderers.clone();
    draw.alive = effect.alive.clone();
    draw.aux = effect.aux.clone();
    draw.indirect = effect.indirect.clone();
    draw.render_globals = effect.render_globals.clone();
    draw.sort_range = UVec2::new(0, effect.total_slots);
}

fn verify(
    app: &App,
    cameras: &[Entity; 2],
    effect: &GpuEffectBuffers,
    instance: &mut aestra_runtime::EffectInstance,
) {
    assert_eq!(
        app.sub_app(RenderApp)
            .world()
            .resource::<Dispatch>()
            .recorded_time,
        Some(effect.simulation_time)
    );
    instance.seek(effect.simulation_time);
    let mut expected = Vec::new();
    instance.evaluate(&mut expected);
    assert_eq!(
        expected.is_empty(),
        effect.simulation_time == 0.1 || effect.simulation_time == 5.
    );
    assert!(expected.len() < effect.total_slots as usize);
    let buffers = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderAssets<GpuShaderBuffer>>();
    let read =
        |handle, words| crate::trail_native::read(app, &buffers.get(handle).unwrap().buffer, words);
    let command = read(&effect.indirect, 8);
    assert_eq!(&command[..4], &[4, expected.len() as u32, 0, 0]);
    assert_eq!(command[4], 0xae57a001, "production reset telemetry");
    assert_eq!(f32::from_bits(command[7]), effect.simulation_time);
    assert_eq!(read(&effect.counters, 2)[0], expected.len() as u32);
    let words = read(&effect.particles, effect.total_slots as usize * 12);
    let mut live = Vec::new();
    for (slot, particle) in words.as_chunks::<12>().0.iter().enumerate() {
        if particle[10] & 0xffff == 0 {
            continue;
        }
        let cpu = expected
            .iter()
            .find(|p| p.particle_index == particle[11])
            .unwrap();
        for (actual, expected) in particle[4..7]
            .iter()
            .map(|v| f32::from_bits(*v))
            .zip(cpu.position)
        {
            assert!(
                (actual - expected).abs() < 0.004,
                "CPU/GPU position {actual} != {expected}"
            );
        }
        assert!((f32::from_bits(particle[7]) - cpu.size).abs() < 0.004);
        assert!((f32::from_bits(particle[9]) - cpu.normalized_age).abs() < 0.004);
        live.push((slot as u32, cpu.position[2]));
    }
    assert_eq!(live.len(), expected.len());
    let alive = read(&effect.alive, effect.total_slots as usize);
    let mut compacted = alive[..expected.len()].to_vec();
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
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_analytic_simulation_feeds_alpha_and_installed_queues() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            }),
    );
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
        .add_systems(RenderStartup, init_pipeline)
        .add_systems(
            Render,
            prepare_bind_groups.in_set(RenderSystems::PrepareBindGroups),
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
            "aestra_bevy_render/shaders/aestra_simulation.wesl",
            aestra_gpu::shader::SIMULATION_WESL.to_owned(),
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
        "Native analytic simulation integration: {} / {:?} / {}",
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
    let mut reference = instance(257, 12345);
    let mut effect = inputs(world, &reference, 0.1);
    let owner = draw.owner;
    world.entity_mut(owner).insert(effect.clone());
    let draw = world
        .spawn((draw, Aabb::from_min_max(Vec3::splat(-4.), Vec3::splat(4.))))
        .id();
    bind_inputs(world, draw, &effect);
    observer::settle(&mut app, 2, true, 2);
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<Dispatch>()
            .recorded_time
            .is_none()
    );
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<Dispatch>()
        .enabled = true;
    // Time is input; every particle, compacted index and indirect count is GPU-generated.
    for time in [0.1, 0.5, 1.25, 3., 5.] {
        effect.simulation_time = time;
        update(
            app.world_mut(),
            &effect.globals,
            globals(&reference, effect.total_slots, time),
        );
        app.world_mut().entity_mut(owner).insert(effect.clone());
        for _ in 0..4 {
            app.update();
        }
        observer::settle(&mut app, 2, true, 2);
        verify(&app, &cameras, &effect, &mut reference);
    }
    // Replace all inputs at the same capacity, then grow across a sort merge boundary.
    for (capacity, seed) in [(257, 67890), (513, 54321)] {
        reference = instance(capacity, seed);
        effect = inputs(app.world_mut(), &reference, 1.25);
        app.world_mut().entity_mut(owner).insert(effect.clone());
        bind_inputs(app.world_mut(), draw, &effect);
        for _ in 0..4 {
            app.update();
        }
        observer::settle(&mut app, 2, true, 2);
        verify(&app, &cameras, &effect, &mut reference);
    }
    app.world_mut().entity_mut(draw).despawn();
    app.world_mut().entity_mut(owner).despawn();
    observer::settle(&mut app, 0, false, 0);
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<Dispatch>()
            .recorded_time
            .is_none()
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
        "analytic_integration times=5 capacities=257,513 opposite_views=2 cpu_positions=true gpu_compaction=true gpu_indirect=true zero_expired=true source_rebind=true playback_only=true teardown=true test_only_readback=true host_scheduler_qualified=false"
    );
}
