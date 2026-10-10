//! Shared production lockstep scheduler -> event chains/input bursts/output rings -> installed queues.
use crate::{
    alpha_sort_native as observer, coupled_simulation::*, draw_resources::*, effect_inputs::*,
    queue_native::asset, stateful_simulation::*,
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
    max_ticks: u32,
}
#[derive(Resource)]
struct Couplers {
    follower: crate::execution::FieldFollowPipeline,
    spawner: crate::execution::DomainSpawnPipeline,
    gatherer: crate::execution::EventGatherPipeline,
}
fn simulate(
    mut context: RenderContext,
    cache: Res<PipelineCache>,
    pipeline: Option<Res<StatefulSimulationPipeline>>,
    effects: Query<(Entity, &GpuEffectBuffers)>,
    (buffers, device): (Res<RenderAssets<GpuShaderBuffer>>, Res<RenderDevice>),
    mut states: ResMut<StatefulStates>,
    (mut recorded, couplers): (ResMut<Dispatch>, Res<Couplers>),
) {
    recorded.recorded_time = None;
    let Some(pipeline) = pipeline else { return };
    let (Some(death), Some(spawn), Some(present), Some(order)) = (
        cache.get_compute_pipeline(pipeline.death_integrate),
        cache.get_compute_pipeline(pipeline.spawn),
        cache.get_compute_pipeline(pipeline.present),
        cache.get_compute_pipeline(pipeline.order_present),
    ) else {
        return;
    };
    let layout = cache.get_bind_group_layout(&pipeline.layout);
    let absent = physics_scene_buffer(&device, &aestra_gpu::GpuWorldSdf::absent().words);
    for (entity, effect) in &effects {
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
        let (ticks, captured) = run_stateful_dispatches(
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
            &render,
            Some(Coupling {
                domains: &mut [],
                inputs: default(),
                follower: &couplers.follower,
                spawner: &couplers.spawner,
                gatherer: &couplers.gatherer,
            }),
            effect.simulation_time,
            effect.seek_quality,
            effect.statistics_token,
            effect.history_epoch,
            true,
            None,
            None,
        )
        .expect("linked/routed effects use the coupled scheduler");
        recorded.max_ticks = recorded.max_ticks.max(ticks);
        assert!(ticks <= crate::catchup_pacing::stateful_catchup_budget(effect.seek_quality));
        assert_eq!(captured, 0);
        assert!(persistent.iter().all(|s| s.checkpoints.is_empty()));
        recorded.recorded_time = Some(effect.simulation_time);
    }
}
fn instance(seed: u64) -> aestra_runtime::EffectInstance {
    use aestra_core::*;
    let mut effect = EffectAsset::new("Coupled integration", 10.);
    effect.playback_mode = EffectPlaybackMode::Once;
    for index in 0..3 {
        let mut emitter = Emitter::basic_sprite(format!("Generation {index}"), 10.);
        emitter.max_particles = 129;
        emitter.modules[0] = ModuleInstance::emission(0., if index == 0 { 1 } else { 0 });
        emitter.modules[1] = ModuleInstance::shape(EmitterShape::Sphere { radius: 0.2 });
        let life = if index < 2 { 0.05 } else { 10. };
        emitter.modules[2] = ModuleInstance::initialize(
            ScalarRange::new(life, life),
            ScalarRange::new(0., 0.),
            [0., 0., 1.],
            0.,
            ScalarRange::new(0., 0.),
        );
        emitter.modules[3] = ModuleInstance::motion([0.; 3], 0., 0.);
        emitter.modules.push(ModuleInstance::persistent());
        effect.emitters.push(emitter);
    }
    let mut first = EventLink::new(
        effect.emitters[0].id,
        EventTrigger::OnDeath,
        effect.emitters[1].id,
    );
    first.count = 12;
    let mut second = EventLink::new(
        effect.emitters[1].id,
        EventTrigger::OnDeath,
        effect.emitters[2].id,
    );
    second.count = 2;
    effect.events = vec![first, second];
    effect.materials[0].blend = BlendMode::Alpha;
    let compiled = aestra_compiler::EffectCompiler::default()
        .compile(&effect)
        .unwrap();
    aestra_runtime::EffectInstance::with_seed(Arc::new(compiled), seed)
}
fn inputs(world: &mut World, instance: &aestra_runtime::EffectInstance) -> GpuEffectBuffers {
    let artifact = GpuEffectArtifact::from_instance(instance).unwrap();
    let mut effect = crate::simulation_native::inputs(world, instance, 0.);
    effect.stateful_only = true;
    effect.seek_quality = aestra_runtime::SeekQuality::Preview;
    effect.host_events = Arc::new(HostEventHistory::of(instance));
    effect.stateful_dispatch = artifact
        .emitters
        .iter()
        .enumerate()
        .map(|(index, emitter)| {
            let compiled = &instance.effect().emitters[index];
            assert_eq!(emitter.velocity_distribution, 0);
            assert_eq!(emitter.shape_kind, 3);
            assert!(
                compiled.colliders.is_empty()
                    && compiled.homing.is_none()
                    && compiled.attachment.is_none()
                    && compiled.field_follow.is_none()
                    && compiled.domain_spawn.is_none()
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
                event_mask: if index == 0 {
                    2
                } else if index == 1 {
                    3
                } else {
                    1
                },
                distance_emission: None,
                event_signature: 0,
                overflow_word: Some(2 + index as u32),
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
            }
        })
        .collect();
    effect.event_links = instance.effect().event_links.clone();
    effect.routed = true;
    effect.statistics_token = 23;
    effect.history_epoch = 7;
    effect.counters = asset(
        world,
        vec![0u32; (32 + 2 * aestra_gpu::PARTICLE_OUTPUT_RING_WORDS) as usize],
    );
    effect.particle_outputs = [1, 2]
        .into_iter()
        .enumerate()
        .map(|(index, source)| {
            (
                aestra_runtime::CompiledParticleOutput {
                    output: format!("birth_{source}"),
                    source,
                    trigger: aestra_core::EventTrigger::OnSpawn,
                    aggregation: aestra_core::EventAggregation::EachEvent { limit: 3 },
                },
                32 + index as u32 * aestra_gpu::PARTICLE_OUTPUT_RING_WORDS,
            )
        })
        .collect();
    let mut history = HostEventHistory::of(instance);
    history.bursts = [(1, 3), (6, 5)]
        .into_iter()
        .map(|(tick, count)| aestra_runtime::InputSpawnBurst {
            tick,
            route: 0,
            target: 2,
            count,
            events: vec![aestra_runtime::ParticleEvent {
                ordinal: 0,
                position: [0.1, 0.2, -0.3],
                velocity: [0.; 3],
            }],
        })
        .collect();
    effect.host_events = Arc::new(history);
    effect
}

