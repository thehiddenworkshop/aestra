//! Complete host/renderer plugins, asynchronous pipelines and one joint clock.
//! Only authored inputs, a withheld shader asset and bounded readback are test-owned.
use crate::{
    effect_inputs::GpuEffectBuffers,
    host_lights_native as receiver,
    presentation_readiness::PresentedOwners,
    stage_runtimes::StageRuntimes,
    stateful_simulation::{
        STATEFUL_SIMULATION_SHADER_PATH, StatefulSimulationPipeline, StatefulStates,
    },
};
use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_asset::RenderAssets,
        render_resource::PipelineCache,
        renderer::{RenderDevice, RenderQueue},
        storage::GpuShaderBuffer,
        sync_world::MainEntity,
    },
    shader::Shader,
};
use std::sync::{Arc, Mutex};

#[derive(Resource, Clone)]
struct Evidence(Arc<Mutex<Frame>>);
#[derive(Default)]
struct Frame {
    withheld: bool,
    pending_frames: u32,
    owner: Option<Entity>,
    epoch: u32,
    tick: u32,
    domain_tick: u32,
    ready: bool,
    states: usize,
    domains: usize,
    compact_entries: usize,
    cull_entries: usize,
    queue: Option<RenderQueue>,
    // Cloned GPU handles, not CPU copies or fixture-provided presentation inputs.
    buffers: Vec<(&'static str, wgpu::Buffer, usize)>,
    epoch_word: usize,
    telemetry: Option<(wgpu::Buffer, u32)>,
}
#[derive(Resource)]
struct DelayedShader(Handle<Shader>);

fn withhold_shader(
    mut installed: Local<bool>,
    delayed: Res<DelayedShader>,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<StatefulSimulationPipeline>,
) {
    if *installed {
        return;
    }
    *installed = true;
    // Retain the production layout/entry point; delay only its asset dependency.
    // Present readiness gates the whole coupled dispatch, not just drawing.
    let mut descriptor = cache
        .get_compute_pipeline_descriptor(pipeline.present)
        .clone();
    descriptor.shader = delayed.0.clone();
    pipeline.present = cache.queue_compute_pipeline(descriptor);
}

type ObservationResources<'w> = (
    Res<'w, StatefulStates>,
    Res<'w, StageRuntimes>,
    Res<'w, PresentedOwners>,
    Res<'w, RenderAssets<GpuShaderBuffer>>,
    Res<'w, crate::draw_resources::TrailCompaction>,
    Res<'w, crate::draw_resources::TrailCulling>,
    Res<'w, RenderQueue>,
);
fn observe(
    evidence: Res<Evidence>,
    effects: Query<(Entity, &MainEntity, &GpuEffectBuffers)>,
    resources: ObservationResources,
) {
    let (states, domains, ready, buffers, compact, cull, queue) = resources;
    let mut frame = evidence.0.lock().unwrap();
    frame.owner = None;
    frame.ready = false;
    frame.states = states.0.len();
    frame.domains = domains.0.len();
    frame.compact_entries = compact.entries.len();
    frame.cull_entries = cull.entries.len();
    frame.buffers.clear();
    frame.telemetry = None;
    frame.queue = Some(queue.clone());
    for (entity, main, effect) in &effects {
        let (Some(persistent), Some(stages)) = (states.0.get(&entity), domains.0.get(&entity))
        else {
            continue;
        };
        let Some(domain) = stages.timelines.first().and_then(Option::as_ref) else {
            continue;
        };
        assert_eq!(persistent.len(), 1);
        assert!(effect.has_trails && !effect.stateful_dispatch.is_empty());
        assert_eq!(
            effect.history_policy,
            crate::PlaybackHistoryPolicy::PlaybackOnly
        );
        assert!(persistent[0].checkpoints.is_empty());
        assert_eq!(domain.checkpoint_bytes(), 0);
        frame.owner = Some(main.id());
        frame.epoch = effect.history_epoch;
        frame.tick = persistent[0].last_tick;
        frame.domain_tick = domain.last_tick();
        frame.ready = ready.owners.contains(&entity);
        assert_eq!(
            frame.domain_tick, frame.tick,
            "coupled clocks must never diverge"
        );
        if frame.withheld {
            assert!(!frame.ready, "missing shader cannot publish readiness");
            assert_eq!(frame.tick, 0, "missing shader cannot consume joint ticks");
            assert_eq!(persistent[0].spawn_accumulator, 0.);
            frame.pending_frames += 1;
        }
        let raw = |buffer: &wgpu::Buffer| buffer.clone();
        frame.telemetry = buffers
            .get(&effect.indirect)
            .map(|buffer| (raw(&buffer.buffer), effect.statistics_token));
        for (name, handle) in [("particles", &effect.particles), ("aux", &effect.aux)] {
            if let Some(buffer) = buffers.get(handle) {
                let words = if name == "aux" {
                    // Only persistent owner/sample metadata, not transient paged sort scratch.
                    buffers.get(&effect.particles).unwrap().buffer.size() as usize / 16
                } else {
                    buffer.buffer.size() as usize / 4
                };
                frame.buffers.push((name, raw(&buffer.buffer), words));
            }
        }
        frame.epoch_word = effect.trail_roots[0].1 as usize * 3;
        for (name, resource) in [
            ("density", aestra_fluid::RESOURCE_DENSITY),
            ("velocity", aestra_fluid::RESOURCE_VELOCITY),
        ] {
            let buffer = domain.executor().buffer(resource).unwrap();
            frame
                .buffers
                .push((name, raw(buffer), buffer.size() as usize / 4));
        }
        if compact.dispatched {
            for entry in compact
                .entries
                .values()
                .filter(|entry| entry.owner == main.id())
            {
                frame.buffers.push(("candidates", raw(&entry.output), 4));
            }
        }
    }
}

