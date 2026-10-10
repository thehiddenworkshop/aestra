//! Actual history recording -> installed compaction/culling -> installed queues.
//! Time and pose are fixture inputs; the production host/replay scheduler is not emulated.
use crate::{
    draw_resources::*,
    effect_inputs::GpuEffectBuffers,
    queue_native::{asset, update},
    simulation_pipeline::*,
    trail_native::{Captured, capture, read},
};
use aestra_gpu::{GpuGlobals, GpuRenderGlobals, GpuRenderParams};
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
        sync_world::MainEntity,
    },
};
use std::sync::Arc;

#[derive(Resource, Default)]
struct Recorded(Option<(f32, bool)>);

fn simulate(
    mut context: RenderContext,
    cache: Res<PipelineCache>,
    pipeline: Option<Res<SimulationPipeline>>,
    effects: Query<(&GpuEffectBuffers, &GpuBindGroup)>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    device: Res<RenderDevice>,
    mut recorded: ResMut<Recorded>,
) {
    recorded.0 = None;
    let Some(pipeline) = pipeline else {
        return;
    };
    let (Some(reset), Some(simulate), Some(update)) = (
        cache.get_compute_pipeline(pipeline.reset),
        cache.get_compute_pipeline(pipeline.simulate),
        cache.get_compute_pipeline(pipeline.update_trails),
    ) else {
        return;
    };
    let paged = pipeline
        .paged_trails
        .map(|id| cache.get_compute_pipeline(id));
    let paged = paged.into_iter().collect::<Option<Vec<_>>>();
    for (effect, group) in &effects {
        assert!(effect.has_trails && !effect.has_ribbons && effect.stateful_dispatch.is_empty());
        assert_eq!(
            effect.history_policy,
            aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly
        );
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
        if let Some(pages) = &pages {
            assert!(pages.workgroups() > 0);
        }
        record_particles(
            context.command_encoder(),
            group,
            effect,
            reset,
            simulate,
            None,
            None,
        );
        assert!(record_trails(
            context.command_encoder(),
            group,
            effect,
            TrailPipelines {
                update,
                link_ribbons: None,
                paged: pages.as_ref()
            },
            &buffers,
            TrailTimestamps {
                history: None,
                paged_end: None
            },
            None
        ));
        recorded.0 = Some((effect.simulation_time, pages.is_some()));
    }
}

fn instance(capacity: u32, seed: u64) -> aestra_runtime::EffectInstance {
    use aestra_core::*;
    let mut effect = EffectAsset::new("Trail simulation integration", 3.);
    effect.playback_mode = EffectPlaybackMode::Once;
    let mut emitter = Emitter::basic_sprite("Three heads", 3.);
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
    effect.emitters.push(emitter);
    let compiled = aestra_compiler::EffectCompiler::default()
        .compile(&effect)
        .unwrap();
    aestra_runtime::EffectInstance::with_seed(Arc::new(compiled), seed)
}

fn step(
    app: &mut App,
    owner: Entity,
    effect: &mut GpuEffectBuffers,
    seed: u64,
    time: f32,
    shift: f32,
) {
    effect.simulation_time = time;
    let mut globals = GpuGlobals {
        time,
        total_slots: effect.total_slots,
        seed: aestra_gpu::fold_seed(seed),
        emitter_count: 1,
        duration: 3.,
        ..default()
    };
    globals._padding.x = effect.history_epoch;
    globals.world_from_effect.w_axis.x = shift;
    let render = GpuRenderGlobals {
        time,
        seed: globals.seed,
        world_from_effect: globals.world_from_effect,
        ..default()
    };
    update(app.world_mut(), &effect.globals, globals);
    update(app.world_mut(), &effect.render_globals, render);
    app.world_mut().entity_mut(owner).insert(effect.clone());
    // Identical-time updates must not append duplicate samples or rebuild history.
    for _ in 0..4 {
        app.update();
    }
    settle(app, 1, 2, 2);
}

