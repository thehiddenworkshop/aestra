//! Production trail producers, extraction, bindings and queues in one native schedule.
//! Histories are seeded inputs, not a substitute simulation or a duplicate dispatch.
use crate::{draw_resources::*, effect_inputs::*, pipeline_native, queue_native::asset};
use aestra_gpu::{GpuParticle, GpuRenderGlobals, GpuRenderParams, GpuRenderer};
use bevy::{
    asset::{RenderAssetUsages, io::embedded::EmbeddedAssetRegistry},
    camera::{RenderTarget, ShadowLodOrigin, primitives::Aabb},
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        render_resource::*,
        renderer::{RenderAdapterInfo, RenderDevice, RenderGraph, RenderQueue},
        sync_world::MainEntity,
    },
};
use std::sync::Arc;

#[test]
#[ignore = "Requires native timestamp-capable hardware; run explicitly and serially"]
fn native_020_timestamp_transport_bounds_and_recycles_in_flight_batches() {
    crate::timestamp_transport::tests::
        gpu_timestamp_batches_resolve_and_recycle_without_unbounded_allocation(true);
}

#[derive(Resource)]
struct Gates {
    compact: bool,
    cull: bool,
}
fn compact_enabled(g: Res<Gates>) -> bool {
    g.compact
}
fn cull_enabled(g: Res<Gates>) -> bool {
    g.cull
}
#[derive(Resource, Default)]
struct Captured(Vec<Draw>);
fn capture(submissions: Res<Submissions>, mut captured: ResMut<Captured>) {
    let mut frame = submissions.0.lock().unwrap();
    assert!(!frame.overflow);
    captured.0 = std::mem::take(&mut frame.draws);
}

fn read(app: &App, source: &Buffer, words: usize) -> Vec<u32> {
    let world = app.sub_app(RenderApp).world();
    let device = world.resource::<RenderDevice>().wgpu_device();
    let queue = world.resource::<RenderQueue>();
    let size = (words * 4) as u64;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only trail readback"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&default());
    encoder.copy_buffer_to_buffer(source, 0, &staging, 0, size);
    queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        send.send(r).unwrap();
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
    let bytes = staging.slice(..).get_mapped_range().unwrap();
    let result = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| u32::from_le_bytes(*v))
        .collect();
    drop(bytes);
    staging.unmap();
    result
}

fn settle(app: &mut App, entries: usize, views: usize, draws: usize) {
    for _ in 0..160 {
        app.update();
        let w = app.sub_app(RenderApp).world();
        let compact = w.resource::<TrailCompaction>();
        let cull = w.resource::<TrailCulling>();
        let gates = w.resource::<Gates>();
        if compact.entries.len() == entries
            && cull.entries.len() == views
            && compact.dispatched == gates.compact
            && cull.dispatched == gates.cull
            && w.resource::<Captured>().0.len() == draws
        {
            return;
        }
        std::thread::yield_now();
    }
    let w = app.sub_app(RenderApp).world();
    panic!(
        "trail schedule did not converge: compact={} cull={} draws={}",
        w.resource::<TrailCompaction>().entries.len(),
        w.resource::<TrailCulling>().entries.len(),
        w.resource::<Captured>().0.len()
    );
}

fn histories(owners: u32, active: &[u32], empty_bounds: bool) -> (Vec<GpuParticle>, Vec<u32>) {
    let mut particles = vec![GpuParticle::default(); 1 + owners as usize * 5];
    let mut aux = vec![0; particles.len() * 3];
    particles[0] = GpuParticle {
        packed_emitter_alive: 1,
        rotation: 1.,
        particle_index: 7,
        size: if empty_bounds { 2. } else { 1. },
        position: [-1., -1., -1.].into(),
        color: [2., 1., 1., 0.1].into(),
        ..default()
    };
    aux[0] = 9;
    for &owner in active {
        let base = (1 + owner * 5) as usize;
        particles[base] = GpuParticle {
            packed_emitter_alive: 1,
            position: [2., 0., 0.].into(),
            rotation: 1.2,
            size: 0.1,
            color: [1.; 4].into(),
            ..default()
        };
        aux[base * 3] = 2;
        aux[base * 3 + 1] = 2;
        particles[base + 1].rotation = 0.;
        particles[base + 2].position = [1., 0., 0.].into();
        // The older body segment is exactly expired; only the current-head link survives.
        particles[base + 2].rotation = 0.;
    }
    (particles, aux)
}

