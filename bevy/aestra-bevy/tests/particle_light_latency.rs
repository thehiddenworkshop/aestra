//! F7E2: paced, pipelined, final-image light/star registration measurement.
//! Screenshots are test instrumentation, not part of production light transport.
use aestra_bevy::{Curve, Gradient, *};
use bevy::{
    camera::{ScalingMode, ShadowLodOrigin},
    prelude::*,
    render::{
        render_resource::TextureFormat,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[path = "support/particle_light_gpu_proof.rs"]
mod gpu_proof;

const HZ: f64 = 60.0;
const WIDTH: u32 = 768;
const HEIGHT: u32 = 384;

#[derive(Resource, Default)]
struct Captures(Vec<(String, Image)>);

#[derive(Component)]
struct ControlLight;

// A test-only zero-readback reference, synchronized to the SAME presented
// playback time as the GPU sprite. Never used by the production adapter.
#[derive(Resource)]
struct Motion {
    owner: Entity,
    speed: f32,
}

fn control_position(
    motion: Res<Motion>,
    players: Query<&EffectPlayer>,
    mut lights: Query<&mut Transform, With<ControlLight>>,
) {
    let Ok(player) = players.get(motion.owner) else {
        return;
    };
    let t = player.instance().time();
    for mut transform in &mut lights {
        transform.translation = Vec3::new(motion.speed * (t - 0.75), 1.0, 0.0);
    }
}

fn source(speed: f32, width: f32) -> Arc<CompiledEffect> {
    let mut asset = EffectAsset::new("paced light registration", 10.0);
    let mut emitter = Emitter::basic_sprite("fast star", 10.0);
    emitter.max_particles = 1;
    emitter.modules = vec![
        ModuleInstance::emission(0.0, 1),
        ModuleInstance::shape(EmitterShape::Point),
        ModuleInstance::initialize(
            ScalarRange::new(10.0, 10.0),
            ScalarRange::new(speed, speed),
            [1.0, 0.0, 0.0],
            0.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0; 3], 0.0, 0.0),
        ModuleInstance::appearance(
            Curve::new(vec![CurveKey::new(0.0, width * 0.01)]),
            Curve::new(vec![CurveKey::new(0.0, 1.0)]),
            Gradient::new(vec![ColorKey::new(0.0, [8.0, 0.0, 0.0, 1.0])]),
        ),
    ];
    let mut light = ParticlePointLightProperties::new(40_000.0, 8.0);
    light.intensity_curve = Curve::new(vec![CurveKey::new(0.0, 40_000.0)]);
    light.color_source = ParticleLightColorSource::Constant([0.0, 1.0, 0.0]);
    emitter
        .scene_outputs
        .push(SceneOutputInstance::particle_point_light(light));
    asset.emitters.push(emitter);
    Arc::new(EffectCompiler::default().compile(&asset).unwrap())
}

fn headless(include_gpu: bool, production: bool) -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::log::LogPlugin>()
            .disable::<bevy::winit::WinitPlugin>(),
    )
    // Deliberately keep PipelinedRenderingPlugin, unlike the paused F7E1 probe.
    .add_plugins((AestraPlugin, AestraParticleLightPlugin))
    .insert_resource(AestraParticleLightSettings {
        max_lights: 1,
        ..default()
    })
    .insert_resource(ParticleLightReadbackSettings {
        max_lights: 1,
        ..default()
    })
    .init_resource::<Captures>()
    .add_systems(
        PostUpdate,
        control_position.before(bevy::transform::TransformSystems::Propagate),
    );
    if include_gpu && !production {
        app.add_plugins(gpu_proof::ProofPlugin);
    }
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

fn request(app: &mut App, target: &Handle<Image>, label: String) {
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            move |event: On<ScreenshotCaptured>, mut captures: ResMut<Captures>| {
                captures.0.push((label.clone(), event.image.clone()));
            },
        );
}

/// Independent red HDR-star and green diffuse receiver centroids in the SAME
/// final tonemapped image. Orthographic top-down framing removes X parallax.
fn registration(image: &Image) -> Option<(f64, f64)> {
    let image = image.clone().try_into_dynamic().unwrap().to_rgb8();
    registration_pixels(image.enumerate_pixels().map(|(x, _, pixel)| (x, pixel.0)))
}

