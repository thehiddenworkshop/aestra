//! F7E4B2B: an explicitly synthetic tracer under unchanged authored show load.
//! Final-image simultaneous centroids, not callback-time GPU positions/latency.
use super::*;
use aestra_bevy::{
    ColorKey, Curve, CurveKey, Emitter, EmitterShape, Gradient, ModuleInstance,
    ParticleLightColorSource, ParticlePointLightProperties, PlaybackHistoryPolicy, ScalarRange,
    SceneOutputInstance,
};
use bevy::transform::TransformSystems;

const HZ: f64 = 60.0;
const Y: f32 = 38.0;
const Z: f32 = 92.0;
const FIRST_LOAD_FRAME: u64 = 1020;
const SAMPLE_TICKS: [u64; 10] = [12, 16, 20, 24, 28, 32, 36, 40, 44, 48];

#[derive(Component)]
struct AnalyticLight;
#[derive(Resource)]
struct Motion {
    owner: Entity,
    speed: f32,
    delay_seconds: f32,
}
#[derive(Resource, Default)]
struct Images(Vec<(u64, Image)>);

fn position(
    motion: Res<Motion>,
    players: Query<&EffectPlayer>,
    mut lamps: Query<&mut Transform, With<AnalyticLight>>,
) {
    let Ok(player) = players.get(motion.owner) else {
        return;
    };
    for mut lamp in &mut lamps {
        lamp.translation = Vec3::new(
            motion.speed * (player.simulation_time() - 0.75 - motion.delay_seconds),
            Y,
            Z,
        );
    }
}

fn tracer(speed: f32, output: bool) -> Arc<aestra_bevy::CompiledEffect> {
    let mut effect = EffectAsset::new(
        "registration calibration tracer (not authored fireworks)",
        10.0,
    );
    let mut emitter = Emitter::basic_sprite("red calibration tracer", 10.0);
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
            Curve::new(vec![CurveKey::new(0.0, 0.4)]),
            Curve::new(vec![CurveKey::new(0.0, 1.0)]),
            Gradient::new(vec![ColorKey::new(0.0, [8.0, 0.0, 0.0, 1.0])]),
        ),
    ];
    if output {
        let mut light = ParticlePointLightProperties::new(40_000.0, 8.0);
        light.color_source = ParticleLightColorSource::Constant([0.0, 1.0, 0.0]);
        light.intensity_curve = Curve::new(vec![CurveKey::new(0.0, 40_000.0)]);
        light.priority = u32::MAX; // One known calibration slot; the remaining cap serves authored sources.
        light.max_lights_by_quality = [("high".into(), 1)].into();
        emitter
            .scene_outputs
            .push(SceneOutputInstance::particle_point_light(light));
    }
    effect.emitters.push(emitter);
    Arc::new(EffectCompiler::default().compile(&effect).unwrap())
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct Centroids {
    star: [f64; 2],
    receiver: [f64; 2],
    red_energy: u64,
    green_energy: u64,
}
fn centroids(image: &RgbaImage, roi: [u32; 4]) -> Option<Centroids> {
    let [left, top, right, bottom] = roi;
    assert!(left < right && top < bottom && right <= image.width() && bottom <= image.height());
    let mut sums = [0_u64; 2];
    let mut weighted = [[0.0; 2]; 2];
    for y in top..bottom {
        for x in left..right {
            let p = image.get_pixel(x, y);
            let weights = [
                p[0].saturating_sub(p[1].max(p[2])),
                p[1].saturating_sub(p[0].max(p[2])),
            ];
            for (i, weight) in weights.into_iter().enumerate() {
                if weight > 3 {
                    sums[i] += u64::from(weight);
                    weighted[i][0] += f64::from(weight) * f64::from(x);
                    weighted[i][1] += f64::from(weight) * f64::from(y);
                }
            }
        }
    }
    (sums.iter().all(|&sum| sum > 200)).then(|| Centroids {
        star: weighted[0].map(|v| v / sums[0] as f64),
        receiver: weighted[1].map(|v| v / sums[1] as f64),
        red_energy: sums[0],
        green_energy: sums[1],
    })
}

