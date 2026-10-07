//! F8.3B: saved authored outputs, not synthetic host PointLights or benchmark injection.
use super::*;
use aestra_bevy::{
    AestraParticleLightPlugin, AestraTransientLightPlugin, LightingQualityPolicy,
    ParticleLightGpuSettings, ParticleLightGpuStatistics, ParticleLightMode,
    ParticleLightStatistics, TransientLightSettings, TransientLightStatistics,
};
use bevy::{diagnostic::DiagnosticsStore, render::diagnostic::RenderDiagnosticsPlugin};

fn project(tier: &str) -> Arc<aestra_bevy::CompiledEffectProject> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let effect = EffectAsset::from_ron(
        &fs::read_to_string(root.join("effects/fireworks_smoke_lighting.aestra.ron")).unwrap(),
    )
    .unwrap();
    let resolved = aestra_project::ProjectAssetIndex::scan(root)
        .resolve_effect_project(&effect)
        .unwrap();
    let mut registry = aestra_bevy::ExtensionRegistry::builtin();
    registry.install(&aestra_fluid::FluidExtension).unwrap();
    Arc::new(
        EffectCompiler::with_extensions(registry)
            .with_tier(aestra_bevy::QualityTier::preset(tier).unwrap())
            .compile_resolved_project(&resolved)
            .unwrap(),
    )
}

fn policy(tier: &str) -> LightingQualityPolicy {
    let mut policy = LightingQualityPolicy::preset(tier).unwrap();
    policy.particle.max_lights = match tier {
        "high" => 8,
        "medium" => 4,
        _ => 2,
    };
    policy.representative.max_lights = 2;
    policy
}

fn headless(tier: &str) -> (App, Entity, Handle<Image>) {
    aestra_fluid::link();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(),
    )
    .add_plugins((
        AestraPlugin,
        AestraParticleLightPlugin,
        AestraTransientLightPlugin,
        RenderDiagnosticsPlugin,
    ))
    .insert_resource(AestraSettings {
        presentation: PresentationMode::Gpu,
        ..default()
    })
    .insert_resource(ParticleLightMode::SameFrameGpu)
    .insert_resource(aestra_bevy::gpu::AestraCatchupPacing { paced: false })
    .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::ZERO,
    ))
    .init_resource::<Captured>()
    .init_resource::<CaptureRenderReadiness>();
    policy(tier).apply(app.world_mut()).unwrap();
    app.sub_app_mut(RenderApp)
        .add_systems(ExtractSchedule, publish_capture_render_readiness);
    let started = Instant::now();
    while app.plugins_state() != PluginsState::Ready {
        bevy::tasks::tick_global_task_pools_on_main_thread();
        assert!(started.elapsed() < Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(10));
    }
    app.finish();
    app.cleanup();
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            480,
            360,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
    app.world_mut().spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        RenderTarget::Image(target.clone().into()),
        bevy::camera::ShadowLodOrigin,
        Transform::from_xyz(0.0, 4.0, 16.0).looking_at(Vec3::new(0.0, 3.0, 0.0), Vec3::Y),
    ));
    let mut player = EffectPlayer::from_project(project(tier))
        .with_history_policy(aestra_bevy::PlaybackHistoryPolicy::PlaybackOnly);
    player.set_seed(0xf83b_0000_0000_0001);
    player.playing = false;
    let owner = app
        .world_mut()
        .spawn((player, Transform::from_scale(Vec3::splat(0.1))))
        .id();
    (app, owner, target)
}

fn advance(app: &mut App, owner: Entity, target: u64) {
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .playing = true;
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::from_secs_f64(1.0 / 60.0),
    ));
    let started = Instant::now();
    while app.world().get::<EffectPlayer>(owner).unwrap().frame() < target {
        pump(app, 1);
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "forward playback timeout"
        );
    }
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .playing = false;
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::ZERO,
    ));
}