fn registration_pixels(pixels: impl IntoIterator<Item = (u32, [u8; 3])>) -> Option<(f64, f64)> {
    let mut sums = [0.0; 2];
    let mut weighted = [0.0; 2];
    for (x, pixel) in pixels {
        let red = pixel[0].saturating_sub(pixel[1].max(pixel[2]));
        let green = pixel[1].saturating_sub(pixel[0].max(pixel[2]));
        for (i, value) in [red, green].into_iter().enumerate() {
            // Ignore dithering/compression-scale noise, never accept a black target.
            if value > 3 {
                sums[i] += f64::from(value);
                weighted[i] += f64::from(value) * f64::from(x);
            }
        }
    }
    (sums[0] > 200.0 && sums[1] > 200.0).then(|| (weighted[0] / sums[0], weighted[1] / sums[1]))
}

fn green_energy(image: &Image) -> u64 {
    image
        .clone()
        .try_into_dynamic()
        .unwrap()
        .to_rgb8()
        .pixels()
        .map(|p| p[1].saturating_sub(p[0].max(p[2])))
        .filter(|v| *v > 3)
        .map(u64::from)
        .sum()
}

fn unlit_capture(
    app: &mut App,
    target: &Handle<Image>,
    deadline: &mut Instant,
    directory: &std::path::Path,
    name: &str,
) -> u64 {
    for _ in 0..12 {
        paced_update(app, deadline);
    }
    request(app, target, name.into());
    for _ in 0..12 {
        paced_update(app, deadline);
    }
    let captures = std::mem::take(&mut app.world_mut().resource_mut::<Captures>().0);
    assert_eq!(captures.len(), 1, "missing lifecycle image");
    let (_, image) = captures.into_iter().next().unwrap();
    let energy = green_energy(&image);
    assert!(energy <= 20, "stale GPU light survived {name}: {energy}");
    image
        .try_into_dynamic()
        .unwrap()
        .save(directory.join(format!("{name}.png")))
        .unwrap();
    energy
}

fn directory(include_gpu: bool, production: bool) -> std::path::PathBuf {
    // Separate regression evidence from earlier retained timing/capture hashes.
    if production && let Some(path) = std::env::var_os("AESTRA_GPU_LIGHT_ADAPTER_REPORTS") {
        let path = std::path::PathBuf::from(path);
        return if path.is_absolute() {
            path
        } else {
            // Cargo runs integration tests from the crate directory. Match the
            // viewer's repository-relative report paths, not that private cwd.
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(path)
        };
    }
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(if production {
        "../../target/fireworks-f7/particle-light-gpu-adapter"
    } else if include_gpu {
        "../../target/fireworks-f7/particle-light-gpu-proof"
    } else {
        "../../target/fireworks-f7/particle-light-latency"
    })
}

// Deadline pacing is only in this explicit test. It does not poll/wait for GPU
// results; overruns are recorded rather than hidden by catch-up simulation.
fn paced_update(app: &mut App, deadline: &mut Instant) -> f64 {
    *deadline += Duration::from_secs_f64(1.0 / HZ);
    if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
        std::thread::sleep(wait);
    }
    let started = Instant::now();
    app.update();
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    if Instant::now() > *deadline + Duration::from_secs_f64(1.0 / HZ) {
        *deadline = Instant::now();
    }
    elapsed
}

#[test]
#[ignore = "native paced GPU/HDR receiver latency measurement; run explicitly, alone"]
fn paced_fast_stars_measure_final_image_light_registration() {
    measure(false, false);
}

#[test]
#[ignore = "native paced same-frame selected GPU light/PBR proof; run explicitly, alone"]
fn same_frame_gpu_lights_register_with_fast_stars_on_standard_material() {
    measure(true, false);
}

#[test]
#[ignore = "native paced bounded GPU adapter/PBR registration; run explicitly, alone"]
fn bounded_gpu_adapter_registers_without_selected_light_readback() {
    measure(true, true);
}

fn observation(app: &App, production: bool) -> gpu_proof::Observation {
    if production {
        let stats = app
            .world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot();
        gpu_proof::Observation {
            dispatches: stats.dispatches,
            sequence: stats.sequence,
            rejection: stats
                .rejection
                .filter(|r| *r != ParticleLightGpuRejection::PipelineLoading)
                .map(|r| format!("{r:?}")),
        }
    } else {
        app.world()
            .resource::<gpu_proof::ProofStatistics>()
            .0
            .lock()
            .unwrap()
            .clone()
    }
}

