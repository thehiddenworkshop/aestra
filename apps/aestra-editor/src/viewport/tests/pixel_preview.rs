use super::*;
use crate::viewport::{
    lifecycle::{
        configure_transform_gizmo_overlay_camera, sync_gizmo_overlay_projection,
        sync_preview_camera_viewport,
    },
    navigation::PreviewRenderCamera,
    scene::PreviewPerspectiveCamera,
    shortcuts::PreviewCanvas,
};
use aestra_core::{
    ModuleParameters, ScalarRange,
    dither::{BayerPattern, DitherPattern},
    material::MaterialInput,
};
use bevy::{
    app::PluginsState,
    camera::visibility::RenderLayers,
    math::Affine2,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    window::{ExitCondition, PrimaryWindow, WindowResolution},
};
use std::time::{Duration, Instant};

#[derive(Resource, Default)]
struct Capture(Option<Image>);

struct PreviewFixture {
    app: App,
    root: std::path::PathBuf,
    output: Handle<Image>,
    xy: Entity,
    perspective: Entity,
    canvas: Entity,
}

fn preview_fixture() -> PreviewFixture {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sample-project")
        .canonicalize()
        .unwrap();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::asset::AssetPlugin {
                unapproved_path_mode: bevy::asset::UnapprovedPathMode::Deny,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::log::LogPlugin>()
            .disable::<bevy_winit::WinitPlugin>(),
    )
    .add_plugins(aestra_bevy_render::AestraRenderPlugin)
    .insert_resource(aestra_bevy_render::AestraTextureRoot(Some(root.clone())))
    .insert_resource(ClearColor(theme::APP_BG))
    .insert_resource(PreviewDisplayState {
        pixel_grid: true,
        ..default()
    })
    .init_resource::<Capture>()
    .add_systems(Startup, spawn_pixel_preview)
    .add_systems(PostStartup, configure_transform_gizmo_overlay_camera)
    .add_systems(
        Update,
        (
            configure_xy_projection,
            follow_xy_camera,
            sync_gizmo_overlay_projection,
        )
            .chain(),
    )
    .add_systems(PostUpdate, sync_preview_camera_viewport);
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.plugins_state() != PluginsState::Ready {
        bevy::tasks::tick_global_task_pools_on_main_thread();
        assert!(Instant::now() < deadline, "GPU initialization");
        std::thread::sleep(Duration::from_millis(10));
    }
    app.finish();
    app.cleanup();
    let world = app.world_mut();
    let output_size = UVec2::new(640, 480);
    let output = world
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            output_size.x,
            output_size.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
    world.spawn((
        Window {
            resolution: WindowResolution::new(output_size.x, output_size.y),
            ..default()
        },
        PrimaryWindow,
    ));
    let xy = world
        .spawn((
            Camera2d,
            PreviewXyCamera,
            PreviewRenderCamera,
            RenderTarget::Image(output.clone().into()),
        ))
        .id();
    let perspective = world
        .spawn((
            Camera3d::default(),
            Camera {
                order: -2,
                is_active: false,
                ..default()
            },
            PreviewPerspectiveCamera,
            RenderTarget::Image(output.clone().into()),
        ))
        .id();
    let ui_camera = world
        .spawn((
            crate::shell::editor_ui_camera(),
            RenderTarget::Image(output.clone().into()),
        ))
        .id();
    world.spawn((
        Camera3d::default(),
        RenderLayers::layer(15),
        RenderTarget::Image(output.clone().into()),
    ));
    // A visible UI control inside the overlay viewport must survive the final
    // camera's blit in both the pixel and standard perspective views.
    world.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(85.0),
            top: Val::Px(54.0),
            width: Val::Px(16.0),
            height: Val::Px(16.0),
            ..default()
        },
        BackgroundColor(Color::srgb_u8(255, 0, 255)),
        bevy::ui::UiTargetCamera(ui_camera),
    ));
    let canvas = world
        .spawn((
            PreviewCanvas,
            dock_rect(UVec2::new(31, 19), UVec2::new(405, 281)),
        ))
        .id();
    for (position, color) in [
        (Vec2::new(-0.5, 0.5), Color::srgb_u8(255, 0, 0)),
        (Vec2::new(0.5, 0.5), Color::srgb_u8(0, 255, 0)),
        (Vec2::new(-95.5, 63.5), Color::srgb_u8(0, 255, 255)),
        (Vec2::new(95.5, -63.5), Color::srgb_u8(255, 255, 0)),
    ] {
        world.spawn((
            Sprite::from_color(color, Vec2::ONE),
            Transform::from_translation(position.extend(-1.0)),
        ));
    }
    // Replace only the compositor's output. Scene, layout, projection, and
    // upscaling still use the shipping editor systems.
    app.update();
    let world = app.world_mut();
    let compositor = world
        .query::<(Entity, &PixelPreviewPass)>()
        .iter(world)
        .find_map(|(entity, pass)| matches!(pass, PixelPreviewPass::Composite).then_some(entity))
        .unwrap();
    world
        .entity_mut(compositor)
        .insert(RenderTarget::Image(output.clone().into()));

    PreviewFixture {
        app,
        root,
        output,
        xy,
        perspective,
        canvas,
    }
}

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_pixel_preview_composites_exact_blocks_at_dock_offsets_and_after_resize() {
    let PreviewFixture {
        mut app,
        output,
        xy,
        perspective,
        canvas,
        ..
    } = preview_fixture();
    let deadline = Instant::now() + Duration::from_secs(20);
    let origin = UVec2::new(41, 31);
    let red = origin + UVec2::new(95, 63) * 2;
    let first = loop {
        let image = capture(&mut app, &output)
            .try_into_dynamic()
            .unwrap()
            .to_rgb8();
        if image.get_pixel(red.x, red.y).0 == [255, 0, 0]
            && image.get_pixel(93, 62).0 == [255, 0, 255]
        {
            break image;
        }
        assert!(
            Instant::now() < deadline,
            "pixel viewport did not settle: source pixel {:?}, toolbar {:?}",
            image.get_pixel(red.x, red.y).0,
            image.get_pixel(93, 62).0,
        );
    };
    assert_fit(&first, origin, 2);

    // Crossing an integer scale threshold moves the content to the new origin
    // and keeps the same one-pixel source features exactly three pixels wide.
    app.world_mut()
        .entity_mut(canvas)
        .insert(dock_rect(UVec2::new(11, 13), UVec2::new(600, 400)));
    let second = capture(&mut app, &output)
        .try_into_dynamic()
        .unwrap()
        .to_rgb8();
    assert_fit(&second, UVec2::new(23, 21), 3);
    assert_eq!(
        second.get_pixel(93, 62).0,
        [255, 0, 255],
        "resized PX toolbar"
    );

    let world = app.world_mut();
    world.entity_mut(xy).remove::<PreviewRenderCamera>();
    world.entity_mut(perspective).insert(PreviewRenderCamera);
    let perspective = capture(&mut app, &output)
        .try_into_dynamic()
        .unwrap()
        .to_rgb8();
    assert_eq!(perspective.get_pixel(93, 62).0, [255, 0, 255], "3D toolbar");
}

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_dither_anchors_match_editor_viewport_coordinates() {
    let PreviewFixture {
        mut app, root, xy, ..
    } = preview_fixture();
    for pattern in [
        DitherPattern::Bayer(BayerPattern::TwoByTwo),
        DitherPattern::Bayer(BayerPattern::FourByFour),
        DitherPattern::Bayer(BayerPattern::EightByEight),
        DitherPattern::BlueNoise,
    ] {
        assert_dither_anchor(
            &mut app,
            &root,
            xy,
            MaterialInput::EffectPositionXY,
            pattern,
        );
    }
    for anchor in [
        MaterialInput::ScreenPixel,
        MaterialInput::WorldPositionXY,
        MaterialInput::ParticlePositionXY,
    ] {
        assert_dither_anchor(&mut app, &root, xy, anchor, DitherPattern::BlueNoise);
    }
}

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_pixel_lightning_uses_trail_grid_in_the_editor_camera() {
    assert_pixel_strip_grid("pixel_lightning.aestra.ron", 0.45, true);
}

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_pixel_ribbon_uses_trail_grid_in_the_editor_camera() {
    assert_pixel_strip_grid("pixel_ribbon.aestra.ron", 0.95, false);
}

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_pixel_beam_snaps_endpoints_in_the_editor_camera() {
    assert_pixel_strip_grid("pixel_beam.aestra.ron", 0.4, false);
}