fn offset(c: &Centroids) -> [f64; 2] {
    [c.star[0] - c.receiver[0], c.star[1] - c.receiver[1]]
}

/// Bias at the actual visible star position, not screenshot request/callback time.
/// No extrapolation can hide a tracer that left the calibrated screen interval.
fn calibrated_bias(control: &[Centroids], x: f64) -> [f64; 2] {
    assert!(control.len() >= 2 && control.windows(2).all(|w| w[0].star[0] < w[1].star[0]));
    assert!(
        x >= control[0].star[0] - 1.0 && x <= control.last().unwrap().star[0] + 1.0,
        "star outside calibrated interval: {x}"
    );
    let pair = control
        .windows(2)
        .find(|w| x <= w[1].star[0])
        .unwrap_or(&control[control.len() - 2..]);
    let alpha = ((x - pair[0].star[0]) / (pair[1].star[0] - pair[0].star[0])).clamp(0.0, 1.0);
    let a = offset(&pair[0]);
    let b = offset(&pair[1]);
    [a[0] + alpha * (b[0] - a[0]), a[1] + alpha * (b[1] - a[1])]
}

fn residual_metres(c: &Centroids, controls: &[Centroids], pixels_per_metre: f64) -> f64 {
    assert!(pixels_per_metre.is_finite() && pixels_per_metre > 0.0);
    let bias = calibrated_bias(controls, c.star[0]);
    let measured = offset(c);
    (measured[0] - bias[0]).hypot(measured[1] - bias[1]) / pixels_per_metre
}

fn percentile(values: &[f64], percentile: f64) -> f64 {
    assert!(!values.is_empty() && values.iter().all(|v| v.is_finite()));
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[((sorted.len() as f64 * percentile).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1)]
}

fn paced(app: &mut App, deadline: &mut Instant) -> (f64, bool) {
    *deadline += Duration::from_secs_f64(1.0 / HZ);
    if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
        std::thread::sleep(wait);
    }
    let started = Instant::now();
    app.update();
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let missed = Instant::now() > *deadline + Duration::from_secs_f64(1.0 / HZ);
    if missed {
        *deadline = Instant::now();
    }
    (elapsed, missed)
}

fn request(app: &mut App, target: &Handle<Image>, tick: u64) {
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            move |event: On<ScreenshotCaptured>, mut images: ResMut<Images>| {
                images.0.push((tick, event.image.clone()));
            },
        );
}

#[derive(Serialize, Deserialize)]
struct Sample {
    request_tick: u64,
    image: String,
    centroids: Centroids,
    calibrated_offset_metres: Option<f64>,
}
#[derive(Serialize, Deserialize)]
struct Group {
    speed_m_s: f32,
    bloom_intensity: f32,
    mode: String,
    load_start_frame: u64,
    load_end_frame: u64,
    peak_live_particles_observation: u32,
    peak_active_source_instances: usize,
    unavailable_live_observations: u64,
    live_particles_observations: Vec<Option<u32>>,
    active_source_observations: Vec<usize>,
    interval_ms: Vec<f64>,
    update_ms: Vec<f64>,
    deadline_misses: u64,
    adapter_dispatches_start: u64,
    adapter_dispatches_end: u64,
    samples: Vec<Sample>,
    spatial_gate_metres: f64,
    spatial_p95_metres: Option<f64>,
}

fn registration_directory() -> PathBuf {
    let path = std::env::var_os("AESTRA_GPU_LIGHT_REGISTRATION_REPORTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/fireworks-f7/gpu-authored-registration"));
    if path.is_absolute() {
        path
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
    }
}