fn measure(include_gpu: bool, production: bool) {
    let mut app = headless(include_gpu, production);
    assert!(app.is_plugin_added::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>());
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            WIDTH,
            HEIGHT,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            ShadowLodOrigin,
            AmbientLight {
                brightness: 0.0,
                ..default()
            },
            Camera {
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            bevy::camera::RenderTarget::Image(target.clone().into()),
            Transform::from_xyz(0.0, 100.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z),
        ))
        .id();
    preview::PhotographicPreview {
        bloom_intensity: 0.0,
        tonemapping: preview::DisplayTransform::Reinhard,
        ..default()
    }
    .apply(&mut app.world_mut().commands().entity(camera));
    app.world_mut().flush();
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Plane3d::default().mesh().size(600.0, 600.0));
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.7, 0.7, 0.7),
            perceptual_roughness: 1.0,
            ..default()
        });
    app.world_mut()
        .spawn((Mesh3d(mesh), MeshMaterial3d(material)));
    let mut report = String::from(
        "speed_m_s,mode,request_tick,star_x_px,receiver_x_px,signed_offset_px,equivalent_frames\n",
    );
    let mut telemetry = String::from(
        "speed_m_s,mode,tick,interval_ms,update_ms,active,allocated,sequence,frame_lag,accepted_age_ms,pending,staging_bytes,failed,expired,proof_dispatches,proof_sequence,reserved_slots,readback_submitted\n",
    );
    let mut lifecycle = String::from("speed_m_s,check,green_energy\n");
    let directory = directory(include_gpu, production);
    std::fs::create_dir_all(&directory).unwrap();
    for speed in [25.0_f32, 75.0, 150.0] {
        let width = speed * 1.7;
        app.world_mut()
            .entity_mut(camera)
            .insert(Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::Fixed {
                    width,
                    height: width / 2.0,
                },
                ..OrthographicProjection::default_3d()
            }));
        let mut player = EffectPlayer::from_compiled(source(speed, width))
            .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
        player.playing = false;
        player.instance_mut().set_playback_time(0.25);
        let owner = app
            .world_mut()
            .spawn((player, Transform::from_xyz(-speed * 0.75, 1.0, 0.0)))
            .id();
        app.insert_resource(Motion { owner, speed });
        let mut control_bias = 0.0;
        let modes: &[&str] = if include_gpu {
            &["control", "async", "gpu"]
        } else {
            &["control", "async"]
        };
        for &mode in modes {
            app.world_mut()
                .resource_mut::<AestraParticleLightSettings>()
                .max_lights = u32::from(mode != "control");
            app.world_mut()
                .resource_mut::<ParticleLightReadbackSettings>()
                .max_lights = u32::from(mode == "async" || production);
            if production {
                *app.world_mut().resource_mut::<ParticleLightMode>() = if mode == "gpu" {
                    ParticleLightMode::SameFrameGpu
                } else {
                    ParticleLightMode::PortableAsync
                };
            }
            if include_gpu && !production {
                *app.world_mut().resource_mut::<gpu_proof::ProofSettings>() =
                    gpu_proof::ProofSettings {
                        enabled: mode == "gpu",
                        owner: Some(owner),
                    };
            }
            let control = (mode == "control").then(|| {
                app.world_mut()
                    .spawn((
                        ControlLight,
                        PointLight {
                            color: Color::linear_rgb(0.0, 1.0, 0.0),
                            intensity: 40_000.0,
                            range: 8.0,
                            shadow_maps_enabled: false,
                            contact_shadows_enabled: false,
                            ..default()
                        },
                    ))
                    .id()
            });
            // Forward-only: each phase gets its own time interval; no seek/replay.
            let gpu_slot = (mode == "gpu" && !production).then(|| {
                app.world_mut()
                    .spawn((
                        gpu_proof::ProofSlot,
                        PointLight {
                            intensity: 0.0,
                            range: 200.0,
                            shadow_maps_enabled: false,
                            contact_shadows_enabled: false,
                            ..default()
                        },
                        Transform::from_xyz(0.0, 1.0, 0.0),
                    ))
                    .id()
            });
            let start_time = match mode {
                "control" => 0.25,
                "async" => 1.25,
                _ => 2.25,
            };
            // Shift the root for the second continuous interval, then settle paused.
            app.world_mut()
                .get_mut::<Transform>(owner)
                .unwrap()
                .translation
                .x = -speed * (start_time + 0.5);
            app.world_mut()
                .get_mut::<EffectPlayer>(owner)
                .unwrap()
                .instance_mut()
                .set_playback_time(start_time);
            let mut deadline = Instant::now();
            let warmup_start = Instant::now();
            loop {
                for _ in 0..30 {
                    paced_update(&mut app, &mut deadline);
                }
                request(&mut app, &target, "warmup".into());
                for _ in 0..12 {
                    paced_update(&mut app, &mut deadline);
                }
                if mode == "gpu" {
                    let stats = observation(&app, production);
                    assert!(stats.rejection.is_none(), "{stats:?}");
                }
                let ready = app
                    .world_mut()
                    .resource_mut::<Captures>()
                    .0
                    .drain(..)
                    .any(|(_, image)| registration(&image).is_some());
                if ready {
                    break;
                }
                assert!(
                    warmup_start.elapsed() < Duration::from_secs(30),
                    "HDR sprite/receiver never became visible, speed={speed} mode={mode}"
                );
            }
            let mut last = Instant::now();
            let proof_start = if include_gpu {
                observation(&app, production).dispatches
            } else {
                0
            };
            for tick in 1..=60 {
                app.world_mut()
                    .get_mut::<EffectPlayer>(owner)
                    .unwrap()
                    .instance_mut()
                    .set_playback_time(start_time + tick as f32 / HZ as f32);
                if (12..=48).contains(&tick) && tick % 4 == 0 {
                    request(&mut app, &target, tick.to_string());
                }
                let update_ms = paced_update(&mut app, &mut deadline);
                let now = Instant::now();
                let interval_ms = now.duration_since(last).as_secs_f64() * 1000.0;
                last = now;
                let stats = app.world().resource::<ParticleLightStatistics>();
                assert_eq!(
                    app.world()
                        .get::<EffectRuntimeStatus>(owner)
                        .unwrap()
                        .active,
                    ActiveBackend::Gpu
                );
                assert!(
                    stats.allocated <= 1
                        && stats.readback.pending <= 3
                        && stats.readback.staging_bytes <= 192
                );
                assert_eq!(stats.readback.failed, 0);
                let mut proof_dispatches = 0;
                let mut proof_sequence = 0;
                if mode == "gpu" {
                    assert_eq!(stats.active, 0);
                    assert_eq!(stats.allocated, 0);
                    assert_eq!(stats.readback.pending, 0);
                    assert_eq!(stats.readback.staging_bytes, 0);
                    let proof = observation(&app, production);
                    assert!(proof.rejection.is_none(), "{proof:?}");
                    if tick >= 3 {
                        assert!(proof.dispatches > proof_start, "{proof:?}");
                    }
                    proof_dispatches = proof.dispatches;
                    proof_sequence = proof.sequence;
                }
                telemetry.push_str(&format!(
                    "{speed},{mode},{tick},{interval_ms},{update_ms},{},{},{},{},{},{},{},{},{},{proof_dispatches},{proof_sequence},{},{}\n",
                    stats.active,
                    stats.allocated,
                    stats.last_sequence,
                    stats.frame_lag,
                    stats.update_age_seconds * 1000.0,
                    stats.readback.pending,
                    stats.readback.staging_bytes,
                    stats.readback.failed,
                    stats.expired,
                    if production { app.world().resource::<ParticleLightGpuStatistics>().snapshot().reserved_slots as usize } else { usize::from(gpu_slot.is_some()) },
                    stats.readback.submitted,
                ));
            }
            // Drain screenshot callbacks without assigning callback-time positions
            // to older images. All registration uses simultaneous image pixels.
            for _ in 0..12 {
                paced_update(&mut app, &mut deadline);
            }
            let captures = std::mem::take(&mut app.world_mut().resource_mut::<Captures>().0);
            assert_eq!(captures.len(), 10, "missing final-image samples");
            let mut offsets = Vec::new();
            for (tick, image) in captures {
                let (star, receiver) =
                    registration(&image).expect("missing HDR star or receiver response");
                let offset = star - receiver;
                offsets.push(offset);
                let frames = offset / (f64::from(WIDTH) / f64::from(width) * f64::from(speed) / HZ);
                report.push_str(&format!(
                    "{speed},{mode},{tick},{star},{receiver},{offset},{frames}\n"
                ));
                image
                    .try_into_dynamic()
                    .unwrap()
                    .save(directory.join(format!("{speed}-{mode}-{tick}.png")))
                    .unwrap();
            }
            if mode == "control" {
                control_bias = offsets.iter().sum::<f64>() / offsets.len() as f64;
                assert!(offsets.iter().all(|v| v.abs() <= 1.0));
            } else if mode == "gpu" {
                for offset in offsets {
                    let metres =
                        (offset - control_bias).abs() / (f64::from(WIDTH) / f64::from(width));
                    assert!(
                        metres <= (f64::from(speed) / HZ).min(2.0),
                        "same-frame registration gate failed: {metres}m at {speed}m/s"
                    );
                }
                // Keep the slot entity: every fail-closed path must clear the
                // GPU override, not rely on despawning a lit object to hide it.
                app.world_mut()
                    .resource_mut::<AestraParticleLightSettings>()
                    .max_lights = 0;
                let off = unlit_capture(
                    &mut app,
                    &target,
                    &mut deadline,
                    &directory,
                    &format!("{speed}-disabled"),
                );
                lifecycle.push_str(&format!("{speed},global-disable,{off}\n"));
                app.world_mut()
                    .resource_mut::<AestraParticleLightSettings>()
                    .max_lights = 1;
                for _ in 0..18 {
                    paced_update(&mut app, &mut deadline);
                }
                request(&mut app, &target, "re-enabled".into());
                for _ in 0..12 {
                    paced_update(&mut app, &mut deadline);
                }
                let captures = std::mem::take(&mut app.world_mut().resource_mut::<Captures>().0);
                assert_eq!(captures.len(), 1);
                assert!(
                    registration(&captures[0].1).is_some(),
                    "GPU light failed to re-enable"
                );
                app.world_mut().despawn(owner);
                let removed = unlit_capture(
                    &mut app,
                    &target,
                    &mut deadline,
                    &directory,
                    &format!("{speed}-removed"),
                );
                lifecycle.push_str(&format!("{speed},owner-removal,{removed}\n"));
            }
            if let Some(entity) = control {
                app.world_mut().despawn(entity);
            }
            if let Some(entity) = gpu_slot {
                app.world_mut().despawn(entity);
            }
        }
        if app.world().entities().contains(owner) {
            app.world_mut().despawn(owner);
        }
    }
    if production {
        adapter_contract(&mut app, &target, &directory);
    }
    // Raw test evidence is generated output, not source edits.
    std::fs::write(directory.join("registration.csv"), report).unwrap();
    std::fs::write(directory.join("telemetry.csv"), telemetry).unwrap();
    if include_gpu {
        std::fs::write(directory.join("lifecycle.csv"), lifecycle).unwrap();
    }
    let capabilities = app.world().resource::<GpuCapabilities>();
    let mut metadata = String::from("key,value\n");
    for (key, value) in [
        ("adapter", capabilities.adapter_name.as_str()),
        ("backend", capabilities.backend.as_str()),
        ("driver", capabilities.driver.as_str()),
        ("device_type", capabilities.device_type.as_str()),
        ("pipeline", "pipelined"),
        ("history", "playback-only"),
        ("cadence_hz", "60"),
        ("target", "768x384"),
        (
            "response",
            "HDR/Reinhard/EV0/no bloom/no ambient/no shadows",
        ),
        ("readback_cap", "1"),
        ("slots", "3"),
        ("light_range", "8"),
    ] {
        metadata.push_str(&format!("{key},\"{}\"\n", value.replace('"', "\"\"")));
    }
    if production {
        metadata.push_str("gpu_adapter,\"bounded reserved slots, native Bevy GPU clustering, StandardMaterial\"\n");
    } else if include_gpu {
        metadata.push_str("gpu_proof,\"one reserved zero-lumen slot, native Bevy GPU clustering, StandardMaterial\"\n");
    }
    std::fs::write(directory.join("metadata.csv"), metadata).unwrap();
    println!("F7E measurements: {}", directory.display());
}

