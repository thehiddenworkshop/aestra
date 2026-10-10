//! Installed production compute -> per-view bindings -> real transparent draw queues.
//! Seeded simulation inputs; the only additional compute/readback is a test observer.
use crate::{alpha_sort, draw_resources::*, pipeline_native, queue_native::asset};
use aestra_gpu::{GpuParticle, GpuRenderGlobals, GpuRenderParams, GpuRenderer};
use bevy::{
    asset::{RenderAssetUsages, io::embedded::EmbeddedAssetRegistry},
    camera::{RenderTarget, ShadowLodOrigin, primitives::Aabb},
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        render_resource::*,
        renderer::{
            RenderAdapterInfo, RenderContext, RenderDevice, RenderGraph, RenderGraphSystems,
        },
        sync_world::MainEntity,
    },
};
use std::collections::BTreeMap;

#[derive(Resource)]
struct DispatchEnabled(bool);
fn dispatch_enabled(gate: Res<DispatchEnabled>) -> bool {
    gate.0
}

#[derive(Resource)]
pub(super) struct Observer {
    pipeline: wgpu::ComputePipeline,
    outputs: BTreeMap<Entity, (Buffer, Buffer)>,
    pub(super) draws: usize,
}

pub(super) fn observe(
    mut context: RenderContext,
    device: Res<RenderDevice>,
    state: Res<AlphaSort>,
    views: Query<&MainEntity>,
    bindings: Query<&crate::draw_commands::GpuRenderBindGroup>,
    mut observer: ResMut<Observer>,
) {
    observer.outputs.clear();
    if !state.dispatched {
        return;
    }
    for ((view, draw), entry) in &state.entries {
        // The consumer's actual prepared bind group must exist for this exact pair.
        assert!(bindings.get(*draw).unwrap().2.contains_key(view));
        let output = device.create_buffer(&BufferDescriptor {
            label: Some("qualification-only permutation observation"),
            size: u64::from(entry.count) * 4,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let layout = observer.pipeline.get_bind_group_layout(0);
        let group = device
            .wgpu_device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: entry.indices.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: output.as_entire_binding(),
                    },
                ],
            });
        {
            let mut pass = context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("qualification-only observer; not an Aestra producer"),
                    timestamp_writes: None,
                });
            pass.set_pipeline(&observer.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(entry.count.div_ceil(64), 1, 1);
        }
        observer.outputs.insert(
            views.get(*view).unwrap().id(),
            (entry.indices.clone(), output),
        );
    }
}

pub(super) fn capture(submissions: Res<Submissions>, mut observer: ResMut<Observer>) {
    let mut frame = submissions.0.lock().unwrap();
    assert!(!frame.overflow);
    observer.draws = frame.draws.drain(..).count();
}

pub(super) fn settle(app: &mut App, pairs: usize, dispatched: bool, draws: usize) {
    for _ in 0..160 {
        app.update();
        let world = app.sub_app(RenderApp).world();
        let state = world.resource::<AlphaSort>();
        if state.entries.len() == pairs
            && state.dispatched == dispatched
            && world.resource::<Observer>().draws == draws
        {
            return;
        }
        std::thread::yield_now();
    }
    let world = app.sub_app(RenderApp).world();
    panic!(
        "alpha compute/queue did not converge: pairs={} dispatched={} draws={}",
        world.resource::<AlphaSort>().entries.len(),
        world.resource::<AlphaSort>().dispatched,
        world.resource::<Observer>().draws
    );
}

pub(super) fn permutation(app: &App, camera: Entity) -> (Buffer, Vec<u32>) {
    let world = app.sub_app(RenderApp).world();
    let device = world.resource::<RenderDevice>().wgpu_device();
    let queue = world.resource::<bevy::render::renderer::RenderQueue>();
    let (source, output) = &world.resource::<Observer>().outputs[&camera];
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("qualification-only readback"),
        size: output.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&default());
    encoder.copy_buffer_to_buffer(output, 0, &readback, 0, output.size());
    queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap();
        });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    receive
        .recv_timeout(std::time::Duration::from_secs(60))
        .unwrap()
        .unwrap();
    let mapped = readback.slice(..).get_mapped_range().unwrap();
    let indices = mapped
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| u32::from_le_bytes(*v))
        .collect();
    drop(mapped);
    readback.unmap();
    (source.clone(), indices)
}