fn settle(app: &mut App, entries: usize, views: usize, draws: usize) {
    for _ in 0..160 {
        app.update();
        let world = app.sub_app(RenderApp).world();
        let compact = world.resource::<TrailCompaction>();
        let cull = world.resource::<TrailCulling>();
        if compact.entries.len() == entries
            && cull.entries.len() == views
            && compact.dispatched
            && cull.dispatched
            && world.resource::<Captured>().0.len() == draws
            && world.resource::<Recorded>().0.is_some() == (entries != 0)
        {
            return;
        }
        std::thread::yield_now();
    }
    let world = app.sub_app(RenderApp).world();
    panic!(
        "actual trail schedule did not converge: compact={} dispatched={} cull={} dispatched={} draws={} recorded={:?}",
        world.resource::<TrailCompaction>().entries.len(),
        world.resource::<TrailCompaction>().dispatched,
        world.resource::<TrailCulling>().entries.len(),
        world.resource::<TrailCulling>().dispatched,
        world.resource::<Captured>().0.len(),
        world.resource::<Recorded>().0
    );
}

struct Expected<'a> {
    samples: &'a [(f32, f32)], // Observation time and recorded world-space X.
    segments: &'a [u32],
    live: bool,
    near: Entity,
}

fn verify(app: &App, effect: &GpuEffectBuffers, expected: Expected<'_>) -> Buffer {
    let world = app.sub_app(RenderApp).world();
    assert_eq!(
        world.resource::<Recorded>().0,
        Some((effect.simulation_time, effect.trail_plan.paged()))
    );
    let buffers = world.resource::<RenderAssets<GpuShaderBuffer>>();
    let buffer = |handle| &buffers.get(handle).unwrap().buffer;
    let root = effect.trail_roots[0].1 as usize;
    let words = read(
        app,
        buffer(&effect.particles),
        (root + 1 + effect.total_slots as usize * 5) * 12,
    );
    let aux = read(
        app,
        buffer(&effect.aux),
        effect.trail_plan.aux_words as usize,
    );
    let stats = read(app, buffer(&effect.counters), 8);
    let occupied = if expected.samples.is_empty() { 0 } else { 3 };
    assert_eq!(stats[0], if expected.live { 3 } else { 0 });
    assert_eq!(stats[2], occupied);
    assert_eq!(stats[3], if expected.live { 0 } else { occupied });
    assert_eq!(stats[4], 0, "no owner eviction");
    assert_eq!(stats[6], effect.history_epoch);
    assert_eq!(stats[7], 0, "no truncated samples");
    let f = |slot: usize, word: usize| f32::from_bits(words[slot * 12 + word]);
    assert_eq!(f(root, 8), effect.simulation_time);
    assert_eq!(aux[root * 3], effect.history_epoch);
    assert_eq!(
        f(root, 7),
        if occupied == 0 { 2. } else { 1. },
        "current known bounds"
    );
    let mut owners = 0;
    for index in 0..effect.total_slots as usize {
        let base = root + 1 + index * 5;
        if words[base * 12 + 10] & 0xffff == 0 {
            continue;
        }
        owners += 1;
        assert_eq!(words[base * 12 + 11], index as u32, "stable spawn identity");
        assert_eq!(
            aux[base * 3 + 1] as usize,
            expected.samples.len(),
            "repeated time must not append"
        );
        let last = expected.samples.last().unwrap();
        assert!((f(base, 4) - last.1).abs() < 1e-5);
        assert_eq!(f(base, 8), last.0);
        for (i, (time, x)) in expected.samples.iter().enumerate() {
            let slot = base + 1 + (aux[base * 3] as usize + 4 - expected.samples.len() + i) % 4;
            assert_eq!(f(slot, 8), *time);
            assert!(
                (f(slot, 4) - x).abs() < 1e-5,
                "history must retain its original world pose"
            );
            assert!(f(slot, 5).abs() < 1e-5 && f(slot, 6).abs() < 1e-5);
        }
    }
    assert_eq!(owners, occupied);
    let entry = world
        .resource::<TrailCompaction>()
        .entries
        .values()
        .next()
        .unwrap();
    let candidates = (0..occupied)
        .flat_map(|owner| {
            expected
                .segments
                .iter()
                .map(move |segment| owner * 4 + segment)
        })
        .collect::<Vec<_>>();
    let output = read(app, &entry.output, candidates.len() + 4);
    assert_eq!(&output[..4], &[4, candidates.len() as u32, 0, 0]);
    assert_eq!(&output[4..], candidates);
    let views = world.resource::<TrailCulling>();
    for ((view, _), result) in &views.entries {
        let main = world.get::<MainEntity>(*view).unwrap().id();
        let output = read(app, &result.indirect, 4);
        assert_eq!(
            output,
            [
                4,
                if main == expected.near {
                    candidates.len() as u32
                } else {
                    0
                },
                0,
                0
            ]
        );
    }
    // Verify the precise GPU-written indirect buffers consumed by installed commands.
    let mut consumed = world
        .resource::<Captured>()
        .0
        .iter()
        .map(|draw| {
            assert_eq!(draw.owner, entry.owner);
            assert!(matches!(draw.topology, Topology::Strip));
            let (buffer, offset) = draw.command.as_ref().unwrap();
            assert_eq!(*offset, 0);
            buffer.id()
        })
        .collect::<Vec<_>>();
    let mut produced = views
        .entries
        .values()
        .map(|entry| entry.indirect.id())
        .collect::<Vec<_>>();
    consumed.sort_unstable();
    produced.sort_unstable();
    assert_eq!(consumed, produced);
    entry.output.clone()
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_actual_trail_history_feeds_installed_producers_and_queues() {
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
    app.finish();
    app.cleanup();
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
                Transform::from_xyz(x, 0., 8.).looking_at(Vec3::new(x, 0., 0.), Vec3::Y),
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
    let samples = [(0.125, 0.125), (0.25, 0.5), (0.375, 0.875), (0.5, 1.25)];
    let mut previous_output = None;
    // Same capacity source replacement, followed by the paged/1024-owner boundary.
    for (capacity, seed) in [(70, 7), (70, 11), (1025, 13)] {
        let instance = instance(capacity, seed);
        let mut effect = crate::simulation_native::inputs(app.world_mut(), &instance, 0.125);
        assert_eq!(effect.trail_plan.paged(), capacity > 1024);
        effect.history_epoch = 9;
        app.world_mut().entity_mut(owner).insert(effect.clone());
        crate::simulation_native::bind_inputs(app.world_mut(), draw, &effect);
        {
            let mut value = app
                .world_mut()
                .get_mut::<crate::GpuDrawInstance>(draw)
                .unwrap();
            value.trail_owners = capacity;
            value.trail_instances = Some(capacity * 4);
        }
        for (index, &(time, _)) in samples.iter().enumerate() {
            step(
                &mut app,
                owner,
                &mut effect,
                seed,
                time,
                index as f32 * 0.25,
            );
            let segments = (0..index as u32).collect::<Vec<_>>();
            let output = verify(
                &app,
                &effect,
                Expected {
                    samples: &samples[..=index],
                    segments: &segments,
                    live: true,
                    near: cameras[0],
                },
            );
            if index == 0 {
                if let Some((old_capacity, old_id)) = previous_output {
                    assert_eq!(
                        old_id == output.id(),
                        old_capacity == capacity,
                        "reuse at same capacity, grow at boundary"
                    );
                }
                previous_output = Some((capacity, output.id()));
            }
        }
        for (time, segments) in [(0.75, &[0, 1, 2][..]), (1., &[1, 2][..]), (1.125, &[2][..])] {
            step(&mut app, owner, &mut effect, seed, time, 40.);
            verify(
                &app,
                &effect,
                Expected {
                    samples: &samples,
                    segments,
                    live: false,
                    near: cameras[0],
                },
            );
        }
        step(&mut app, owner, &mut effect, seed, 1.25, 40.);
        verify(
            &app,
            &effect,
            Expected {
                samples: &[],
                segments: &[],
                live: false,
                near: cameras[0],
            },
        );
        // Epoch discontinuity rebuilds from a single observation, without replay/checkpoints.
        effect.history_epoch += 1;
        step(&mut app, owner, &mut effect, seed, 0.375, 0.5);
        verify(
            &app,
            &effect,
            Expected {
                samples: &samples[2..3],
                segments: &[],
                live: true,
                near: cameras[0],
            },
        );
        step(&mut app, owner, &mut effect, seed, 0.5, 0.75);
        verify(
            &app,
            &effect,
            Expected {
                samples: &samples[2..],
                segments: &[0],
                live: true,
                near: cameras[0],
            },
        );
    }
    app.world_mut().entity_mut(draw).despawn();
    app.world_mut().entity_mut(owner).despawn();
    settle(&mut app, 0, 0, 0);
    device
        .wgpu_device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    assert!(pollster::block_on(scope.pop()).is_none());
    println!(
        "actual_trail_history owners=70,1025 observations_per_case=10 inline_and_paged=true world_space_history=true retained_dead_heads=true partial_expiry=true epoch_reset=true exact_compaction=true consumed_cull_buffers=true source_rebind=true playback_only=true host_scheduler_qualified=false test_only_readback=true"
    );
}