// Test oracle only. No production system waits for GPU completion or reads particles.
fn read(
    device: &RenderDevice,
    queue: &RenderQueue,
    source: &wgpu::Buffer,
    words: usize,
) -> Vec<u32> {
    let device = device.wgpu_device();
    let size = (words * 4) as u64;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only full-plugin coupled readback"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&default());
    encoder.copy_buffer_to_buffer(source, 0, &staging, 0, size);
    queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    staging
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

fn snapshot(
    app: &App,
    device: &RenderDevice,
) -> std::collections::BTreeMap<&'static str, Vec<u32>> {
    let frame = app.world().resource::<Evidence>().0.lock().unwrap();
    let queue = frame.queue.as_ref().unwrap();
    if frame.ready {
        let (indirect, token) = frame.telemetry.as_ref().unwrap();
        let words = read(device, queue, indirect, indirect.size() as usize / 4);
        assert_eq!(
            &words[words.len() - 4..],
            &[
                aestra_gpu::PARTICLE_STATISTICS_MAGIC,
                *token,
                frame.epoch,
                crate::stateful_simulation::stateful_tick_time(frame.tick).to_bits(),
            ],
            "canonical telemetry must be valid at exact host frame boundaries"
        );
    }
    let mut result = std::collections::BTreeMap::new();
    for (name, buffer, words) in &frame.buffers {
        let mut value = read(device, queue, buffer, *words);
        if *name == "aux" {
            // Restarts deliberately rebase discontinuity tags, not recorded positions.
            value[frame.epoch_word] = 0;
        }
        result.insert(*name, value);
    }
    assert_eq!(
        result.len(),
        5,
        "real particle, trail and domain buffers required"
    );
    result
}

