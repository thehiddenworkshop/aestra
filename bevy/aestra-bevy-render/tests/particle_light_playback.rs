//! Native headless render-app proof, not the F7E light realization adapter.
use aestra_bevy_render::{AestraRenderPlugin, PresentedEffect, gpu::particle_lights::*};
use aestra_core::{
    Curve, CurveKey, EffectAsset, Emitter, ParticlePointLightProperties, SceneOutputInstance,
};
use aestra_runtime::PlaybackHistoryPolicy;
use bevy::{
    prelude::*,
    render::{RenderApp, renderer::RenderDevice},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn snapshot(app: &App) -> (SelectedGpuParticleLights, Vec<u32>) {
    let render = app.get_sub_app(RenderApp).unwrap();
    let frame = render
        .world()
        .resource::<GpuSelectedParticleLights>()
        .frame()
        .expect("selected GPU frame")
        .clone();
    let device = render.world().resource::<RenderDevice>().wgpu_device();
    let queue = render
        .world()
        .resource::<bevy::render::renderer::RenderQueue>();
    let size = u64::from(frame.selected_capacity) * 48;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: size + 16,
        mapped_at_creation: false,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(&frame.records, 0, &buffer, 0, size);
    encoder.copy_buffer_to_buffer(&frame.counters, 0, &buffer, size, 16);
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = buffer.slice(..).get_mapped_range();
    let words = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| u32::from_le_bytes(*v))
        .collect();
    drop(bytes);
    buffer.unmap();
    (frame, words)
}

