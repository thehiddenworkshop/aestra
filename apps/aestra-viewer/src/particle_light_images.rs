//! F7E4B2A native image gate. Paused forward ticks are NOT a latency benchmark.
//! Only the adapter cap changes within each off/on/off triplet. No selected
//! positions cross to the CPU; screenshot readback is explicit test tooling.
use crate::*;
use aestra_bevy::{
    AestraParticleLightPlugin, AestraTransientLightPlugin, ParticleLightGpuSettings,
    ParticleLightGpuStatistics, ParticleLightMode, ParticleLightStatistics, TransientLightSettings,
    gpu::particle_lights::AestraParticleLightSettings,
};
use bevy::{app::PluginsState, camera::RenderTarget, post_process::bloom::Bloom};
use serde::{Deserialize, Serialize};

mod registration;

const CAP: u32 = 96;
const WALL_Z: f32 = -2.0;
const WALL_CENTER_Y: f32 = 35.0;
const WALL_HALF_SIZE: Vec2 = Vec2::new(80.0, 50.0);

#[derive(Resource, Default)]
struct Captured(Vec<Image>);

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct Difference {
    receiver_pixels: u64,
    positive_pixels: u64,
    positive_energy: u64,
    maximum_channel_delta: i16,
    restored_changed_pixels: u64,
    restored_significant_pixels: u64,
    restored_maximum_channel_delta: u8,
}

/// A conservative interior of the projected diffuse receiver. Matched additive
/// particles are identical in the controls; >3/channel rejects rounding noise.
fn difference(off: &RgbaImage, on: &RgbaImage, restored: &RgbaImage, roi: [u32; 4]) -> Difference {
    assert_eq!(off.dimensions(), on.dimensions());
    assert_eq!(off.dimensions(), restored.dimensions());
    let [left, top, right, bottom] = roi;
    assert!(left < right && top < bottom && right <= off.width() && bottom <= off.height());
    let mut result = Difference {
        receiver_pixels: u64::from(right - left) * u64::from(bottom - top),
        positive_pixels: 0,
        positive_energy: 0,
        maximum_channel_delta: 0,
        restored_changed_pixels: off
            .pixels()
            .zip(restored.pixels())
            .filter(|(a, b)| a != b)
            .count() as u64,
        restored_significant_pixels: 0,
        restored_maximum_channel_delta: 0,
    };
    for (a, b) in off.pixels().zip(restored.pixels()) {
        let delta = (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap();
        result.restored_maximum_channel_delta = result.restored_maximum_channel_delta.max(delta);
        result.restored_significant_pixels += u64::from(delta > 3);
    }
    for y in top..bottom {
        for x in left..right {
            let before = off.get_pixel(x, y);
            let after = on.get_pixel(x, y);
            let delta = (0..3)
                .map(|c| i16::from(after[c]) - i16::from(before[c]))
                .max()
                .unwrap();
            result.maximum_channel_delta = result.maximum_channel_delta.max(delta);
            if delta > 3 {
                result.positive_pixels += 1;
                result.positive_energy += delta as u64;
            }
        }
    }
    result
}

#[derive(Serialize)]
struct Sample {
    workload: &'static str,
    requested_frame: u64,
    simulation_frame: u64,
    bloom_intensity: f32,
    roi: [u32; 4],
    difference: Difference,
    adapter_dispatches: u64,
    adapter_sequence: u64,
    adapter_written_capacity_bound: u32,
    live_particles_observation: Option<u32>,
    occupied_trails_observation: Option<u32>,
    active_source_instances: usize,
    images: [String; 3],
}

fn cases() -> [(&'static str, &'static str, &'static [u64]); 3] {
    [
        ("f4-reference-hero", "hero", &[0, 85, 110, 150, 300, 600]),
        (
            "f5-secondary-volley",
            "volley",
            &[0, 85, 110, 145, 175, 240, 600],
        ),
        ("f6-show", "show", &[0, 180, 600, 840, 1110, 1560, 1920]),
    ]
}

fn directory() -> PathBuf {
    let path = std::env::var_os("AESTRA_GPU_LIGHT_IMAGE_REPORTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/fireworks-f7/gpu-authored-images"));
    if path.is_absolute() {
        path
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
    }
}

fn config(probe: &str) -> ViewerConfig {
    ViewerConfig::from_iter(
        [
            "--fireworks-f0",
            "--fireworks-f0-probe",
            probe,
            "--camera",
            "audience",
            "--backend",
            "gpu",
            "--history",
            "playback-only",
            "--tier",
            "high",
            "--hdr",
            "--exposure",
            "0",
            "--bloom",
            "0.15",
            "--particle-light-bench",
            "--particle-light-realization",
            "--particle-light-mode",
            "gpu",
            // Configuration validation only. No timing/counter bench systems run in this test.
            "--gpu-bench",
            "unused-image-gate.json",
            "--headless-bench",
        ]
        .map(String::from),
    )
    .unwrap()
}

fn pump(app: &mut App) {
    app.update();
    // Give pipelined rendering/callbacks room without synchronously polling the GPU.
    std::thread::sleep(Duration::from_millis(1));
}

fn settle(app: &mut App) {
    let started = Instant::now();
    let mut ready_frames = 0;
    while ready_frames < 16 {
        pump(app);
        let readiness = app.world().resource::<CaptureRenderReadiness>();
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "pipeline timeout: {}",
            readiness.detail
        );
        ready_frames = if readiness.ready { ready_frames + 1 } else { 0 };
    }
}