pub(super) fn install_observer(world: &mut World) {
    let device = world.resource::<RenderDevice>().clone();
    let module = device
        .wgpu_device()
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                "@group(0) @binding(0) var<storage, read> source: array<u32>;
            @group(0) @binding(1) var<storage, read_write> output: array<u32>;
            @compute @workgroup_size(64) fn observe(@builtin(global_invocation_id) id: vec3<u32>) {
                if id.x < arrayLength(&output) { output[id.x] = source[id.x]; }
            }"
                .into(),
            ),
        });
    let pipeline = device
        .wgpu_device()
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("observe"),
            compilation_options: default(),
            cache: None,
        });
    world.insert_resource(Observer {
        pipeline,
        outputs: default(),
        draws: 0,
    });
}

fn particles(capacity: u32) -> Vec<GpuParticle> {
    (0..capacity + 3)
        .map(|slot| GpuParticle {
            position: [0., 0., ((slot * 17) % 23) as f32 / 6. - 2.].into(),
            particle_index: slot,
            size: 0.1,
            color: [1.; 4].into(),
            packed_emitter_alive: 1,
            ..default()
        })
        .collect()
}

fn expected(capacity: u32, live: u32, reverse: bool) -> Vec<u32> {
    let particles = particles(capacity);
    let mut slots = (3..3 + live).collect::<Vec<_>>();
    slots.sort_by(|a, b| {
        let z = |slot: u32| particles[slot as usize].position.z * if reverse { -1. } else { 1. };
        z(*a).total_cmp(&z(*b)).then(a.cmp(b))
    });
    slots
}

