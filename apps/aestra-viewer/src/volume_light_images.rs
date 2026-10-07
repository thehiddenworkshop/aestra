//! F8.3A native volume-lighting control. Screenshot readback is test tooling only.
use crate::*;
use aestra_bevy::{ExtensionRegistry, ModuleParameters, PlaybackHistoryPolicy, Value};
use bevy::{app::PluginsState, camera::RenderTarget, render::render_resource::TextureFormat};

mod outputs;

#[derive(Resource, Default)]
struct Captured(Vec<Image>);

fn smoke(scene_gain: f32, scene_limit: u32) -> Arc<aestra_bevy::CompiledEffect> {
    let mut registry = ExtensionRegistry::builtin();
    registry.install(&aestra_fluid::FluidExtension).unwrap();
    let mut effect = aestra_fluid::smoke_effect(&registry);
    effect.emitters.clear(); // Isolate smoke: no emissive particles or bloom can counterfeit the gate.
    for module in &mut effect.simulation_stages[0].modules {
        if module.module_type.0 == aestra_fluid::MODULE_VOLUME_LOOK {
            let ModuleParameters::Custom(values) = &mut module.parameters else {
                unreachable!()
            };
            for (name, value) in [
                ("scene_light_intensity", Value::Scalar(scene_gain)),
                ("scene_light_limit", Value::U32(scene_limit)),
                ("ambient", Value::Scalar(0.05)),
                ("light_intensity", Value::Scalar(0.0)),
                ("shadow_steps", Value::U32(0)),
            ] {
                values.insert(name.into(), value);
            }
        }
    }
    Arc::new(
        EffectCompiler::with_extensions(registry)
            .compile(&effect)
            .unwrap(),
    )
}

fn pump(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn capture(app: &mut App, target: &Handle<Image>) -> RgbaImage {
    let started = Instant::now();
    let mut ready_frames = 0;
    while ready_frames < 24 {
        pump(app, 1);
        let ready = app.world().resource::<CaptureRenderReadiness>();
        assert!(
            !ready.detail.starts_with("Composer error"),
            "smoke shader validation failed: {}",
            ready.detail
        );
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "volume pipeline timeout: {}",
            ready.detail
        );
        ready_frames = if ready.ready { ready_frames + 1 } else { 0 };
    }
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |event: On<ScreenshotCaptured>, mut images: ResMut<Captured>| {
                images.0.push(event.image.clone())
            },
        );
    let started = Instant::now();
    while app.world().resource::<Captured>().0.is_empty() {
        pump(app, 1);
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "volume screenshot timeout"
        );
    }
    app.world_mut()
        .resource_mut::<Captured>()
        .0
        .remove(0)
        .try_into_dynamic()
        .unwrap()
        .to_rgba8()
}

