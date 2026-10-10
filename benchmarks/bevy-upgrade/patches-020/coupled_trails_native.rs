//! Production fluid domain + fixed-tick particles + actual joint trail observer.
//! Only authored inputs and bounded test readbacks are fixture-owned.

use crate::{
    coupled_simulation::*,
    draw_resources::*,
    effect_inputs::*,
    queue_native::{asset, update},
    simulation_pipeline::*,
    stateful_simulation::*,
    trail_native::{Captured, capture, read},
};
use aestra_gpu::{GpuEffectArtifact, GpuGlobals, GpuRenderParams};
use aestra_runtime::PlaybackHistoryPolicy;
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
            RenderQueue,
        },
        storage::GpuShaderBuffer,
        sync_world::MainEntity,
    },
};
use std::sync::Arc;

#[derive(Resource, Default)]
struct Recorded {
    tick: Option<u32>,
    max_ticks: u32,
    observed: u32,
}
#[derive(Resource, Default)]
struct Histories(crate::stateful_trails::Histories);
#[derive(Resource)]
struct Couplers {
    follower: crate::execution::FieldFollowPipeline,
    spawner: crate::execution::DomainSpawnPipeline,
    gatherer: crate::execution::EventGatherPipeline,
    domains: Vec<Option<crate::execution::StageTimeline>>,
}
fn simulate(
    mut context: RenderContext,
    cache: Res<PipelineCache>,
    pipelines: (
        Option<Res<SimulationPipeline>>,
        Option<Res<StatefulSimulationPipeline>>,
    ),
    effects: Query<(Entity, &GpuEffectBuffers, &GpuBindGroup)>,
    (buffers, device): (Res<RenderAssets<GpuShaderBuffer>>, Res<RenderDevice>),
    (mut states, mut histories): (ResMut<StatefulStates>, ResMut<Histories>),
    (mut recorded, mut couplers): (ResMut<Recorded>, ResMut<Couplers>),
) {
    recorded.tick = None;
    histories.0.retain(|entity, _| effects.get(*entity).is_ok());
    let (Some(analytic), Some(stateful)) = pipelines else {
        return;
    };
    let (
        Some(reset),
        Some(simulate),
        Some(update),
        Some(death),
        Some(spawn),
        Some(present),
        Some(order),
    ) = (
        cache.get_compute_pipeline(analytic.reset),
        cache.get_compute_pipeline(analytic.simulate),
        cache.get_compute_pipeline(analytic.update_trails),
        cache.get_compute_pipeline(stateful.death_integrate),
        cache.get_compute_pipeline(stateful.spawn),
        cache.get_compute_pipeline(stateful.present),
        cache.get_compute_pipeline(stateful.order_present),
    )
    else {
        return;
    };
    let paged = analytic
        .paged_trails
        .map(|id| cache.get_compute_pipeline(id))
        .into_iter()
        .collect::<Option<Vec<_>>>();
    let layout = cache.get_bind_group_layout(&stateful.layout);
    let absent = physics_scene_buffer(&device, &aestra_gpu::GpuWorldSdf::absent().words);
    for (entity, effect, group) in &effects {
        let Some(persistent) = states.0.get_mut(&entity) else {
            continue;
        };
        let resolve = |handle| buffers.get(handle).map(|gpu| &gpu.buffer);
        let (
            Some(particles),
            Some(alive),
            Some(dead),
            Some(counters),
            Some(indirect),
            Some(aux),
            Some(globals),
            Some(render_globals),
        ) = (
            resolve(&effect.particles),
            resolve(&effect.alive),
            resolve(&effect.dead),
            resolve(&effect.counters),
            resolve(&effect.indirect),
            resolve(&effect.aux),
            resolve(&effect.globals),
            resolve(&effect.render_globals),
        )
        else {
            continue;
        };
        let pages = if effect.trail_plan.paged() {
            let Some(pipelines) = paged.as_ref() else {
                continue;
            };
            Some(crate::paged_trails::Dispatch::new(
                &device,
                effect.trail_plan,
                pipelines.as_slice().try_into().unwrap(),
                effect.trail_workgroups,
            ))
        } else {
            None
        };
        let (source, history) = histories
            .0
            .entry(entity)
            .or_insert_with(|| (effect.particles.id(), default()));
        if *source != effect.particles.id() {
            *source = effect.particles.id();
            *history = default();
        }
        history.sync(effect, persistent);
        let mut observer = crate::stateful_trails::Observer {
            history,
            effect,
            group: &group.0,
            reset,
            simulate,
            update,
            paged: pages.as_ref(),
            ribbons: None,
            globals,
            render_globals,
            buffers: [particles, alive, dead, counters, indirect, aux],
            memory_budget: crate::trail_checkpoints::MEMORY_LIMIT,
            diagnostics: None,
            observations: 0,
        };
        let Couplers {
            domains,
            follower,
            spawner,
            gatherer,
        } = &mut *couplers;
        for domain in domains.iter_mut().flatten() {
            domain.set_history_policy(effect.history_policy);
        }
        let (ticks, _) = run_stateful_dispatches(
            &device,
            context.command_encoder(),
            (death, spawn, present, Some(order)),
            &layout,
            persistent,
            &effect.stateful_dispatch,
            &effect.event_links,
            Some(RouteWiring {
                bursts: &effect.host_events.bursts,
                outputs: &effect.particle_outputs,
                output_epoch: effect.history_epoch,
                output_suppress_through: effect.output_suppress_through,
            }),
            &StatefulRenderBuffers {
                particles,
                alive,
                indirect,
                counters,
                world: &absent,
                physics: &absent,
            },
            Some(Coupling {
                domains,
                inputs: default(),
                follower,
                spawner,
                gatherer,
            }),
            effect.simulation_time,
            effect.seek_quality,
            effect.statistics_token,
            effect.history_epoch,
            true,
            None,
            Some(&mut observer),
        )
        .unwrap();
        assert!(ticks <= crate::catchup_pacing::stateful_catchup_budget(effect.seek_quality));
        let tick = persistent[0].last_tick;
        assert_eq!(observer.history.tick, Some(tick));
        assert_eq!(observer.history.epoch, Some(effect.history_epoch));
        assert!(domains.iter().flatten().all(|d| d.last_tick() == tick));
        if effect.history_policy == PlaybackHistoryPolicy::PlaybackOnly {
            assert!(persistent.iter().all(|s| s.checkpoints.is_empty()));
            assert_eq!(observer.history.checkpoints.bytes(), 0);
            assert!(domains.iter().flatten().all(|d| d.checkpoint_bytes() == 0));
        }
        recorded.observed += observer.observations;
        recorded.max_ticks = recorded.max_ticks.max(ticks);
        recorded.tick = Some(tick);
    }
}

