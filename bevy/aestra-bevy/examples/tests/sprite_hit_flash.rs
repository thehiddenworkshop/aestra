use std::time::{Duration, Instant};

use bevy::{
    asset::RenderAssetUsages,
    camera::RenderTarget,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
};

use super::*;

#[test]
fn sample_project_compiles_all_authored_sprite_effects() {
    let overlays = SpriteOverlays::load();
    assert!(overlays.flash.frame_bounds.is_none());
    assert!(overlays.outline.frame_bounds.is_some());
    assert!(overlays.dissolve.source_size.is_some());
    assert!(overlays.dissolve.sprite_tint.is_some());
    assert!(overlays.palette_swap.sprite_tint.is_some());
    assert!(overlays.distortion.source_size.is_some());
    for overlay in [
        overlays.flash,
        overlays.outline,
        overlays.dissolve,
        overlays.distortion,
    ] {
        assert_eq!(
            overlay.project.root.playback_mode,
            aestra_bevy::EffectPlaybackMode::Once
        );
        assert!(
            overlay
                .template
                .material_binding(overlay.material)
                .is_some()
        );
    }
    assert_eq!(
        overlays.palette_swap.project.root.playback_mode,
        aestra_bevy::EffectPlaybackMode::LoopRestart
    );
    assert!(
        overlays
            .palette_swap
            .template
            .material_binding(overlays.palette_swap.material)
            .is_some()
    );
}

#[derive(Default, Resource)]
struct Capture(Option<Image>);

