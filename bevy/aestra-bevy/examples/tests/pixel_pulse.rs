use super::*;
use bevy::{
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    window::ExitCondition,
};
use std::time::{Duration, Instant};

const AMBER_SWATCH: [u8; 3] = [240, 190, 80];
const ROSE_SWATCH: [u8; 3] = [248, 70, 146];

fn matches_swatch(pixel: [u8; 3], swatch: [u8; 3]) -> bool {
    pixel
        .into_iter()
        .zip(swatch)
        .all(|(a, b)| a.abs_diff(b) <= 1)
}

#[test]
fn sample_project_resolves_the_nearest_filtered_flipbook() {
    use aestra_core::material::*;

    let project = compile_pulse(false);
    let effect = &project.root;
    assert_eq!(effect.flipbooks[0].frames.len(), 2);
    assert_eq!(
        effect.material_programs[0].parameters[0].value_type,
        MaterialValueType::Texture2D(MaterialTextureDescriptor {
            color_space: MaterialTextureColorSpace::SrgbColor,
            sampler: MaterialSamplerDescriptor {
                filter: MaterialFilterMode::Nearest,
                mip_filter: MaterialMipFilterMode::None,
                ..default()
            },
        })
    );
}

#[derive(Resource, Default)]
struct Capture(Option<Image>);

fn capture_app(project: Arc<CompiledEffectProject>) -> App {
    capture_app_with_sample(project, false)
}