fn settle(app: &mut App, selected: u32) {
    let start = Instant::now();
    loop {
        app.update();
        let render = app.get_sub_app(RenderApp).unwrap();
        let state = render.world().resource::<GpuSelectedParticleLights>();
        assert!(state.rejection.is_none(), "{:?}", state.rejection);
        if state.frame().is_some() {
            let (frame, words) = snapshot(app);
            if words[frame.selected_capacity as usize * 12 + 2] == selected {
                break;
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "selection pipelines did not become ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn headless(cap: u32) -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::log::LogPlugin>()
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>(),
    )
    .add_plugins(AestraRenderPlugin)
    .insert_resource(AestraParticleLightSettings {
        max_lights: cap,
        max_scratch_bytes: 64 * 1024 * 1024,
    });
    let start = Instant::now();
    while app.plugins_state() != bevy::app::PluginsState::Ready {
        bevy::tasks::tick_global_task_pools_on_main_thread();
        assert!(start.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(10));
    }
    app.finish();
    app.cleanup();
    app
}

#[test]
#[ignore = "native headless Bevy GPU playback and lifecycle probe; run explicitly"]
fn live_gpu_selection_caps_all_instances_and_cleans_up_without_replay() {
    let mut app = headless(5);
    let mut source = EffectAsset::new("light-only playback", 3.0);
    let mut emitter = Emitter::basic_sprite("embers", 3.0);
    emitter.max_particles = 128;
    emitter.renderers.clear();
    let mut light = ParticlePointLightProperties::new(1000.0, 12.0);
    light.intensity_curve = Curve::new(vec![CurveKey::new(0.0, 1000.0)]);
    light.max_lights_by_quality.insert("high".into(), 3);
    emitter
        .scene_outputs
        .push(SceneOutputInstance::particle_point_light(light));
    source.emitters.push(emitter);
    let compiled = Arc::new(
        aestra_compiler::EffectCompiler::default()
            .compile(&source)
            .unwrap(),
    );
    let mut owners = Vec::new();
    for x in [100.0, -100.0] {
        let mut player = PresentedEffect::new(compiled.clone())
            .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
        player.instance.set_seed(42);
        player.instance.set_playback_time(0.5);
        owners.push(
            app.world_mut()
                .spawn((player, Transform::from_xyz(x, 0.0, 0.0)))
                .id(),
        );
    }
    settle(&mut app, 5);
    let (frame, words) = snapshot(&app);
    assert_eq!(frame.manifest.len(), 2);
    assert!(frame.selected_capacity <= 5);
    assert!(frame.reserved_bytes > frame.scratch_bytes);
    assert!(frame.reserved_bytes <= 64 * 1024 * 1024);
    let counter_offset = frame.selected_capacity as usize * 12;
    let counts = &words[counter_offset..];
    assert_eq!(counts[2], 5);
    assert_eq!(counts[3], counts[1] - 5);
    let mut selected = std::collections::BTreeMap::<_, u32>::new();
    for record in words[..5 * 12].as_chunks::<12>().0 {
        let source = &frame.manifest[record[10] as usize];
        *selected.entry(source.owner).or_default() += 1;
        let player = app.world().get::<PresentedEffect>(source.owner).unwrap();
        let mut samples = Vec::new();
        player.instance.evaluate(&mut samples);
        let sample = samples
            .iter()
            .find(|p| p.particle_index == record[11])
            .unwrap();
        let transform = app.world().get::<GlobalTransform>(source.owner).unwrap();
        let expected = transform.transform_point(Vec3::from_array(sample.position));
        let actual = Vec3::new(
            f32::from_bits(record[0]),
            f32::from_bits(record[1]),
            f32::from_bits(record[2]),
        );
        assert!(
            actual.abs_diff_eq(expected, 0.002),
            "{actual:?} != {expected:?}"
        );
        assert_eq!(record[7], 1000.0f32.to_bits());
    }
    assert!(selected.values().all(|count| *count <= 3));
    assert!(frame.manifest.windows(2).all(|w| w[0] < w[1]));
    // Forward time refreshes samples but needs no replay/checkpoints.
    for owner in &owners {
        app.world_mut()
            .get_mut::<PresentedEffect>(*owner)
            .unwrap()
            .instance
            .set_playback_time(0.75);
    }
    settle(&mut app, 5);
    let (next, next_words) = snapshot(&app);
    assert!(next.sequence > frame.sequence);
    assert_eq!(next_words[next.selected_capacity as usize * 12 + 2], 5);
    app.world_mut().despawn(owners[0]);
    settle(&mut app, 3);
    let (remaining, remaining_words) = snapshot(&app);
    assert_eq!(remaining.manifest.len(), 1);
    assert_eq!(
        remaining_words[remaining.selected_capacity as usize * 12 + 2],
        3
    );
    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_lights = 0;
    for _ in 0..3 {
        app.update();
    }
    assert!(
        app.get_sub_app(RenderApp)
            .unwrap()
            .world()
            .resource::<GpuSelectedParticleLights>()
            .frame()
            .is_none()
    );
    // Re-enable; old global buffers/manifest must not be resurrected.
    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_lights = 2;
    settle(&mut app, 2);
    let (limited, limited_words) = snapshot(&app);
    assert_eq!(
        limited_words[limited.selected_capacity as usize * 12 + 2],
        2
    );
    assert_eq!(limited.manifest.len(), 1);
    // Invalid live gradient values must drop the output, not retain its last
    // valid uploaded plan. Inject an unresolved live slot into this test plan.
    let mut invalid = (*compiled).clone();
    let aestra_runtime::SceneOutputPlanKind::ParticlePointLight(light) =
        &mut invalid.emitters[0].scene_outputs[0].kind;
    light.color =
        aestra_runtime::ParticleLightColorPlan::GradientParameter(aestra_runtime::ParameterSlot(0));
    let mut player = PresentedEffect::new(Arc::new(invalid))
        .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    player.instance.set_playback_time(0.75);
    app.world_mut().entity_mut(owners[1]).insert(player);
    for _ in 0..3 {
        app.update();
    }
    assert!(
        app.get_sub_app(RenderApp)
            .unwrap()
            .world()
            .resource::<GpuSelectedParticleLights>()
            .frame()
            .is_none()
    );
    let mut player =
        PresentedEffect::new(compiled).with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    player.instance.set_seed(43);
    player.instance.set_playback_time(0.75);
    app.world_mut().entity_mut(owners[1]).insert(player);
    settle(&mut app, 2);
    assert_eq!(snapshot(&app).0.manifest[0].seed, 43);
    // Resource pressure fails visibly without uncapped local fallback.
    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_scratch_bytes = 1;
    for _ in 0..3 {
        app.update();
    }
    let render = app.get_sub_app(RenderApp).unwrap();
    let state = render.world().resource::<GpuSelectedParticleLights>();
    assert!(state.frame().is_none());
    assert!(state.rejection.is_some());
    eprintln!(
        "F7D2 playback-only: requested={} candidates={} selected={} dropped={} sources={} scratch_bytes={}; global cap, world transform, forward refresh, despawn, disable/re-enable and resource rejection passed",
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        frame.manifest.len(),
        frame.scratch_bytes
    );
}

#[test]
#[ignore = "native authored stateful overlapping-shell light selection; run explicitly"]
fn authored_event_spawned_stars_select_lights_from_forward_gpu_presentation() {
    use aestra_bevy_render::gpu::EffectOutputContext;
    use aestra_core::EffectClipId;
    let mut app = headless(48);
    let mut asset = EffectAsset::from_ron(include_str!(
        "../../../assets/test/effects/fireworks_peony.aestra.ron"
    ))
    .unwrap();
    // Isolate stateful particle presentation: material/trail draw work and the
    // full F6 show's lighting/visual budget are separate acceptance probes.
    asset.material_instances.clear();
    asset
        .emitters
        .retain(|emitter| emitter.name == "Main stars" || emitter.name == "Launch shell");
    asset.events.retain(|link| {
        asset
            .emitters
            .iter()
            .any(|emitter| emitter.id == link.target)
    });
    for emitter in &mut asset.emitters {
        emitter.renderers.clear();
        let stars = emitter.name == "Main stars";
        if stars || emitter.name == "Launch shell" {
            let mut light = ParticlePointLightProperties::new(1000.0, 12.0);
            light.intensity_curve = Curve::new(vec![CurveKey::new(0.0, 1000.0)]);
            light.priority = u32::from(stars);
            light
                .max_lights_by_quality
                .insert("high".into(), if stars { 32 } else { 1 });
            emitter
                .scene_outputs
                .push(SceneOutputInstance::particle_point_light(light));
        }
    }
    let compiled = Arc::new(
        aestra_compiler::EffectCompiler::default()
            .compile(&asset)
            .unwrap(),
    );
    let root = app.world_mut().spawn_empty().id();
    let mut owners = Vec::new();
    for x in [100.0, -100.0] {
        let mut player = PresentedEffect::new(compiled.clone())
            .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
        player.instance.set_seed(42);
        player.instance.set_playback_time(0.1);
        owners.push(
            app.world_mut()
                .spawn((
                    player,
                    Transform::from_xyz(x, 0.0, 0.0),
                    EffectOutputContext {
                        root,
                        clip_path: vec![EffectClipId::new()],
                        playback_epoch: 7,
                    },
                ))
                .id(),
        );
    }
    // Wait for actual launch presentation, not merely loaded selection shaders.
    settle(&mut app, 2);
    for tick in 7..=150 {
        for owner in &owners {
            app.world_mut()
                .get_mut::<PresentedEffect>(*owner)
                .unwrap()
                .instance
                .set_playback_time(tick as f32 / 60.0);
        }
        app.update();
    }
    settle(&mut app, 48);
    let (frame, words) = snapshot(&app);
    assert_eq!(frame.manifest.len(), 4);
    let counts = &words[frame.selected_capacity as usize * 12..];
    assert_eq!(counts, &[512, 512, 48, 464]);
    let mut by_owner = std::collections::BTreeMap::<_, u32>::new();
    for record in words[..48 * 12].as_chunks::<12>().0 {
        let source = &frame.manifest[record[10] as usize];
        assert_eq!(source.root, root);
        assert_eq!(source.root_epoch, 7);
        assert_eq!(source.clip_path.len(), 1);
        assert_eq!(
            record[9], 1,
            "only event-spawned stars are live at this time"
        );
        assert!(record[11] < 256);
        *by_owner.entry(source.owner).or_default() += 1;
    }
    assert!(by_owner.values().all(|count| *count <= 32));
    eprintln!(
        "F7D2 overlapping authored peony: requested={} candidates={} selected={} dropped={} sources={} scratch_bytes={}; event-born stateful stars, quality/global caps and nested occurrence identities passed (no material/trail rendering)",
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        frame.manifest.len(),
        frame.scratch_bytes
    );
    // Changes to an occurrence epoch must replace, never reuse the old manifest.
    for owner in owners {
        app.world_mut()
            .get_mut::<EffectOutputContext>(owner)
            .unwrap()
            .playback_epoch = 8;
    }
    settle(&mut app, 48);
    assert!(
        snapshot(&app)
            .0
            .manifest
            .iter()
            .all(|source| source.root_epoch == 8)
    );
}