fn capture_app() -> App {
    let mut app = App::new();
    app.insert_resource(AestraSettings {
        presentation: PresentationMode::Gpu,
        ..default()
    })
    .insert_resource(SpriteOverlays::load())
    .insert_resource(ClearColor(Color::BLACK))
    .init_resource::<Capture>()
    .add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: asset_root().to_string_lossy().into_owned(),
                ..default()
            })
            .set(ImagePlugin::default_nearest())
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(),
    )
    .add_plugins(AestraPlugin)
    .add_systems(Update, retire_overlays);
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

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn native_overlays_preserve_the_mirrored_sprite_and_isolate_its_atlas_frame() {
    let mut app = capture_app();
    let red = [210, 32, 56, 255];
    let transparent = [0, 0, 0, 0];
    // The atlas's first frame is solid. Its second frame has only one visible texel.
    let atlas = Image::new(
        Extent3d {
            width: 4,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        [
            red,
            red,
            red,
            transparent,
            red,
            red,
            transparent,
            transparent,
        ]
        .concat(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let world = app.world_mut();
    let mut images = world.resource_mut::<Assets<Image>>();
    let image = images.add(atlas);
    let target = images.add(aestra_bevy::pixel_render_target(UVec2::splat(64)).unwrap());
    let layout =
        world
            .resource_mut::<Assets<TextureAtlasLayout>>()
            .add(TextureAtlasLayout::from_grid(
                UVec2::splat(2),
                2,
                1,
                None,
                None,
            ));
    let mut sprite = Sprite::from_atlas_image(image, TextureAtlas { layout, index: 1 });
    sprite.flip_x = true;
    sprite.custom_size = Some(Vec2::new(32.0, 24.0));
    let anchor = Anchor::BOTTOM_LEFT;
    world.spawn((
        HostSprite::default(),
        sprite,
        anchor,
        Transform::from_xyz(-16.0, -12.0, 0.0),
    ));
    world.spawn((
        Camera2d,
        Msaa::Off,
        RenderTarget::Image(target.clone().into()),
        replacement_pipelines(world),
    ));

    let before = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert_eq!(before.get_pixel(40, 25).0, red[..3]);
    assert_eq!(before.get_pixel(24, 25).0, [0; 3]);

    let flash = trigger_at_frame_two(&mut app, KeyCode::Space);
    let after = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert_eq!(after.get_pixel(40, 25).0, [255; 3]);
    for (x, y, pixel) in after.enumerate_pixels() {
        let expected = if (32..48).contains(&x) && (20..32).contains(&y) {
            [255; 3]
        } else {
            before.get_pixel(x, y).0
        };
        assert_eq!(pixel.0, expected, "pixel at ({x}, {y})");
    }

    let mut player = app.world_mut().get_mut::<EffectPlayer>(flash).unwrap();
    let duration = player.effect().duration;
    player.seek(duration * 0.65);
    let fading = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    let [_, green, _] = fading.get_pixel(40, 25).0;
    assert!(
        (33..255).contains(&green),
        "expected a partially faded flash"
    );
    assert_eq!(fading.get_pixel(24, 25).0, [0; 3]);

    app.world_mut()
        .get_mut::<EffectPlayer>(flash)
        .unwrap()
        .playing = true;
    let finished = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert!(app.world().get_entity(flash).is_err());
    assert_eq!(finished, before);

    trigger_at_frame_two(&mut app, KeyCode::KeyO);
    let outlined = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    for (x, y, pixel) in outlined.enumerate_pixels() {
        // The four immediate neighbours of the solid source texel are outlined,
        // including the two outside the source rectangle. Keep the diagonals clear.
        let horizontal = (20..32).contains(&y) && ((16..32).contains(&x) || (48..64).contains(&x));
        let vertical = (32..48).contains(&x) && ((8..20).contains(&y) || (32..44).contains(&y));
        if horizontal || vertical {
            assert!(
                pixel
                    .0
                    .into_iter()
                    .zip([48, 245, 194])
                    .all(|(found, color)| found.abs_diff(color) <= 1),
                "expected an outline at ({x}, {y}), got {pixel:?}",
            );
        } else {
            assert_eq!(
                pixel,
                before.get_pixel(x, y),
                "unexpected change at ({x}, {y})"
            );
        }
    }
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn native_dissolve_waits_for_2d_pipeline_hides_original_and_restores_layers() {
    let mut app = capture_app();
    let world = app.world_mut();
    let mut images = world.resource_mut::<Assets<Image>>();
    let neighbor = [0, 255, 0, 255];
    let sprite_texel = [220, 130, 65, 255];
    let sprite_pixel = |x, y| match (x, y) {
        (0..=4, _) => neighbor,
        (5, 0) => [0, 0, 0, 0],
        (6, 2) => [220, 130, 65, 128],
        _ => sprite_texel,
    };
    let image = images.add(Image::new(
        Extent3d {
            width: 8,
            height: 4,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        (0..4)
            .flat_map(|y| (0..8).flat_map(move |x| sprite_pixel(x, y)))
            .collect(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let target = images.add(aestra_bevy::pixel_render_target(UVec2::splat(64)).unwrap());
    let layout =
        world
            .resource_mut::<Assets<TextureAtlasLayout>>()
            .add(TextureAtlasLayout::from_grid(
                UVec2::splat(4),
                2,
                1,
                None,
                None,
            ));
    let mut sprite = Sprite::from_atlas_image(image, TextureAtlas { layout, index: 1 });
    sprite.rect = Some(Rect::new(1.0, 0.0, 4.0, 4.0));
    sprite.flip_x = true;
    sprite.custom_size = Some(Vec2::new(24.0, 32.0));
    sprite.color = Color::srgba_u8(170, 235, 180, 200);
    let layers = RenderLayers::layer(2);
    let host = world
        .spawn((HostSprite::default(), sprite, layers.clone()))
        .id();
    let camera = world
        .spawn((
            Camera2d,
            Msaa::Sample4,
            aestra_bevy::PixelGrid,
            RenderTarget::Image(target.clone().into()),
            layers.clone(),
            replacement_pipelines(world),
        ))
        .id();

    press(&mut app, KeyCode::KeyD);
    assert!(matches!(
        app.world().get::<HostSprite>(host),
        Some(HostSprite::ReplacementRequested(Replacement::Dissolve))
    ));
    press(&mut app, KeyCode::KeyR);
    assert!(matches!(
        app.world().get::<HostSprite>(host),
        Some(HostSprite::Original)
    ));
    press(&mut app, KeyCode::KeyD);
    assert_eq!(app.world().get::<RenderLayers>(host), Some(&layers));
    let before = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert_ne!(before.get_pixel(24, 20).0, [0; 3]);
    assert_eq!(before.get_pixel(40, 20).0, [0; 3]); // Flipped transparent texel.
    wait_for_replacement_pipelines(&mut app, camera);

    app.world_mut().run_system_cached(trigger_overlay).unwrap();
    let dissolve = pause_overlay(&mut app);
    assert_eq!(
        app.world().get::<RenderLayers>(host),
        Some(&RenderLayers::none())
    );
    assert_eq!(app.world().get::<RenderLayers>(dissolve), Some(&layers));
    let start = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    for (x, y, pixel) in start.enumerate_pixels() {
        assert!(
            within_rounding(pixel.0, before.get_pixel(x, y).0),
            "start changed ({x}, {y})"
        );
    }

    let mut player = app.world_mut().get_mut::<EffectPlayer>(dissolve).unwrap();
    let duration = player.effect().duration;
    player.seek(duration * 0.5);
    let middle = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    let mut kept = 0;
    let mut removed = 0;
    for y in (16..48).step_by(8) {
        for x in (20..44).step_by(8) {
            let clear_cell = middle.get_pixel(x, y).0 == [0; 3];
            if before.get_pixel(x, y).0 != [0; 3] {
                if clear_cell {
                    removed += 1;
                } else {
                    kept += 1;
                }
            }
            for dy in 0..8 {
                for dx in 0..8 {
                    let expected = if clear_cell {
                        [0; 3]
                    } else {
                        before.get_pixel(x + dx, y + dy).0
                    };
                    assert!(within_rounding(
                        middle.get_pixel(x + dx, y + dy).0,
                        expected
                    ));
                }
            }
        }
    }
    assert!(
        kept > 0 && removed > 0,
        "the dissolve should remove some whole cells"
    );

    let mut player = app.world_mut().get_mut::<EffectPlayer>(dissolve).unwrap();
    player.seek(duration * 0.92);
    let completed = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert!(completed.pixels().all(|pixel| pixel.0 == [0; 3]));
    app.world_mut()
        .get_mut::<EffectPlayer>(dissolve)
        .unwrap()
        .playing = true;
    let retired = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert!(app.world().get_entity(dissolve).is_err());
    assert_eq!(retired, completed);

    press(&mut app, KeyCode::KeyR);
    let restored = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert_eq!(restored, before);
    assert_eq!(app.world().get::<RenderLayers>(host), Some(&layers));
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn native_palette_swap_replaces_matching_atlas_colors_and_preserves_alpha() {
    let mut app = capture_app();
    let world = app.world_mut();
    let mut images = world.resource_mut::<Assets<Image>>();
    let amber = [255, 185, 57, 255];
    let pink = [255, 63, 132, 255];
    let orange = [255, 94, 56, 255];
    let neighbor = [0, 255, 0, 255];
    let image = images.add(Image::new(
        Extent3d {
            width: 8,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        [
            neighbor,
            neighbor,
            neighbor,
            neighbor,
            amber,
            [255, 63, 132, 128],
            [255, 185, 58, 255],
            [255, 185, 57, 0],
            neighbor,
            neighbor,
            neighbor,
            neighbor,
            orange,
            pink,
            neighbor,
            amber,
        ]
        .concat(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let target = images.add(aestra_bevy::pixel_render_target(UVec2::splat(64)).unwrap());
    let layout =
        world
            .resource_mut::<Assets<TextureAtlasLayout>>()
            .add(TextureAtlasLayout::from_grid(
                UVec2::new(4, 2),
                2,
                1,
                None,
                None,
            ));
    let mut sprite = Sprite::from_atlas_image(image, TextureAtlas { layout, index: 1 });
    sprite.custom_size = Some(Vec2::new(32.0, 16.0));
    sprite.flip_x = true;
    let host = world.spawn((HostSprite::default(), sprite)).id();
    let camera = world
        .spawn((
            Camera2d,
            Msaa::Off,
            RenderTarget::Image(target.clone().into()),
            replacement_pipelines(world),
        ))
        .id();
    let before = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert_eq!(before.get_pixel(44, 28).0, amber[..3]);
    wait_for_replacement_pipelines(&mut app, camera);
    press(&mut app, KeyCode::KeyP);
    let world = app.world_mut();
    let effect_entity = world
        .query_filtered::<Entity, With<ActiveOverlay>>()
        .single(world)
        .unwrap();
    assert!(!world.get::<EffectPlayer>(effect_entity).unwrap().playing);
    assert_eq!(world.get::<RenderLayers>(host), Some(&RenderLayers::none()));

    let deadline = Instant::now() + Duration::from_secs(20);
    let swapped = loop {
        let image = capture(&mut app, &target)
            .try_into_dynamic()
            .unwrap()
            .into_rgb8();
        if within_rounding(image.get_pixel(44, 28).0, [84, 225, 255]) {
            break image;
        }
        assert!(
            Instant::now() < deadline,
            "palette assets did not reach the render world"
        );
    };
    let half = Color::srgb_u8(111, 119, 255).to_linear();
    let half = Color::linear_rgb(
        half.red * (128.0 / 255.0),
        half.green * (128.0 / 255.0),
        half.blue * (128.0 / 255.0),
    )
    .to_srgba()
    .to_u8_array();
    for (x, y, pixel) in swapped.enumerate_pixels() {
        let expected = match (x, y) {
            (40..48, 24..32) | (16..24, 32..40) => [84, 225, 255],
            (32..40, 24..32) => [half[0], half[1], half[2]],
            (40..48, 32..40) => [30, 116, 187],
            (32..40, 32..40) => [111, 119, 255],
            _ => before.get_pixel(x, y).0,
        };
        assert!(
            within_rounding(pixel.0, expected),
            "wrong swatch or alpha at ({x}, {y})"
        );
    }

    // An unavailable target palette must leave the original RGB and alpha intact.
    let world = app.world_mut();
    let overlay = &world.resource::<SpriteOverlays>().palette_swap;
    let material = overlay.material;
    let target_palette = overlay.project.root.material_programs[0]
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Target Palette")
        .unwrap()
        .id;
    let mut effect = world.get_mut::<PresentedEffect>(effect_entity).unwrap();
    let mut binding = effect.material_binding(material).unwrap().clone();
    binding
        .set_value(target_palette, MaterialValue::Texture2D(AssetId::new()))
        .unwrap();
    effect.bind_material(material, binding);
    let fallback = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    for (x, y, pixel) in fallback.enumerate_pixels() {
        assert!(within_rounding(pixel.0, before.get_pixel(x, y).0));
    }

    press(&mut app, KeyCode::KeyR);
    let restored = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert_eq!(restored, before);
    assert!(app.world().get::<RenderLayers>(host).is_none());
    assert!(app.world().get_entity(effect_entity).is_err());
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn native_sprite_wave_warps_a_mirrored_crop_and_restores_the_host() {
    let mut app = capture_app();
    let world = app.world_mut();
    let mut images = world.resource_mut::<Assets<Image>>();
    let columns = [
        [230, 63, 49, 255],
        [230, 150, 38, 128],
        [75, 137, 250, 255],
        [190, 50, 200, 255],
    ];
    let atlas_pixel = |x, y| match (x, y) {
        (8, 1) => [0; 4],
        (7..11, _) => columns[x - 7],
        _ => [0, 255, 0, 255],
    };
    let image = images.add(Image::new(
        Extent3d {
            width: 12,
            height: 4,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        (0..4)
            .flat_map(|y| (0..12).flat_map(move |x| atlas_pixel(x, y)))
            .collect(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let target = images.add(aestra_bevy::pixel_render_target(UVec2::splat(64)).unwrap());
    let layout =
        world
            .resource_mut::<Assets<TextureAtlasLayout>>()
            .add(TextureAtlasLayout::from_grid(
                UVec2::new(6, 4),
                2,
                1,
                None,
                None,
            ));
    let mut sprite = Sprite::from_atlas_image(image, TextureAtlas { layout, index: 1 });
    sprite.rect = Some(Rect::new(1.0, 0.0, 5.0, 4.0));
    sprite.custom_size = Some(Vec2::splat(32.0));
    sprite.flip_x = true;
    sprite.flip_y = true;
    let layers = RenderLayers::layer(2);
    let host = world
        .spawn((
            HostSprite::default(),
            sprite,
            Anchor::BOTTOM_LEFT,
            Transform::from_xyz(-16.0, -16.0, 0.0),
            layers.clone(),
        ))
        .id();
    let camera = world
        .spawn((
            Camera2d,
            Msaa::Off,
            RenderTarget::Image(target.clone().into()),
            layers.clone(),
            replacement_pipelines(world),
        ))
        .id();

    let before = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert_eq!(before.get_pixel(20, 20).0, columns[3][..3]);
    assert_eq!(before.get_pixel(36, 36).0, [0; 3]);
    wait_for_replacement_pipelines(&mut app, camera);
    let wave = trigger_at_frame_two(&mut app, KeyCode::KeyW);
    assert_eq!(
        app.world().get::<RenderLayers>(host),
        Some(&RenderLayers::none())
    );
    let start = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert_eq!(before, start);

    let time = 0.5;
    app.world_mut()
        .get_mut::<EffectPlayer>(wave)
        .unwrap()
        .seek(time);
    let warped = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert_ne!(warped, before);
    assert!(warped.get_pixel(44, 18).0 == [0; 3]);
    for (x, y, pixel) in warped.enumerate_pixels() {
        let expected = if (16..48).contains(&x) && (16..48).contains(&y) {
            let v = (y as f32 - 15.5) / 32.0;
            let phase = std::f32::consts::TAU * (v - 1.5 * time);
            let warped_u = (x as f32 - 15.5) / 32.0 + phase.sin() * (1.75 / 4.0);
            if (0.0..1.0).contains(&warped_u) {
                let source_column = (warped_u * 4.0) as u32;
                before.get_pixel(20 + source_column * 8, y).0
            } else {
                [0; 3]
            }
        } else {
            before.get_pixel(x, y).0
        };
        assert!(
            within_rounding(pixel.0, expected),
            "wrong sample at ({x}, {y})"
        );
    }

    let mut player = app.world_mut().get_mut::<EffectPlayer>(wave).unwrap();
    player.seek(0.95);
    let settled = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert_eq!(settled, before);
    app.world_mut()
        .get_mut::<EffectPlayer>(wave)
        .unwrap()
        .playing = true;
    let restored = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .into_rgb8();
    assert_eq!(restored, before);
    assert_eq!(app.world().get::<RenderLayers>(host), Some(&layers));
    assert!(matches!(
        app.world().get::<HostSprite>(host),
        Some(HostSprite::Original)
    ));
    assert!(app.world().get_entity(wave).is_err());
}

fn replacement_pipelines(world: &World) -> EffectPipelinePreparation {
    world.resource::<SpriteOverlays>().replacement_pipelines()
}

fn wait_for_replacement_pipelines(app: &mut App, camera: Entity) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status = app
            .world()
            .get::<EffectPipelinePreparation>(camera)
            .unwrap()
            .status();
        if status != EffectPipelinePreparationStatus::Pending {
            assert_eq!(
                status,
                EffectPipelinePreparationStatus::Ready { pipelines: 3 }
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "2D replacement pipelines did not prepare"
        );
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn within_rounding(a: [u8; 3], b: [u8; 3]) -> bool {
    a.into_iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1)
}

fn press(app: &mut App, key: KeyCode) {
    let world = app.world_mut();
    world.resource_mut::<ButtonInput<KeyCode>>().press(key);
    world.run_system_cached(trigger_overlay).unwrap();
    world.resource_mut::<ButtonInput<KeyCode>>().reset(key);
}

fn trigger_at_frame_two(app: &mut App, key: KeyCode) -> Entity {
    press(app, key);
    let entity = pause_overlay(app);
    app.world_mut()
        .get_mut::<EffectPlayer>(entity)
        .unwrap()
        .seek_frame(2);
    entity
}

fn pause_overlay(app: &mut App) -> Entity {
    let world = app.world_mut();
    let entity = world
        .query_filtered::<Entity, With<ActiveOverlay>>()
        .single(world)
        .unwrap();
    world.get_mut::<EffectPlayer>(entity).unwrap().playing = false;
    entity
}

fn capture(app: &mut App, target: &Handle<Image>) -> Image {
    // Allow pipeline creation and render extraction to reach the offscreen target.
    for _ in 0..70 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |event: On<ScreenshotCaptured>, mut capture: ResMut<Capture>| {
                capture.0 = Some(event.image.clone());
            },
        );
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        app.update();
        if let Some(image) = app.world_mut().resource_mut::<Capture>().0.take() {
            return image;
        }
        assert!(Instant::now() < deadline, "no offscreen screenshot");
        std::thread::sleep(Duration::from_millis(10));
    }
}