fn step(app: &mut App, owner: Entity, effect: &mut GpuEffectBuffers, tick: u32) {
    // A small positive subtick avoids f32 floor ambiguities while also exercising presentation.
    effect.simulation_time = tick as f32 * STATEFUL_TICK_DT + 0.0001;
    assert_eq!(
        aestra_runtime::trace_tick(effect.simulation_time),
        u64::from(tick)
    );
    app.world_mut().entity_mut(owner).insert(effect.clone());
    for _ in 0..160 {
        app.update();
        let world = app.sub_app(RenderApp).world();
        if world
            .resource::<StatefulStates>()
            .0
            .values()
            .next()
            .is_some_and(|states| states.iter().all(|state| state.last_tick == tick))
            && world.resource::<Dispatch>().recorded_time == Some(effect.simulation_time)
        {
            observer::settle(app, 2, true, 2);
            return;
        }
        std::thread::yield_now();
    }
    panic!("stateful scheduling did not converge to tick {tick}");
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_coupled_routes_feed_alpha_and_installed_queues() {
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
        "Native coupled simulation integration: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    let device = world.resource::<RenderDevice>().clone();
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    world.insert_resource(Couplers {
        follower: crate::execution::FieldFollowPipeline::new(device.wgpu_device()),
        spawner: crate::execution::DomainSpawnPipeline::new(device.wgpu_device()),
        gatherer: crate::execution::EventGatherPipeline::new(device.wgpu_device()),
    });
    observer::install_observer(world);
    app.world_mut()
        .resource_mut::<crate::AestraRenderSettings>()
        .transparent_order = crate::TransparentOrderMode::DepthBackToFront;
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

    let mut effect = inputs(world, &instance(7));
    let owner = draw.owner;
    let draw = world
        .spawn((draw, Aabb::from_min_max(Vec3::splat(-4.), Vec3::splat(4.))))
        .id();
    crate::simulation_native::bind_inputs(world, draw, &effect);
    {
        let mut draw = world.entity_mut(draw);
        let mut draw = draw.get_mut::<crate::GpuDrawInstance>().unwrap();
        draw.sort_range = UVec2::new(258, 129);
        draw.emitter_index = 2;
        draw.renderer_order = 2;
        draw.indirect_offset = 32;
    }
    world.entity_mut(owner).insert(effect.clone());
    for tick in 0..=8 {
        step(&mut app, owner, &mut effect, tick);
    }
    let snapshot = |app: &App, effect: &GpuEffectBuffers| {
        let world = app.sub_app(RenderApp).world();
        let buffers = world.resource::<RenderAssets<GpuShaderBuffer>>();
        let read = |handle, words| {
            crate::trail_native::read(app, &buffers.get(handle).unwrap().buffer, words)
        };
        let indirect = read(&effect.indirect, 16);
        assert_eq!(indirect[..12], [6, 0, 0, 0, 6, 0, 0, 0, 6, 32, 0, 0]);
        assert_eq!(
            indirect[12..],
            [
                aestra_gpu::PARTICLE_STATISTICS_MAGIC,
                23,
                7,
                effect.simulation_time.to_bits()
            ]
        );
        let counters = read(
            &effect.counters,
            (32 + 2 * aestra_gpu::PARTICLE_OUTPUT_RING_WORDS) as usize,
        );
        assert_eq!(counters[0], 32);
        assert_eq!(&counters[2..5], &[0, 0, 0], "no source event overflow");
        assert_eq!(
            &counters[5..11],
            &[12, 0, 12, 24, 0, 24],
            "requested/dropped/accepted link counters"
        );
        let words = read(&effect.particles, effect.total_slots as usize * 12);
        let mut live = Vec::new();
        for (index, particle) in words.as_chunks::<12>().0.iter().enumerate().skip(258) {
            if particle[10] & 0xffff == 0 {
                continue;
            }
            let position: Vec<_> = particle[4..7]
                .iter()
                .map(|word| f32::from_bits(*word))
                .collect();
            assert!(position.iter().all(|v| v.is_finite()));
            live.push((index as u32, position, particle[11]));
        }
        assert_eq!(live.len(), 32);
        assert_eq!(
            live.iter()
                .filter(|(_, p, _)| p.as_slice() == [0.1, 0.2, -0.3])
                .count(),
            8,
            "host bursts use their event position"
        );
        for (index, camera) in cameras.iter().enumerate() {
            let (_, permutation) = observer::permutation(app, *camera);
            let mut order = live.clone();
            order.sort_by(|a, b| {
                let sign = if index == 0 { 1. } else { -1. };
                (a.1[2] * sign)
                    .total_cmp(&(b.1[2] * sign))
                    .then(a.2.cmp(&b.2))
                    .then(a.0.cmp(&b.0))
            });
            assert_eq!(
                &permutation[..32],
                order.iter().map(|v| v.0).collect::<Vec<_>>()
            );
            assert!(permutation[32..].iter().all(|v| *v == u32::MAX));
        }
        let states = world
            .resource::<StatefulStates>()
            .0
            .values()
            .next()
            .unwrap();
        for (state, count) in states.iter().zip([1, 12, 32]) {
            assert_eq!(
                crate::trail_native::read(app, &state.spawn_counter, 1)[0],
                count
            );
            assert!(state.checkpoints.is_empty());
        }
        for (state, live) in states.iter().zip([0, 0, 32]) {
            assert_eq!(
                crate::trail_native::read(app, &state.free_count, 1)[0] + live,
                129
            );
        }
        counters[32..].to_vec()
    };
    let outputs = snapshot(&app, &effect);
    let decode = |words: &[u32]| {
        words
            .as_chunks::<{ aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS as usize }>()
            .0
            .iter()
            .filter_map(|slot| aestra_gpu::read_particle_output_slot(slot))
            .filter(|record| record.count > 0)
            .map(|r| (r.tick, r.epoch, r.count))
            .collect::<Vec<_>>()
    };
    let ring = aestra_gpu::PARTICLE_OUTPUT_RING_WORDS as usize;
    assert_eq!(decode(&outputs[..ring]), [(4, 7, 12)]);
    // The tick-7 ring aggregates both linked births (24) and the host burst (5).
    assert_eq!(decode(&outputs[ring..]), [(2, 7, 3), (7, 7, 29)]);
    step(&mut app, owner, &mut effect, 8);
    assert_eq!(
        snapshot(&app, &effect),
        outputs,
        "pause cannot emit or export twice"
    );
    // A long jump is split into bounded render frames, not a GPU wait or CPU simulation.
    step(&mut app, owner, &mut effect, 120);
    snapshot(&app, &effect);
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<Dispatch>()
            .max_ticks
            > 1
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
        "coupled_integration generations=3 links=2 external_births=8 output_rings=2 live_particles=32 opposite_views=2 repeated_time=true bounded_preview=true playback_only=true test_only_readback=true fluid_domain_qualified=false joint_trail_history_qualified=false"
    );
}