fn source(
    registry: &aestra_extension::ExtensionRegistry,
    capacity: u32,
) -> aestra_core::EffectAsset {
    use aestra_core::*;
    let mut source = aestra_fluid::smoke_effect(registry);
    let grid = &mut source.simulation_stages[0].modules[0];
    let ModuleParameters::Custom(values) = &mut grid.parameters else {
        panic!()
    };
    values.insert("resolution".into(), Value::U32(16));
    values.insert("cell_size".into(), Value::Scalar(6.));
    values.insert("center".into(), Value::Vec3([0.; 3]));
    let ModuleParameters::Custom(values) = &mut source.simulation_stages[0].modules[1].parameters
    else {
        panic!()
    };
    values.insert("position".into(), Value::Vec3([0.; 3]));
    source.playback_mode = EffectPlaybackMode::Once;
    let mut emitter = Emitter::basic_sprite("Fluid-following heads", 4.);
    emitter.max_particles = capacity;
    emitter.modules[0] = ModuleInstance::emission(0., 3);
    emitter.modules[1] = ModuleInstance::shape(EmitterShape::Point);
    emitter.modules[2] = ModuleInstance::initialize(
        ScalarRange::new(0.625, 0.625),
        ScalarRange::new(1., 1.),
        [1., 0., 0.],
        0.,
        ScalarRange::new(0., 0.),
    );
    emitter.modules[3] = ModuleInstance::motion([0.; 3], 0., 0.);
    emitter.modules.push(ModuleInstance::persistent());
    emitter.modules.push(ModuleInstance::follow_field(8.));
    emitter.renderers[0].renderer_type = RendererTypeId::new(RENDERER_TRAIL);
    emitter.renderers[0].properties = RendererProperties::Trail {
        width: 0.1,
        sample_interval: 0.125,
        lifetime: 0.75,
        max_points: 5,
        max_trails: capacity,
        sampling: TrailSamplingMode::Time,
        sample_distance: 0.1,
        curve_tolerance: 0.01,
        uv_mode: TrailUvMode::Stretch,
        tile_length: 1.,
        end_cap: TrailEndCap::Flat,
    };
    source.emitters = vec![emitter];
    source
}
fn instance(
    registry: &aestra_extension::ExtensionRegistry,
    capacity: u32,
    seed: u64,
) -> aestra_runtime::EffectInstance {
    let source = source(registry, capacity);
    let compiled = aestra_compiler::EffectCompiler::with_extensions(registry.clone())
        .compile(&source)
        .unwrap();
    assert!(compiled.emitters[0].field_follow.is_some());
    aestra_runtime::EffectInstance::with_seed(Arc::new(compiled), seed)
}
fn inputs(world: &mut World, instance: &aestra_runtime::EffectInstance) -> GpuEffectBuffers {
    let artifact = GpuEffectArtifact::from_instance(instance).unwrap();
    let mut effect = crate::simulation_native::inputs(world, instance, 0.);
    effect.stateful_only = true;
    effect.seek_quality = aestra_runtime::SeekQuality::Preview;
    effect.stateful_dispatch = artifact
        .emitters
        .iter()
        .enumerate()
        .map(|(index, emitter)| {
            let compiled = &instance.effect().emitters[index];
            assert_eq!(emitter.velocity_distribution, 0);
            assert_eq!(emitter.shape_kind, 0);
            assert!(
                compiled.colliders.is_empty()
                    && compiled.homing.is_none()
                    && compiled.attachment.is_none()
            );
            StatefulDispatch {
                capacity: emitter.max_particles,
                slot_offset: emitter.slot_offset,
                emitter_index: index as u32,
                emitter_count: artifact.emitters.len() as u32,
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
                shape_kind: 0,
                shape_radius: emitter.shape_radius,
                shape_half_extents: [0.; 3],
                gravity: emitter.gravity.to_array(),
                seed: instance.seed(),
                colliders: vec![],
                field_follow: compiled.field_follow.clone(),
                domain_spawn: compiled.domain_spawn.clone(),
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
                    stop_tick: None,
                    kill_tick: None,
                },
                placement: aestra_runtime::SpawnPlacement::IDENTITY,
                appearance: StatefulAppearance {
                    size: emitter.size,
                    opacity: emitter.opacity,
                    color: emitter.color,
                    max_scale: emitter.max_scale,
                },
            }
        })
        .collect();

    effect.history_epoch = 9;
    effect.statistics_token = 23;
    let mut context = crate::trail_context::TrailContext::default();
    for (target, value) in context.key[6..]
        .iter_mut()
        .zip(Mat4::IDENTITY.to_cols_array())
    {
        *target = value.to_bits();
    }
    effect.checkpoint_context = Arc::new(context);
    upload_globals(world, instance, &effect);
    effect
}
fn upload_globals(
    world: &mut World,
    instance: &aestra_runtime::EffectInstance,
    effect: &GpuEffectBuffers,
) {
    let mut globals = GpuGlobals {
        total_slots: effect.total_slots,
        emitter_count: 1,
        duration: 4.,
        seed: aestra_gpu::fold_seed(instance.seed()),
        ..default()
    };
    globals._padding.x = effect.history_epoch;
    globals._padding.y = effect.statistics_token;
    update(world, &effect.globals, globals);
}
fn step(app: &mut App, owner: Entity, effect: &mut GpuEffectBuffers, tick: u32) {
    effect.simulation_time = tick as f32 * STATEFUL_TICK_DT + 0.0001;
    app.world_mut().entity_mut(owner).insert(effect.clone());
    for _ in 0..180 {
        app.update();
        let world = app.sub_app(RenderApp).world();
        if world.resource::<Recorded>().tick == Some(tick)
            && world.resource::<Captured>().0.len() == 2
            && world.resource::<TrailCompaction>().dispatched
            && world.resource::<TrailCulling>().dispatched
        {
            return;
        }
        std::thread::yield_now();
    }
    panic!("coupled fluid/trail schedule did not converge to {tick}");
}
#[derive(PartialEq)]
struct Snapshot {
    particles: Vec<u32>,
    aux: Vec<u32>,
    density: Vec<u32>,
    velocity: Vec<u32>,
    candidates: Vec<u32>,
}
impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("candidates", &self.candidates)
            .finish_non_exhaustive()
    }
}
fn assert_same(actual: &Snapshot, expected: &Snapshot) {
    for (name, a, b) in [
        ("particles", &actual.particles, &expected.particles),
        ("aux", &actual.aux, &expected.aux),
        ("density", &actual.density, &expected.density),
        ("velocity", &actual.velocity, &expected.velocity),
        ("candidates", &actual.candidates, &expected.candidates),
    ] {
        assert_eq!(a.len(), b.len(), "{name} length");
        assert!(
            a == b,
            "{name} differs first at {:?}",
            a.iter().zip(b).position(|(a, b)| a != b)
        );
    }
}
fn snapshot(app: &App, effect: &GpuEffectBuffers, near: Entity) -> Snapshot {
    let world = app.sub_app(RenderApp).world();
    let buffers = world.resource::<RenderAssets<GpuShaderBuffer>>();
    let buffer = |handle| &buffers.get(handle).unwrap().buffer;
    let particles = read(
        app,
        buffer(&effect.particles),
        (effect.trail_roots[0].1 as usize + 1 + effect.total_slots as usize * 5) * 12,
    );
    // Paged passes rebuild transient sort/scan scratch even on a paused tick.
    // Only persistent owner/sample metadata is part of the history contract.
    let mut aux = read(app, buffer(&effect.aux), particles.len() / 4);
    // Discontinuity tags deliberately change on a restored epoch, not the recorded history.
    aux[effect.trail_roots[0].1 as usize * 3] = 0;
    let entry = world
        .resource::<TrailCompaction>()
        .entries
        .values()
        .next()
        .unwrap();
    let header = read(app, &entry.output, 4);
    let candidates = read(app, &entry.output, 4 + header[1] as usize);
    let views = world.resource::<TrailCulling>();
    for ((view, _), result) in &views.entries {
        let main = world.get::<MainEntity>(*view).unwrap().id();
        assert_eq!(
            read(app, &result.indirect, 4),
            [4, if main == near { header[1] } else { 0 }, 0, 0]
        );
    }
    let mut consumed = world
        .resource::<Captured>()
        .0
        .iter()
        .map(|draw| {
            assert!(matches!(draw.topology, Topology::Strip));
            let (buffer, offset) = draw.command.as_ref().unwrap();
            assert_eq!(*offset, 0);
            buffer.id()
        })
        .collect::<Vec<_>>();
    let mut produced = views
        .entries
        .values()
        .map(|v| v.indirect.id())
        .collect::<Vec<_>>();
    consumed.sort_unstable();
    produced.sort_unstable();
    assert_eq!(consumed, produced);
    let couplers = world.resource::<Couplers>();
    let domain = couplers.domains[0].as_ref().unwrap();
    let read_field = |name| {
        let raw = domain.executor().buffer(name).unwrap();
        read(app, raw, raw.size() as usize / 4)
    };
    Snapshot {
        particles,
        aux,
        candidates,
        density: read_field(aestra_fluid::RESOURCE_DENSITY),
        velocity: read_field(aestra_fluid::RESOURCE_VELOCITY),
    }
}
fn app() -> App {
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
    crate::trail_compaction::install(render);
    crate::trail_culling::install(render);
    render
        .init_resource::<AlphaSort>()
        .init_resource::<Submissions>()
        .init_resource::<Captured>()
        .init_resource::<crate::preparation_context::PreparationMailboxes>()
        .init_resource::<Recorded>()
        .init_resource::<Histories>()
        .init_resource::<StatefulStates>()
        .add_systems(RenderStartup, (init_pipeline, init_stateful_pipeline))
        .add_systems(
            Render,
            prepare_stateful_states.in_set(RenderSystems::PrepareBindGroups),
        )
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
        .add_systems(Render, capture.in_set(RenderSystems::Cleanup));
    for (name, source) in [
        (
            "aestra_simulation",
            aestra_gpu::shader::SIMULATION_WESL.to_owned(),
        ),
        (
            "aestra_sprite_render",
            aestra_gpu::shader::SPRITE_RENDER_WESL.to_owned(),
        ),
        (
            "aestra_mesh_wireframe",
            aestra_gpu::shader::mesh_wireframe_wesl(),
        ),
        (
            "aestra_trail_compact",
            aestra_gpu::shader::trail_compact_wesl(),
        ),
        ("aestra_trail_cull", aestra_gpu::shader::trail_cull_wesl()),
    ] {
        let path = format!("aestra_bevy_render/shaders/{name}.wesl");
        app.world()
            .resource::<EmbeddedAssetRegistry>()
            .insert_asset(
                std::path::PathBuf::from(&path),
                std::path::Path::new(&path),
                source.into_bytes(),
            );
    }
    let path = "aestra_bevy_render/shaders/aestra_stateful_simulation.wgsl";
    app.world()
        .resource::<EmbeddedAssetRegistry>()
        .insert_asset(
            std::path::PathBuf::from(path),
            std::path::Path::new(path),
            aestra_gpu::stateful_simulation_wgsl().into_bytes(),
        );
    app.finish();
    app.cleanup();
    app
}
fn install_domain(
    app: &mut App,
    instance: &aestra_runtime::EffectInstance,
    registry: &aestra_extension::ExtensionRegistry,
) {
    let device = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderDevice>()
        .clone();
    let queue = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderQueue>()
        .clone();
    let executor = crate::execution::StageExecutor::new(
        device.wgpu_device(),
        &queue,
        &instance.effect().extension_stages[0].block,
        &registry.programs,
        4,
    )
    .unwrap();
    app.sub_app_mut(RenderApp)
        .world_mut()
        .insert_resource(Couplers {
            follower: crate::execution::FieldFollowPipeline::new(device.wgpu_device()),
            spawner: crate::execution::DomainSpawnPipeline::new(device.wgpu_device()),
            gatherer: crate::execution::EventGatherPipeline::new(device.wgpu_device()),
            domains: vec![Some(crate::execution::StageTimeline::new(
                executor,
                default(),
                7,
            ))],
        });
}
fn scene(app: &mut App, distance: f32) -> (Entity, Entity, [Entity; 2]) {
    let cameras: [Entity; 2] = std::array::from_fn(|index| {
        let target =
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_target_texture(
                    64,
                    64,
                    TextureFormat::Bgra8UnormSrgb,
                    None,
                ));
        let x = index as f32 * 100.;
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
                Transform::from_xyz(x, 0., distance).looking_at(Vec3::new(x, 0., 0.), Vec3::Y),
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
        &aestra_core::material::MaterialProgram::additive_sprite("History"),
        "history",
    );
    let mut draw = crate::pipeline_native::draw(material);
    draw.semantic_material = None;
    draw.renderer_kind = 4;
    draw.blend = aestra_gpu::GpuBlend::Additive;
    draw.render_params = asset(world, GpuRenderParams::default());
    draw.texture = texture.clone();
    draw.fallback_texture = texture;
    let owner = world.spawn_empty().id();
    draw.owner = owner;
    let draw = world
        .spawn((
            draw,
            Aabb::from_min_max(Vec3::splat(-200.), Vec3::splat(200.)),
        ))
        .id();

    (owner, draw, cameras)
}
#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_fluid_coupling_and_joint_trails_feed_installed_queues() {
    let mut app = app();
    let world = app.sub_app(RenderApp).world();
    let info = world.resource::<RenderAdapterInfo>();
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    assert_ne!(info.device_type, wgpu::DeviceType::Cpu);
    println!(
        "Native actual trail simulation: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    let device = world.resource::<RenderDevice>().clone();
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let mut registry = aestra_extension::ExtensionRegistry::builtin();
    registry.install(&aestra_fluid::FluidExtension).unwrap();
    let instance = instance(&registry, 70, 7);
    install_domain(&mut app, &instance, &registry);
    let (owner, draw, cameras) = scene(&mut app, 8.);
    let mut effect = inputs(app.world_mut(), &instance);
    app.world_mut().entity_mut(owner).insert(effect.clone());
    crate::simulation_native::bind_inputs(app.world_mut(), draw, &effect);
    {
        let mut value = app
            .world_mut()
            .get_mut::<crate::GpuDrawInstance>(draw)
            .unwrap();
        value.trail_owners = 70;
        value.trail_instances = Some(70 * 4);
    }
    step(&mut app, owner, &mut effect, 30);
    let live = snapshot(&app, &effect, cameras[0]);
    assert_eq!(
        live.particles[..70 * 12]
            .as_chunks::<12>()
            .0
            .iter()
            .filter(|p| p[10] & 0xffff != 0)
            .count(),
        3
    );
    assert!(live.density.iter().any(|v| f32::from_bits(*v) > 0.));
    assert!(
        live.velocity
            .as_chunks::<4>()
            .0
            .iter()
            .any(|v| f32::from_bits(v[1]).abs() > 0.)
    );
    let heads = live.particles[..70 * 12].as_chunks::<12>().0;
    assert!(
        heads
            .iter()
            .filter(|p| p[10] & 0xffff != 0)
            .all(|p| f32::from_bits(p[5]).abs() > 0.00001),
        "actual field-follow changes heads, not only domain buffers"
    );
    assert!(live.candidates[1] > 0);
    step(&mut app, owner, &mut effect, 30);
    assert_eq!(
        snapshot(&app, &effect, cameras[0]),
        live,
        "paused time is idempotent"
    );
    step(&mut app, owner, &mut effect, 70);
    let playback = snapshot(&app, &effect, cameras[0]);
    assert_eq!(
        playback.particles[..70 * 12]
            .as_chunks::<12>()
            .0
            .iter()
            .filter(|p| p[10] & 0xffff != 0)
            .count(),
        0
    );
    assert!(playback.candidates[1] > 0, "dead heads retain live tails");
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<Recorded>()
            .max_ticks
            > 1
    );

    // Reset once into optional replay, then compare a common domain/particle/trail restore.
    effect.history_policy = PlaybackHistoryPolicy::ReplayEnabled;
    effect.history_epoch += 1;
    upload_globals(app.world_mut(), &instance, &effect);
    step(&mut app, owner, &mut effect, 0);
    step(&mut app, owner, &mut effect, 70);
    let replay = snapshot(&app, &effect, cameras[0]);
    assert_eq!(
        replay, playback,
        "history policy must not change live simulation"
    );
    {
        let world = app.sub_app(RenderApp).world();
        let states = world
            .resource::<StatefulStates>()
            .0
            .values()
            .next()
            .unwrap();
        let histories = &world.resource::<Histories>().0;
        let history = &histories.values().next().unwrap().1;
        assert!(states[0].checkpoints.iter().any(|c| c.tick == 60));
        assert!(history.checkpoints.contains(60. * STATEFUL_TICK_DT));
        assert!(history.checkpoints.bytes() > 0);
        assert!(
            world.resource::<Couplers>().domains[0]
                .as_ref()
                .unwrap()
                .checkpoint_ticks()
                .contains(&60)
        );
        // Also cover the shared analytic buffer-identity guard, without cloning its algorithm.
        let gpu = world.resource::<RenderAssets<GpuShaderBuffer>>();
        let mut guard = crate::trail_checkpoints::TrailHistory::default();
        guard.sync_buffers(&[&gpu.get(&effect.particles).unwrap().buffer]);
        assert_eq!(guard.checkpoints.bytes(), 0);
        guard.replay.context_changed();
    }
    step(&mut app, owner, &mut effect, 100);
    assert_eq!(
        snapshot(&app, &effect, cameras[0]).candidates[1],
        0,
        "tails expire"
    );
    effect.history_epoch += 1;
    upload_globals(app.world_mut(), &instance, &effect);
    step(&mut app, owner, &mut effect, 70);
    assert_eq!(
        snapshot(&app, &effect, cameras[0]),
        replay,
        "joint tick-60 restoration is exact"
    );
    effect.history_policy = PlaybackHistoryPolicy::PlaybackOnly;
    step(&mut app, owner, &mut effect, 71);
    // Replacing the source must restart domain and history too, including paged storage.
    for (capacity, seed) in [(70, 11), (1025, 13)] {
        let replacement = self::instance(&registry, capacity, seed);
        effect = inputs(app.world_mut(), &replacement);
        assert_eq!(effect.trail_plan.paged(), capacity > 1024);
        crate::simulation_native::bind_inputs(app.world_mut(), draw, &effect);
        {
            let mut value = app
                .world_mut()
                .get_mut::<crate::GpuDrawInstance>(draw)
                .unwrap();
            value.trail_owners = capacity;
            value.trail_instances = Some(capacity * 4);
        }
        step(&mut app, owner, &mut effect, 0);
        assert_eq!(snapshot(&app, &effect, cameras[0]).candidates[1], 0);
        step(&mut app, owner, &mut effect, 30);
        let current = snapshot(&app, &effect, cameras[0]);
        assert!(current.candidates[1] > 0);
        step(&mut app, owner, &mut effect, 30);
        assert_same(&snapshot(&app, &effect, cameras[0]), &current);
    }
    app.world_mut().entity_mut(draw).despawn();
    app.world_mut().entity_mut(owner).despawn();
    for _ in 0..8 {
        app.update();
    }
    let world = app.sub_app(RenderApp).world();
    assert!(world.resource::<Histories>().0.is_empty());
    assert!(world.resource::<StatefulStates>().0.is_empty());
    assert!(world.resource::<Captured>().0.is_empty());
    device
        .wgpu_device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    assert!(pollster::block_on(scope.pop()).is_none());
    println!(
        "fluid_joint_trails actual_solver=true field_follow=true lockstep=true skipped_ticks_recorded=true pause_idempotent=true dead_head_tails=true tail_expiry=true joint_tick60_restore=true playback_only_no_snapshots=true inline_and_paged=true source_replacement=true installed_cull_buffers_consumed=true test_only_readback=true"
    );
}