#[test]
#[ignore = "native fluid-volume off/on/range/off image controls; run explicitly, alone"]
fn clustered_point_lights_illuminate_frozen_smoke_and_restore_baseline() {
    aestra_fluid::link();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>(),
    )
    .add_plugins(AestraPlugin)
    .insert_resource(AestraSettings {
        presentation: PresentationMode::Gpu,
        ..default()
    })
    .insert_resource(aestra_bevy::gpu::AestraCatchupPacing { paced: false })
    .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::ZERO,
    ))
    .init_resource::<Captured>()
    .init_resource::<CaptureRenderReadiness>();
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
    let parent = app
        .world_mut()
        .spawn((Transform::IDENTITY, Visibility::Visible))
        .id();
    app.world_mut().spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        RenderTarget::Image(target.clone().into()),
        bevy::camera::ShadowLodOrigin,
        Transform::from_xyz(0.0, 4.0, 16.0).looking_at(Vec3::new(0.0, 3.0, 0.0), Vec3::Y),
        ChildOf(parent),
    ));
    let mut player = EffectPlayer::from_compiled(smoke(1.0, 8))
        .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    player.playing = false;
    player.seek_frame(90);
    let owner = app
        .world_mut()
        .spawn((
            PresentedEffect::new(player.effect().clone()),
            player,
            Transform::from_scale(Vec3::splat(0.1)),
            ChildOf(parent),
        ))
        .id();
    let position = Vec3::new(0.0, 2.0, 1.0);
    let light = app
        .world_mut()
        .spawn((
            PointLight {
                intensity: 0.0,
                range: 6.0,
                radius: 0.1,
                color: Color::srgb(1.0, 0.05, 0.01),
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(position),
            ChildOf(parent),
        ))
        .id();
    // No wall, sky, particles or bloom. This test must render the volume, not merely its box.
    let off = capture(&mut app, &target);
    let identity = app
        .world()
        .get::<EffectPlayer>(owner)
        .unwrap()
        .effect()
        .clone();
    let epoch = app
        .world()
        .get::<EffectPlayer>(owner)
        .unwrap()
        .instance()
        .history_epoch();
    app.world_mut()
        .get_mut::<PointLight>(light)
        .unwrap()
        .intensity = 1500.0;
    let on = capture(&mut app, &target);
    app.world_mut()
        .get_mut::<Transform>(light)
        .unwrap()
        .translation = Vec3::new(500.0, 2.0, 1.0);
    let range_off = capture(&mut app, &target);
    app.world_mut()
        .get_mut::<Transform>(light)
        .unwrap()
        .translation = position;
    app.world_mut()
        .get_mut::<PointLight>(light)
        .unwrap()
        .intensity = 0.0;
    let restored = capture(&mut app, &target);
    // Rigidly move/rotate the camera, volume and light together: world-space lookup must
    // preserve their relative relationship through the hierarchy, rather than sample grid units.
    *app.world_mut().get_mut::<Transform>(parent).unwrap() = Transform::from_xyz(20.0, 7.0, -11.0)
        .with_rotation(Quat::from_euler(EulerRot::YXZ, 0.7, 0.2, 0.1));
    let nested_off = capture(&mut app, &target);
    app.world_mut()
        .get_mut::<PointLight>(light)
        .unwrap()
        .intensity = 1500.0;
    let nested_on = capture(&mut app, &target);
    app.world_mut()
        .get_mut::<PointLight>(light)
        .unwrap()
        .intensity = 0.0;
    let nested_restored = capture(&mut app, &target);
    // Also exercise a rotated, nonuniformly scaled volume independently of the camera.
    let placement = Transform::from_xyz(1.0, 0.0, 0.0)
        .with_rotation(Quat::from_rotation_y(0.5))
        .with_scale(Vec3::new(0.13, 0.08, 0.1));
    *app.world_mut().get_mut::<Transform>(owner).unwrap() = placement;
    app.world_mut()
        .get_mut::<Transform>(light)
        .unwrap()
        .translation = placement.transform_point(Vec3::new(0.0, 20.0, 10.0));
    let scaled_off = capture(&mut app, &target);
    app.world_mut()
        .get_mut::<PointLight>(light)
        .unwrap()
        .intensity = 1500.0;
    let scaled_on = capture(&mut app, &target);
    app.world_mut()
        .get_mut::<PointLight>(light)
        .unwrap()
        .intensity = 0.0;
    let scaled_restored = capture(&mut app, &target);
    let player = app.world().get::<EffectPlayer>(owner).unwrap();
    assert!(Arc::ptr_eq(player.effect(), &identity));
    assert_eq!(player.frame(), 90);
    assert_eq!(player.instance().history_epoch(), epoch);
    let seed = format!("{:#018x}", player.instance().seed());
    // Recompiled look controls at the same paused frame, not claims of live parameter binding.
    // The solver block is identical (locked by the portable contract test).
    app.world_mut()
        .get_mut::<PointLight>(light)
        .unwrap()
        .intensity = 1500.0;
    let mut disabled_images = Vec::new();
    for (name, gain, limit) in [("gain-zero", 0.0, 8), ("budget-zero", 1.0, 0)] {
        let mut player = EffectPlayer::from_compiled(smoke(gain, limit))
            .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
        player.playing = false;
        player.seek_frame(90);
        *app.world_mut().get_mut::<EffectPlayer>(owner).unwrap() = player;
        disabled_images.push((name, capture(&mut app, &target)));
    }
    let positive_pixels = |off: &RgbaImage, on: &RgbaImage| {
        off.pixels()
            .zip(on.pixels())
            .filter(|(a, b)| b[0] > a[0].saturating_add(3))
            .count()
    };
    let positive = positive_pixels(&off, &on);
    let nested_positive = positive_pixels(&nested_off, &nested_on);
    let scaled_positive = positive_pixels(&scaled_off, &scaled_on);
    let maximum_delta = |off: &RgbaImage, image: &RgbaImage| {
        off.pixels()
            .zip(image.pixels())
            .map(|(a, b)| (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap())
            .max()
            .unwrap()
    };
    let directory = std::env::var_os("AESTRA_VOLUME_LIGHT_IMAGE_REPORTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/fireworks-f8/volume-light-images")
        });
    fs::create_dir_all(&directory).unwrap();
    for (name, image) in [
        ("off", &off),
        ("on", &on),
        ("range-off", &range_off),
        ("restored", &restored),
        ("nested-off", &nested_off),
        ("nested-on", &nested_on),
        ("nested-restored", &nested_restored),
        ("scaled-off", &scaled_off),
        ("scaled-on", &scaled_on),
        ("scaled-restored", &scaled_restored),
    ] {
        image.save(directory.join(format!("{name}.png"))).unwrap();
    }
    for (name, image) in &disabled_images {
        image.save(directory.join(format!("{name}.png"))).unwrap();
    }
    let capabilities = app.world().resource::<GpuCapabilities>();
    let report = serde_json::json!({"slice":"F8.3A", "simulation_frame":90, "positive_red_pixels":positive,
        "adapter":capabilities.adapter_name, "backend":capabilities.backend, "seed":seed,
        "dimensions":[480,360], "driver":capabilities.driver,
        "range_off_max_delta":maximum_delta(&off, &range_off), "restored_max_delta":maximum_delta(&off, &restored),
        "nested_positive_red_pixels":nested_positive, "nested_restored_max_delta":maximum_delta(&nested_off, &nested_restored),
        "rigid_on_max_delta":maximum_delta(&on, &nested_on),
        "scaled_positive_red_pixels":scaled_positive, "scaled_restored_max_delta":maximum_delta(&scaled_off, &scaled_restored),
        "gain_zero_max_delta":maximum_delta(&scaled_off, &disabled_images[0].1),
        "budget_zero_max_delta":maximum_delta(&scaled_off, &disabled_images[1].1),
        "point_light_lumens":1500, "range_metres":6, "scene_light_gain":1, "scene_light_limit":8,
        "bloom":false, "particles":false, "limitations":"unshadowed point lights; no selected-GPU, representative-event, cost or particle-smoke qualification"});
    fs::write(
        directory.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{report}");
    assert!(
        positive >= 100,
        "smoke failed to respond; evidence retained"
    );
    assert!(
        nested_positive >= 100 && scaled_positive >= 100,
        "transformed smoke failed to respond"
    );
    assert!(
        maximum_delta(&off, &range_off) <= 1,
        "out-of-range light leaks"
    );
    assert!(
        maximum_delta(&on, &nested_on) <= 1,
        "rigid hierarchy must preserve lighting"
    );
    for (a, b) in [
        (&off, &restored),
        (&nested_off, &nested_restored),
        (&scaled_off, &scaled_restored),
    ] {
        assert!(
            maximum_delta(a, b) <= 1,
            "light removal must restore frozen smoke"
        );
    }
    for (_, image) in &disabled_images {
        assert!(
            maximum_delta(&scaled_off, image) <= 1,
            "disabled scene lighting must preserve the authored look"
        );
    }
}