fn delta(a: &RgbaImage, b: &RgbaImage) -> (usize, u8) {
    assert_eq!(a.dimensions(), b.dimensions());
    a.pixels()
        .zip(b.pixels())
        .fold((0, 0), |(positive, maximum), (a, b)| {
            let gain = (0..3)
                .map(|c| i16::from(b[c]) - i16::from(a[c]))
                .max()
                .unwrap();
            let difference = (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap();
            (positive + usize::from(gain > 3), maximum.max(difference))
        })
}

/// Frozen matched receiver workload: not a whole-show or simulation benchmark.
/// Diagnostics arrive asynchronously; deduplicate by timestamp and exclude warm-up.
fn costs(app: &mut App) -> serde_json::Value {
    pump(app, 40);
    let mut seen = BTreeMap::new();
    let mut samples = BTreeMap::<String, Vec<f64>>::new();
    for diagnostic in app.world().resource::<DiagnosticsStore>().iter() {
        if let Some(value) = diagnostic.measurement() {
            seen.insert(diagnostic.path().as_str().to_owned(), value.time);
        }
    }
    for _ in 0..200 {
        pump(app, 1);
        for diagnostic in app.world().resource::<DiagnosticsStore>().iter() {
            let path = diagnostic.path().as_str();
            if !path.ends_with("elapsed_gpu")
                || !(path.contains("main_transparent_pass_3d")
                    || path.contains("cluster")
                    || path.contains("particle_light"))
            {
                continue;
            }
            if let Some(value) = diagnostic.measurement()
                && value.value.is_finite()
                && seen.get(path).is_none_or(|time| *time < value.time)
            {
                seen.insert(path.to_owned(), value.time);
                samples
                    .entry(path.to_owned())
                    .or_default()
                    .push(value.value);
            }
        }
    }
    let metrics: BTreeMap<_, _> = samples.into_iter().map(|(path, mut values)| {
        values.sort_by(f64::total_cmp);
        let n = values.len();
        (path, serde_json::json!({"samples":n, "median_ms":values[n / 2], "p95_ms":values[(n - 1) * 95 / 100]}))
    }).collect();
    assert!(
        metrics
            .iter()
            .any(|(path, value)| path.contains("main_transparent_pass_3d")
                && value["samples"].as_u64().unwrap() >= 100),
        "missing GPU receiver timestamps"
    );
    serde_json::to_value(metrics).unwrap()
}

fn assert_native_transport(app: &App) {
    let cpu = app.world().resource::<ParticleLightStatistics>();
    assert_eq!(
        (
            cpu.active,
            cpu.allocated,
            cpu.copied_bytes,
            cpu.readback.submitted,
            cpu.readback.staging_bytes
        ),
        (0, 0, 0, 0, 0)
    );
    let gpu = app
        .world()
        .resource::<ParticleLightGpuStatistics>()
        .snapshot();
    assert!(gpu.rejection.is_none(), "{gpu:?}");
    assert_eq!(gpu.invalid_sources, 0);
    let cap = app
        .world()
        .resource::<ParticleLightGpuSettings>()
        .max_lights;
    assert!(gpu.reserved_slots <= cap && gpu.written_capacity <= cap);
}

#[test]
fn difference_gate_rejects_noise_and_detects_stale_light() {
    let off = RgbaImage::from_pixel(10, 10, Rgba([10, 10, 10, 255]));
    let noise = RgbaImage::from_pixel(10, 10, Rgba([13, 10, 10, 255]));
    assert_eq!(delta(&off, &noise), (0, 3));
    let lit = RgbaImage::from_pixel(10, 10, Rgba([20, 10, 10, 255]));
    assert_eq!(delta(&off, &lit), (100, 10));
}

#[test]
#[ignore = "native F8.3B authored burst + selected star smoke image/cost gate; run alone"]
fn authored_outputs_light_smoke_and_retire_without_position_readback() {
    let directory = std::env::var_os("AESTRA_VOLUME_OUTPUT_REPORTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fireworks-f8/output-smoke")
        });
    for tier in ["high", "medium", "low"] {
        let directory = directory.join(tier);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("report.json"), br#"{"accepted":false}"#).unwrap();
        let (mut app, owner, target) = headless(tier);
        let initial = capture(&mut app, &target);
        let artifact = app
            .world()
            .get::<EffectPlayer>(owner)
            .unwrap()
            .effect()
            .clone();
        let epoch_before_controls = app
            .world()
            .get::<EffectPlayer>(owner)
            .unwrap()
            .instance()
            .history_epoch();
        advance(&mut app, owner, 80);
        let before_birth = capture(&mut app, &target);
        app.world_mut()
            .resource_mut::<ParticleLightGpuSettings>()
            .max_lights = 0;
        let before_birth_off = capture(&mut app, &target);
        let before_birth_delta = delta(&before_birth, &before_birth_off);
        let cap = policy(tier).particle.max_lights;
        app.world_mut()
            .resource_mut::<ParticleLightGpuSettings>()
            .max_lights = cap;
        advance(&mut app, owner, 115);
        let both = capture(&mut app, &target);
        let frame = app.world().get::<EffectPlayer>(owner).unwrap().frame();
        assert_eq!(frame, 115);
        let representative = app.world().resource::<TransientLightStatistics>().clone();
        assert_eq!(
            representative.accepted, 2,
            "real native particle events must reach saved bindings"
        );
        assert_eq!(representative.active, 2);
        assert_native_transport(&app);
        let gpu = app
            .world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot();
        let both_cost = costs(&mut app);
        // Cap-only changes, no effect recompilation, synthetic cue, particle enumeration or new smoke.
        app.world_mut()
            .resource_mut::<ParticleLightGpuSettings>()
            .max_lights = 0;
        let rep = capture(&mut app, &target);
        let rep_cost = costs(&mut app);
        app.world_mut()
            .resource_mut::<TransientLightSettings>()
            .enabled = false;
        let off = capture(&mut app, &target);
        let off_cost = costs(&mut app);
        app.world_mut()
            .resource_mut::<ParticleLightGpuSettings>()
            .max_lights = cap;
        let stars = capture(&mut app, &target);
        let star_cost = costs(&mut app);
        assert_native_transport(&app);
        app.world_mut()
            .resource_mut::<ParticleLightGpuSettings>()
            .max_lights = 0;
        let restored = capture(&mut app, &target);
        let rep_delta = delta(&off, &rep);
        let star_delta = delta(&off, &stars);
        let both_delta = delta(&off, &both);
        let restored_delta = delta(&off, &restored);
        // Re-enable a one-slot host budget to qualify live budget changes on this artifact.
        app.world_mut()
            .resource_mut::<ParticleLightGpuSettings>()
            .max_lights = 1;
        let one = capture(&mut app, &target);
        let one_delta = delta(&off, &one);
        assert!(
            app.world()
                .resource::<ParticleLightGpuStatistics>()
                .snapshot()
                .written_capacity
                <= 1
        );
        app.world_mut()
            .resource_mut::<ParticleLightGpuSettings>()
            .max_lights = cap;
        let restored_stars = capture(&mut app, &target);
        let restored_budget_delta = delta(&stars, &restored_stars);
        let player = app.world().get::<EffectPlayer>(owner).unwrap();
        assert!(Arc::ptr_eq(player.effect(), &artifact));
        assert_eq!(player.instance().history_epoch(), epoch_before_controls);
        assert_eq!(player.frame(), frame);
        // Expiry while forward playing: the output capacity is not a readback count.
        app.world_mut()
            .resource_mut::<TransientLightSettings>()
            .enabled = true;
        app.world_mut()
            .resource_mut::<ParticleLightGpuSettings>()
            .max_lights = cap;
        advance(&mut app, owner, 240);
        let expired = capture(&mut app, &target);
        assert_eq!(app.world().resource::<TransientLightStatistics>().active, 0);
        app.world_mut()
            .resource_mut::<ParticleLightGpuSettings>()
            .max_lights = 0;
        let expired_off = capture(&mut app, &target);
        let expiry_delta = delta(&expired, &expired_off);
        // Restart invalidates both output families and resets the existing fluid domain.
        let epoch = app
            .world()
            .get::<EffectPlayer>(owner)
            .unwrap()
            .instance()
            .history_epoch();
        app.world_mut()
            .get_mut::<EffectPlayer>(owner)
            .unwrap()
            .restart();
        app.world_mut()
            .get_mut::<EffectPlayer>(owner)
            .unwrap()
            .playing = false;
        app.world_mut()
            .resource_mut::<ParticleLightGpuSettings>()
            .max_lights = cap;
        let restart = capture(&mut app, &target);
        assert_ne!(
            app.world()
                .get::<EffectPlayer>(owner)
                .unwrap()
                .instance()
                .history_epoch(),
            epoch
        );
        let restart_delta = delta(&initial, &restart);
        advance(&mut app, owner, 115);
        let rebound = capture(&mut app, &target);
        assert_eq!(
            app.world().resource::<TransientLightStatistics>().accepted,
            4
        );
        let rebound_delta = delta(&both, &rebound);
        app.world_mut()
            .resource_mut::<TransientLightSettings>()
            .enabled = false;
        app.world_mut()
            .resource_mut::<ParticleLightGpuSettings>()
            .max_lights = 0;
        let rebound_off = capture(&mut app, &target);
        let smoke_repeat_delta = delta(&off, &rebound_off);
        // Remove a genuinely active owner, not one whose lights were already disabled.
        app.world_mut()
            .resource_mut::<TransientLightSettings>()
            .enabled = true;
        app.world_mut()
            .resource_mut::<ParticleLightGpuSettings>()
            .max_lights = cap;
        app.world_mut()
            .get_mut::<EffectPlayer>(owner)
            .unwrap()
            .restart();
        app.world_mut()
            .get_mut::<EffectPlayer>(owner)
            .unwrap()
            .playing = false;
        pump(&mut app, 24);
        advance(&mut app, owner, 115);
        let retirement_active = capture(&mut app, &target);
        assert_eq!(app.world().resource::<TransientLightStatistics>().active, 2);
        let retirement_active_delta = delta(&both, &retirement_active);
        app.world_mut().despawn(owner);
        let retired = capture(&mut app, &target);
        let retirement_delta = delta(&initial, &retired);
        assert_eq!(app.world().resource::<TransientLightStatistics>().active, 0);
        assert_eq!(
            app.world()
                .resource::<ParticleLightGpuStatistics>()
                .snapshot()
                .written_capacity,
            0
        );
        assert_native_transport(&app);
        for (name, image) in [
            ("initial", &initial),
            ("before-birth", &before_birth),
            ("before-birth-off", &before_birth_off),
            ("both", &both),
            ("representative", &rep),
            ("off", &off),
            ("selected", &stars),
            ("restored", &restored),
            ("one-slot", &one),
            ("restored-budget", &restored_stars),
            ("expired", &expired),
            ("expired-off", &expired_off),
            ("restart", &restart),
            ("rebound", &rebound),
            ("rebound-off", &rebound_off),
            ("retired", &retired),
            ("retirement-active", &retirement_active),
        ] {
            image.save(directory.join(format!("{name}.png"))).unwrap();
        }
        let caps = app.world().resource::<GpuCapabilities>();
        let accepted = rep_delta.0 >= 100
            && star_delta.0 >= 100
            && both_delta.0 >= 100
            && one_delta.0 >= 100
            && restored_delta.1 <= 1
            && expiry_delta.1 <= 1
            && restart_delta.1 <= 1
            && rebound_delta.1 <= 1
            && smoke_repeat_delta.1 <= 1
            && before_birth_delta.1 <= 1
            && restored_budget_delta.1 <= 1
            && retirement_delta.1 <= 1
            && retirement_active_delta.1 <= 1;
        let report = serde_json::json!({"slice":"F8.3B", "accepted":accepted, "tier":tier, "frame":frame,
            "adapter":caps.adapter_name, "backend":caps.backend, "driver":caps.driver,
            "dimensions":[480,360], "history":"playback-only", "seed":"0xf83b000000000001",
            "representative_accepted":representative.accepted, "representative_active":representative.active,
            "selected_capacity_bound":gpu.written_capacity, "selected_buffer_bytes":gpu.buffer_bytes,
            "selected_host_cap":cap, "representative_host_cap":2, "scene_light_visit_limit_high":16,
            "selected_positions_read_back":false, "bloom":false, "particle_renderers":false,
            "representative_delta":rep_delta, "selected_delta":star_delta, "both_delta":both_delta,
            "one_slot_delta":one_delta, "restored_delta":restored_delta, "expiry_delta":expiry_delta,
            "restart_delta":restart_delta, "rebound_delta":rebound_delta,
            "smoke_repeat_delta":smoke_repeat_delta,
            "before_birth_delta":before_birth_delta, "restored_budget_delta":restored_budget_delta,
            "retirement_delta":retirement_delta, "compiled_particle_capacity":artifact.max_particles,
            "retirement_active_delta":retirement_active_delta,
            "cost_scope":"GPU diagnostic milliseconds; 40 warm-up + 200 paused updates per case; fixed frame/volume/camera. Transparent pass includes volume, not whole renderer. Asynchronous timestamps deduplicated, not paired whole-frame or full-show costs; do not sum medians.",
            "costs":{"both":both_cost, "representative":rep_cost, "off":off_cost, "selected":star_cost},
            "limitations":"bounded two-burst smoke fixture; existing fluid source, not generic injection; default layers; unshadowed isotropic point-light scattering; no lit sprites/full-show/star-art/finale certification"});
        fs::write(
            directory.join("report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("{report}");
        assert!(
            accepted,
            "authored smoke lighting failed; evidence retained at {}",
            directory.display()
        );
    }
}
