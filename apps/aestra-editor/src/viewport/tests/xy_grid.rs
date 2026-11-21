use super::*;
use bevy::{
    app::PluginsState,
    camera::{RenderTarget, Viewport},
    render::{
        render_resource::TextureFormat,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
};
use std::time::{Duration, Instant};

#[derive(Resource, Default)]
struct GridCapture(Option<Image>);

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_xy_grid_tracks_the_world_and_sits_behind_sprites() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::log::LogPlugin>()
            .disable::<bevy_winit::WinitPlugin>(),
    )
    .init_resource::<MenuState>()
    .init_resource::<GridCapture>()
    .add_systems(Update, sync_xy_grid_visibility);
    register(&mut app);
    app.world_mut().resource_mut::<MenuState>().show_grid = true;
    let start = Instant::now();
    while app.plugins_state() != PluginsState::Ready {
        bevy::tasks::tick_global_task_pools_on_main_thread();
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "GPU initialization"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    app.finish();
    app.cleanup();
    let world = app.world_mut();
    let target = world
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            160,
            128,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
    let camera = world
        .spawn((
            Camera2d,
            PreviewXyCamera,
            PreviewRenderCamera,
            Camera {
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            RenderTarget::Image(target.clone().into()),
            Msaa::Off,
            RenderLayers::from_layers(&[0, XY_GRID_LAYER]),
        ))
        .id();
    world.spawn((
        Sprite::from_color(Color::srgb_u8(25, 85, 230), Vec2::splat(16.0)),
        Transform::from_xyz(0.0, 0.0, -125.0),
    ));

    let start = Instant::now();
    let initial = loop {
        let pixels = capture(&mut app, &target)
            .try_into_dynamic()
            .unwrap()
            .to_rgb8();
        // The sprite and grid pipelines can finish on different frames. Check
        // sprite readiness away from the axes before asserting their overlap.
        if is_y_axis(pixels.get_pixel(80, 33).0) && pixels.get_pixel(84, 68).0 == [25, 85, 230] {
            break pixels;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "XY grid or sprite pipeline was not ready"
        );
    };
    let [r, g, b] = initial.get_pixel(100, 64).0;
    assert!(r > g && r > b, "X axis is red");
    assert_eq!(initial.get_pixel(80, 64).0, [25, 85, 230]);

    let world = app.world_mut();
    world.get_mut::<Transform>(camera).unwrap().translation.x = 24.0;
    let panned = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .to_rgb8();
    assert!(is_y_axis(panned.get_pixel(56, 33).0));
    assert!(!is_y_axis(panned.get_pixel(80, 33).0));

    let mut projection = app.world_mut().get_mut::<Projection>(camera).unwrap();
    if let Projection::Orthographic(projection) = &mut *projection {
        projection.scale = 0.5;
    }
    let zoomed = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .to_rgb8();
    assert!(is_y_axis(zoomed.get_pixel(32, 33).0));
    assert_eq!(zoomed.get_pixel(32, 64).0, [25, 85, 230]);

    app.world_mut().get_mut::<Camera>(camera).unwrap().viewport = Some(Viewport {
        physical_position: UVec2::new(16, 8),
        physical_size: UVec2::new(120, 96),
        ..default()
    });
    let resized = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .to_rgb8();
    assert!(is_y_axis(resized.get_pixel(28, 26).0));
    assert_eq!(resized.get_pixel(28, 56).0, [25, 85, 230]);

    app.world_mut().resource_mut::<MenuState>().show_grid = false;
    let hidden = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .to_rgb8();
    assert_eq!(hidden.get_pixel(28, 26).0, [0; 3]);

    app.world_mut().resource_mut::<MenuState>().show_grid = true;
    app.world_mut()
        .entity_mut(camera)
        .insert(RenderLayers::layer(0));
    let other_view = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .to_rgb8();
    assert_eq!(other_view.get_pixel(28, 26).0, [0; 3]);
    assert_eq!(other_view.get_pixel(28, 56).0, [25, 85, 230]);
}

fn is_y_axis([r, g, b]: [u8; 3]) -> bool {
    g > r && g > b
}

fn capture(app: &mut App, target: &Handle<Image>) -> Image {
    for _ in 0..6 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |capture: On<ScreenshotCaptured>, mut output: ResMut<GridCapture>| {
                output.0 = Some(capture.image.clone());
            },
        );
    let start = Instant::now();
    loop {
        app.update();
        if let Some(image) = app.world_mut().resource_mut::<GridCapture>().0.take() {
            return image;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no XY grid capture"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