fn capture_app_with_sample(project: Arc<CompiledEffectProject>, path: bool) -> App {
    let mut app = App::new();
    app.insert_resource(PulseProject(project))
        .insert_resource(settings())
        .init_resource::<Capture>()
        .add_plugins(
            DefaultPlugins
                .set(asset_plugin())
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<bevy::log::LogPlugin>()
                .disable::<bevy::winit::WinitPlugin>(),
        )
        .add_plugins(AestraPlugin)
        .add_systems(
            PostUpdate,
            fit_output
                .after(CameraUpdateSystems)
                .before(TransformSystems::Propagate),
        );
    if path {
        app.add_systems(Startup, (spawn_cameras, spawn_path_effect).chain());
    } else {
        app.add_systems(Startup, (spawn_cameras, spawn_layers));
    }
    let start = Instant::now();
    while app.plugins_state() != bevy::app::PluginsState::Ready {
        bevy::tasks::tick_global_task_pools_on_main_thread();
        assert!(start.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(10));
    }
    app.finish();
    app.cleanup();

    app.update();
    app
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn pixel_lightning_switches_trail_placement_at_the_same_playback_frame() {
    let mut app = capture_app_with_sample(compile_path_sample("pixel_lightning.aestra.ron"), true);
    let world = app.world_mut();
    let (camera, target) = world
        .query_filtered::<(Entity, &RenderTarget), With<PixelCamera>>()
        .single(world)
        .map(|(camera, target)| (camera, target.as_image().unwrap().clone()))
        .unwrap();
    let owner = world
        .query_filtered::<Entity, With<EffectPlayer>>()
        .single(world)
        .unwrap();
    world.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::from_secs_f64(1.0 / 60.0),
    ));
    world.get_mut::<EffectPlayer>(owner).unwrap().playing = false;
    // Keep the initial head alive while the compute pipelines load. The manual clock
    // otherwise plays almost the entire burst before the render world has compiled them.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        app.update();
        let world = app.world();
        let instance = &world.get::<PresentedEffect>(owner).unwrap().instance;
        if world
            .get::<aestra_bevy_render::gpu::GpuTrailStatistics>(owner)
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
    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .playing = true;
    for _ in 0..60 {
        if app.world().get::<EffectPlayer>(owner).unwrap().frame() >= 27 {
            break;
        }
        app.update();
    }
    let mut player = app.world_mut().get_mut::<EffectPlayer>(owner).unwrap();
    assert_eq!(player.frame(), 27);
    player.playing = false;
    let image = |app: &mut App| {
        let image = capture(app, &target).try_into_dynamic().unwrap().to_rgb8();
        let visible = image
            .pixels()
            .filter(|pixel| i16::from(pixel[2]) - i16::from(pixel[0]) > 48)
            .count();
        assert!(
            visible > 70,
            "expected a visible lightning trail, got {visible} pixels"
        );
        image
    };
    let snapped = image(&mut app);
    let history_epoch = app
        .world()
        .get::<PresentedEffect>(owner)
        .unwrap()
        .instance
        .history_epoch();

    app.world_mut()
        .entity_mut(camera)
        .remove::<PixelTrailGrid>();
    let smooth = image(&mut app);
    assert_ne!(smooth, snapped);
    app.world_mut().entity_mut(camera).insert(PixelTrailGrid);
    assert_eq!(image(&mut app), snapped);
    assert_eq!(app.world().get::<EffectPlayer>(owner).unwrap().frame(), 27);
    assert_eq!(
        app.world()
            .get::<PresentedEffect>(owner)
            .unwrap()
            .instance
            .history_epoch(),
        history_epoch
    );
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn pixel_ribbon_keeps_segments_joined_when_the_camera_grid_changes() {
    assert_strip_grid("pixel_ribbon.aestra.ron", 57, 65..145);
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn pixel_beam_spans_its_endpoints_in_both_camera_modes() {
    assert_strip_grid("pixel_beam.aestra.ron", 24, 37..155);
}

fn assert_strip_grid(file_name: &str, playback_frame: u64, columns: std::ops::Range<u32>) {
    let mut app = capture_app_with_sample(compile_path_sample(file_name), true);
    let world = app.world_mut();
    let (camera, target) = world
        .query_filtered::<(Entity, &RenderTarget), With<PixelCamera>>()
        .single(world)
        .map(|(camera, target)| (camera, target.as_image().unwrap().clone()))
        .unwrap();
    let owner = world
        .query_filtered::<Entity, With<EffectPlayer>>()
        .single(world)
        .unwrap();
    let mut player = world.get_mut::<EffectPlayer>(owner).unwrap();
    player.playing = false;
    player.seek_frame(playback_frame);

    let image = |app: &mut App| {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let image = capture(app, &target).try_into_dynamic().unwrap().to_rgb8();
            if image
                .pixels()
                .any(|pixel| pixel[2].saturating_sub(pixel[0]) > 48)
            {
                for x in columns.clone() {
                    assert!(
                        (0..image.height()).any(|y| {
                            let pixel = image.get_pixel(x, y);
                            pixel[2].saturating_sub(pixel[0]) > 48
                        }),
                        "gap in {file_name} at column {x}"
                    );
                }
                break image;
            }
            assert!(Instant::now() < deadline, "{file_name} did not render");
        }
    };
    let snapped = image(&mut app);
    app.world_mut()
        .entity_mut(camera)
        .remove::<PixelTrailGrid>();
    assert_ne!(image(&mut app), snapped);
    app.world_mut().entity_mut(camera).insert(PixelTrailGrid);
    assert_eq!(image(&mut app), snapped);
    assert_eq!(
        app.world().get::<EffectPlayer>(owner).unwrap().frame(),
        playback_frame
    );
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn pixel_stamp_trail_deposits_discrete_stationary_sprites() {
    let mut app =
        capture_app_with_sample(compile_path_sample("pixel_stamp_trail.aestra.ron"), true);
    let world = app.world_mut();
    let (camera, target) = world
        .query_filtered::<(Entity, &RenderTarget), With<PixelCamera>>()
        .single(world)
        .map(|(camera, target)| (camera, target.as_image().unwrap().clone()))
        .unwrap();
    let owner = world
        .query_filtered::<Entity, With<EffectPlayer>>()
        .single(world)
        .unwrap();
    let mut player = world.get_mut::<EffectPlayer>(owner).unwrap();
    player.playing = false;
    player.set_playback_time(0.9);
    let frame = |app: &mut App| {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let image = capture(app, &target).try_into_dynamic().unwrap().to_rgb8();
            let columns = (0..image.width()).map(|x| {
                (0..image.height()).any(|y| matches_swatch(image.get_pixel(x, y).0, ROSE_SWATCH))
            });
            let runs = std::iter::once(false)
                .chain(columns.clone())
                .zip(columns)
                .filter(|&(before, after)| !before && after)
                .count();
            if runs >= 8 {
                break image;
            }
            assert!(
                Instant::now() < deadline,
                "expected separate stamps, got {runs} visible runs"
            );
        }
    };
    let snapped = frame(&mut app);
    app.world_mut().entity_mut(camera).remove::<PixelGrid>();
    assert_ne!(frame(&mut app), snapped);
    app.world_mut().entity_mut(camera).insert(PixelGrid);
    assert_eq!(frame(&mut app), snapped);

    app.world_mut()
        .get_mut::<EffectPlayer>(owner)
        .unwrap()
        .set_playback_time(1.1);
    let later = frame(&mut app);
    for x in 75..115 {
        for y in 0..snapped.height() {
            assert_eq!(
                snapped.get_pixel(x, y),
                later.get_pixel(x, y),
                "stamps moved with the head"
            );
        }
    }
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn stateful_pulse_holds_geometry_while_gpu_ticks_advance() {
    let mut app = capture_app(compile_pulse(true));
    let world = app.world_mut();
    let target = world
        .query_filtered::<&RenderTarget, With<PixelCamera>>()
        .single(world)
        .unwrap()
        .as_image()
        .unwrap()
        .clone();
    let owner = world
        .query_filtered::<Entity, With<MovablePulse>>()
        .single(world)
        .unwrap();
    world
        .query::<&mut EffectPlayer>()
        .iter_mut(world)
        .for_each(|mut player| player.playing = false);
    let diagnostics = world
        .resource::<aestra_bevy::gpu::GpuPlaybackDiagnostics>()
        .clone();
    diagnostics.set_enabled(true);
    assert_ne!(
        world.get::<EffectPlayer>(owner).unwrap().seek_mode(),
        aestra_runtime::SimulationSeekMode::StatelessDirect
    );
    let sample = |app: &mut App, tick: u32, cadence| {
        let mut player = app.world_mut().get_mut::<EffectPlayer>(owner).unwrap();
        player.set_presentation_cadence(cadence);
        player.seek_frame(u64::from(tick));
        let frame = capture(app, &target).try_into_dynamic().unwrap().to_rgb8();
        let encoded = diagnostics
            .snapshot()
            .samples
            .into_iter()
            .find(|sample| sample.owner == owner)
            .unwrap();
        assert_eq!(encoded.encoded_time, Some(tick as f32 / 60.0));
        frame
    };
    let cadence = Some(PresentationCadence::FPS_12);
    let boundary = sample(&mut app, 45, cadence);
    let held = sample(&mut app, 49, cadence);
    assert_eq!(held, boundary);
    let smooth = sample(&mut app, 49, None);
    assert_ne!(smooth, held);
    assert_eq!(
        sample(&mut app, 49, cadence),
        smooth,
        "mid-frame enable seeds from the current pose"
    );
    let next = sample(&mut app, 50, cadence);
    assert_ne!(next, smooth);
    assert_eq!(
        sample(&mut app, 47, cadence),
        boundary,
        "a real seek reconstructs the sampled boundary"
    );
    assert_eq!(
        sample(&mut app, 50, cadence),
        next,
        "skipping display frames preserves the sample"
    );
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn host_sort_policy_changes_overlap_without_changing_effect_z() {
    let mut app = capture_app(compile_pulse(false));
    let world = app.world_mut();
    let target = world
        .query_filtered::<&RenderTarget, With<PixelCamera>>()
        .single(world)
        .unwrap()
        .as_image()
        .unwrap()
        .clone();
    let owner = world
        .query_filtered::<Entity, With<MovablePulse>>()
        .single(world)
        .unwrap();
    let behind_z = BACKGROUND_Z - 1.0;
    let mut players = world.query::<(&mut EffectPlayer, &mut Transform)>();
    for (mut player, mut transform) in players.iter_mut(world) {
        player.playing = false;
        player.seek(0.125);
        transform.translation = Vec3::new(0.0, 0.0, behind_z);
    }
    let pixel = |app: &mut App| {
        capture(app, &target)
            .try_into_dynamic()
            .unwrap()
            .to_rgb8()
            .get_pixel(108, 64)
            .0
    };
    let hidden = pixel(&mut app);
    assert!(hidden[1] > hidden[0] && hidden[1] > hidden[2]);

    app.world_mut()
        .entity_mut(owner)
        .insert(AestraSort2d::Custom(FOREGROUND_Z + 1.0));
    let in_front = pixel(&mut app);
    assert!(matches_swatch(in_front, ROSE_SWATCH), "got {in_front:?}");

    app.world_mut()
        .entity_mut(owner)
        .insert(AestraSort2d::WorldY { units_per_y: 16.0 });
    assert_eq!(pixel(&mut app), hidden);
    app.world_mut()
        .get_mut::<Transform>(owner)
        .unwrap()
        .translation
        .y = -1.0;
    assert_eq!(pixel(&mut app), in_front);

    app.world_mut().entity_mut(owner).remove::<AestraSort2d>();
    assert_eq!(pixel(&mut app), hidden);
    assert_eq!(
        app.world().get::<Transform>(owner).unwrap().translation.z,
        behind_z
    );
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn overlapping_pulses_follow_host_sprite_z_and_preserve_nearest_texels() {
    let mut app = capture_app(compile_pulse(false));
    let world = app.world_mut();
    let (camera, target) = world
        .query_filtered::<(Entity, &RenderTarget), With<PixelCamera>>()
        .single(world)
        .map(|(camera, target)| {
            let RenderTarget::Image(target) = target else {
                panic!("expected the virtual image target");
            };
            (camera, target.handle.clone())
        })
        .unwrap();
    world.entity_mut(camera).remove::<PixelGrid>();
    let mut players = world.query::<(&mut EffectPlayer, &mut Transform, Has<MovablePulse>)>();
    for (mut player, mut transform, primary) in players.iter_mut(world) {
        player.playing = false;
        player.seek(if primary { 0.125 } else { 0.375 });
        transform.translation.x = 0.0;
    }

    let start = Instant::now();
    let behind = loop {
        let image = capture(&mut app, &target)
            .try_into_dynamic()
            .unwrap()
            .to_rgb8();
        let amber = image.get_pixel(96, 64).0;
        if matches_swatch(amber, AMBER_SWATCH) {
            break image;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "expected amber palette swatch, got {amber:?}"
        );
    };
    let green = behind.get_pixel(108, 64).0;
    assert!(
        green[1] > green[0] && green[1] > green[2],
        "foreground: {green:?}"
    );

    let world = app.world_mut();
    for (_, mut transform, primary) in players.iter_mut(world) {
        transform.translation.z = if primary {
            FOREGROUND_Z + 1.0
        } else {
            BACKGROUND_Z - 1.0
        };
    }
    let in_front = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .to_rgb8();
    let pink = in_front.get_pixel(108, 64).0;
    assert!(
        matches_swatch(pink, ROSE_SWATCH),
        "expected rose palette swatch, got {pink:?}"
    );
    assert_eq!(in_front.get_pixel(96, 64).0, pink);
    assert_eq!(
        in_front.get_pixel(72, 64).0,
        pink,
        "full texel exactly at the edge"
    );
    assert_eq!(
        in_front.get_pixel(71, 64),
        behind.get_pixel(52, 64),
        "adjacent pixel is background with no filtered transition"
    );

    let world = app.world_mut();
    for (_, mut transform, _) in players.iter_mut(world) {
        transform.translation.z = BACKGROUND_Z - 1.0;
    }
    let occluded = capture(&mut app, &target)
        .try_into_dynamic()
        .unwrap()
        .to_rgb8();
    assert_eq!(occluded.get_pixel(96, 64), occluded.get_pixel(52, 64));
    assert_eq!(occluded.get_pixel(108, 64).0, green);

    let world = app.world_mut();
    for (_, mut transform, primary) in players.iter_mut(world) {
        if primary {
            transform.translation.z = FOREGROUND_Z + 1.0;
            transform.scale.x = 1.03;
        }
    }
    // The stretched sprite's left texel edge crosses a pixel center with a
    // small camera pan. Snap the sprite after projection to keep that edge put.
    let capture_pan = |app: &mut App, right: f32| {
        app.world_mut()
            .get_mut::<Transform>(camera)
            .unwrap()
            .translation
            .x = -right;
        capture(app, &target).try_into_dynamic().unwrap().to_rgb8()
    };
    let smooth_a = capture_pan(&mut app, 0.1);
    let smooth_b = capture_pan(&mut app, 0.4);
    assert_ne!(smooth_a.get_pixel(71, 64), smooth_b.get_pixel(71, 64));
    app.world_mut().entity_mut(camera).insert(PixelGrid);
    let snapped_a = capture_pan(&mut app, 0.1);
    let snapped_b = capture_pan(&mut app, 0.4);
    let snapped_next = capture_pan(&mut app, 0.75);
    assert_eq!(snapped_a.get_pixel(72, 64).0, pink);
    for x in 64..90 {
        assert_eq!(snapped_a.get_pixel(x, 64), snapped_b.get_pixel(x, 64));
        assert_eq!(
            snapped_a.get_pixel(x, 64),
            snapped_next.get_pixel(x + 1, 64)
        );
    }

    // Drive the example's real compositor at an odd output size. Each virtual
    // pixel should produce a solid 2x2 block with a sharp letterbox boundary.
    let world = app.world_mut();
    let output_size = UVec2::new(405, 281);
    let output = world
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            output_size.x,
            output_size.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
    let output_camera = world
        .query_filtered::<Entity, With<OutputCamera>>()
        .single(world)
        .unwrap();
    world
        .entity_mut(output_camera)
        .insert(RenderTarget::Image(output.clone().into()));
    let upscaled = capture(&mut app, &output)
        .try_into_dynamic()
        .unwrap()
        .to_rgb8();
    let origin = UVec2::new(10, 12);
    assert_eq!(upscaled.get_pixel(origin.x - 1, origin.y).0, [0; 3]);
    assert_eq!(upscaled.get_pixel(origin.x, origin.y - 1).0, [0; 3]);
    for y in [0, 64, 127] {
        for x in 0..VIRTUAL_SIZE.x {
            for dy in 0..2 {
                for dx in 0..2 {
                    let p = origin + UVec2::new(2 * x + dx, 2 * y + dy);
                    assert_eq!(upscaled.get_pixel(p.x, p.y), snapped_next.get_pixel(x, y));
                }
            }
        }
    }

    let world = app.world_mut();
    let primary = world
        .query_filtered::<Entity, With<MovablePulse>>()
        .single(world)
        .unwrap();
    let sample = |app: &mut App, tick: u64, cadence| {
        let mut player = app.world_mut().get_mut::<EffectPlayer>(primary).unwrap();
        player.set_presentation_cadence(cadence);
        player.seek_frame(tick);
        let frame = capture(app, &target).try_into_dynamic().unwrap().to_rgb8();
        assert_eq!(
            app.world().get::<EffectPlayer>(primary).unwrap().frame(),
            tick
        );
        frame
    };
    let cadence = Some(PresentationCadence::FPS_12);
    let frame_start = sample(&mut app, 45, None);
    let stepped = sample(&mut app, 47, cadence);
    let held = sample(&mut app, 49, cadence);
    assert_eq!(stepped, frame_start);
    assert_eq!(held, stepped);
    assert_ne!(sample(&mut app, 49, None), held);
    assert_ne!(sample(&mut app, 50, cadence), held);
    assert_eq!(sample(&mut app, 47, cadence), held, "backward seek");
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn palette_ramp_steps_through_swatches_and_clamps_outside_its_range() {
    use aestra_core::{
        MaterialExpressionId,
        material::{
            MaterialExpression, MaterialExpressionKind as Kind, MaterialInput, MaterialValue,
        },
    };

    let mut project = compile_pulse(false);
    let effect = Arc::get_mut(&mut Arc::get_mut(&mut project).unwrap().root).unwrap();
    let program = &mut effect.material_programs[0];
    let time = MaterialExpressionId::new();
    let offset = MaterialExpressionId::new();
    let factor = MaterialExpressionId::new();
    program.expressions.extend([
        MaterialExpression {
            id: time,
            kind: Kind::Input(MaterialInput::EffectTime),
        },
        MaterialExpression {
            id: offset,
            kind: Kind::Constant(MaterialValue::Float(0.25)),
        },
        MaterialExpression {
            id: factor,
            kind: Kind::Subtract(time, offset),
        },
    ]);
    let output = program
        .expressions
        .iter_mut()
        .find(|expression| expression.id == program.outputs.color)
        .unwrap();
    let Kind::NearestPaletteColor { palette, .. } = output.kind else {
        panic!("Pixel Pulse must bind its project palette");
    };
    output.kind = Kind::PaletteRamp { palette, factor };

    capture_palette_sequence(
        project,
        &[
            (0.125, AMBER_SWATCH),  // Factor below zero.
            (0.75, ROSE_SWATCH),    // Middle band.
            (1.25, [244, 102, 63]), // Factor exactly one.
            (1.5, [244, 102, 63]),  // Factor above one.
        ],
    );
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn palette_shift_cycles_and_wraps_signed_whole_steps() {
    use aestra_core::{
        MaterialExpressionId,
        material::{
            MaterialExpression, MaterialExpressionKind as Kind, MaterialInput, MaterialValue,
        },
    };

    let mut project = compile_pulse(false);
    let effect = Arc::get_mut(&mut Arc::get_mut(&mut project).unwrap().root).unwrap();
    let program = &mut effect.material_programs[0];
    let time = MaterialExpressionId::new();
    let rate = MaterialExpressionId::new();
    let scaled_time = MaterialExpressionId::new();
    let phase = MaterialExpressionId::new();
    let steps = MaterialExpressionId::new();
    let color = MaterialExpressionId::new();
    let [red, green, blue] = AMBER_SWATCH.map(|value| f32::from(value) / 255.0);
    program.expressions.extend([
        MaterialExpression {
            id: time,
            kind: Kind::Input(MaterialInput::EffectTime),
        },
        MaterialExpression {
            id: rate,
            kind: Kind::Constant(MaterialValue::Float(4.0)),
        },
        MaterialExpression {
            id: scaled_time,
            kind: Kind::Multiply(time, rate),
        },
        MaterialExpression {
            id: phase,
            kind: Kind::Constant(MaterialValue::Float(3.0)),
        },
        MaterialExpression {
            id: steps,
            kind: Kind::Subtract(scaled_time, phase),
        },
        MaterialExpression {
            id: color,
            kind: Kind::Constant(MaterialValue::ColorSrgb([red, green, blue, 1.0])),
        },
    ]);
    let output = program
        .expressions
        .iter_mut()
        .find(|expression| expression.id == program.outputs.color)
        .unwrap();
    let Kind::NearestPaletteColor { palette, .. } = output.kind else {
        panic!("Pixel Pulse must bind its project palette");
    };
    output.kind = Kind::PaletteShift {
        palette,
        color,
        steps,
    };

    capture_palette_sequence(
        project,
        &[
            (0.25, ROSE_SWATCH),    // Two steps backward wraps to the middle swatch.
            (0.50, [244, 102, 63]), // One step backward wraps to the final swatch.
            (0.625, AMBER_SWATCH),  // A negative fractional step truncates toward zero.
            (1.00, ROSE_SWATCH),    // One step forward.
            (1.25, [244, 102, 63]), // Two steps forward.
            (1.50, AMBER_SWATCH),   // The full palette wraps back to the first swatch.
        ],
    );
}

fn capture_palette_sequence(project: Arc<CompiledEffectProject>, samples: &[(f32, [u8; 3])]) {
    let mut app = capture_app(project);
    let world = app.world_mut();
    let target = world
        .query_filtered::<&RenderTarget, With<PixelCamera>>()
        .single(world)
        .unwrap()
        .as_image()
        .unwrap()
        .clone();
    let mut players = world.query::<(&mut EffectPlayer, &mut Transform, Has<MovablePulse>)>();
    for (mut player, mut transform, primary) in players.iter_mut(world) {
        player.playing = false;
        player.set_presentation_cadence(None);
        transform.translation.x = if primary { 0.0 } else { 200.0 };
    }
    for &(time, swatch) in samples {
        let world = app.world_mut();
        world
            .query_filtered::<&mut EffectPlayer, With<MovablePulse>>()
            .single_mut(world)
            .unwrap()
            .seek(time);
        let pixel = capture(&mut app, &target)
            .try_into_dynamic()
            .unwrap()
            .to_rgb8()
            .get_pixel(96, 64)
            .0;
        assert!(
            matches_swatch(pixel, swatch),
            "time {time}: expected {swatch:?}, got {pixel:?}"
        );
    }
}

#[test]
#[ignore = "native GPU image test; run explicitly"]
fn dither_anchors_match_bevy_view_coordinates() {
    for pattern in [
        DitherPattern::Bayer(BayerPattern::TwoByTwo),
        DitherPattern::Bayer(BayerPattern::FourByFour),
        DitherPattern::Bayer(BayerPattern::EightByEight),
        DitherPattern::BlueNoise,
    ] {
        assert_dither_anchor(MaterialInput::EffectPositionXY, pattern);
    }
    for anchor in [
        MaterialInput::ScreenPixel,
        MaterialInput::WorldPositionXY,
        MaterialInput::ParticlePositionXY,
    ] {
        assert_dither_anchor(anchor, DitherPattern::BlueNoise);
    }
}

fn assert_dither_anchor(anchor: MaterialInput, pattern: DitherPattern) {
    use aestra_core::{ModuleParameters, ScalarRange};

    const COVERAGE: f32 = 0.30;
    let mut effect =
        EffectAsset::load_ron(asset_root().join("effects/pixel_pulse.aestra.ron")).unwrap();
    for module in &mut effect.emitters[0].modules {
        if let ModuleParameters::Initialize {
            speed, direction, ..
        } = &mut module.parameters
        {
            *speed = ScalarRange::new(10.0, 10.0);
            *direction = [1.0, 0.0, 0.0];
        }
    }
    let mut project = compile_sample_effect(&effect);
    enable_dither(&mut project, pattern, COVERAGE, anchor);
    let mut app = capture_app(project);
    let world = app.world_mut();
    let (camera, target) = world
        .query_filtered::<(Entity, &RenderTarget), With<PixelCamera>>()
        .single(world)
        .map(|(camera, target)| (camera, target.as_image().unwrap().clone()))
        .unwrap();
    let mut players =
        world.query::<(Entity, &mut EffectPlayer, &mut Transform, Has<MovablePulse>)>();
    let mut owner = None;
    for (entity, mut player, mut transform, primary) in players.iter_mut(world) {
        player.playing = false;
        player.set_presentation_cadence(None);
        transform.translation.x = if primary { 0.0 } else { 200.0 };
        transform.translation.z = FOREGROUND_Z + 1.0;
        if primary {
            owner = Some(entity);
        }
    }
    let owner = owner.unwrap();
    for (time, effect_x, particle_x, camera_x, swatch) in [
        (0.1, 5.0, 1.0, 1.0, ROSE_SWATCH),
        (0.3, 8.0, 3.0, -2.0, AMBER_SWATCH),
    ] {
        let world = app.world_mut();
        world.get_mut::<Transform>(owner).unwrap().translation.x = effect_x;
        world.get_mut::<Transform>(camera).unwrap().translation.x = camera_x;
        world.get_mut::<EffectPlayer>(owner).unwrap().seek(time);
        let center = (VIRTUAL_SIZE.x as f32 * 0.5 + effect_x + particle_x - camera_x) as u32;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let pixels = capture(&mut app, &target)
                .try_into_dynamic()
                .unwrap()
                .to_rgb8();
            let mismatch = (60..68)
                .flat_map(|y| (center - 4..center + 4).map(move |x| (x, y)))
                .find_map(|(x, y)| {
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
                    let color = pixels.get_pixel(x, y).0;
                    (matches_swatch(color, swatch) != kept).then_some((x, y, color, kept))
                });
            let Some(mismatch) = mismatch else {
                break;
            };
            assert!(
                Instant::now() < deadline,
                "Bevy {anchor:?} {pattern:?}, time {time}, dither sample mismatch {mismatch:?}"
            );
        }
    }
}

fn capture(app: &mut App, target: &Handle<Image>) -> Image {
    // Let texture/pipeline preparation and the pipelined render app settle.
    for _ in 0..70 {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
    }
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |capture: On<ScreenshotCaptured>, mut result: ResMut<Capture>| {
                result.0 = Some(capture.image.clone());
            },
        );
    let start = Instant::now();
    loop {
        app.update();
        if let Some(image) = app.world_mut().resource_mut::<Capture>().0.take() {
            return image;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "no offscreen image"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