fn assert_pixel_strip_grid(file_name: &str, time: f32, warm_trail_history: bool) {
    use aestra_bevy_render::{PresentedEffect, gpu::GpuTrailStatistics};

    let PreviewFixture {
        mut app, root, xy, ..
    } = preview_fixture();
    let effect = aestra_core::EffectAsset::load_ron(root.join("effects").join(file_name)).unwrap();
    let compiled = crate::ProjectEffectCatalog::scan(&root)
        .compile_project(&effect)
        .unwrap();
    let world = app.world_mut();
    let (camera, target) = world
        .query_filtered::<(Entity, &RenderTarget), With<PixelSceneCamera>>()
        .single(world)
        .map(|(entity, target)| (entity, target.as_image().unwrap().clone()))
        .unwrap();
    assert!(world.get::<PixelTrailGrid>(camera).is_some());
    assert!(world.get::<PixelTrailGrid>(xy).is_none());
    let owner = world.spawn(PresentedEffect::new(compiled.root)).id();
    if warm_trail_history {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            app.update();
            let world = app.world();
            let instance = &world.get::<PresentedEffect>(owner).unwrap().instance;
            if world
                .get::<GpuTrailStatistics>(owner)
                .and_then(|stats| stats.usage(instance))
                .is_some_and(|usage| usage.occupied == 1)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "lightning head did not initialize"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    app.world_mut()
        .get_mut::<PresentedEffect>(owner)
        .unwrap()
        .instance
        .seek(time);
    let pixels = |app: &mut App| {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let image = capture(app, &target).try_into_dynamic().unwrap().to_rgb8();
            let visible = image
                .pixels()
                .filter(|pixel| i16::from(pixel[2]) - i16::from(pixel[0]) > 48)
                .count();
            if visible > 70 {
                break image;
            }
            assert!(
                Instant::now() < deadline,
                "expected a visible strip in {file_name}, got {visible} pixels"
            );
        }
    };
    let snapped = pixels(&mut app);
    app.world_mut()
        .entity_mut(camera)
        .remove::<PixelTrailGrid>();
    assert_ne!(pixels(&mut app), snapped);
    app.world_mut().entity_mut(camera).insert(PixelTrailGrid);
    assert_eq!(pixels(&mut app), snapped);
}

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_pixel_stamp_trail_renders_distance_events_in_the_editor_camera() {
    use aestra_bevy_render::PresentedEffect;

    let PreviewFixture { mut app, root, .. } = preview_fixture();
    let effect =
        aestra_core::EffectAsset::load_ron(root.join("effects/pixel_stamp_trail.aestra.ron"))
            .unwrap();
    let compiled = crate::ProjectEffectCatalog::scan(&root)
        .compile_project(&effect)
        .unwrap();
    let world = app.world_mut();
    let (camera, target) = world
        .query_filtered::<(Entity, &RenderTarget), With<PixelSceneCamera>>()
        .single(world)
        .map(|(entity, target)| (entity, target.as_image().unwrap().clone()))
        .unwrap();
    assert!(world.get::<PixelGrid>(camera).is_some());
    let mut player = PresentedEffect::new(compiled.root);
    player.instance.seek(0.9);
    world.spawn(player);
    let stamps = |app: &mut App| {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let image = capture(app, &target).try_into_dynamic().unwrap().to_rgb8();
            if image
                .pixels()
                .filter(|pixel| pixel.0 == [248, 70, 146])
                .count()
                > 80
            {
                break image;
            }
            assert!(
                Instant::now() < deadline,
                "distance events did not produce visible stamps"
            );
        }
    };
    let snapped = stamps(&mut app);
    app.world_mut().entity_mut(camera).remove::<PixelGrid>();
    assert_ne!(stamps(&mut app), snapped);
    app.world_mut().entity_mut(camera).insert(PixelGrid);
    assert_eq!(stamps(&mut app), snapped);
}

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_pixel_dissolve_animates_whole_cells_and_posterizes_color() {
    let PreviewFixture { mut app, root, .. } = preview_fixture();
    let catalog = crate::ProjectEffectCatalog::scan(&root);
    let effect =
        aestra_core::EffectAsset::load_ron(root.join("effects/pixel_dissolve.aestra.ron")).unwrap();
    let compiled = catalog.compile_project(&effect).unwrap();
    let world = app.world_mut();
    let target = world
        .query_filtered::<&RenderTarget, With<PixelSceneCamera>>()
        .single(world)
        .unwrap()
        .as_image()
        .unwrap()
        .clone();
    let deadline = Instant::now() + Duration::from_secs(20);
    let empty = loop {
        let frame = capture(&mut app, &target)
            .try_into_dynamic()
            .unwrap()
            .to_rgb8();
        if frame.get_pixel(95, 63).0 == [255, 0, 0] {
            break frame;
        }
        assert!(
            Instant::now() < deadline,
            "viewport background did not settle"
        );
    };
    let owner = app
        .world_mut()
        .spawn((
            aestra_bevy_render::PresentedEffect::new(compiled.root),
            Transform::default(),
        ))
        .id();
    for (time, expected_coverage) in [(0.125, 256..=256), (0.925, 1..=255), (1.8, 0..=0)] {
        app.world_mut()
            .get_mut::<aestra_bevy_render::PresentedEffect>(owner)
            .unwrap()
            .instance
            .seek(time);
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let image = capture(&mut app, &target)
                .try_into_dynamic()
                .unwrap()
                .to_rgb8();
            let is_visible = |x, y| image.get_pixel(x, y) != empty.get_pixel(x, y);
            let coverage = (56..72)
                .flat_map(|y| (88..104).map(move |x| (x, y)))
                .filter(|&(x, y)| is_visible(x, y))
                .count();
            if expected_coverage.contains(&coverage) {
                for y in (56..72).step_by(4) {
                    for x in (88..104).step_by(4) {
                        let visible = is_visible(x, y);
                        for row in y..y + 4 {
                            for column in x..x + 4 {
                                assert_eq!(
                                    is_visible(column, row),
                                    visible,
                                    "split cell at {time}"
                                );
                                if visible {
                                    for channel in image.get_pixel(column, row).0 {
                                        assert!(
                                            [0, 137, 188, 225, 255]
                                                .into_iter()
                                                .any(|level| channel.abs_diff(level) <= 1),
                                            "channel {channel} was outside the five linear levels"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                break;
            }
            assert!(
                Instant::now() < deadline,
                "coverage {coverage} at time {time} was outside {expected_coverage:?}"
            );
        }
    }
}

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_pixel_outline_follows_the_current_frame_without_atlas_bleed() {
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension},
    };

    let PreviewFixture { mut app, root, .. } = preview_fixture();
    let catalog = crate::ProjectEffectCatalog::scan(&root);
    let effect =
        aestra_core::EffectAsset::load_ron(root.join("effects/pixel_outline.aestra.ron")).unwrap();
    let compiled = catalog.compile_project(&effect).unwrap();

    // Two neighboring 4×4 frames. Frame one's white texel must never outline
    // frame zero's transparent right edge, even though they share the same row.
    let mut pixels = vec![0; 8 * 4 * 4];
    for x in [1, 4] {
        let start = (8 + x) * 4;
        pixels[start..start + 4].fill(255);
    }
    let image = Image::new(
        Extent3d {
            width: 8,
            height: 4,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    let world = app.world_mut();
    let handle = world.resource_mut::<Assets<Image>>().add(image);
    let target = world
        .query_filtered::<&RenderTarget, With<PixelSceneCamera>>()
        .single(world)
        .unwrap()
        .as_image()
        .unwrap()
        .clone();
    let deadline = Instant::now() + Duration::from_secs(20);
    let empty = loop {
        let frame = capture(&mut app, &target)
            .try_into_dynamic()
            .unwrap()
            .to_rgb8();
        if frame.get_pixel(95, 63).0 == [255, 0, 0] {
            break frame;
        }
        assert!(
            Instant::now() < deadline,
            "viewport background did not settle"
        );
    };
    let pulse = aestra_bevy_render::PresentedEffect::new(compiled.root)
        .with_texture_overrides([(effect.assets[0].id, handle)].into());
    let owner = app.world_mut().spawn((pulse, Transform::default())).id();

    for (time, solid_x, border_x) in [(0.125, 88, 104), (0.625, 72, 88)] {
        app.world_mut()
            .get_mut::<aestra_bevy_render::PresentedEffect>(owner)
            .unwrap()
            .instance
            .seek(time);
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let frame = capture(&mut app, &target)
                .try_into_dynamic()
                .unwrap()
                .to_rgb8();
            let outline = frame.get_pixel(border_x, 55).0;
            if frame.get_pixel(solid_x, 55).0 == [255, 255, 255]
                && outline
                    .into_iter()
                    .zip([48, 245, 194])
                    .all(|(v, e)| v.abs_diff(e) <= 1)
            {
                assert_eq!(
                    frame.get_pixel(120, 55),
                    empty.get_pixel(120, 55),
                    "atlas edge at {time}"
                );
                assert_eq!(
                    frame.get_pixel(border_x, 71),
                    empty.get_pixel(border_x, 71),
                    "diagonal at {time}"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "frame {time} did not render its solid texel and one-texel outline (outline = {outline:?})"
            );
        }
    }
}

#[test]
#[ignore = "requires native GPU and shader compilation"]
fn native_pattern_dither_wraps_an_authored_tile_and_bypasses_an_empty_slot() {
    use aestra_compiler::{MaterialCompiler, MaterialGraphCreateKind, MaterialGraphFunction};
    use aestra_core::material::{MaterialExpressionKind, MaterialParameterValue, MaterialValue};
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension},
    };

    let PreviewFixture { mut app, root, .. } = preview_fixture();
    let catalog = crate::ProjectEffectCatalog::scan(&root);
    let effect =
        aestra_core::EffectAsset::load_ron(root.join("effects/pixel_pulse.aestra.ron")).unwrap();
    let mut compiled = catalog.compile_project(&effect).unwrap();
    let effect = std::sync::Arc::make_mut(&mut compiled.root);
    let program = &mut effect.material_programs[0];
    let bayer = program
        .expressions
        .iter()
        .find(|node| node.kind.bayer_pattern().is_some())
        .unwrap();
    let old_node = bayer.id;
    let MaterialExpressionKind::BayerDither4x4 { alpha, .. } = bayer.kind else {
        panic!("expected authored Bayer node");
    };
    let plan = MaterialCompiler
        .plan_graph_node_creation(
            program,
            MaterialGraphCreateKind::Function(MaterialGraphFunction::PatternDither),
            Some(alpha),
        )
        .unwrap();
    *program = plan.replacement;
    let pattern = program.expressions.pop().unwrap();
    let MaterialExpressionKind::PatternDither { cell_size, .. } = pattern.kind else {
        panic!("expected custom pattern node");
    };
    for node in &mut program.expressions {
        if node.id == old_node {
            node.kind = pattern.kind.clone();
        }
        if node.id == cell_size {
            node.kind = MaterialExpressionKind::Constant(MaterialValue::Float(2.0));
        }
    }
    let parameter = program.parameters.last().unwrap().id;
    let coverage = program
        .parameters
        .iter()
        .find(|p| p.name == "Dither Coverage")
        .unwrap()
        .id;
    effect.material_instances[0].values.insert(
        coverage,
        MaterialParameterValue::Constant(MaterialValue::Float(0.5)),
    );

    let world = app.world_mut();
    let target = world
        .query_filtered::<&RenderTarget, With<PixelSceneCamera>>()
        .single(world)
        .unwrap()
        .as_image()
        .unwrap()
        .clone();
    let mut pulse = aestra_bevy_render::PresentedEffect::new(compiled.root.clone());
    pulse.instance.seek(0.125);
    let owner = world.spawn((pulse, Transform::default())).id();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let frame = capture(&mut app, &target)
            .try_into_dynamic()
            .unwrap()
            .to_rgb8();
        if (120..235).contains(&frame.get_pixel(94, 61).0[0]) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "empty pattern did not preserve half opacity"
        );
    }
    app.world_mut().despawn(owner);

    let ranks = [[0u8, 175, 64], [255, 127, 240]];
    let image = Image::new(
        Extent3d {
            width: 3,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        ranks
            .into_iter()
            .flatten()
            .flat_map(|rank| [255, 255, 255, rank])
            .collect(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    let asset = aestra_core::AssetId::new();
    let handle = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    std::sync::Arc::make_mut(&mut compiled.root).material_instances[0]
        .values
        .insert(
            parameter,
            MaterialParameterValue::Constant(MaterialValue::Texture2D(asset)),
        );
    let mut pulse = aestra_bevy_render::PresentedEffect::new(compiled.root)
        .with_texture_overrides([(asset, handle)].into());
    pulse.instance.seek(0.125);
    let owner = app.world_mut().spawn((pulse, Transform::default())).id();
    for shift in [0, 3] {
        app.world_mut()
            .get_mut::<Transform>(owner)
            .unwrap()
            .translation
            .x = shift as f32;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let frame = capture(&mut app, &target)
                .try_into_dynamic()
                .unwrap()
                .to_rgb8();
            let matches = (60i32..68).all(|y| {
                (92i32..100).all(|x| {
                    let tile_x = (x - 96).div_euclid(2).rem_euclid(3) as usize;
                    let tile_y = (63 - y).div_euclid(2).rem_euclid(2) as usize;
                    let kept = ranks[tile_y][tile_x] < 128;
                    let rose = frame
                        .get_pixel(x as u32 + shift, y as u32)
                        .0
                        .into_iter()
                        .zip([248, 70, 146])
                        .all(|(channel, expected)| channel.abs_diff(expected) <= 1);
                    rose == kept
                })
            });
            if matches {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "custom tile did not match at shift {shift}"
            );
        }
    }
}

fn assert_dither_anchor(
    app: &mut App,
    root: &std::path::Path,
    camera: Entity,
    anchor: MaterialInput,
    pattern: DitherPattern,
) {
    use aestra_core::material::{MaterialExpressionKind, MaterialParameterValue, MaterialValue};
    const COVERAGE: f32 = 0.30;

    let catalog = crate::ProjectEffectCatalog::scan(root);
    let mut effect =
        aestra_core::EffectAsset::load_ron(root.join("effects/pixel_pulse.aestra.ron")).unwrap();
    for module in &mut effect.emitters[0].modules {
        if let ModuleParameters::Initialize {
            speed, direction, ..
        } = &mut module.parameters
        {
            *speed = ScalarRange::new(10.0, 10.0);
            *direction = [1.0, 0.0, 0.0];
        }
    }
    let mut compiled = catalog.compile_project(&effect).unwrap();
    let effect = std::sync::Arc::make_mut(&mut compiled.root);
    let program = &mut effect.material_programs[0];
    let coverage = program
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Dither Coverage")
        .unwrap()
        .id;
    effect.material_instances[0].values.insert(
        coverage,
        MaterialParameterValue::Constant(MaterialValue::Float(COVERAGE)),
    );
    let node = program
        .expressions
        .iter_mut()
        .find(|node| node.kind.bayer_pattern().is_some())
        .unwrap();
    let MaterialExpressionKind::BayerDither4x4 {
        alpha,
        position,
        cell_size,
    } = node.kind
    else {
        panic!("expected authored 4×4 Bayer node");
    };
    node.kind = MaterialExpressionKind::dither(pattern, alpha, position, cell_size);
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == position)
        .unwrap()
        .kind = MaterialExpressionKind::Input(anchor);

    let world = app.world_mut();
    let target = world
        .query_filtered::<&RenderTarget, With<PixelSceneCamera>>()
        .single(world)
        .unwrap()
        .as_image()
        .unwrap()
        .clone();
    let owner = world
        .spawn((
            aestra_bevy_render::PresentedEffect::new(compiled.root),
            Transform::default(),
        ))
        .id();
    for (time, effect_x, particle_x, camera_x, swatch) in [
        (0.1, 5.0, 1.0, 1.0, [248, 70, 146]),
        (0.3, 8.0, 3.0, -2.0, [240, 190, 80]),
    ] {
        let world = app.world_mut();
        world.get_mut::<Transform>(owner).unwrap().translation.x = effect_x;
        world.get_mut::<Transform>(camera).unwrap().translation.x = camera_x;
        world
            .get_mut::<aestra_bevy_render::PresentedEffect>(owner)
            .unwrap()
            .instance
            .seek(time);
        let center = (VIRTUAL_SIZE.x as f32 * 0.5 + effect_x + particle_x - camera_x) as u32;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let image = capture(app, &target).try_into_dynamic().unwrap().to_rgb8();
            let matches_mask = (60..68).all(|y| {
                (center - 4..center + 4).all(|x| {
                    let screen = [x as f32 + 0.5, y as f32 + 0.5];
                    let world = [screen[0] - 96.0 + camera_x, 64.0 - screen[1]];
                    let sample = match anchor {
                        MaterialInput::ScreenPixel => screen,
                        MaterialInput::WorldPositionXY => world,
                        MaterialInput::EffectPositionXY => [world[0] - effect_x, world[1]],
                        MaterialInput::ParticlePositionXY => {
                            [world[0] - effect_x - particle_x, world[1]]
                        }
                        _ => unreachable!("only dither coordinate inputs are tested"),
                    };
                    let kept = pattern.sample(COVERAGE, sample, 1.0) == 1.0;
                    let visible = image
                        .get_pixel(x, y)
                        .0
                        .into_iter()
                        .zip(swatch)
                        .all(|(channel, expected)| channel.abs_diff(expected) <= 1);
                    visible == kept
                })
            });
            if matches_mask {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "editor {anchor:?} {pattern:?} mask did not match at time {time}"
            );
        }
    }
    app.world_mut().despawn(owner);
}

fn dock_rect(origin: UVec2, size: UVec2) -> (ComputedNode, UiGlobalTransform) {
    (
        ComputedNode {
            size: size.as_vec2(),
            ..default()
        },
        UiGlobalTransform::from(Affine2::from_translation(
            origin.as_vec2() + size.as_vec2() * 0.5,
        )),
    )
}

fn assert_fit(image: &image::RgbImage, origin: UVec2, factor: u32) {
    for (texel, color) in [
        (UVec2::new(95, 63), [255, 0, 0]),
        (UVec2::new(96, 63), [0, 255, 0]),
        (UVec2::ZERO, [0, 255, 255]),
        (VIRTUAL_SIZE - UVec2::ONE, [255, 255, 0]),
    ] {
        let min = origin + texel * factor;
        let max = min + UVec2::splat(factor);
        for y in min.y..max.y {
            for x in min.x..max.x {
                assert_eq!(image.get_pixel(x, y).0, color, "source texel {texel}");
            }
        }
        for p in [
            min - UVec2::X,
            min - UVec2::Y,
            UVec2::new(max.x, max.y - 1),
            UVec2::new(max.x - 1, max.y),
        ] {
            assert_ne!(
                image.get_pixel(p.x, p.y).0,
                color,
                "source texel {texel} spilled"
            );
        }
    }
}

fn capture(app: &mut App, target: &Handle<Image>) -> Image {
    for _ in 0..6 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |capture: On<ScreenshotCaptured>, mut output: ResMut<Capture>| {
                output.0 = Some(capture.image.clone());
            },
        );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.update();
        if let Some(image) = app.world_mut().resource_mut::<Capture>().0.take() {
            return image;
        }
        assert!(Instant::now() < deadline, "no pixel preview capture");
        std::thread::sleep(Duration::from_millis(10));
    }
}
