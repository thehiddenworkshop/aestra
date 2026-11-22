use std::time::{Duration, Instant};

use aestra_bevy::{PixelGrid, pixel_render_target};
use bevy::{
    camera::RenderTarget,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    window::ExitCondition,
};

use super::*;

#[test]
fn editor_sample_compiles_as_a_sprite_and_flipbook() {
    for sample in [
        LightingSample::Flat,
        LightingSample::NormalMapped,
        LightingSample::Palette,
    ] {
        drop(player(sample));
    }
}

#[derive(Resource, Default)]
struct Capture(Option<Image>);

fn capture(app: &mut App, target: &Handle<Image>, red_samples: &[(u32, u32)]) -> Vec<u8> {
    capture_colors(app, target, red_samples)
        .into_iter()
        .map(|color| color[0])
        .collect()
}

fn capture_colors(app: &mut App, target: &Handle<Image>, samples: &[(u32, u32)]) -> Vec<[u8; 3]> {
    for _ in 0..50 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |event: On<ScreenshotCaptured>, mut result: ResMut<Capture>| {
                result.0 = Some(event.image.clone());
            },
        );
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        app.update();
        if let Some(image) = app.world_mut().resource_mut::<Capture>().0.take() {
            let image = image.try_into_dynamic().unwrap().into_rgb8();
            return samples
                .iter()
                .map(|&(x, y)| image.get_pixel(x, y).0)
                .collect();
        }
        assert!(Instant::now() < deadline, "no 2D lighting screenshot");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn capture_app() -> App {
    let mut app = App::new();
    app.insert_resource(settings())
        .insert_resource(ClearColor(Color::BLACK))
        .init_resource::<Capture>()
        .add_plugins((
            DefaultPlugins
                .set(asset_plugin())
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<bevy::winit::WinitPlugin>()
                .disable::<bevy::log::LogPlugin>(),
            AestraPlugin,
        ));
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.plugins_state() != bevy::app::PluginsState::Ready {
        bevy::tasks::tick_global_task_pools_on_main_thread();
        assert!(Instant::now() < deadline, "native renderer did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    app.finish();
    app.cleanup();
    app
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn native_camera_2d_previews_the_grid_stage_without_particle_emitters() {
    use aestra_bevy::{
        EffectDimension,
        gpu::{AestraDebugViews, GpuStageProgress},
    };

    aestra_fluid::link();
    let effect = EffectAsset::load_ron(asset_root().join("effects/grid_2d_diffusion.aestra.ron"))
        .expect("load the Grid2D sample");
    assert_eq!(effect.dimension(), EffectDimension::TwoD);
    assert!(effect.emitters.is_empty());
    let mut player =
        EffectPlayer::new(&effect).with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    player.playing = false;
    player.set_playback_time(1.0 / 60.0);

    let mut app = capture_app();
    app.insert_resource(AestraDebugViews { field_slices: true });
    let world = app.world_mut();
    let target = world
        .resource_mut::<Assets<Image>>()
        .add(pixel_render_target(UVec2::new(128, 96)).unwrap());
    world.spawn((
        Camera2d,
        Msaa::Off,
        Transform::from_xyz(0.0, 0.0, 140.0),
        RenderTarget::Image(target.clone().into()),
    ));
    world.spawn((
        Camera3d::default(),
        Camera {
            is_active: false,
            ..default()
        },
    ));
    let owner = world.spawn((player, Transform::default())).id();
    let pixels = [(64, 48), (78, 48), (5, 5)];
    let deadline = Instant::now() + Duration::from_secs(6);
    let seed = loop {
        let samples = capture(&mut app, &target, &pixels);
        if samples[0] > 0 || Instant::now() >= deadline {
            break samples;
        }
    };
    let world = app.world();
    let player = world.get::<EffectPlayer>(owner).unwrap();
    assert!(
        world
            .get::<GpuStageProgress>(owner)
            .is_some_and(|progress| progress.is_ready(player.instance())),
        "Grid2D stage did not reach the seed tick"
    );
    let children = world
        .get::<Children>(owner)
        .expect("the field view has a quad");
    let quad = children
        .iter()
        .find_map(|entity| world.get::<ViewVisibility>(entity));
    assert!(
        quad.copied().is_some_and(ViewVisibility::get),
        "the field view is culled: {quad:?}"
    );
    assert!(seed[0] > 180 && seed[1] == 0 && seed[2] == 0, "{seed:?}");

    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .set_playback_time(30.0 / 60.0);
    let diffused = capture(&mut app, &target, &pixels);
    assert!(
        diffused[0] > 0 && diffused[0] < seed[0] && diffused[1] > 0 && diffused[2] == 0,
        "seed {seed:?}, diffused {diffused:?}"
    );
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn native_grid_2d_flow_moves_an_authored_follower_in_xy() {
    use aestra_bevy::{
        ColorKey, Curve, CurveKey, EmitterShape, Gradient, ModuleInstance, ModuleParameters,
    };

    aestra_fluid::link();
    let mut effect = EffectAsset::load_ron(asset_root().join("effects/grid_2d_flow.aestra.ron"))
        .expect("load the Grid2D Flow sample");
    // Use one stationary birth from the authored emitter so its displacement is only from the grid.
    let emitter = &mut effect.emitters[0];
    for module in &mut emitter.modules {
        match &mut module.parameters {
            ModuleParameters::Emission {
                spawn_rate,
                burst_count,
                ..
            } => {
                *spawn_rate = 0.0;
                *burst_count = 1;
            }
            ModuleParameters::Shape { shape } => *shape = EmitterShape::Point,
            ModuleParameters::Appearance { .. } => {
                module.parameters = ModuleInstance::appearance(
                    Curve::new(vec![CurveKey::new(0.0, 3.0)]),
                    Curve::new(vec![CurveKey::new(0.0, 1.0)]),
                    Gradient::new(vec![ColorKey::new(0.0, [1.0; 4])]),
                )
                .parameters;
            }
            _ => {}
        }
    }
    let mut player =
        EffectPlayer::new(&effect).with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    player.playing = false;
    player.set_playback_time(1.0 / 60.0);
    let mut app = capture_app();
    app.insert_resource(aestra_bevy::gpu::AestraDebugViews { field_slices: true });
    let world = app.world_mut();
    let target = world
        .resource_mut::<Assets<Image>>()
        .add(pixel_render_target(UVec2::new(128, 96)).unwrap());
    world.spawn((
        Camera2d,
        Msaa::Off,
        Transform::from_xyz(0.0, 0.0, 140.0),
        RenderTarget::Image(target.clone().into()),
    ));
    let owner = world.spawn((player, Transform::IDENTITY)).id();
    let above_birth: Vec<_> = (30..44)
        .flat_map(|y| (68..82).map(move |x| (x, y)))
        .collect();
    let start = (79, 48);
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let sample = capture_colors(&mut app, &target, &[start]);
        // The XY vector preview writes red and green. Only the tracer contributes blue.
        if sample[0][2] > 110 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "tracer did not appear at its birth position"
        );
    }

    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .set_playback_time(0.5);
    let after = capture_colors(&mut app, &target, &above_birth);
    assert!(
        after.iter().any(|color| color[2] > 110),
        "tracer did not follow the vortex"
    );
    let [red, green, blue] = capture_colors(&mut app, &target, &[start])[0];
    assert_eq!(blue, 0, "tracer stayed at its birth position");
    assert!(red > 0 || green > 0, "the vector field preview is missing");
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn native_grid_2d_smoke_rises_in_the_xy_view() {
    use aestra_bevy::gpu::{AestraDebugViews, GpuStageProgress};

    aestra_fluid::link();
    let effect = EffectAsset::load_ron(asset_root().join("effects/grid_2d_smoke.aestra.ron"))
        .expect("load the Grid2D Smoke sample");
    let mut player =
        EffectPlayer::new(&effect).with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    player.playing = false;
    player.set_playback_time(0.4);

    let mut app = capture_app();
    app.insert_resource(AestraDebugViews { field_slices: true });
    let world = app.world_mut();
    let target = world
        .resource_mut::<Assets<Image>>()
        .add(pixel_render_target(UVec2::new(128, 112)).unwrap());
    world.spawn((
        Camera2d,
        Msaa::Off,
        Transform::from_xyz(0.0, 0.0, 140.0),
        RenderTarget::Image(target.clone().into()),
    ));
    let owner = world.spawn((player, Transform::IDENTITY)).id();
    let plume: Vec<_> = (56..65)
        .flat_map(|y| (38..96).map(move |x| (x, y)))
        .collect();
    let mut samples = vec![(64, 84), (6, 6)];
    samples.extend(plume);
    let deadline = Instant::now() + Duration::from_secs(6);
    let early = loop {
        let pixels = capture(&mut app, &target, &samples);
        if pixels[0] > 80 {
            break pixels;
        }
        assert!(Instant::now() < deadline, "no density at the source");
    };
    assert!(
        early[1..].iter().all(|&red| red == 0),
        "smoke begins at the source"
    );

    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .set_playback_time(2.5);
    let later = capture(&mut app, &target, &samples);
    let world = app.world();
    let player = world.get::<EffectPlayer>(owner).unwrap();
    assert!(
        world
            .get::<GpuStageProgress>(owner)
            .is_some_and(|progress| progress.is_ready(player.instance())),
        "smoke did not reach the requested tick"
    );
    assert!(
        later[2..].iter().filter(|&&red| red > 40).count() > 20,
        "smoke did not rise through the view"
    );
    assert_eq!(later[1], 0, "the corner outside the grid stays clear");
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn native_camera_2d_lights_move_quantize_and_clear_per_view() {
    let mut app = capture_app();
    let world = app.world_mut();
    let mut images = world.resource_mut::<Assets<Image>>();
    let target = images.add(pixel_render_target(UVec2::new(160, 112)).unwrap());
    let ambient_target = images.add(pixel_render_target(UVec2::new(160, 112)).unwrap());
    let light_position = Vec2::new(-12.0, 0.0);
    let camera = world
        .spawn((
            Camera2d,
            Msaa::Off,
            PixelGrid,
            RenderTarget::Image(target.clone().into()),
            AestraLights2d {
                lights: vec![point_light(light_position, PointLightFalloff2d::Continuous)],
            },
        ))
        .id();
    world.spawn((
        Camera2d,
        Msaa::Off,
        RenderTarget::Image(ambient_target.clone().into()),
    ));
    world.spawn((player(LightingSample::Flat), Transform::IDENTITY));

    // Load the atlas, compile both view variants, and let the GPU reach its
    // first visible frame before comparing the two targets.
    for _ in 0..90 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }

    // Sample the opaque centers of the two atlas silhouettes, their interior
    // sprite gradient, and a transparent corner. Both atlas frames cover the center.
    let samples = [(40, 56), (47, 56), (116, 56), (17, 30)];
    let ambient = capture(&mut app, &ambient_target, &samples);
    let continuous = capture(&mut app, &target, &samples);
    assert!(
        ambient[0] > 0 && ambient[2] > 0 && ambient[0] <= 65,
        "{ambient:?}"
    );
    assert!(continuous[1] > continuous[0] + 5, "{continuous:?}");
    assert!(
        continuous[2] > ambient[2] + 30,
        "{continuous:?} vs {ambient:?}"
    );
    assert_eq!(continuous[3], 0, "the light must preserve atlas alpha");

    app.world_mut()
        .get_mut::<AestraLights2d>(camera)
        .unwrap()
        .lights[0]
        .falloff = PointLightFalloff2d::Stepped(NonZeroU32::new(4).unwrap());
    let stepped = capture(&mut app, &target, &samples);
    assert!(stepped[0] > ambient[0] + 40);
    assert!(
        stepped[0].abs_diff(stepped[1]) <= 2,
        "falloff should stay constant within a band: {stepped:?}"
    );

    app.world_mut()
        .get_mut::<AestraLights2d>(camera)
        .unwrap()
        .lights[0]
        .position = Vec2::new(54.0, 0.0);
    let moved = capture(&mut app, &target, &samples);
    assert_eq!(moved[0], ambient[0], "the sprite is now outside the radius");
    assert!(moved[2] > stepped[2] + 30, "{moved:?} vs {stepped:?}");

    app.world_mut().entity_mut(camera).remove::<PixelGrid>();
    assert_eq!(capture(&mut app, &target, &samples), moved);

    app.world_mut()
        .entity_mut(camera)
        .remove::<AestraLights2d>();
    assert_eq!(capture(&mut app, &target, &samples), ambient);
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn native_camera_2d_lights_respond_to_sprite_and_flipbook_normal_maps() {
    let mut app = capture_app();
    let world = app.world_mut();
    let target = world
        .resource_mut::<Assets<Image>>()
        .add(pixel_render_target(UVec2::new(160, 112)).unwrap());
    let camera = world
        .spawn((
            Camera2d,
            Msaa::Off,
            PixelGrid,
            RenderTarget::Image(target.clone().into()),
            AestraLights2d::default(),
        ))
        .id();
    world.spawn((player(LightingSample::NormalMapped), Transform::IDENTITY));
    for _ in 0..90 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }

    // Measure +X-facing texels. Set the light at equal distances on either
    // side of each texel, so only the normal can explain a strong difference.
    for (x, world_x) in [(47, -32.5), (119, 39.5)] {
        let mut red = |side: f32| {
            let light = point_light(
                Vec2::new(world_x + 28.0 * side, -0.5),
                PointLightFalloff2d::Continuous,
            );
            app.world_mut()
                .get_mut::<AestraLights2d>(camera)
                .unwrap()
                .lights = vec![light];
            capture(&mut app, &target, &[(x, 56), (17, 30)])
        };
        let away = red(-1.0);
        let toward = red(1.0);
        assert!(toward[0] > away[0] + 50, "{toward:?} vs {away:?} at {x}");
        assert_eq!(
            toward[1], 0,
            "normal shading must preserve transparent texels"
        );
    }
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn native_palette_lighting_selects_authored_swatches_on_sprite_and_flipbook() {
    let mut app = capture_app();
    let world = app.world_mut();
    let target = world
        .resource_mut::<Assets<Image>>()
        .add(pixel_render_target(UVec2::new(160, 112)).unwrap());
    let camera = world
        .spawn((
            Camera2d,
            Msaa::Off,
            PixelGrid,
            RenderTarget::Image(target.clone().into()),
            AestraLights2d::default(),
        ))
        .id();
    world.spawn((player(LightingSample::Palette), Transform::IDENTITY));
    for _ in 0..90 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }

    let swatches = [
        [32, 36, 70],
        [94, 64, 138],
        [230, 112, 114],
        [255, 225, 155],
    ];
    let left = Vec2::new(-32.5, -0.5);
    let right = Vec2::new(39.5, -0.5);
    let continuous = PointLightFalloff2d::Continuous;
    let stepped = PointLightFalloff2d::Stepped(NonZeroU32::new(4).unwrap());
    let samples = [(47, 56), (119, 56), (17, 30)];
    for (position, intensity, falloff, expected) in [
        (left, 0.0, continuous, [0, 0]),
        (left, 0.13, continuous, [1, 0]),
        (left, 0.27, continuous, [2, 0]),
        (left, 0.44, continuous, [3, 0]),
        (right, 0.44, continuous, [0, 3]),
        (left, 0.44, stepped, [3, 0]),
    ] {
        let mut light = point_light(position, falloff);
        light.color = Vec3::ONE;
        light.intensity = intensity;
        app.world_mut()
            .get_mut::<AestraLights2d>(camera)
            .unwrap()
            .lights = vec![light];
        let colors = capture_colors(&mut app, &target, &samples);
        for (color, index) in colors.iter().zip(expected) {
            let swatch = swatches[index];
            assert!(
                color
                    .iter()
                    .zip(swatch)
                    .all(|(actual, channel)| actual.abs_diff(channel) <= 1),
                "expected swatch {swatch:?} at {position:?}, intensity {intensity}; got {colors:?}"
            );
        }
        assert_eq!(
            colors[2], [0; 3],
            "transparent texels should reveal the background"
        );
    }
}