fn effect(draw: &crate::GpuDrawInstance) -> GpuEffectBuffers {
    GpuEffectBuffers {
        emitters: draw.aux.clone(),
        renderers: draw.renderers.clone(),
        particles: draw.particles.clone(),
        alive: draw.alive.clone(),
        dead: draw.aux.clone(),
        counters: draw.aux.clone(),
        indirect: draw.indirect.clone(),
        globals: draw.aux.clone(),
        aux: draw.aux.clone(),
        render_globals: draw.render_globals.clone(),
        workgroups: 0,
        has_ribbons: false,
        has_trails: true,
        ribbon_workgroups: 0,
        trail_workgroups: 0,
        trail_plan: aestra_gpu::TrailScratchPlan {
            aux_words: 1,
            max_heads: 0,
            max_owners: draw.trail_owners,
        },
        total_slots: 1 + draw.trail_owners * 5,
        simulation_time: 1.,
        seek_quality: aestra_runtime::SeekQuality::Exact,
        history_policy: aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly,
        history_epoch: 9,
        statistics_token: 42,
        checkpoint_context: Arc::new(TrailContext {
            emitters: vec![],
            key: [0; 22],
            motion: None,
        }),
        trail_roots: vec![],
        simulation_state: aestra_gpu::GpuSimulationState {
            stride: 16,
            records: 0,
        },
        stateful_dispatch: vec![],
        event_links: vec![],
        routed: false,
        particle_outputs: vec![],
        output_suppress_through: 0,
        host_events: Arc::new(HostEventHistory {
            bursts: vec![],
            events: vec![],
        }),
        physics: Arc::from([]),
        stateful_only: false,
    }
}