fn capture(app: &mut App, target: &Handle<Image>) -> RgbaImage {
    settle(app);
    assert!(app.world().resource::<Captured>().0.is_empty());
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |event: On<ScreenshotCaptured>, mut captured: ResMut<Captured>| {
                captured.0.push(event.image.clone());
            },
        );
    let started = Instant::now();
    while app.world().resource::<Captured>().0.is_empty() {
        pump(app);
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "screenshot timeout"
        );
    }
    let mut images = std::mem::take(&mut app.world_mut().resource_mut::<Captured>().0);
    assert_eq!(images.len(), 1);
    images.remove(0).try_into_dynamic().unwrap().to_rgba8()
}

fn receivers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Test-only, nonemissive diffuse wall. Close enough to the explicit fixture's
    // 12 m lights; no brighter curves/ranges or replacement particle materials.
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(
            WALL_HALF_SIZE.x * 2.0,
            WALL_HALF_SIZE.y * 2.0,
            1.0,
        ))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.6, 0.6),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            ..default()
        })),
        Transform::from_xyz(0.0, WALL_CENTER_Y, WALL_Z),
    ));
}

fn headless() -> App {
    let config = config("f4-reference-hero");
    let prepared = prepare_viewer(&config).unwrap_or_else(|e| panic!("{}", e.message));
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: prepared.asset_root.to_string_lossy().into_owned(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::log::LogPlugin>()
            .disable::<bevy::winit::WinitPlugin>(),
    )
    .add_plugins((
        AestraPlugin,
        AestraParticleLightPlugin,
        AestraTransientLightPlugin,
    ))
    .insert_resource(AestraSettings {
        presentation: PresentationMode::Gpu,
        ..default()
    })
    .insert_resource(ParticleLightMode::SameFrameGpu)
    .insert_resource(AestraParticleLightSettings {
        max_lights: CAP,
        ..default()
    })
    .insert_resource(ParticleLightGpuSettings {
        max_lights: 0,
        ..default()
    })
    .insert_resource(TransientLightSettings {
        max_lights: 8,
        ..default()
    })
    .insert_resource(aestra_bevy::gpu::AestraCatchupPacing { paced: false })
    .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::from_secs_f64(1.0 / 60.0),
    ))
    .insert_resource(prepared)
    .insert_resource(config)
    .init_resource::<Captured>()
    .init_resource::<CaptureRenderReadiness>()
    .add_systems(
        Startup,
        (setup, receivers, particle_light_bench::prime_clusters),
    );
    app.sub_app_mut(RenderApp)
        .add_systems(ExtractSchedule, publish_capture_render_readiness);
    let started = Instant::now();
    while app.plugins_state() != PluginsState::Ready {
        bevy::tasks::tick_global_task_pools_on_main_thread();
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "plugin startup timeout"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    app.finish();
    app.cleanup();
    pump(&mut app);
    settle(&mut app);
    app
}