fn verify(app: &App, cameras: &[Entity; 2], capacity: u32, live: u32) -> [Buffer; 2] {
    std::array::from_fn(|index| {
        let (source, actual) = permutation(app, cameras[index]);
        assert_eq!(
            &actual[..live as usize],
            expected(capacity, live, index == 1)
        );
        assert!(actual[live as usize..].iter().all(|v| *v == u32::MAX));
        source
    })
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_alpha_compute_feeds_installed_queues_and_gates_stale_indices() {
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
    let statistics = alpha_sort::GpuAlphaSortStatistics::default();
    let render = app.sub_app_mut(RenderApp);
    crate::render::install(render);
    alpha_sort::install(render);
    render
        .insert_resource(statistics.clone())
        .insert_resource(DispatchEnabled(false))
        .init_resource::<TrailCompaction>()
        .init_resource::<TrailCulling>()
        .init_resource::<Submissions>()
        .configure_sets(RenderGraph, SortAlpha.run_if(dispatch_enabled))
        .add_systems(
            RenderGraph,
            observe.after(SortAlpha).before(RenderGraphSystems::Render),
        )
        .add_systems(Render, capture.in_set(RenderSystems::Cleanup));
    for (path, source) in [
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
        "Native installed alpha compute: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    let device = world.resource::<RenderDevice>().clone();
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    install_observer(world);
    // Settings are extracted from the actual main-world resource each frame.
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
    let mut renderer = GpuRenderer {
        emitter_index: 0,
        blend_mode: 0,
        softness: 1.,
        textured: 0,
        uv_min: [0.; 2].into(),
        uv_max: [1.; 2].into(),
        tint: [1.; 4].into(),
        particle_color: 1,
        renderer_kind: 0,
        frame_count: 1,
        playback_mode: 0,
        flipbook_flags: 0,
        frame_rate: 0.,
        attribute_flags: [0; 3].into(),
        frames: [[0.; 4].into(); aestra_gpu::MAX_FLIPBOOK_FRAMES],
    };
    // Keep the legacy shader path independent of semantic material compilation.
    renderer.blend_mode = aestra_gpu::GpuBlend::Alpha as u32;
    let material = crate::queue_native::material(
        world,
        &aestra_core::material::MaterialProgram::additive_sprite("Alpha fixture"),
        "alpha",
    );
    let mut draw = pipeline_native::draw(material);
    draw.semantic_material = None;
    draw.renderers = asset(world, vec![renderer]);
    draw.particles = asset(world, particles(257));
    draw.alive = asset(world, (0..260u32).collect::<Vec<_>>());
    draw.aux = asset(world, vec![0u32; 64]);
    draw.render_globals = asset(world, GpuRenderGlobals::default());
    draw.render_params = asset(
        world,
        GpuRenderParams {
            alive_offset: 3,
            ..default()
        },
    );
    draw.indirect = crate::queue_native::indirect_asset(world, vec![4, 129, 0, 0]);
    draw.sort_range = UVec2::new(3, 257);
    draw.texture = texture.clone();
    draw.fallback_texture = texture;
    draw.owner = world.spawn_empty().id();
    let draw = world
        .spawn((draw, Aabb::from_min_max(Vec3::splat(-3.), Vec3::splat(3.))))
        .id();
    // Prepared but undispatched must not consume uninitialized or previous-frame permutations.
    settle(&mut app, 2, false, 0);
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<DispatchEnabled>()
        .0 = true;
    settle(&mut app, 2, true, 2);
    let sources = verify(&app, &cameras, 257, 129);
    settle(&mut app, 2, true, 2);
    assert_eq!(statistics.snapshot().allocated_buffers_this_frame, 0);
    assert_eq!(statistics.snapshot().pairs, 2);
    assert!(statistics.snapshot().owned_buffer_bytes > 0);
    let reused = verify(&app, &cameras, 257, 129);
    assert_eq!(sources.map(|b| b.id()), reused.map(|b| b.id()));
    // Camera movement must refresh uniforms without reallocating view-local output.
    for (index, camera) in cameras.iter().enumerate() {
        *app.world_mut().get_mut::<Transform>(*camera).unwrap() =
            Transform::from_xyz(0., 0., if index == 0 { -8. } else { 8. })
                .looking_at(Vec3::ZERO, Vec3::Y);
    }
    for _ in 0..4 {
        app.update();
    }
    verify(&app, &[cameras[1], cameras[0]], 257, 129);
    // Rebind changed sources at the SAME capacity, where cached output is reused.
    let mut reflected = particles(257);
    for particle in &mut reflected {
        particle.position.z = -particle.position.z;
    }
    let replacement = asset(app.world_mut(), reflected);
    app.world_mut()
        .get_mut::<crate::GpuDrawInstance>(draw)
        .unwrap()
        .particles = replacement;
    for _ in 0..4 {
        app.update();
    }
    let rebound = verify(&app, &cameras, 257, 129);
    assert_eq!(statistics.snapshot().allocated_buffers_this_frame, 0);
    // Restore camera orientation for the subsequent growth/empty cases.
    for (index, camera) in cameras.iter().enumerate() {
        *app.world_mut().get_mut::<Transform>(*camera).unwrap() =
            Transform::from_xyz(0., 0., if index == 0 { 8. } else { -8. })
                .looking_at(Vec3::ZERO, Vec3::Y);
    }
    // Gate after a successful frame: preparation must reset dispatched, not reuse stale data.
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<DispatchEnabled>()
        .0 = false;
    settle(&mut app, 2, false, 0);
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<DispatchEnabled>()
        .0 = true;
    // Source replacement plus capacity growth forces a new merge plan and rebinds.
    let world = app.world_mut();
    let new_particles = asset(world, particles(513));
    let alive = asset(world, (0..516u32).collect::<Vec<_>>());
    let indirect = crate::queue_native::indirect_asset(world, vec![4, 513, 0, 0]);
    {
        let mut value = world.get_mut::<crate::GpuDrawInstance>(draw).unwrap();
        value.particles = new_particles;
        value.alive = alive;
        value.indirect = indirect;
        value.sort_range.y = 513;
    }
    // Allow changed assets and extraction to reach the render world before inspecting data.
    for _ in 0..4 {
        app.update();
    }
    settle(&mut app, 2, true, 2);
    let grown = verify(&app, &cameras, 513, 513);
    assert_ne!(rebound.map(|b| b.id()), grown.map(|b| b.id()));
    let empty = crate::queue_native::indirect_asset(app.world_mut(), vec![4, 0, 0, 0]);
    app.world_mut()
        .get_mut::<crate::GpuDrawInstance>(draw)
        .unwrap()
        .indirect = empty;
    for _ in 0..4 {
        app.update();
    }
    settle(&mut app, 2, true, 2);
    verify(&app, &cameras, 513, 0);
    app.world_mut()
        .resource_mut::<crate::AestraRenderSettings>()
        .transparent_order = crate::TransparentOrderMode::Fast;
    settle(&mut app, 0, false, 2);
    app.world_mut()
        .get_mut::<Visibility>(draw)
        .map(|mut value| *value = Visibility::Hidden)
        .unwrap();
    settle(&mut app, 0, false, 0);
    app.world_mut().entity_mut(draw).despawn();
    for camera in cameras {
        app.world_mut().entity_mut(camera).despawn();
    }
    settle(&mut app, 0, false, 0);
    device
        .wgpu_device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    assert!(pollster::block_on(scope.pop()).is_none());
    println!(
        "alpha_compute_schedule views=2 capacities=257,513 zero_partial_full=true reuse=true camera_motion=true dispatch_gate=true source_rebind=true teardown=true test_only_readback=true"
    );
}
