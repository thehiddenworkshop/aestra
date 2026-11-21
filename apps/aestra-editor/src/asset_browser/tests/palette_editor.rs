use super::*;
use crate::feathers::color_picker::{
    ColorPickerHexInput, ColorPickerPlane, handle_color_hex_change, handle_color_plane_change,
};
use bevy::ecs::system::RunSystemOnce;
use bevy::image::{CompressedImageFormats, ImageLoader};
use std::time::{Duration, Instant};

#[test]
fn docked_palette_previews_swatches_and_supports_discard_and_repeated_saves() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("pixel_pulse.palette.png");
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sample-project/textures/pixel_pulse.palette.png");
    std::fs::copy(sample, &path).unwrap();
    let original = std::fs::read(&path).unwrap();
    let mut app = super::super::tests::browser_app_with_assets(
        root.path(),
        AssetPlugin {
            unapproved_path_mode: bevy::asset::UnapprovedPathMode::Deny,
            ..default()
        },
    );
    setup_panel(&mut app);
    app.register_asset_loader(ImageLoader::new(CompressedImageFormats::NONE));
    let server = app.world().resource::<AssetServer>().clone();
    let (source, version, path) = {
        let catalog = app.world().resource::<ProjectEffectCatalog>();
        let entry = catalog
            .content()
            .source_tree()
            .at_relative_path("pixel_pulse.palette.png")
            .unwrap();
        (entry.id, catalog.content_revision(), entry.path.clone())
    };
    let handle = server
        .load_builder()
        .override_unapproved()
        .load::<Image>(AssetPath::from_path_buf(path.clone()));
    let deadline = Instant::now() + Duration::from_secs(5);
    while app
        .world()
        .resource::<Assets<Image>>()
        .get(&handle)
        .is_none()
    {
        app.update();
        assert!(Instant::now() < deadline, "palette image did not load");
        std::thread::yield_now();
    }
    let image_data = |app: &App| {
        app.world()
            .resource::<Assets<Image>>()
            .get(&handle)
            .unwrap()
            .data
            .clone()
            .unwrap()
    };
    let initial_pixels = image_data(&app);
    let open = |app: &mut App| {
        app.world_mut().trigger(Open(source, version));
        app.update();
    };
    open(&mut app);
    assert!(
        app.world()
            .resource::<WorkspaceLayout>()
            .is_active(ToolPanel::PaletteEditor)
    );
    {
        let world = app.world_mut();
        assert!(
            world
                .query_filtered::<Entity, With<crate::feathers::palette_picker::PalettePickerRoot>>(
                )
                .single(world)
                .is_ok()
        );
    }
    change(&mut app, "##oops");
    assert_eq!(image_data(&app), initial_pixels);
    change(&mut app, "#010203");
    assert_eq!(&image_data(&app)[..4], &[1, 2, 3, 255]);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    app.world_mut()
        .resource_mut::<WorkspaceLayout>()
        .close(ToolPanel::PaletteEditor);
    app.update();
    assert_eq!(image_data(&app), initial_pixels);

    open(&mut app);
    assert_eq!(&image_data(&app)[..4], &[1, 2, 3, 255]);
    activate(&mut app, Action::Discard);
    assert_eq!(image_data(&app), initial_pixels);

    let plane = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<ColorPickerPlane>>()
            .single(world)
            .unwrap()
    };
    app.world_mut().trigger(ValueChange {
        source: plane,
        value: Vec2::new(0.3, 0.9),
        is_final: true,
    });
    app.update();
    assert_ne!(image_data(&app), initial_pixels);
    activate(&mut app, Action::Discard);
    change(&mut app, "#010203");
    activate(&mut app, Action::MoveRight);
    assert_eq!(&image_data(&app)[4..8], &[1, 2, 3, 255]);
    activate(&mut app, Action::Add);
    assert_eq!(&image_data(&app)[8..12], &[1, 2, 3, 255]);
    activate(&mut app, Action::Remove);
    activate(&mut app, Action::Save);
    io::drain(app.world_mut());
    assert!(
        !app.world()
            .resource::<PaletteEditor>()
            .0
            .as_ref()
            .unwrap()
            .dirty()
    );
    assert_ne!(std::fs::read(&path).unwrap(), original);
    let colors = image::open(&path).unwrap().into_rgb8();
    assert_eq!(colors.dimensions(), (3, 1));
    assert_eq!(colors.get_pixel(1, 0).0, [1, 2, 3]);
    assert_eq!(image_data(&app)[4..8], [1, 2, 3, 255]);
    activate(&mut app, Action::Select(1));
    change(&mut app, "#040506");
    activate(&mut app, Action::Save);
    io::drain(app.world_mut());
    let saved = image::open(&path).unwrap().into_rgb8();
    assert_eq!(saved.get_pixel(1, 0).0, [4, 5, 6]);
}