fn settle(app: &mut App, owner: Entity) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
    receiver::until(app, |app| {
        let presented = app.world().get::<crate::PresentedEffect>(owner).unwrap();
        let frame = app.world().resource::<Evidence>().0.lock().unwrap();
        assert!(
            std::time::Instant::now() < deadline,
            "coupled owner did not settle: owner={:?} epoch={}/{} tick={}/{} domain={} ready={} buffers={} compact={} cull={} progress={:?}",
            frame.owner,
            frame.epoch,
            presented.instance.history_epoch(),
            frame.tick,
            aestra_runtime::trace_tick(presented.instance.time()),
            frame.domain_tick,
            frame.ready,
            frame.buffers.len(),
            frame.compact_entries,
            frame.cull_entries,
            app.world().get::<crate::GpuStageProgress>(owner)
        );
        frame.owner == Some(owner)
            && frame.ready
            && frame.epoch == presented.instance.history_epoch()
            && frame.tick == aestra_runtime::trace_tick(presented.instance.time()) as u32
            && frame.domain_tick == frame.tick
            && frame.buffers.len() == 5
            && frame.compact_entries == 1
            && frame.cull_entries == 1
            && app
                .world()
                .get::<crate::GpuStageProgress>(owner)
                .is_some_and(|progress| progress.is_ready(&presented.instance))
    });
}
fn seek(app: &mut App, owner: Entity, tick: u32) {
    app.world_mut()
        .get_mut::<crate::EffectPlayer>(owner)
        .unwrap()
        .seek_frame(u64::from(tick));
    settle(app, owner);
}
fn allocation(app: &App, owner: Entity) -> [AssetId<bevy::render::storage::ShaderBuffer>; 3] {
    let effect = app.world().get::<GpuEffectBuffers>(owner).unwrap();
    [effect.particles.id(), effect.aux.id(), effect.globals.id()]
}

fn submitted_trails(app: &mut App, owner: Entity, expected_heads: u32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
    receiver::until(app, |app| {
        let presented = app.world().get::<crate::PresentedEffect>(owner).unwrap();
        let statistics = app
            .world()
            .get::<crate::particle_statistics::GpuParticleStatistics>(owner)
            .unwrap();
        assert!(
            std::time::Instant::now() < deadline,
            "actual trails did not submit: expected_heads={expected_heads} counts={:?} geometry={:?}",
            statistics.observation(&presented.instance),
            statistics.geometry
        );
        statistics.observation(&presented.instance)
            .is_some_and(|(_, counts)| counts == [expected_heads])
            && app.world().get::<crate::EffectProfiler>(owner).is_some_and(|profile| {
                matches!(profile.0.submitted_primitives, aestra_runtime::ProfileValue::Measured(n) if n > 0)
            })
    });
}