fn channel_energy(image: &Image, channel: usize, half: Option<bool>) -> u64 {
    image
        .clone()
        .try_into_dynamic()
        .unwrap()
        .to_rgb8()
        .enumerate_pixels()
        .filter(|(x, _, _)| half.is_none_or(|right| (*x >= WIDTH / 2) == right))
        .map(|(_, _, p)| p[channel].saturating_sub(p[(channel + 1) % 3].max(p[(channel + 2) % 3])))
        .filter(|v| *v > 3)
        .map(u64::from)
        .sum()
}
fn adapter_capture(
    app: &mut App,
    target: &Handle<Image>,
    deadline: &mut Instant,
    directory: &std::path::Path,
    name: &str,
) -> Image {
    for _ in 0..24 {
        paced_update(app, deadline);
    }
    request(app, target, name.into());
    for _ in 0..12 {
        paced_update(app, deadline);
    }
    let captures = std::mem::take(&mut app.world_mut().resource_mut::<Captures>().0);
    assert_eq!(captures.len(), 1);
    let (_, image) = captures.into_iter().next().unwrap();
    image
        .clone()
        .try_into_dynamic()
        .unwrap()
        .save(directory.join(format!("{name}.png")))
        .unwrap();
    image
}
fn adapter_contract(app: &mut App, target: &Handle<Image>, directory: &std::path::Path) {
    *app.world_mut().resource_mut::<ParticleLightMode>() = ParticleLightMode::SameFrameGpu;
    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_lights = 4;
    app.world_mut()
        .resource_mut::<ParticleLightGpuSettings>()
        .max_lights = 3;
    // Two separate roots/artifacts, two selected records, three reserved slots.
    let owners = [-50.0, 50.0].map(|x| {
        let mut player = EffectPlayer::from_compiled(source(0.0, 255.0))
            .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
        player.playing = false;
        player.instance_mut().set_playback_time(0.25);
        app.world_mut()
            .spawn((player, Transform::from_xyz(x, 1.0, 0.0)))
            .id()
    });
    let host = app
        .world_mut()
        .spawn((
            PointLight {
                color: Color::linear_rgb(0.0, 0.0, 1.0),
                intensity: 40_000.0,
                range: 8.0,
                shadow_maps_enabled: false,
                contact_shadows_enabled: false,
                ..default()
            },
            Transform::from_xyz(100.0, 1.0, 0.0),
        ))
        .id();
    let mut deadline = Instant::now();
    let image = adapter_capture(app, target, &mut deadline, directory, "multiple-roots");
    assert!(
        channel_energy(&image, 1, Some(false)) > 200 && channel_energy(&image, 1, Some(true)) > 200
    );
    assert!(
        channel_energy(&image, 2, None) > 200,
        "host light was overwritten"
    );
    let stats = app
        .world()
        .resource::<ParticleLightGpuStatistics>()
        .snapshot();
    assert!(stats.rejection.is_none(), "{stats:?}");
    assert_eq!(stats.reserved_slots, 3);
    assert!(
        stats.buffer_bytes
            <= app
                .world()
                .resource::<ParticleLightGpuSettings>()
                .max_buffer_bytes
    );
    assert_eq!(stats.invalid_sources, 0);
    let before = app
        .world_mut()
        .query_filtered::<Entity, With<ParticleLightGpuSlot>>()
        .iter(app.world())
        .collect::<Vec<_>>();
    for _ in 0..12 {
        paced_update(app, &mut deadline);
    }
    let after = app
        .world_mut()
        .query_filtered::<Entity, With<ParticleLightGpuSlot>>()
        .iter(app.world())
        .collect::<Vec<_>>();
    assert_eq!(before, after, "reserved pool churned on stable playback");
    app.world_mut().despawn(owners[0]);
    let image = adapter_capture(app, target, &mut deadline, directory, "one-root-removed");
    assert_eq!(
        channel_energy(&image, 1, Some(false)),
        0,
        "removed source retained its light"
    );
    assert!(channel_energy(&image, 1, Some(true)) > 200 && channel_energy(&image, 2, None) > 200);
    let parent = app
        .world_mut()
        .spawn((Transform::IDENTITY, Visibility::Hidden))
        .id();
    app.world_mut()
        .entity_mut(owners[1])
        .insert(ChildOf(parent));
    let image = adapter_capture(app, target, &mut deadline, directory, "hierarchy-hidden");
    assert_eq!(
        channel_energy(&image, 1, None),
        0,
        "hidden hierarchy retained a light"
    );
    assert!(channel_energy(&image, 2, None) > 200);
    app.world_mut().entity_mut(owners[1]).remove::<ChildOf>();
    app.world_mut().despawn(parent);
    // Explicitly reject unsupported layers, no automatic async fallback.
    app.world_mut()
        .entity_mut(owners[1])
        .insert(bevy::camera::visibility::RenderLayers::layer(7));
    let image = adapter_capture(app, target, &mut deadline, directory, "layer-rejected");
    assert_eq!(channel_energy(&image, 1, None), 0);
    assert_eq!(
        app.world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot()
            .rejection,
        Some(ParticleLightGpuRejection::NonDefaultLayers)
    );
    app.world_mut()
        .entity_mut(owners[1])
        .remove::<bevy::camera::visibility::RenderLayers>();
    app.world_mut()
        .resource_mut::<ParticleLightGpuSettings>()
        .max_manifest_bytes = 0;
    let image = adapter_capture(app, target, &mut deadline, directory, "manifest-rejected");
    assert_eq!(channel_energy(&image, 1, None), 0);
    assert_eq!(
        app.world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot()
            .rejection,
        Some(ParticleLightGpuRejection::ManifestBudget)
    );
    app.world_mut()
        .resource_mut::<ParticleLightGpuSettings>()
        .max_manifest_bytes = 1024 * 1024;
    app.world_mut()
        .resource_mut::<ParticleLightGpuSettings>()
        .max_buffer_bytes = 0;
    let image = adapter_capture(app, target, &mut deadline, directory, "budget-rejected");
    assert_eq!(channel_energy(&image, 1, None), 0);
    assert_eq!(
        app.world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot()
            .rejection,
        Some(ParticleLightGpuRejection::BufferBudget)
    );
    assert_eq!(
        app.world_mut()
            .query::<&ParticleLightGpuSlot>()
            .iter(app.world())
            .count(),
        0
    );
    app.world_mut()
        .resource_mut::<ParticleLightGpuSettings>()
        .max_buffer_bytes = 1024 * 1024;
    app.world_mut()
        .resource_mut::<ParticleLightGpuSettings>()
        .max_lights = 1;
    let image = adapter_capture(app, target, &mut deadline, directory, "budget-recovered");
    assert!(channel_energy(&image, 1, Some(true)) > 200);
    assert_eq!(
        app.world_mut()
            .query::<&ParticleLightGpuSlot>()
            .iter(app.world())
            .count(),
        1
    );
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<Camera3d>>()
        .single(app.world())
        .unwrap();
    let receiver = app
        .world_mut()
        .query_filtered::<Entity, With<Mesh3d>>()
        .single(app.world())
        .unwrap();
    for entity in [camera, receiver, owners[1], host] {
        app.world_mut()
            .get_mut::<Transform>(entity)
            .unwrap()
            .translation
            .x += 10_000.0;
    }
    let image = adapter_capture(app, target, &mut deadline, directory, "far-world-origin");
    assert!(
        channel_energy(&image, 1, Some(true)) > 200,
        "CPU placeholder culled a visible GPU light far from the origin"
    );
    assert!(channel_energy(&image, 2, None) > 200);
    assert!(
        app.world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot()
            .rejection
            .is_none()
    );
    app.world_mut().despawn(owners[1]);
    let image = adapter_capture(app, target, &mut deadline, directory, "all-roots-removed");
    assert_eq!(channel_energy(&image, 1, None), 0);
    assert!(channel_energy(&image, 2, None) > 200);
    let readback = &app.world().resource::<ParticleLightStatistics>().readback;
    assert_eq!((readback.pending, readback.staging_bytes), (0, 0));
    assert_eq!(
        app.world().get::<PointLight>(host).unwrap().intensity,
        40_000.0
    );
    app.world_mut().despawn(host);
}

#[test]
fn registration_rejects_empty_or_single_channel_images_and_separates_centroids() {
    assert_eq!(registration_pixels([(0, [0; 3]), (1, [2; 3])]), None);
    assert_eq!(registration_pixels([(0, [255, 0, 0])]), None);
    assert_eq!(registration_pixels([(0, [0, 255, 0])]), None);
    assert_eq!(
        registration_pixels([(2, [255, 0, 0]), (7, [0, 255, 0]), (12, [255; 3])]),
        Some((2.0, 7.0))
    );
}