fn projection(app: &App, camera: Entity) -> ([u32; 4], f64) {
    let c = app.world().get::<Camera>(camera).unwrap();
    let t = app.world().get::<GlobalTransform>(camera).unwrap();
    let project = |p| c.world_to_viewport(t, p).unwrap();
    let top = project(Vec3::new(0.0, Y + 12.0, Z - 1.5)).y;
    let bottom = project(Vec3::new(0.0, Y - 12.0, Z - 1.5)).y;
    let roi = [
        12,
        (top + 10.0).max(0.0) as u32,
        VIEW_WIDTH - 12,
        (bottom - 10.0).min(VIEW_HEIGHT as f32) as u32,
    ];
    let scale = project(Vec3::new(1.0, Y, Z)).x - project(Vec3::new(0.0, Y, Z)).x;
    (roi, f64::from(scale))
}

fn spawn_panel(app: &mut App) {
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::new(220.0, 24.0, 1.0));
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.7, 0.7, 0.7),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            ..default()
        });
    app.world_mut().spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, Y, Z - 2.0),
    ));
}

#[test]
fn registration_classifier_and_calibration_detect_a_delayed_receiver() {
    let mut image = RgbaImage::from_pixel(32, 16, Rgba([10, 10, 10, 255]));
    for y in 5..10 {
        image.put_pixel(20, y, Rgba([255, 0, 0, 255]));
        image.put_pixel(10, y, Rgba([0, 255, 0, 255]));
    }
    let c = centroids(&image, [0, 0, 32, 16]).unwrap();
    assert_eq!(offset(&c), [10.0, 0.0]);
    let controls = [
        Centroids {
            star: [0.0, 7.0],
            receiver: [0.0, 7.0],
            red_energy: 1000,
            green_energy: 1000,
        },
        Centroids {
            star: [30.0, 7.0],
            receiver: [30.0, 7.0],
            red_energy: 1000,
            green_energy: 1000,
        },
    ];
    assert_eq!(residual_metres(&c, &controls, 5.0), 2.0);
    assert!(
        centroids(
            &RgbaImage::from_pixel(32, 16, Rgba([13, 10, 10, 255])),
            [0, 0, 32, 16]
        )
        .is_none()
    );
    assert_eq!(percentile(&[2.0, 3.0, 1.0], 0.95), 3.0);
}