/// No fixture-written particles or emission records: every birth comes from the actual solver.
fn spawning_instance(
    registry: &aestra_extension::ExtensionRegistry,
) -> aestra_runtime::EffectInstance {
    spawning_instance_with_lifetime(registry, 0.625)
}
fn spawning_instance_with_lifetime(
    registry: &aestra_extension::ExtensionRegistry,
    lifetime: f32,
) -> aestra_runtime::EffectInstance {
    use aestra_core::*;
    let mut source = source(registry, 10);
    let mut emission = registry
        .modules
        .instantiate(&ModuleTypeId::new(aestra_fluid::MODULE_SECONDARY_EMISSION))
        .unwrap();
    let ModuleParameters::Custom(values) = &mut emission.parameters else {
        panic!()
    };
    values.insert("rate".into(), Value::Scalar(60.));
    values.insert("threshold".into(), Value::Scalar(0.001));
    values.insert("capacity".into(), Value::U32(16));
    emission.stage = StageKind::Simulation(aestra_fluid::SMOKE_STAGE.into());
    source.simulation_stages[0].modules.push(emission);
    let emitter = &mut source.emitters[0];
    emitter.modules[0] = ModuleInstance::emission(0., 0);
    emitter.modules[2] = ModuleInstance::initialize(
        ScalarRange::new(lifetime, lifetime),
        ScalarRange::new(0., 0.),
        [1., 0., 0.],
        0.,
        ScalarRange::new(0., 0.),
    );
    // Remove field-follow so inherited domain velocity is directly observable at birth.
    emitter.modules.pop().unwrap();
    emitter.modules.push(ModuleInstance::spawn_from_domain(0.5));
    if let aestra_core::RendererProperties::Trail {
        sample_interval, ..
    } = &mut emitter.renderers[0].properties
    {
        *sample_interval = (*sample_interval).min(lifetime * 0.25);
    }
    let compiled = aestra_compiler::EffectCompiler::with_extensions(registry.clone())
        .compile(&source)
        .unwrap();
    assert!(compiled.emitters[0].field_follow.is_none());
    assert_eq!(
        compiled.emitters[0]
            .domain_spawn
            .as_ref()
            .unwrap()
            .emission
            .capacity,
        16
    );
    aestra_runtime::EffectInstance::with_seed(Arc::new(compiled), 7)
}
fn output_ring(app: &App, effect: &GpuEffectBuffers) -> Vec<u32> {
    let gpu = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderAssets<GpuShaderBuffer>>();
    read(
        app,
        &gpu.get(&effect.counters).unwrap().buffer,
        (32 + aestra_gpu::PARTICLE_OUTPUT_RING_WORDS) as usize,
    )[32..]
        .to_vec()
}
fn output_at(ring: &[u32], tick: u32) -> Option<aestra_gpu::ParticleOutputRecord> {
    let stride = aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS as usize;
    let start = (tick % aestra_gpu::PARTICLE_OUTPUT_RING_TICKS) as usize * stride;
    aestra_gpu::read_particle_output_slot(&ring[start..start + stride])
}
fn persistent(app: &App) -> &StatefulPersistentState {
    &app.sub_app(RenderApp)
        .world()
        .resource::<StatefulStates>()
        .0
        .values()
        .next()
        .unwrap()[0]
}
#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_domain_births_feed_particles_trails_and_output_rings() {
    let mut app = app();
    let world = app.sub_app(RenderApp).world();
    let info = world.resource::<RenderAdapterInfo>();
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    assert_ne!(info.device_type, wgpu::DeviceType::Cpu);
    println!(
        "Native domain births: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    let device = world.resource::<RenderDevice>().clone();
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let mut registry = aestra_extension::ExtensionRegistry::builtin();
    registry.install(&aestra_fluid::FluidExtension).unwrap();
    let instance = spawning_instance(&registry);
    install_domain(&mut app, &instance, &registry);
    let (owner, draw, cameras) = scene(&mut app, 40.);
    let mut effect = inputs(app.world_mut(), &instance);
    effect.routed = true;
    effect.stateful_dispatch[0].event_mask = 1;
    effect.counters = asset(
        app.world_mut(),
        vec![0u32; (32 + aestra_gpu::PARTICLE_OUTPUT_RING_WORDS) as usize],
    );
    effect.particle_outputs = vec![(
        aestra_runtime::CompiledParticleOutput {
            output: "domain_birth".into(),
            source: 0,
            trigger: aestra_core::EventTrigger::OnSpawn,
            aggregation: aestra_core::EventAggregation::EachEvent { limit: 3 },
        },
        32,
    )];
    crate::simulation_native::bind_inputs(app.world_mut(), draw, &effect);
    {
        let mut value = app
            .world_mut()
            .get_mut::<crate::GpuDrawInstance>(draw)
            .unwrap();
        value.trail_owners = 10;
        value.trail_instances = Some(40);
    }
    step(&mut app, owner, &mut effect, 0);
    assert_eq!(read(&app, &persistent(&app).spawn_counter, 1), [0]);
    step(&mut app, owner, &mut effect, 1);
    let world = app.sub_app(RenderApp).world();
    let domain = world.resource::<Couplers>().domains[0].as_ref().unwrap();
    let list = read(
        &app,
        domain
            .executor()
            .buffer(aestra_fluid::RESOURCE_EMISSION)
            .unwrap(),
        4 + 16 * 8,
    );
    assert_eq!(
        list[0], 16,
        "real qualifying cells saturate the emission list"
    );
    let state = read(
        &app,
        &persistent(&app).state,
        10 * aestra_gpu::STATEFUL_STATE_STRIDE as usize,
    );
    assert_eq!(read(&app, &persistent(&app).free_count, 1), [0]);
    assert_eq!(read(&app, &persistent(&app).spawn_counter, 1), [10]);
    let mut positions = [[0.; 3]; 10];
    for particle in state
        .as_chunks::<{ aestra_gpu::STATEFUL_STATE_STRIDE as usize }>()
        .0
    {
        let ordinal = particle[8] as usize;
        assert!(ordinal < 10);
        let record = &list[4 + ordinal * 8..4 + (ordinal + 1) * 8];
        assert_eq!(
            particle[..3],
            record[..3],
            "accepted prefix preserves actual domain positions"
        );
        for axis in 0..3 {
            assert_eq!(
                f32::from_bits(particle[3 + axis]),
                0.5 * f32::from_bits(record[4 + axis])
            );
            positions[ordinal][axis] = f32::from_bits(record[axis]);
        }
        assert_eq!(f32::from_bits(particle[6]), 0.);
        assert_eq!(f32::from_bits(particle[7]), 0.625);
    }
    assert!(
        positions.iter().all(|p| p.iter().any(|v| v.abs() > 0.01)),
        "births are not seeded at the emitter origin"
    );
    assert!(
        list[4..]
            .as_chunks::<8>()
            .0
            .iter()
            .any(|r| f32::from_bits(r[5]).abs() > 0.00001)
    );
    let first = snapshot(&app, &effect, cameras[0]);
    let ring = output_ring(&app, &effect);
    let output = output_at(&ring, 1).unwrap();
    assert_eq!((output.tick, output.epoch, output.count), (1, 9, 10));
    assert_eq!(
        output.first,
        (0..3).map(|i| (i as u64, positions[i])).collect::<Vec<_>>()
    );
    step(&mut app, owner, &mut effect, 1);
    assert_same(&snapshot(&app, &effect, cameras[0]), &first);
    assert_eq!(
        output_ring(&app, &effect),
        ring,
        "pause does not re-export births"
    );
    step(&mut app, owner, &mut effect, 2);
    assert_eq!(read(&app, &persistent(&app).spawn_counter, 1), [10]);
    assert_eq!(
        output_at(&output_ring(&app, &effect), 2).unwrap().count,
        0,
        "full emitter exports no rejected births"
    );
    // Later generations reuse freed slots; actual recorded samples reach both installed view queues.
    for tick in 3..=70 {
        step(&mut app, owner, &mut effect, tick);
    }
    assert!(read(&app, &persistent(&app).spawn_counter, 1)[0] > 10);
    let playback = snapshot(&app, &effect, cameras[0]);
    assert!(playback.candidates[1] > 0);
    // Replay is optional and must reconstruct the same actual domain births and joint trails.
    effect.history_policy = PlaybackHistoryPolicy::ReplayEnabled;
    effect.history_epoch += 1;
    upload_globals(app.world_mut(), &instance, &effect);
    let before_replay = output_ring(&app, &effect);
    step(&mut app, owner, &mut effect, 0);
    step(&mut app, owner, &mut effect, 70);
    assert_same(&snapshot(&app, &effect, cameras[0]), &playback);
    assert_eq!(
        output_ring(&app, &effect),
        before_replay,
        "reconstruction does not export duplicate births"
    );
    {
        let world = app.sub_app(RenderApp).world();
        assert!(persistent(&app).checkpoints.iter().any(|c| c.tick == 60));
        assert!(
            world
                .resource::<Histories>()
                .0
                .values()
                .next()
                .unwrap()
                .1
                .checkpoints
                .contains(60. * STATEFUL_TICK_DT)
        );
        assert!(
            world.resource::<Couplers>().domains[0]
                .as_ref()
                .unwrap()
                .checkpoint_ticks()
                .contains(&60)
        );
    }
    step(&mut app, owner, &mut effect, 100);
    effect.history_epoch += 1;
    upload_globals(app.world_mut(), &instance, &effect);
    step(&mut app, owner, &mut effect, 70);
    assert_same(&snapshot(&app, &effect, cameras[0]), &playback);
    assert_eq!(output_ring(&app, &effect), before_replay);
    effect.history_policy = PlaybackHistoryPolicy::PlaybackOnly;
    step(&mut app, owner, &mut effect, 71);
    app.world_mut().entity_mut(draw).despawn();
    app.world_mut().entity_mut(owner).despawn();
    for _ in 0..8 {
        app.update();
    }
    let world = app.sub_app(RenderApp).world();
    assert!(world.resource::<Histories>().0.is_empty());
    assert!(world.resource::<StatefulStates>().0.is_empty());
    assert!(world.resource::<Captured>().0.is_empty());
    device
        .wgpu_device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    assert!(pollster::block_on(scope.pop()).is_none());
    println!(
        "domain_births actual_emission_list=true accepted_prefix=true inherited_velocity=true capacity_bounded=true rejected_births_not_exported=true pause_idempotent=true slot_reuse=true installed_trail_queues=true joint_tick60_restore=true replay_no_duplicate_outputs=true playback_only_no_snapshots=true test_only_readback=true"
    );
}