fn roi(app: &mut App, camera: Entity) -> [u32; 4] {
    let camera_component = app.world().get::<Camera>(camera).unwrap();
    let transform = app.world().get::<GlobalTransform>(camera).unwrap();
    let points = [-1.0, 1.0]
        .into_iter()
        .flat_map(|x| [-1.0, 1.0].map(move |y| (x, y)))
        .map(|(x, y)| {
            camera_component
                .world_to_viewport(
                    transform,
                    Vec3::new(
                        x * WALL_HALF_SIZE.x,
                        WALL_CENTER_Y + y * WALL_HALF_SIZE.y,
                        WALL_Z + 0.5,
                    ),
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    // This camera/wall has no roll or horizontal parallax. Use the conservative
    // inscribed rectangle, not a bounding box that includes pixels off the wall.
    let left = points
        .iter()
        .filter(|p| p.x < VIEW_WIDTH as f32 * 0.5)
        .map(|p| p.x)
        .fold(0.0_f32, f32::max);
    let right = points
        .iter()
        .filter(|p| p.x > VIEW_WIDTH as f32 * 0.5)
        .map(|p| p.x)
        .fold(VIEW_WIDTH as f32, f32::min);
    let top = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    let bottom = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
    [
        (left + 10.0).clamp(0.0, VIEW_WIDTH as f32) as u32,
        (top + 10.0).clamp(0.0, VIEW_HEIGHT as f32) as u32,
        (right - 10.0).clamp(0.0, VIEW_WIDTH as f32) as u32,
        (bottom - 10.0).clamp(0.0, VIEW_HEIGHT as f32) as u32,
    ]
}

fn assert_transport(app: &App) {
    let pool = app.world().resource::<ParticleLightStatistics>();
    assert_eq!((pool.active, pool.allocated, pool.copied_bytes), (0, 0, 0));
    assert_eq!(
        (
            pool.readback.submitted,
            pool.readback.completed,
            pool.readback.staging_bytes,
            pool.readback.pending
        ),
        (0, 0, 0, 0)
    );
    assert!(pool.readback.rejection.is_none());
    let stats = app
        .world()
        .resource::<ParticleLightGpuStatistics>()
        .snapshot();
    assert!(stats.rejection.is_none(), "{stats:?}");
    assert_eq!(stats.invalid_sources, 0);
    assert!(stats.reserved_slots <= CAP && stats.written_capacity <= CAP);
}

#[test]
fn image_gate_rejects_rounding_noise_and_detects_stale_controls() {
    let off = RgbaImage::from_pixel(10, 10, Rgba([10, 10, 10, 255]));
    let noise = RgbaImage::from_pixel(10, 10, Rgba([13, 10, 10, 255]));
    assert_eq!(
        difference(&off, &noise, &off, [1, 1, 9, 9]).positive_pixels,
        0
    );
    let lit = RgbaImage::from_pixel(10, 10, Rgba([20, 10, 10, 255]));
    let result = difference(&off, &lit, &lit, [1, 1, 9, 9]);
    assert_eq!(
        (
            result.receiver_pixels,
            result.positive_pixels,
            result.positive_energy,
            result.restored_changed_pixels
        ),
        (64, 64, 640, 100)
    );
    assert_eq!(result.restored_significant_pixels, 100);
    assert_eq!(result.restored_maximum_channel_delta, 10);
}

#[test]
#[ignore = "native authored perspective/HDR receiver triplets; run explicitly, alone"]
fn authored_gpu_lights_illuminate_receivers_and_restore_controls() {
    let directory = directory();
    fs::create_dir_all(&directory).unwrap();
    // Invalidate earlier acceptance before doing any native work. A failed
    // rerun must not leave a previous report that a validator could accept.
    fs::write(directory.join("report.json"), br#"{"schema_version":1,"milestone":"F7E4B2A","accepted":false,"status":"running_or_failed"}"#).unwrap();
    let mut app = headless();
    assert!(app.is_plugin_added::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>());
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<Camera3d>>()
        .single(app.world())
        .unwrap();
    let RenderTarget::Image(target) = app.world().get::<RenderTarget>(camera).unwrap() else {
        panic!("offscreen target required")
    };
    let target = target.handle.clone();
    let mut samples = Vec::new();
    for (probe, workload, frames) in cases() {
        let config = config(probe);
        let prepared = prepare_viewer(&config).unwrap_or_else(|e| panic!("{}", e.message));
        let old = app
            .world_mut()
            .query::<(Entity, &EffectPlayer)>()
            .iter(app.world())
            .map(|(e, _)| e)
            .collect::<Vec<_>>();
        for entity in old {
            app.world_mut().despawn(entity);
        }
        settle(&mut app);
        let mut player = EffectPlayer::from_project(Arc::clone(&prepared.project))
            .with_history_policy(config.history_policy);
        player.set_seed(config.resolved_seed());
        player.playing = false;
        let presented = PresentedEffect::new(player.effect().clone());
        let owner = app.world_mut().spawn((player, presented)).id();
        let transform = if workload == "show" {
            fireworks_show::camera(config.fireworks_camera)
        } else {
            fireworks_hero::camera(config.fireworks_camera)
        };
        app.world_mut()
            .entity_mut(camera)
            .insert((transform, bevy::camera::ShadowLodOrigin));
        settle(&mut app);
        let receiver_roi = roi(&mut app, camera);
        let mut previous = 0;
        let mut responsive = [false; 2];
        for &frame in frames {
            for tick in previous + 1..=frame {
                app.world_mut()
                    .get_mut::<EffectPlayer>(owner)
                    .unwrap()
                    .set_playback_time(tick as f32 / 60.0);
                pump(&mut app);
            }
            previous = frame;
            settle(&mut app);
            assert_eq!(
                app.world()
                    .get::<EffectRuntimeStatus>(owner)
                    .unwrap()
                    .active,
                ActiveBackend::Gpu
            );
            let simulation_frame = app.world().get::<EffectPlayer>(owner).unwrap().frame();
            // Root-only profiles miss authored show children. Skip the empty
            // composition root; propagate unavailable observations as null.
            let profiles = &app
                .world()
                .get::<aestra_bevy::ProjectProfiler>(owner)
                .unwrap()
                .0
                .instances;
            let sources = profiles
                .iter()
                .filter(|p| !p.profile.emitters.is_empty())
                .collect::<Vec<_>>();
            let source_count = sources.len();
            let live = sources.iter().try_fold(0_u32, |sum, p| {
                p.profile.alive_particles.value().map(|v| sum + v)
            });
            let trails = sources.iter().try_fold(0_u32, |sum, p| {
                p.profile.occupied_trails.value().map(|v| sum + v)
            });
            if workload == "hero" && frame == 85 {
                assert!(live.is_some_and(|v| v >= 600));
            }
            if workload == "volley" && frame == 175 {
                assert!(live.is_some_and(|v| v >= 1000));
            }
            if workload == "show" && frame == 1110 {
                assert!(source_count >= 2, "overlap workload is missing");
            }
            for (bloom_index, bloom) in [0.0, 0.15].into_iter().enumerate() {
                if bloom == 0.0 {
                    app.world_mut().entity_mut(camera).remove::<Bloom>();
                } else {
                    app.world_mut().entity_mut(camera).insert(Bloom {
                        intensity: bloom,
                        ..Bloom::NATURAL
                    });
                }
                let mut images = Vec::new();
                let mut names = Vec::new();
                let mut dispatched = None;
                for (label, cap) in [("off", 0), ("on", CAP), ("restored", 0)] {
                    app.world_mut()
                        .resource_mut::<ParticleLightGpuSettings>()
                        .max_lights = cap;
                    let image = capture(&mut app, &target);
                    assert_transport(&app);
                    let stats = app
                        .world()
                        .resource::<ParticleLightGpuStatistics>()
                        .snapshot();
                    if cap == 0 {
                        assert_eq!(
                            (
                                stats.reserved_slots,
                                stats.written_capacity,
                                stats.buffer_bytes
                            ),
                            (0, 0, 0)
                        );
                    } else {
                        dispatched = Some(stats);
                    }
                    let name = format!("{workload}-{frame:04}-bloom{bloom_index}-{label}.png");
                    image.save(directory.join(&name)).unwrap();
                    names.push(name);
                    images.push(image);
                }
                let delta = difference(&images[0], &images[1], &images[2], receiver_roi);
                println!(
                    "{workload} frame {frame} bloom {bloom}: {delta:?}; live={live:?}; adapter={dispatched:?}"
                );
                assert_eq!(
                    delta.restored_significant_pixels, 0,
                    "{workload} frame {frame}: off/on/off controls differ: {delta:?}"
                );
                if frame == 0 || frame == *frames.last().unwrap() {
                    assert_eq!(
                        images[0], images[1],
                        "{workload} must match before birth/after cleanup"
                    );
                } else if delta.positive_pixels >= 100 && delta.positive_energy >= 1000 {
                    responsive[bloom_index] = true;
                }
                let stats = dispatched.unwrap();
                samples.push(Sample {
                    workload,
                    requested_frame: frame,
                    simulation_frame,
                    bloom_intensity: bloom,
                    roi: receiver_roi,
                    difference: delta,
                    adapter_dispatches: stats.dispatches,
                    adapter_sequence: stats.sequence,
                    adapter_written_capacity_bound: stats.written_capacity,
                    live_particles_observation: live,
                    occupied_trails_observation: trails,
                    active_source_instances: source_count,
                    images: names.try_into().unwrap(),
                });
            }
        }
        assert_eq!(
            responsive,
            [true, true],
            "{workload} has no diffuse receiver response with/without bloom"
        );
    }
    let capabilities = app.world().resource::<GpuCapabilities>();
    let report = serde_json::json!({
        "schema_version": 1, "milestone": "F7E4B2A", "accepted": true,
        "scope": "Paused forward playback image gate, not paced registration, art/finale acceptance or a cost benchmark. Same-frame default-layer native adapter; only its cap changes per off/on/off triplet. Fixed nonemissive diffuse wall is host validation geometry; normal authored materials/events/trails/transforms and representative pulses remain intact. Profile values are asynchronous observations, capacity is an upper bound, not a light count.",
        "adapter": capabilities.adapter_name, "backend": capabilities.backend,
        "tier": "high", "seed": fireworks_f0::SEED, "history": "playback_only", "mode": "same_frame_gpu",
        "target": [VIEW_WIDTH, VIEW_HEIGHT], "camera": "audience_perspective",
        "hdr": true, "exposure_stops": 0, "tonemapping": "tony", "pipelined": true,
        "global_cap": CAP, "adapter_on_cap": CAP, "adapter_off_cap": 0,
        "selected_record_readback_submissions": 0, "portable_proxy_allocations": 0,
        "cluster_initial_capacities": particle_light_bench::cluster_capacities("high"),
        "receiver_wall": {"center": [0.0, WALL_CENTER_Y, WALL_Z], "size": [160.0, 100.0, 1.0], "base_color_srgb": [0.6, 0.6, 0.6], "metallic": 0, "roughness": 1},
        "samples": samples,
    });
    fs::write(
        directory.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("authored image gate: {}", directory.display());
}

#[test]
#[ignore = "read-only revalidation of the documented native authored image artifacts"]
fn retained_authored_receiver_images_pass_the_gate() {
    let directory = directory();
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("report.json")).unwrap()).unwrap();
    for (key, expected) in [
        ("schema_version", serde_json::json!(1)),
        ("milestone", serde_json::json!("F7E4B2A")),
        ("accepted", serde_json::json!(true)),
        ("history", serde_json::json!("playback_only")),
        ("target", serde_json::json!([VIEW_WIDTH, VIEW_HEIGHT])),
        ("hdr", serde_json::json!(true)),
        ("pipelined", serde_json::json!(true)),
        ("mode", serde_json::json!("same_frame_gpu")),
        ("tier", serde_json::json!("high")),
        ("seed", serde_json::json!(fireworks_f0::SEED)),
        ("global_cap", serde_json::json!(CAP)),
        ("adapter_on_cap", serde_json::json!(CAP)),
        ("adapter_off_cap", serde_json::json!(0)),
        ("selected_record_readback_submissions", serde_json::json!(0)),
        ("portable_proxy_allocations", serde_json::json!(0)),
    ] {
        assert_eq!(report[key], expected, "metadata: {key}");
    }
    let samples = report["samples"].as_array().unwrap();
    assert_eq!(
        samples.len(),
        cases()
            .iter()
            .map(|(_, _, frames)| frames.len() * 2)
            .sum::<usize>()
    );
    let mut index = 0;
    for (_, workload, frames) in cases() {
        let mut responsive = [false; 2];
        for &frame in frames {
            for (bloom_index, bloom) in [0.0, 0.15].into_iter().enumerate() {
                let sample = &samples[index];
                index += 1;
                assert_eq!(sample["workload"], workload);
                assert_eq!(sample["requested_frame"], frame);
                assert_eq!(sample["bloom_intensity"].as_f64().unwrap() as f32, bloom);
                assert!(sample["simulation_frame"].as_u64().unwrap() <= frame);
                assert!(sample["adapter_dispatches"].as_u64().unwrap() > 0);
                assert!(sample["adapter_sequence"].as_u64().unwrap() > 0);
                assert!(
                    sample["adapter_written_capacity_bound"].as_u64().unwrap() <= u64::from(CAP)
                );
                let images = ["off", "on", "restored"]
                    .into_iter()
                    .enumerate()
                    .map(|(i, label)| {
                        let expected =
                            format!("{workload}-{frame:04}-bloom{bloom_index}-{label}.png");
                        assert_eq!(sample["images"][i], expected);
                        // Only fixed basenames, never report-provided arbitrary paths.
                        let image = image::open(directory.join(expected)).unwrap().to_rgba8();
                        assert_eq!(image.dimensions(), (VIEW_WIDTH, VIEW_HEIGHT));
                        image
                    })
                    .collect::<Vec<_>>();
                let roi = serde_json::from_value(sample["roi"].clone()).unwrap();
                let actual = difference(&images[0], &images[1], &images[2], roi);
                let recorded: Difference =
                    serde_json::from_value(sample["difference"].clone()).unwrap();
                assert_eq!(
                    actual, recorded,
                    "altered pixels/report at {workload}/{frame}/{bloom}"
                );
                assert_eq!(
                    actual.restored_significant_pixels, 0,
                    "stale light at {workload}/{frame}/{bloom}"
                );
                if frame == 0 || frame == *frames.last().unwrap() {
                    assert_eq!(images[0], images[1], "before/after control mismatch");
                } else if actual.positive_pixels >= 100 && actual.positive_energy >= 1000 {
                    responsive[bloom_index] = true;
                }
            }
        }
        assert_eq!(
            responsive,
            [true, true],
            "no receiver response for {workload}"
        );
    }
}
