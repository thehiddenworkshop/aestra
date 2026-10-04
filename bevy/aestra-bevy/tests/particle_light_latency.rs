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
    let t = players.get(motion.owner).unwrap().instance().time();
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

fn headless() -> App {
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

fn directory() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/fireworks-f7/particle-light-latency")
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
    let mut app = headless();
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
        "speed_m_s,mode,tick,interval_ms,update_ms,active,allocated,sequence,frame_lag,accepted_age_ms,pending,staging_bytes,failed,expired\n",
    );
    std::fs::create_dir_all(directory()).unwrap();
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
        for mode in ["control", "async"] {
            app.world_mut()
                .resource_mut::<AestraParticleLightSettings>()
                .max_lights = u32::from(mode == "async");
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
            let start_time = if mode == "control" { 0.25 } else { 1.25 };
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
                telemetry.push_str(&format!(
                    "{speed},{mode},{tick},{interval_ms},{update_ms},{},{},{},{},{},{},{},{},{}\n",
                    stats.active,
                    stats.allocated,
                    stats.last_sequence,
                    stats.frame_lag,
                    stats.update_age_seconds * 1000.0,
                    stats.readback.pending,
                    stats.readback.staging_bytes,
                    stats.readback.failed,
                    stats.expired
                ));
            }
            // Drain screenshot callbacks without assigning callback-time positions
            // to older images. All registration uses simultaneous image pixels.
            for _ in 0..12 {
                paced_update(&mut app, &mut deadline);
            }
            let captures = std::mem::take(&mut app.world_mut().resource_mut::<Captures>().0);
            assert_eq!(captures.len(), 10, "missing final-image samples");
            for (tick, image) in captures {
                let (star, receiver) =
                    registration(&image).expect("missing HDR star or receiver response");
                let offset = star - receiver;
                let frames = offset / (f64::from(WIDTH) / f64::from(width) * f64::from(speed) / HZ);
                report.push_str(&format!(
                    "{speed},{mode},{tick},{star},{receiver},{offset},{frames}\n"
                ));
                image
                    .try_into_dynamic()
                    .unwrap()
                    .save(directory().join(format!("{speed}-{mode}-{tick}.png")))
                    .unwrap();
            }
            if let Some(entity) = control {
                app.world_mut().despawn(entity);
            }
        }
        app.world_mut().despawn(owner);
    }
    // Raw test evidence is generated output, not source edits.
    std::fs::write(directory().join("registration.csv"), report).unwrap();
    std::fs::write(directory().join("telemetry.csv"), telemetry).unwrap();
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
    std::fs::write(directory().join("metadata.csv"), metadata).unwrap();
    println!("F7E2 measurements: {}", directory().display());
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