#[test]
#[ignore = "native paced perspective/HDR calibration under authored show overlap; run alone"]
fn paced_gpu_tracer_registers_under_authored_show_overlap() {
    let directory = registration_directory();
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("report.json"),
        br#"{"accepted":false,"status":"running_or_failed"}"#,
    )
    .unwrap();
    let config = config("f6-show");
    let prepared = prepare_viewer(&config).unwrap_or_else(|e| panic!("{}", e.message));
    let project = prepared.project;
    let mut app = headless();
    app.init_resource::<Images>().add_systems(
        PostUpdate,
        position
            .before(TransformSystems::Propagate)
            .run_if(resource_exists::<Motion>),
    );
    app.world_mut()
        .resource_mut::<ParticleLightGpuSettings>()
        .max_lights = CAP;
    // Isolate color-centroid calibration from representative 80 m colored flashes.
    // B2A already retains pulse coexistence; this is not a flash integration gate.
    app.world_mut()
        .resource_mut::<TransientLightSettings>()
        .max_lights = 0;
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<Camera3d>>()
        .single(app.world())
        .unwrap();
    app.world_mut().entity_mut(camera).insert((
        fireworks_show::camera(FireworksCamera::Audience),
        bevy::camera::ShadowLodOrigin,
    ));
    let RenderTarget::Image(target) = app.world().get::<RenderTarget>(camera).unwrap() else {
        panic!("offscreen target required")
    };
    let target = target.handle.clone();
    spawn_panel(&mut app);
    settle(&mut app);
    let (roi, pixels_per_metre) = projection(&app, camera);
    let mut groups = Vec::new();
    let mut failures = Vec::new();
    for speed in [25.0, 75.0, 150.0] {
        for (bloom_index, bloom) in [0.0, 0.15].into_iter().enumerate() {
            if bloom == 0.0 {
                app.world_mut().entity_mut(camera).remove::<Bloom>();
            } else {
                app.world_mut().entity_mut(camera).insert(Bloom {
                    intensity: bloom,
                    ..Bloom::NATURAL
                });
            }
            let old = app
                .world_mut()
                .query::<(Entity, &EffectPlayer)>()
                .iter(app.world())
                .map(|(e, _)| e)
                .collect::<Vec<_>>();
            for e in old {
                app.world_mut().despawn(e);
            }
            settle(&mut app);
            let mut load = EffectPlayer::from_project(project.clone())
                .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
            load.set_seed(fireworks_f0::SEED);
            load.playing = false;
            let shown = PresentedEffect::new(load.effect().clone());
            let owner = app.world_mut().spawn((load, shown)).id();
            for frame in 1..=FIRST_LOAD_FRAME {
                app.world_mut()
                    .get_mut::<EffectPlayer>(owner)
                    .unwrap()
                    .set_playback_time(frame as f32 / HZ as f32);
                pump(&mut app);
            }
            settle(&mut app);
            let mut load_frame = FIRST_LOAD_FRAME;
            let mut controls = Vec::new();
            for mode in ["control", "gpu", "delayed-control"] {
                let mut probe = EffectPlayer::from_compiled(tracer(speed, mode == "gpu"))
                    .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
                probe.playing = false;
                probe.set_playback_time(0.25);
                let presented = PresentedEffect::new(probe.effect().clone());
                let marker = app
                    .world_mut()
                    .spawn((probe, presented, Transform::from_xyz(-speed * 0.75, Y, Z)))
                    .id();
                app.insert_resource(Motion {
                    owner: marker,
                    speed,
                    delay_seconds: if mode == "delayed-control" {
                        3.0 / HZ as f32
                    } else {
                        0.0
                    },
                });
                let lamp = (mode != "gpu").then(|| {
                    app.world_mut()
                        .spawn((
                            AnalyticLight,
                            PointLight {
                                color: Color::linear_rgb(0.0, 1.0, 0.0),
                                intensity: 40_000.0,
                                range: 8.0,
                                radius: 0.1,
                                shadow_maps_enabled: false,
                                contact_shadows_enabled: false,
                                ..default()
                            },
                        ))
                        .id()
                });
                settle(&mut app);
                let dispatch_start = app
                    .world()
                    .resource::<ParticleLightGpuStatistics>()
                    .snapshot()
                    .dispatches;
                let start_frame = load_frame;
                let mut intervals = Vec::new();
                let mut updates = Vec::new();
                let mut misses = 0;
                let mut peak_live = 0;
                let mut peak_sources = 0;
                let mut unavailable_live = 0;
                let mut live_observations = Vec::new();
                let mut source_observations = Vec::new();
                let mut deadline = Instant::now();
                let mut previous = deadline;
                for tick in 1..=72 {
                    load_frame += 1;
                    app.world_mut()
                        .get_mut::<EffectPlayer>(owner)
                        .unwrap()
                        .set_playback_time(load_frame as f32 / HZ as f32);
                    app.world_mut()
                        .get_mut::<EffectPlayer>(marker)
                        .unwrap()
                        .set_playback_time(0.25 + tick as f32 / HZ as f32);
                    if SAMPLE_TICKS.contains(&tick) {
                        request(&mut app, &target, tick);
                    }
                    let (update_ms, missed) = paced(&mut app, &mut deadline);
                    let now = Instant::now();
                    if tick <= 60 {
                        intervals.push(now.duration_since(previous).as_secs_f64() * 1000.0);
                        updates.push(update_ms);
                        misses += u64::from(missed);
                    }
                    previous = now;
                    assert_transport(&app);
                    for entity in [owner, marker] {
                        assert_eq!(
                            app.world()
                                .get::<EffectRuntimeStatus>(entity)
                                .unwrap()
                                .active,
                            ActiveBackend::Gpu
                        );
                    }
                    let profiles = &app
                        .world()
                        .get::<aestra_bevy::ProjectProfiler>(owner)
                        .unwrap()
                        .0
                        .instances;
                    let active = profiles
                        .iter()
                        .filter(|p| !p.profile.emitters.is_empty())
                        .collect::<Vec<_>>();
                    let live = active.iter().try_fold(0_u32, |sum, p| {
                        p.profile.alive_particles.value().map(|v| sum + v)
                    });
                    if let Some(live) = live {
                        peak_live = peak_live.max(live);
                    } else {
                        unavailable_live += 1;
                    }
                    peak_sources = peak_sources.max(active.len());
                    live_observations.push(live);
                    source_observations.push(active.len());
                }
                if peak_live < 500 || peak_sources < 2 {
                    failures.push(format!("{speed}/{bloom}/{mode}: missing authored overlap"));
                }
                if mode == "gpu"
                    && (live_observations.iter().any(|v| v.is_none_or(|n| n < 500))
                        || source_observations.iter().any(|&n| n < 2))
                {
                    failures.push(format!(
                        "{speed}/{bloom}/{mode}: authored load not sustained"
                    ));
                }
                if percentile(&intervals, 0.95) > 25.0 {
                    failures.push(format!("{speed}/{bloom}/{mode}: cadence p95 exceeds 25 ms"));
                }
                let stats = app
                    .world()
                    .resource::<ParticleLightGpuStatistics>()
                    .snapshot();
                assert!(stats.dispatches > dispatch_start && stats.sequence > 0);
                let mut images = std::mem::take(&mut app.world_mut().resource_mut::<Images>().0);
                images.sort_by_key(|(tick, _)| *tick);
                assert_eq!(
                    images.iter().map(|(tick, _)| *tick).collect::<Vec<_>>(),
                    SAMPLE_TICKS
                );
                let mut samples = Vec::new();
                let mut residuals = Vec::new();
                let gate = (f64::from(speed) / HZ).min(2.0);
                for (tick, image) in images {
                    let image = image.try_into_dynamic().unwrap().to_rgba8();
                    let centroid =
                        centroids(&image, roi).expect("missing final-image star or lit receiver");
                    let residual = (mode != "control")
                        .then(|| residual_metres(&centroid, &controls, pixels_per_metre));
                    if let Some(value) = residual {
                        residuals.push(value);
                    } else {
                        controls.push(centroid.clone());
                    }
                    let name = format!("{speed:.0}-bloom{bloom_index}-{mode}-{tick:02}.png");
                    image.save(directory.join(&name)).unwrap();
                    samples.push(Sample {
                        request_tick: tick,
                        image: name,
                        centroids: centroid,
                        calibrated_offset_metres: residual,
                    });
                }
                let p95 = (!residuals.is_empty()).then(|| percentile(&residuals, 0.95));
                println!(
                    "speed={speed} bloom={bloom} mode={mode} spatial_p95={p95:?} gate={gate} cadence_p95={} load_peak={peak_live}/{peak_sources}",
                    percentile(&intervals, 0.95)
                );
                if mode == "gpu" && residuals.iter().any(|&m| m > gate) {
                    failures.push(format!("{speed}/{bloom}/{mode}: registration failed"));
                }
                if mode == "delayed-control" && p95.unwrap() <= gate {
                    failures.push(format!("{speed}/{bloom}/{mode}: known delay not detected"));
                }
                groups.push(Group {
                    speed_m_s: speed,
                    bloom_intensity: bloom,
                    mode: mode.into(),
                    load_start_frame: start_frame,
                    load_end_frame: load_frame,
                    peak_live_particles_observation: peak_live,
                    peak_active_source_instances: peak_sources,
                    unavailable_live_observations: unavailable_live,
                    live_particles_observations: live_observations,
                    active_source_observations: source_observations,
                    interval_ms: intervals,
                    update_ms: updates,
                    deadline_misses: misses,
                    adapter_dispatches_start: dispatch_start,
                    adapter_dispatches_end: stats.dispatches,
                    samples,
                    spatial_gate_metres: gate,
                    spatial_p95_metres: p95,
                });
                if let Some(lamp) = lamp {
                    app.world_mut().despawn(lamp);
                }
                app.world_mut().despawn(marker);
                app.world_mut().remove_resource::<Motion>();
                settle(&mut app);
            }
        }
    }
    let capabilities = app.world().resource::<GpuCapabilities>();
    let report = serde_json::json!({ "schema_version": 1, "milestone": "F7E4B2B", "accepted": failures.is_empty(), "failures": failures,
        "scope": "Paced calibration tracer under unchanged high-tier authored show overlap. Simultaneous final-image red/green centroids; position-dependent analytic control bias. Not natural authored-star art acceptance, measured display latency, work-matched cost or native cluster memory certification. Fixed foreground receiver occludes background in the ROI; representative flashes disabled to isolate calibration colors. No selected-position readback. Warmup unpaced; measured ticks deadline-paced 60 Hz, forward playback-only.",
        "adapter": capabilities.adapter_name, "backend": capabilities.backend, "mode": "same_frame_gpu", "tier": "high", "history": "playback_only",
        "pipelined": true, "target": [VIEW_WIDTH, VIEW_HEIGHT], "camera": "show_audience_perspective", "hdr": true, "tonemapping": "tony", "exposure_stops": 0,
        "global_cap": CAP, "calibration_output_cap": 1, "calibration_priority": u32::MAX, "delayed_control_ticks": 3,
        "selected_record_readback_submissions": 0, "portable_proxy_allocations": 0, "roi": roi, "pixels_per_metre": pixels_per_metre,
        "receiver": { "center": [0.0, Y, Z-2.0], "size": [220.0, 24.0, 1.0], "metallic": 0, "roughness": 1, "base_color_srgb": [0.7, 0.7, 0.7] }, "groups": groups });
    fs::write(
        directory.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("paced authored-load calibration: {}", directory.display());
    assert!(
        failures.is_empty(),
        "registration/cadence gate failed: {failures:?}"
    );
}

