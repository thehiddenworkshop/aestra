//! Native async readback + pooled receiver proof. No blocking GPU poll, replay,
//! sprite rendering, bloom, ambient light or representative flash in the images.
use aestra_bevy::Curve;
use aestra_bevy::*;
use bevy::{
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

#[derive(Resource, Default)]
struct Capture(Option<Image>);

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
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>(),
    )
    .add_plugins((
        AestraPlugin,
        AestraParticleLightPlugin,
        AestraTransientLightPlugin,
    ))
    .insert_resource(AestraParticleLightSettings {
        max_lights: 2,
        ..default()
    })
    .insert_resource(ParticleLightReadbackSettings {
        max_lights: 1,
        ..default()
    })
    .init_resource::<Capture>();
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
fn source() -> Arc<CompiledEffect> {
    let mut asset = EffectAsset::new("generic light-only moving stars", 10.0);
    let mut emitter = Emitter::basic_sprite("embers", 10.0);
    let appearance = emitter.modules.last().unwrap().clone();
    emitter.renderers.clear();
    emitter.modules = vec![
        ModuleInstance::emission(0.0, 3),
        ModuleInstance::shape(EmitterShape::Point),
        ModuleInstance::initialize(
            ScalarRange::new(10.0, 10.0),
            ScalarRange::new(2.0, 2.0),
            [1.0, 0.0, 0.0],
            0.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0; 3], 0.0, 0.0),
    ];
    emitter.modules.push(appearance);
    let mut properties = ParticlePointLightProperties::new(40_000.0, 8.0);
    properties.intensity_curve = Curve::new(vec![CurveKey::new(0.0, 40_000.0)]);
    properties.color_source = ParticleLightColorSource::Constant([1.0, 0.6, 0.2]);
    emitter
        .scene_outputs
        .push(SceneOutputInstance::particle_point_light(properties));
    asset.emitters.push(emitter);
    Arc::new(EffectCompiler::default().compile(&asset).unwrap())
}
fn tick_until(app: &mut App, predicate: impl Fn(&World) -> bool) {
    let start = Instant::now();
    loop {
        app.update();
        if predicate(app.world()) {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(45),
            "native async path did not settle: {:?}",
            app.world().resource::<ParticleLightStatistics>()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn position(world: &mut World) -> Vec3 {
    let mut lights = world.query_filtered::<&Transform, With<ParticleLightProxy>>();
    lights.single(world).unwrap().translation
}
fn capture(app: &mut App, target: &Handle<Image>, name: &str) -> Image {
    // Let camera/light extraction settle, then request an asynchronous screenshot.
    for _ in 0..5 {
        app.update();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |event: On<ScreenshotCaptured>, mut capture: ResMut<Capture>| {
                capture.0 = Some(event.image.clone());
            },
        );
    tick_until(app, |w| w.resource::<Capture>().0.is_some());
    let image = app.world_mut().resource_mut::<Capture>().0.take().unwrap();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/fireworks-f7/particle-light-receiver");
    std::fs::create_dir_all(&directory).unwrap();
    image
        .clone()
        .try_into_dynamic()
        .unwrap()
        .save(directory.join(format!("{name}.png")))
        .unwrap();
    image
}
fn response(image: &Image) -> (f64, f64) {
    let image = image.clone().try_into_dynamic().unwrap().to_rgb8();
    let mut sum = 0.0;
    let mut xsum = 0.0;
    for (x, _, p) in image.enumerate_pixels() {
        let value = f64::from(p[0]) + f64::from(p[1]) + f64::from(p[2]);
        sum += value;
        xsum += value * f64::from(x);
    }
    (
        sum / f64::from(image.width() * image.height() * 3),
        xsum / sum.max(1.0),
    )
}

fn lit_capture(
    app: &mut App,
    target: &Handle<Image>,
    name: &str,
    previous_centroid: Option<f64>,
) -> (f64, f64) {
    // A ready compute selector does not imply asynchronously compiled PBR receiver
    // pipelines are ready. Wait for a real receiver response, never accept black.
    let start = Instant::now();
    loop {
        let value = response(&capture(app, target, name));
        if value.0 > 5.0 && previous_centroid.is_none_or(|x| (value.1 - x).abs() > 8.0) {
            return value;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "receiver pipeline/lighting failed: {value:?}; {:?}",
            app.world().resource::<ParticleLightStatistics>()
        );
    }
}

#[test]
#[ignore = "native headless selected-light receiver/lifecycle probe; run explicitly"]
fn selected_moving_stars_light_a_receiver_and_obey_pool_lifecycle_without_replay() {
    let mut app = headless();
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            256,
            256,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
    app.world_mut().spawn((
        Camera3d::default(),
        AmbientLight {
            brightness: 0.0,
            ..default()
        },
        Camera {
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        bevy::camera::RenderTarget::Image(target.clone().into()),
        Transform::from_xyz(0.0, 6.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Plane3d::default().mesh().size(10.0, 10.0));
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.7, 0.7, 0.7),
            perceptual_roughness: 1.0,
            ..default()
        });
    app.world_mut()
        .spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default()));
    let mut player = EffectPlayer::from_compiled(source())
        .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    player.playing = false;
    player.instance_mut().set_playback_time(0.5);
    let owner = app
        .world_mut()
        .spawn((player, Transform::from_xyz(-2.0, 1.0, 0.0)))
        .id();
    tick_until(&mut app, |w| {
        w.resource::<ParticleLightStatistics>().active == 1
    });
    let first = position(app.world_mut());
    assert!(
        first.abs_diff_eq(Vec3::new(-1.0, 1.0, 0.0), 0.002),
        "{first:?}"
    );
    let entities: Vec<_> = app
        .world_mut()
        .query_filtered::<Entity, With<ParticleLightProxy>>()
        .iter(app.world())
        .collect();
    let on = lit_capture(&mut app, &target, "on-left", None);
    // Normal forward time, no replay/seek reconstruction. World transform is
    // already applied by GPU selection and must not be applied again here.
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .instance_mut()
        .set_playback_time(1.5);
    tick_until(&mut app, |w| {
        w.get::<Transform>(entities[0])
            .is_some_and(|t| t.translation.x > 0.99)
    });
    let second = position(app.world_mut());
    assert!(
        second.abs_diff_eq(Vec3::new(1.0, 1.0, 0.0), 0.002),
        "{second:?}"
    );
    let moved = lit_capture(&mut app, &target, "on-right", Some(on.1));
    assert!(
        (moved.1 - on.1).abs() > 8.0,
        "receiver centroid did not move: {on:?} -> {moved:?}"
    );
    let stats = app.world().resource::<ParticleLightStatistics>().clone();
    assert_eq!(stats.allocated, 1);
    assert_eq!(stats.copied_bytes, 64);
    assert!(stats.readback.pending <= 3 && stats.readback.staging_bytes <= 3 * 64);
    assert_eq!(stats.readback.failed, 0);
    assert!(stats.max_frame_lag <= 8 && stats.max_update_age_seconds <= 0.1);
    // Separate representative flash pool coexists, with no particle-event fanout.
    let epoch = app
        .world()
        .get::<EffectPlayer>(owner)
        .unwrap()
        .instance()
        .history_epoch();
    app.world_mut().write_message(AestraLightOutput {
        root: owner,
        playback_epoch: epoch,
        key: TransientLightKey {
            clip_path: vec![],
            output: "flash".into(),
            emitter: 0,
            tick: 0,
        },
        light: TransientPointLight {
            world_position: [0.0, 1.0, 0.0],
            root_time_seconds: 1.5,
            pulse: PointLightPulse::flash([1.0; 3], 1000.0, 8.0, 1.0),
        },
    });
    app.update();
    assert_eq!(app.world().resource::<TransientLightStatistics>().active, 1);
    assert_eq!(app.world().resource::<ParticleLightStatistics>().active, 1);
    // Restart must reject an old selected set on the very first main update.
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .restart();
    app.update();
    assert_eq!(app.world().resource::<ParticleLightStatistics>().active, 0);
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .playing = false;
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .instance_mut()
        .set_playback_time(0.5);
    tick_until(&mut app, |w| {
        w.resource::<ParticleLightStatistics>().active == 1
    });
    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_lights = 0;
    app.update();
    assert_eq!(
        app.world().resource::<ParticleLightStatistics>().allocated,
        0
    );
    // Retire the independent flash before the unlit control image.
    app.world_mut()
        .resource_mut::<TransientLightSettings>()
        .enabled = false;
    let off = response(&capture(&mut app, &target, "off"));
    assert!(
        on.0 > off.0 * 4.0 + 1.0 && moved.0 > off.0 * 4.0 + 1.0,
        "receiver not lit: {on:?}/{moved:?} vs {off:?}"
    );
    tick_until(&mut app, |w| {
        w.resource::<ParticleLightStatistics>().readback.pending == 0
            && w.resource::<ParticleLightStatistics>()
                .readback
                .staging_bytes
                == 0
    });
    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_lights = 2;
    tick_until(&mut app, |w| {
        w.resource::<ParticleLightStatistics>().active == 1
    });
    // Lowering a byte budget invalidates held results immediately; never resize
    // still-mapped staging slots or leave a light from the old configuration.
    app.world_mut()
        .resource_mut::<ParticleLightReadbackSettings>()
        .max_staging_bytes = 0;
    app.update();
    assert_eq!(app.world().resource::<ParticleLightStatistics>().active, 0);
    tick_until(&mut app, |w| {
        w.resource::<ParticleLightStatistics>()
            .readback
            .rejection
            .is_some()
    });
    app.world_mut()
        .resource_mut::<ParticleLightReadbackSettings>()
        .max_staging_bytes = 1024 * 1024;
    tick_until(&mut app, |w| {
        w.resource::<ParticleLightStatistics>().active == 1
    });
    app.world_mut()
        .resource_mut::<ParticleLightReadbackSettings>()
        .max_lights = 2;
    tick_until(&mut app, |w| {
        w.resource::<ParticleLightStatistics>().active == 2
    });
    assert_eq!(
        app.world().resource::<ParticleLightStatistics>().allocated,
        2
    );
    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_lights = 1;
    app.update();
    assert!(app.world().resource::<ParticleLightStatistics>().allocated <= 1);
    tick_until(&mut app, |w| {
        w.resource::<ParticleLightStatistics>().active == 1
    });
    app.world_mut().despawn(owner);
    app.update();
    assert_eq!(app.world().resource::<ParticleLightStatistics>().active, 0);
    println!("F7E receiver on={on:?}, moved={moved:?}, off={off:?}; pool={stats:?}");
    println!(
        "F7E_METRICS {{\"on\":[{},{}],\"moved\":[{},{}],\"off\":[{},{}],\"allocated\":{},\"copied_bytes\":{},\"max_age_ms\":{},\"max_frame_lag\":{},\"max_pool_update_ms\":{},\"failed\":{},\"lifecycle_passed\":true}}",
        on.0,
        on.1,
        moved.0,
        moved.1,
        off.0,
        off.1,
        stats.allocated,
        stats.copied_bytes,
        stats.max_update_age_seconds * 1000.0,
        stats.max_frame_lag,
        stats.max_pool_update_ms,
        stats.readback.failed
    );
}