#[test]
#[ignore = "Requires native Vulkan; real pipelined render thread; run serially"]
fn native_020_full_plugins_hold_and_resume_coupled_particles_fluid_and_trails() {
    let evidence = Evidence(Arc::new(Mutex::new(Frame {
        withheld: true,
        ..default()
    })));
    let (mut app, device) = crate::pipelined_installers_native::app_with(|app| {
        let shader = app.world().resource::<Assets<Shader>>().reserve_handle();
        app.insert_resource(DelayedShader(shader.clone()))
            .insert_resource(evidence.clone());
        app.sub_app_mut(RenderApp)
            .insert_resource(DelayedShader(shader))
            .insert_resource(evidence)
            .add_systems(
                Render,
                (withhold_shader, observe)
                    .chain()
                    .in_set(RenderSystems::Cleanup),
            );
    });
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    receiver::configure(&mut app, crate::ParticleLightMode::PortableAsync);
    let (_, target) = receiver::receiver(&mut app);
    // Register the initializer's queued descriptors before copying one. There
    // is no effect owner yet, so nothing can advance before installing the delay.
    app.update();
    let registry = aestra_extension::ExtensionRegistry::linked();
    let source = crate::coupled_trails_native::source(&registry, 70);
    let compiled = aestra_compiler::EffectCompiler::with_extensions(registry)
        .compile(&source)
        .unwrap();
    assert!(compiled.emitters[0].field_follow.is_some());
    let mut player = crate::EffectPlayer::from_compiled(Arc::new(compiled))
        .with_history_policy(crate::PlaybackHistoryPolicy::PlaybackOnly);
    player.playing = false;
    player.instance_mut().set_seed(7);
    player.seek_frame(30);
    let owner = app.world_mut().spawn(player).id();
    receiver::until(&mut app, |app| {
        let frame = app.world().resource::<Evidence>().0.lock().unwrap();
        frame.pending_frames >= 8 && frame.buffers.len() == 5
    });
    let buffers = allocation(&app, owner);
    let pending = snapshot(&app, &device);
    assert!(pending["particles"].iter().all(|word| *word == 0));
    assert!(pending["density"].iter().all(|word| *word == 0));
    assert!(pending["velocity"].iter().all(|word| *word == 0));
    assert_eq!(pending["candidates"][1], 0);
    println!("full-plugin delayed present: allocated joint state held for eight render frames");
    let delayed = app.world().resource::<DelayedShader>().0.clone();
    app.world()
        .resource::<Evidence>()
        .0
        .lock()
        .unwrap()
        .withheld = false;
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
    settle(&mut app, owner);
    assert_eq!(
        allocation(&app, owner),
        buffers,
        "shader readiness must retain allocation"
    );
    let live = snapshot(&app, &device);
    let heads = live["particles"][..70 * 12].as_chunks::<12>().0;
    assert_eq!(heads.iter().filter(|p| p[10] & 0xffff != 0).count(), 3);
    assert!(
        heads
            .iter()
            .filter(|p| p[10] & 0xffff != 0)
            .all(|p| f32::from_bits(p[5]).abs() > 0.00001),
        "actual field-follow affects heads"
    );
    assert!(live["density"].iter().any(|v| f32::from_bits(*v) > 0.));
    assert!(
        live["velocity"]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|v| f32::from_bits(v[1]).abs() > 0.)
    );
    assert!(
        live["candidates"][1] > 0,
        "actual history producer must create trail candidates"
    );
    submitted_trails(&mut app, owner, 3);
    let pixels = receiver::capture(&mut app, &target);
    assert!(
        pixels.0 > 0.5,
        "actual plugin presentation must reach the target: {pixels:?}"
    );
    assert_eq!(
        snapshot(&app, &device),
        live,
        "paused pipeline frames cannot duplicate ticks"
    );
    // Move beyond head death: the same joint clock must preserve still-live tails.
    seek(&mut app, owner, 70);
    let tails = snapshot(&app, &device);
    assert!(
        tails["particles"][..70 * 12]
            .as_chunks::<12>()
            .0
            .iter()
            .all(|p| p[10] & 0xffff == 0)
    );
    assert!(
        tails["candidates"][1] > 0,
        "dead heads must retain living tails"
    );
    submitted_trails(&mut app, owner, 0);
    seek(&mut app, owner, 30);
    assert_eq!(
        snapshot(&app, &device),
        live,
        "backward seek must reconstruct all joint buffers"
    );
    app.world_mut()
        .get_mut::<crate::EffectPlayer>(owner)
        .unwrap()
        .restart();
    settle(&mut app, owner);
    assert_eq!(app.world().resource::<Evidence>().0.lock().unwrap().tick, 0);
    let reset = snapshot(&app, &device);
    assert_eq!(reset["candidates"][1], 0, "restart must clear prior trails");
    seek(&mut app, owner, 30);
    assert_eq!(
        snapshot(&app, &device),
        live,
        "restart must reproduce particles, domains and history"
    );
    assert_eq!(
        allocation(&app, owner),
        buffers,
        "clock transitions must reuse production buffers"
    );
    receiver::until(&mut app, |app| {
        let presented = app.world().get::<crate::PresentedEffect>(owner).unwrap();
        let context = app
            .world()
            .get::<crate::particle_statistics::GpuParticleStatistics>(owner)
            .unwrap();
        app.world()
            .get::<crate::simulation_timing::GpuSimulationTiming>(owner)
            .and_then(|timing| timing.frame_sample(&presented.instance, context))
            .is_some_and(|sample| sample.work.checkpoint_capture_bytes == Some(0))
    });
    app.world_mut().entity_mut(owner).despawn();
    receiver::until(&mut app, |app| {
        let frame = app.world().resource::<Evidence>().0.lock().unwrap();
        frame.owner.is_none()
            && frame.states == 0
            && frame.domains == 0
            && frame.compact_entries == 0
            && frame.cull_entries == 0
    });
    // Release fixture-owned cloned handles before the final device/error check.
    {
        let mut frame = app.world().resource::<Evidence>().0.lock().unwrap();
        frame.buffers.clear();
        frame.telemetry = None;
        frame.queue = None;
    }
    device
        .wgpu_device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    assert!(pollster::block_on(scope.pop()).is_none());
    println!(
        "full_plugins=true render_thread=true delayed_present_frames=8 no_partial_ticks=true fluid_follow=true joint_trail_candidates=true dead_heads_live_tails=true paused_idempotence=true seek_restart_bitwise=true allocation_reuse=true playback_only_no_captures=true teardown=true test_only_readback=true"
    );
}