/// Recompute from saved final images, never trust an accepted flag or serialized centroids.
#[test]
#[ignore = "read-only validation of retained native registration report/PNGs"]
fn retained_paced_registration_images_pass_the_gate() {
    let directory = registration_directory();
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["milestone"], "F7E4B2B");
    assert_eq!(report["accepted"], true);
    assert_eq!(report["failures"], serde_json::json!([]));
    assert_eq!(report["mode"], "same_frame_gpu");
    assert_eq!(report["tier"], "high");
    assert_eq!(report["history"], "playback_only");
    assert_eq!(report["pipelined"], true);
    assert_eq!(
        report["target"],
        serde_json::json!([VIEW_WIDTH, VIEW_HEIGHT])
    );
    assert_eq!(report["camera"], "show_audience_perspective");
    assert_eq!(report["hdr"], true);
    assert_eq!(report["tonemapping"], "tony");
    assert_eq!(report["exposure_stops"], 0);
    assert_eq!(report["global_cap"], CAP);
    assert_eq!(report["calibration_output_cap"], 1);
    assert_eq!(report["calibration_priority"], u32::MAX);
    assert_eq!(report["delayed_control_ticks"], 3);
    assert_eq!(report["selected_record_readback_submissions"], 0);
    assert_eq!(report["portable_proxy_allocations"], 0);
    let roi: [u32; 4] = serde_json::from_value(report["roi"].clone()).unwrap();
    let scale = report["pixels_per_metre"].as_f64().unwrap();
    assert!(scale.is_finite() && scale > 0.0);
    let groups: Vec<Group> = serde_json::from_value(report["groups"].clone()).unwrap();
    assert_eq!(groups.len(), 18);
    let close = |a: f64, b: f64| {
        assert!(a.is_finite() && b.is_finite() && (a - b).abs() <= 1e-10);
    };
    let mut index = 0;
    for speed in [25.0, 75.0, 150.0] {
        for (bloom_index, bloom) in [0.0, 0.15].into_iter().enumerate() {
            let mut controls = Vec::new();
            let mut load_frame = FIRST_LOAD_FRAME;
            for mode in ["control", "gpu", "delayed-control"] {
                let group = &groups[index];
                index += 1;
                assert_eq!(group.speed_m_s, speed);
                assert_eq!(group.bloom_intensity, bloom);
                assert_eq!(group.mode, mode);
                assert_eq!(group.load_start_frame, load_frame);
                load_frame += 72;
                assert_eq!(group.load_end_frame, load_frame);
                assert!(group.peak_live_particles_observation >= 500);
                assert!(group.peak_active_source_instances >= 2);
                assert!(group.unavailable_live_observations <= 72);
                assert_eq!(group.live_particles_observations.len(), 72);
                assert_eq!(group.active_source_observations.len(), 72);
                if mode == "gpu" {
                    assert!(
                        group
                            .live_particles_observations
                            .iter()
                            .all(|v| v.is_some_and(|n| n >= 500))
                    );
                    assert!(group.active_source_observations.iter().all(|&n| n >= 2));
                }
                assert_eq!(
                    group
                        .live_particles_observations
                        .iter()
                        .flatten()
                        .copied()
                        .max(),
                    Some(group.peak_live_particles_observation)
                );
                assert_eq!(
                    group.active_source_observations.iter().copied().max(),
                    Some(group.peak_active_source_instances)
                );
                assert_eq!(
                    group
                        .live_particles_observations
                        .iter()
                        .filter(|v| v.is_none())
                        .count() as u64,
                    group.unavailable_live_observations
                );
                assert_eq!(group.interval_ms.len(), 60);
                assert_eq!(group.update_ms.len(), 60);
                assert!(group.interval_ms.iter().all(|&v| v > 0.0));
                assert!(group.update_ms.iter().all(|&v| v.is_finite() && v >= 0.0));
                assert!(group.deadline_misses <= 60);
                assert!(percentile(&group.interval_ms, 0.95) <= 25.0);
                assert!(group.adapter_dispatches_end > group.adapter_dispatches_start);
                assert_eq!(group.samples.len(), SAMPLE_TICKS.len());
                let gate = (f64::from(speed) / HZ).min(2.0);
                close(group.spatial_gate_metres, gate);
                let mut residuals = Vec::new();
                for (sample, tick) in group.samples.iter().zip(SAMPLE_TICKS) {
                    assert_eq!(sample.request_tick, tick);
                    let name = format!("{speed:.0}-bloom{bloom_index}-{mode}-{tick:02}.png");
                    assert_eq!(sample.image, name);
                    let image = image::open(directory.join(&name)).unwrap().to_rgba8();
                    assert_eq!(image.dimensions(), (VIEW_WIDTH, VIEW_HEIGHT));
                    let measured = centroids(&image, roi).unwrap();
                    close(measured.star[0], sample.centroids.star[0]);
                    close(measured.star[1], sample.centroids.star[1]);
                    close(measured.receiver[0], sample.centroids.receiver[0]);
                    close(measured.receiver[1], sample.centroids.receiver[1]);
                    assert_eq!(measured.red_energy, sample.centroids.red_energy);
                    assert_eq!(measured.green_energy, sample.centroids.green_energy);
                    if mode == "control" {
                        assert!(sample.calibrated_offset_metres.is_none());
                        controls.push(measured);
                    } else {
                        let residual = residual_metres(&measured, &controls, scale);
                        close(residual, sample.calibrated_offset_metres.unwrap());
                        if mode == "gpu" {
                            assert!(residual <= gate, "{name}: {residual} > {gate}");
                        }
                        residuals.push(residual);
                    }
                }
                if residuals.is_empty() {
                    assert!(group.spatial_p95_metres.is_none());
                } else {
                    let p95 = percentile(&residuals, 0.95);
                    close(p95, group.spatial_p95_metres.unwrap());
                    if mode == "delayed-control" {
                        assert!(p95 > gate, "negative control did not detect known delay");
                    }
                }
            }
        }
    }
}