// Verify both GPU results and the precise buffers consumed by the installed commands.
fn verify(app: &App, active: &[u32], culled: &[u32]) -> Buffer {
    let w = app.sub_app(RenderApp).world();
    let compact = w.resource::<TrailCompaction>();
    let cull = w.resource::<TrailCulling>();
    let gates = w.resource::<Gates>();
    let entry = compact.entries.values().next().unwrap();
    if gates.compact {
        let words = read(app, &entry.output, 4 + active.len());
        assert_eq!(&words[..4], &[4, active.len() as u32, 0, 0]);
        assert_eq!(
            &words[4..],
            &active.iter().map(|v| v * 4 + 1).collect::<Vec<_>>()
        );
    }
    let mut counts = Vec::new();
    for view in cull.entries.values() {
        if gates.cull {
            let words = read(app, &view.indirect, 4);
            assert_eq!(words[0], 4);
            assert_eq!(&words[2..], &[0, 0]);
            counts.push(words[1]);
        }
    }
    counts.sort_unstable();
    assert_eq!(counts, culled);
    let mut expected = if gates.cull {
        cull.entries
            .values()
            .map(|v| v.indirect.id())
            .collect::<Vec<_>>()
    } else if gates.compact {
        vec![entry.output.id(); 2]
    } else {
        vec![]
    };
    let captured = &w.resource::<Captured>().0;
    assert_eq!(captured.len(), 2);
    let mut actual = Vec::new();
    for draw in captured {
        assert_eq!(draw.owner, entry.owner);
        assert!(matches!(draw.topology, Topology::Strip));
        if let Some((buffer, offset)) = &draw.command {
            assert_eq!(*offset, 0);
            actual.push(buffer.id());
        } else {
            assert!(!gates.compact && !gates.cull);
            assert_eq!(draw.direct, [4, entry.count]);
        }
    }
    expected.sort_unstable();
    actual.sort_unstable();
    assert_eq!(actual, expected);
    entry.output.clone()
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_trail_producers_feed_installed_queues_and_fail_open() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                    bevy::render::settings::WgpuSettings {
                        backends: Some(wgpu::Backends::VULKAN),
                        features: wgpu::Features::TIMESTAMP_QUERY,
                        ..default()
                    },
                )),
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
        .init_resource::<crate::preparation_context::PreparationMailboxes>()
        .init_resource::<Captured>()
        .insert_resource(Gates {
            compact: true,
            cull: true,
        })
        .configure_sets(
            RenderGraph,
            TrailCompactionSystems::Compact.run_if(compact_enabled),
        )
        .configure_sets(RenderGraph, CullTrails.run_if(cull_enabled))
        .add_systems(Render, capture.in_set(RenderSystems::Cleanup));
    for (name, source) in [
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
    assert_ne!(info.device_type, wgpu::DeviceType::Cpu);
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    println!(
        "Native installed trail producers: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    let device = world.resource::<RenderDevice>().clone();
    assert!(
        device.features().contains(wgpu::Features::TIMESTAMP_QUERY),
        "timestamp-capable hardware required for producer timing qualification"
    );
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let cameras: [Entity; 2] = std::array::from_fn(|i| {
        let target =
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_target_texture(
                    64,
                    64,
                    TextureFormat::Bgra8UnormSrgb,
                    None,
                ));
        let x = i as f32 * 100.;
        app.world_mut()
            .spawn((
                Camera3d::default(),
                ShadowLodOrigin,
                Camera {
                    order: i as isize,
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
        &aestra_core::material::MaterialProgram::additive_sprite("Trail fixture"),
        "trail",
    );
    let mut draw = pipeline_native::draw(material);
    draw.semantic_material = None;
    draw.renderer_kind = 4;
    draw.blend = aestra_gpu::GpuBlend::Additive;
    draw.trail_instances = Some(70 * 4);
    draw.trail_owners = 70;
    let renderer = GpuRenderer {
        emitter_index: 0,
        blend_mode: 1,
        softness: 1.,
        textured: 0,
        uv_min: [0.; 2].into(),
        uv_max: [1.; 2].into(),
        tint: [1.; 4].into(),
        particle_color: 1,
        renderer_kind: 4,
        frame_count: 5,
        playback_mode: 70,
        flipbook_flags: 0,
        frame_rate: 1.,
        attribute_flags: [0, 0.1f32.to_bits(), 0].into(),
        frames: [[0.; 4].into(); aestra_gpu::MAX_FLIPBOOK_FRAMES],
    };
    let active = [0, 32, 69];
    let (particles, aux) = histories(70, &active, false);
    draw.particles = asset(world, particles);
    draw.aux = asset(world, aux);
    draw.renderers = asset(world, vec![renderer]);
    draw.alive = asset(world, vec![0u32; 1]);
    draw.render_globals = asset(
        world,
        GpuRenderGlobals {
            time: 1.,
            seed: 7,
            ..default()
        },
    );
    draw.render_params = asset(world, GpuRenderParams::default());
    draw.indirect = crate::queue_native::indirect_asset(world, vec![4, 280, 0, 0]);
    draw.texture = texture.clone();
    draw.fallback_texture = texture;
    draw.owner = world.spawn(effect(&draw)).id();
    let owner = draw.owner;
    let draw_entity = world
        .spawn((
            draw,
            Aabb::from_min_max(Vec3::splat(-200.), Vec3::splat(200.)),
        ))
        .id();
    settle(&mut app, 1, 2, 2);
    let initial = verify(&app, &active, &[0, 3]);
    // The production timing context must preserve the extracted owner/token.
    let mailboxes = app
        .sub_app(RenderApp)
        .world()
        .resource::<crate::preparation_context::PreparationMailboxes>();
    for mailbox in [&mailboxes.compaction, &mailboxes.culling] {
        if device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            let samples = mailbox.take().expect("completed producer timestamp batch");
            assert_eq!(samples.len(), 1);
            assert_eq!(samples[0].owner, owner);
            assert_eq!(samples[0].token, 42);
            assert_eq!(samples[0].time, 1.);
            assert!(samples[0].sequence > 0);
        } else {
            assert!(mailbox.take().is_none());
        }
    }
    settle(&mut app, 1, 2, 2);
    assert_eq!(initial.id(), verify(&app, &active, &[0, 3]).id());
    // Different source handles at the same capacity must rebind reused output.
    let active = [1, 33, 68];
    let (particles, aux) = histories(70, &active, false);
    let particles = asset(app.world_mut(), particles);
    let aux = asset(app.world_mut(), aux);
    {
        let mut draw = app
            .world_mut()
            .get_mut::<crate::GpuDrawInstance>(draw_entity)
            .unwrap();
        draw.particles = particles;
        draw.aux = aux;
    }
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(initial.id(), verify(&app, &active, &[0, 3]).id());
    // Readiness gates after success cannot retain stale dispatched flags.
    for (compact, cull, counts) in [
        (true, false, vec![]),
        (false, true, vec![0, 280]),
        (false, false, vec![]),
    ] {
        *app.sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<Gates>() = Gates { compact, cull };
        settle(&mut app, 1, 2, 2);
        verify(&app, &active, &counts);
    }
    *app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<Gates>() = Gates {
        compact: true,
        cull: true,
    };
    // Crossing the 1024-owner prefix-page boundary forces output replacement.
    let active = [0, 1023, 1024];
    let (particles, aux) = histories(1025, &active, false);
    let particles = asset(app.world_mut(), particles);
    let aux = asset(app.world_mut(), aux);
    let renderers = asset(
        app.world_mut(),
        vec![GpuRenderer {
            playback_mode: 1025,
            ..renderer
        }],
    );
    {
        let mut draw = app
            .world_mut()
            .get_mut::<crate::GpuDrawInstance>(draw_entity)
            .unwrap();
        draw.particles = particles;
        draw.aux = aux;
        draw.renderers = renderers;
        draw.trail_instances = Some(4100);
        draw.trail_owners = 1025;
    }
    for _ in 0..4 {
        app.update();
    }
    settle(&mut app, 1, 2, 2);
    assert_ne!(initial.id(), verify(&app, &active, &[0, 3]).id());
    // The exact per-view matrix must update, not keep the previous cull decision.
    *app.world_mut().get_mut::<Transform>(cameras[1]).unwrap() =
        Transform::from_xyz(0., 0., 8.).looking_at(Vec3::ZERO, Vec3::Y);
    for _ in 0..4 {
        app.update();
    }
    verify(&app, &active, &[3, 3]);
    // Old history epochs fail open instead of rejecting valid geometry.
    *app.world_mut().get_mut::<Transform>(cameras[1]).unwrap() =
        Transform::from_xyz(100., 0., 8.).looking_at(Vec3::new(100., 0., 0.), Vec3::Y);
    app.world_mut()
        .get_mut::<GpuEffectBuffers>(owner)
        .unwrap()
        .history_epoch = 10;
    for _ in 0..4 {
        app.update();
    }
    verify(&app, &active, &[3, 3]);
    app.world_mut()
        .get_mut::<GpuEffectBuffers>(owner)
        .unwrap()
        .history_epoch = 9;
    let (particles, _) = histories(1025, &active, true);
    let particles = asset(app.world_mut(), particles);
    app.world_mut()
        .get_mut::<crate::GpuDrawInstance>(draw_entity)
        .unwrap()
        .particles = particles;
    for _ in 0..4 {
        app.update();
    }
    verify(&app, &active, &[0, 0]);
    // Expired segments must rebuild the same output to a zero-instance command.
    let globals = asset(
        app.world_mut(),
        GpuRenderGlobals {
            time: 3.,
            seed: 7,
            ..default()
        },
    );
    app.world_mut()
        .get_mut::<crate::GpuDrawInstance>(draw_entity)
        .unwrap()
        .render_globals = globals.clone();
    app.world_mut()
        .get_mut::<GpuEffectBuffers>(owner)
        .unwrap()
        .render_globals = globals;
    for _ in 0..4 {
        app.update();
    }
    verify(&app, &[], &[0, 0]);
    app.world_mut().entity_mut(cameras[1]).despawn();
    settle(&mut app, 1, 1, 1);
    app.world_mut()
        .get_mut::<Visibility>(draw_entity)
        .unwrap()
        .clone_from(&Visibility::Hidden);
    settle(&mut app, 0, 0, 0);
    app.world_mut().entity_mut(draw_entity).despawn();
    app.world_mut().entity_mut(owner).despawn();
    app.world_mut().entity_mut(cameras[0]).despawn();
    settle(&mut app, 0, 0, 0);
    device
        .wgpu_device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    assert!(pollster::block_on(scope.pop()).is_none());
    // Touch actual MainEntity metadata rather than substituting an owner map.
    assert_eq!(
        app.sub_app_mut(RenderApp)
            .world_mut()
            .query::<(&MainEntity, &GpuEffectBuffers)>()
            .iter(app.sub_app(RenderApp).world())
            .count(),
        0
    );
    println!(
        "trail_compute_schedule views=2 owners=70,1025 stable_indices=true reuse=true source_rebind=true camera_motion=true dispatch_fallbacks=3 stale_epoch_fail_open=true empty_bounds=true expired=true view_retirement=true teardown=true test_only_readback=true"
    );
}