fn setup_panel(app: &mut App) {
    app.init_resource::<WorkspaceLayout>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .init_resource::<bevy::feathers::controls::UnitsRegistry>()
        .insert_resource(theme::feathers_theme())
        .add_observer(handle_color_hex_change)
        .add_observer(handle_color_plane_change);
    app.world_mut()
        .run_system_once(|mut commands: Commands| {
            commands
                .spawn(Node::default())
                .with_children(spawn_palette_editor);
        })
        .unwrap();
}

fn change(app: &mut App, value: &str) {
    let world = app.world_mut();
    let source = world
        .query_filtered::<Entity, With<ColorPickerHexInput>>()
        .single(world)
        .unwrap();
    world.trigger(ValueChange {
        source,
        value: value.to_owned(),
        is_final: true,
    });
    app.update();
}

fn activate(app: &mut App, expected: Action) {
    let world = app.world_mut();
    let entity = world
        .query::<(Entity, &Action)>()
        .iter(world)
        .find(|(_, action)| **action == expected)
        .unwrap()
        .0;
    world.trigger(Activate { entity });
    app.update();
}

pub(in crate::asset_browser) fn verify_rendered_preview(
    app: &mut App,
    target: Handle<Image>,
    baseline: &[u8],
) {
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};

    #[derive(Resource, Default)]
    struct Capture(Option<Vec<u8>>);

    app.init_resource::<Capture>();
    setup_panel(app);
    let (file, version) = {
        let project = app.world().resource::<ProjectEffectCatalog>();
        (
            project
                .content()
                .source_tree()
                .at_relative_path("textures/pixel_pulse.palette.png")
                .unwrap()
                .id,
            project.content_revision(),
        )
    };
    app.world_mut().trigger(Open(file, version));
    app.update();
    for index in 0..3 {
        activate(app, Action::Select(index));
        change(app, "#0AE863");
    }

    let capture = |app: &mut App| {
        for _ in 0..12 {
            app.update();
            std::thread::sleep(Duration::from_millis(10));
        }
        app.world_mut()
            .spawn(Screenshot::image(target.clone()))
            .observe(
                |event: On<ScreenshotCaptured>, mut capture: ResMut<Capture>| {
                    capture.0 = Some(
                        event
                            .image
                            .clone()
                            .try_into_dynamic()
                            .unwrap()
                            .into_rgba8()
                            .into_raw(),
                    );
                },
            );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.update();
            if let Some(bytes) = app.world_mut().resource_mut::<Capture>().0.take() {
                return bytes;
            }
            assert!(
                Instant::now() < deadline,
                "Pixel Pulse preview did not capture"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let edited = capture(app);
    assert!(edited.as_chunks::<4>().0.iter().any(|pixel| {
        pixel[..3]
            .iter()
            .zip([10u8, 232, 99])
            .all(|(actual, swatch)| actual.abs_diff(swatch) <= 1)
    }));
    assert_ne!(edited, baseline);
    activate(app, Action::Discard);
    assert_eq!(capture(app), baseline);
}