#[derive(Resource, Default)]
struct Delivered(Vec<crate::output_context::AestraOutputEvent>);
fn collect_outputs(
    mut messages: MessageReader<crate::output_context::AestraOutputEvent>,
    mut delivered: ResMut<Delivered>,
) {
    delivered.0.extend(messages.read().cloned());
}
fn drain_output_reads(app: &mut App, owner: Entity, additional: u64) {
    use crate::particle_output_readback::GpuEventLinkStatistics;
    let initial = app
        .world()
        .get::<GpuEventLinkStatistics>(owner)
        .unwrap()
        .readback_samples;
    for _ in 0..240 {
        app.update();
        if app
            .world()
            .get::<GpuEventLinkStatistics>(owner)
            .unwrap()
            .readback_samples
            >= initial + additional
        {
            return;
        }
        std::thread::yield_now();
    }
    panic!("actual asynchronous readback observer did not deliver {additional} completions");
}
fn host_step(app: &mut App, owner: Entity, effect: &mut GpuEffectBuffers, tick: u32) {
    app.world_mut()
        .get_mut::<crate::PresentedEffect>(owner)
        .unwrap()
        .instance
        .set_playback_time(tick as f32 * STATEFUL_TICK_DT + 0.0001);
    step(app, owner, effect, tick);
}
#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_async_particle_outputs_reach_routed_host_messages_once() {
    use crate::{
        output_context::{AestraOutputEvent, EffectOutputContext},
        particle_output_readback::{
            GpuArrivalReadback, GpuEventLinkStatistics, receive_homing_arrivals,
        },
    };
    use bevy::render::gpu_readback::Readback;
    let mut app = app();
    app.add_message::<AestraOutputEvent>()
        .init_resource::<Delivered>()
        .add_systems(Last, collect_outputs);
    let world = app.sub_app(RenderApp).world();
    let info = world.resource::<RenderAdapterInfo>();
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    assert_ne!(info.device_type, wgpu::DeviceType::Cpu);
    println!(
        "Native async host outputs: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    let device = world.resource::<RenderDevice>().clone();
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let mut registry = aestra_extension::ExtensionRegistry::builtin();
    registry.install(&aestra_fluid::FluidExtension).unwrap();
    let mut instance = spawning_instance_with_lifetime(&registry, 0.05);
    instance.set_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    // Source-tick pose and inherited root time differ from the eventual delivery clock.
    let motion = aestra_core::HostTransformTrack::from_pose_keys(
        vec![
            aestra_core::HostTransformKey {
                time: 0.,
                transform: default(),
            },
            aestra_core::HostTransformKey {
                time: 10.,
                transform: aestra_core::EmitterTransform {
                    translation: [10., 0., 0.],
                    ..default()
                },
            },
        ],
        false,
    );
    instance.set_inherited_host_transform(Arc::new(
        aestra_runtime::InheritedHostTransform::default().for_child(
            Some(Arc::new(
                aestra_runtime::CompiledHostTransformTrack::new(motion).unwrap(),
            )),
            aestra_core::EmitterTransform {
                scale: [2., 0.5, 1.],
                ..default()
            },
            2.,
        ),
    ));
    install_domain(&mut app, &instance, &registry);
    let (owner, draw, cameras) = scene(&mut app, 40.);
    let root = app.world_mut().spawn_empty().id();
    let path = vec![
        aestra_core::EffectClipId::new(),
        aestra_core::EffectClipId::new(),
    ];
    let mut presented = crate::PresentedEffect::new(instance.effect().clone());
    presented.instance = instance.clone();
    let mut effect = inputs(app.world_mut(), &instance);
    effect.history_epoch = instance.history_epoch();
    effect.history_policy = instance.history_policy();
    effect.routed = true;
    effect.stateful_dispatch[0].event_mask = 1;
    effect.counters = asset(
        app.world_mut(),
        vec![0u32; (32 + aestra_gpu::PARTICLE_OUTPUT_RING_WORDS) as usize],
    );
    effect.particle_outputs = vec![(
        aestra_runtime::CompiledParticleOutput {
            output: "domain_birth".into(),
            source: 0,
            trigger: aestra_core::EventTrigger::OnSpawn,
            aggregation: aestra_core::EventAggregation::EachEvent { limit: 3 },
        },
        32,
    )];
    upload_globals(app.world_mut(), &instance, &effect);
    app.world_mut().entity_mut(owner).insert((
        presented,
        Transform::from_xyz(12., 3., -7.),
        EffectOutputContext {
            root,
            clip_path: path.clone(),
            playback_epoch: 77,
        },
        GpuEventLinkStatistics::default(),
    ));
    // Observe actual completions, but delay requesting reads until ring slot order wraps.
    let readback = app
        .world_mut()
        .spawn((
            GpuArrivalReadback {
                effect: owner,
                seen: default(),
                particle_delivery: default(),
            },
            ChildOf(owner),
        ))
        .observe(receive_homing_arrivals)
        .id();
    crate::simulation_native::bind_inputs(app.world_mut(), draw, &effect);
    {
        let mut value = app
            .world_mut()
            .get_mut::<crate::GpuDrawInstance>(draw)
            .unwrap();
        value.trail_owners = 10;
        value.trail_instances = Some(40);
    }
    host_step(&mut app, owner, &mut effect, 0);
    for tick in 1..=41 {
        host_step(&mut app, owner, &mut effect, tick);
    }
    assert!(app.world().resource::<Delivered>().0.is_empty());
    app.world_mut()
        .entity_mut(readback)
        .insert(Readback::buffer(effect.counters.clone()));
    // Bevy submits, maps and triggers its own ReadbackComplete; no synchronous read drives delivery.
    drain_output_reads(&mut app, owner, 8);
    let delivered = app.world().resource::<Delivered>().0.clone();
    assert!(
        delivered.len() >= 6,
        "multiple wrapped-ring birth ticks reach the actual stream"
    );
    assert!(
        delivered
            .windows(2)
            .all(|pair| pair[0].event.tick <= pair[1].event.tick)
    );
    // Only after delivery, read GPU words as an independent payload oracle.
    let ring = output_ring(&app, &effect);
    let mut records = ring
        .as_chunks::<{ aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS as usize }>()
        .0
        .iter()
        .filter_map(|slot| aestra_gpu::read_particle_output_slot(slot))
        .filter(|record| record.count > 0)
        .collect::<Vec<_>>();
    records.sort_by_key(|record| record.tick);
    assert!(records.iter().any(|r| r.tick < 32) && records.iter().any(|r| r.tick >= 32));
    let expected = records
        .iter()
        .flat_map(|record| {
            record
                .first
                .iter()
                .map(move |(_, position)| (record, position))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        delivered.len(),
        expected.len(),
        "every retained birth is delivered once"
    );
    for (output, (record, position)) in delivered.iter().zip(expected) {
        assert_eq!(output.effect, root);
        assert_eq!(output.clip_path, path);
        assert_eq!(output.playback_epoch, Some(77));
        assert_eq!(output.event.kind, "domain_birth");
        assert!(output.event.output.is_empty());
        assert_eq!(output.event.tick, record.tick);
        assert_eq!(output.event.value, position.to_vec());
        assert_eq!(output.event.magnitude, record.count as f32);
        let particle = output.particle.as_ref().unwrap();
        assert_eq!(particle.source_effect, instance.effect().source);
        assert_eq!(particle.seed, 7);
        let time = record.tick as f32 * STATEFUL_TICK_DT;
        assert!((particle.root_time_seconds - (time + 2.)).abs() < 1e-5);
        let expected_world = Vec3::new(12. + time + 2., 3., -7.)
            + Vec3::from_array(*position) * Vec3::new(2., 0.5, 1.);
        assert!(
            Vec3::from_array(particle.world_position.unwrap()).abs_diff_eq(expected_world, 1e-5)
        );
    }
    assert!(snapshot(&app, &effect, cameras[0]).candidates[1] > 0);
    drain_output_reads(&mut app, owner, 8);
    assert_eq!(
        app.world().resource::<Delivered>().0,
        delivered,
        "pause/repeated callbacks cannot duplicate outputs"
    );
    // A new host epoch rejects repeated actual reads of the old ring, before GPU reconstruction.
    app.world_mut()
        .get_mut::<crate::PresentedEffect>(owner)
        .unwrap()
        .instance
        .restart();
    app.world_mut()
        .get_mut::<EffectOutputContext>(owner)
        .unwrap()
        .playback_epoch = 78;
    drain_output_reads(&mut app, owner, 8);
    assert_eq!(app.world().resource::<Delivered>().0, delivered);
    effect.history_epoch = app
        .world()
        .get::<crate::PresentedEffect>(owner)
        .unwrap()
        .instance
        .history_epoch();
    upload_globals(app.world_mut(), &instance, &effect);
    host_step(&mut app, owner, &mut effect, 0);
    host_step(&mut app, owner, &mut effect, 1);
    drain_output_reads(&mut app, owner, 8);
    {
        let now = &app.world().resource::<Delivered>().0;
        assert_eq!(
            now.len(),
            delivered.len() + 3,
            "restart delivers one fresh birth cohort"
        );
        for output in &now[delivered.len()..] {
            assert_eq!(output.event.tick, 1);
            assert_eq!(output.playback_epoch, Some(78));
        }
    }
    // A seek suppresses reconstructed history but allows later genuinely live ticks.
    let seek_time = 70. * STATEFUL_TICK_DT;
    {
        let mut presented = app
            .world_mut()
            .get_mut::<crate::PresentedEffect>(owner)
            .unwrap();
        presented.instance.seek(seek_time);
        effect.history_epoch = presented.instance.history_epoch();
        effect.output_suppress_through =
            aestra_runtime::trace_tick(presented.instance.history_epoch_start_time());
    }
    app.world_mut()
        .get_mut::<EffectOutputContext>(owner)
        .unwrap()
        .playback_epoch = 79;
    upload_globals(app.world_mut(), &instance, &effect);
    host_step(&mut app, owner, &mut effect, 70);
    drain_output_reads(&mut app, owner, 8);
    assert_eq!(
        app.world().resource::<Delivered>().0.len(),
        delivered.len() + 3
    );
    for tick in 71..=80 {
        host_step(&mut app, owner, &mut effect, tick);
    }
    drain_output_reads(&mut app, owner, 8);
    let count = app.world().resource::<Delivered>().0.len();
    assert!(count > delivered.len() + 3);
    for output in &app.world().resource::<Delivered>().0[delivered.len() + 3..] {
        assert!(output.event.tick > 70);
        assert_eq!(output.playback_epoch, Some(79));
    }
    // Missing GPU inputs and owner retirement reject late completions.
    app.world_mut()
        .entity_mut(owner)
        .remove::<GpuEffectBuffers>();
    for _ in 0..12 {
        app.update();
    }
    assert_eq!(app.world().resource::<Delivered>().0.len(), count);
    app.world_mut().entity_mut(draw).despawn();
    app.world_mut().entity_mut(owner).despawn();
    for _ in 0..12 {
        app.update();
    }
    assert!(app.world().get_entity(readback).is_err());
    assert_eq!(app.world().resource::<Delivered>().0.len(), count);
    device
        .wgpu_device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    assert!(pollster::block_on(scope.pop()).is_none());
    println!(
        "async_host_outputs actual_readback_complete=true real_presented_effect=true wrapped_ring_tick_order=true payload_and_world_context=true pause_no_duplicates=true stale_epochs_rejected=true restart_new_outputs=true seek_suppressed=true live_after_seek=true teardown=true full_host_scheduler_qualified=false"
    );
}
