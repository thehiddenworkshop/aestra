//! Opaque StandardMaterial depth/prepass oracle, not lit smoke or arbitrary materials.
use super::*;
use aestra_bevy::{ExperimentalOitDiagnosticSample, ExperimentalOitDiagnostics};
use bevy::core_pipeline::prepass::DepthPrepass;

const WALL_HALF_DEPTH: f32 = 0.1;
const WALL_COLOR: [f32; 3] = [0.0, 0.25, 0.0];

fn expected(red_z: &[f32], opposite: bool, wall_z: f32) -> [f32; 3] {
    let surface = wall_z
        + if opposite {
            -WALL_HALF_DEPTH
        } else {
            WALL_HALF_DEPTH
        };
    let mut layers: Vec<_> = red_z.iter().map(|z| (*z, [1.0, 0.0, 0.0])).collect();
    layers.push((0.0, [0.0, 0.0, 1.0]));
    layers.retain(|(z, _)| if opposite { *z < surface } else { *z > surface });
    layers.sort_by(|a, b| {
        if opposite {
            b.0.total_cmp(&a.0)
        } else {
            a.0.total_cmp(&b.0)
        }
    });
    layers.into_iter().fold(WALL_COLOR, |dst, (_, src)| {
        std::array::from_fn(|i| src[i] * 0.5 + dst[i] * 0.5)
    })
}

fn region_error(image: &RgbaImage, x: u32, expected: [f32; 3]) -> f32 {
    let mut maximum = 0.0_f32;
    for y in 176..184 {
        for x in x..x + 3 {
            for (channel, expected) in expected.into_iter().enumerate() {
                maximum = maximum.max((linear(image.get_pixel(x, y)[channel]) - expected).abs());
            }
        }
    }
    maximum
}

fn diagnostic(app: &App, camera: Entity, previous: u64) -> ExperimentalOitDiagnosticSample {
    assert_eq!(
        app.world()
            .get::<ExperimentalOitStatus>(camera)
            .unwrap()
            .rejection,
        None
    );
    assert!(app.world().get::<DepthPrepass>(camera).is_some());
    let sample = app
        .world()
        .resource::<ExperimentalOitDiagnostics>()
        .snapshot()
        .expect("missing aggregate is not an occlusion pass");
    assert_eq!(sample.camera, camera);
    assert!(
        sample.sequence > previous,
        "must observe a newly completed sample"
    );
    assert_eq!(sample.dropped_fragments, 0);
    assert_eq!(sample.over_sort_pixels, 0);
    assert_eq!(sample.invalid_links, 0);
    sample
}

#[test]
fn opaque_depth_reference_keeps_only_layers_between_wall_and_camera() {
    let z = [-3.0, 2.0];
    for (wall, front, opposite) in [
        (-6.0, [0.625, 0.03125, 0.25], WALL_COLOR),
        (-1.0, [0.5, 0.0625, 0.25], [0.5, 0.125, 0.0]),
        (0.5, [0.5, 0.125, 0.0], [0.5, 0.0625, 0.25]),
        (6.0, WALL_COLOR, [0.625, 0.03125, 0.25]),
    ] {
        assert_eq!(expected(&z, false, wall), front);
        assert_eq!(expected(&z, true, wall), opposite);
    }
}

#[test]
#[ignore = "native OIT opaque depth/prepass partial-occluder oracle; run alone"]
fn fragment_list_depth_preserves_front_layers_and_rejects_occluded_fragments() {
    let root = root("fragment-list-depth");
    let (project, seed, z) = interleaved();
    let mut cases = Vec::new();
    let mut controls = Vec::new();
    for reversed in [false, true] {
        for opposite in [false, true] {
            let (mut app, red, blue, target) = pair(
                project.clone(),
                seed,
                reversed,
                opposite,
                false,
                INTERLEAVED_HOST_Z,
            );
            let camera = enable(&mut app);
            app.world()
                .resource::<ExperimentalOitDiagnostics>()
                .set_enabled(true);
            forward(&mut app, red, blue, 30);
            let name = format!("spawn-{}-view-{}", u8::from(reversed), u8::from(opposite));
            // Camera reversal flips screen-space X: the box covers world X < 0.
            // Stay inside the original calibrated central 8x8 plateau and leave
            // a two-pixel gap across the opaque edge; sprite edges fade by design.
            let (covered_x, open_x) = if opposite { (241, 236) } else { (236, 241) };
            let rb = source_over(&z, opposite);
            let clear_expected = [rb[0], 0.0, rb[1]];
            let clear = capture(&mut app, &target);
            let baseline = diagnostic(&app, camera, 0);
            clear.save(root.join(format!("{name}-clear.png"))).unwrap();
            let clear_error = region_error(&clear, covered_x, clear_expected).max(region_error(
                &clear,
                open_x,
                clear_expected,
            ));
            assert!(clear_error < 0.01, "unoccluded calibration: {clear_error}");

            let mesh = app
                .world_mut()
                .resource_mut::<Assets<Mesh>>()
                .add(Cuboid::new(12.0, 20.0, WALL_HALF_DEPTH * 2.0));
            let material = app
                .world_mut()
                .resource_mut::<Assets<StandardMaterial>>()
                .add(StandardMaterial {
                    base_color: Color::linear_rgb(WALL_COLOR[0], WALL_COLOR[1], WALL_COLOR[2]),
                    unlit: true,
                    alpha_mode: AlphaMode::Opaque,
                    ..default()
                });
            let wall = app
                .world_mut()
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::from_xyz(-6.0, 0.0, 6.0),
                    Visibility::Visible,
                ))
                .id();
            for player in [red, blue] {
                app.world_mut()
                    .entity_mut(player)
                    .insert(Visibility::Hidden);
            }
            let wall_only = capture(&mut app, &target);
            let calibration = region_error(&wall_only, covered_x, WALL_COLOR)
                .max(region_error(&wall_only, open_x, [0.0; 3]));
            assert!(
                calibration < 0.01,
                "solid unlit wall must calibrate: {calibration}"
            );
            assert_eq!(
                diagnostic(&app, camera, baseline.sequence).attempted_fragments,
                0
            );
            wall_only
                .save(root.join(format!("{name}-wall-only.png")))
                .unwrap();
            for player in [red, blue] {
                app.world_mut()
                    .entity_mut(player)
                    .insert(Visibility::Visible);
            }
            let mut sequence = baseline.sequence;
            // Non-monotonic depth changes catch stale depth/prepass contents.
            for wall_z in [-6.0, 0.5, -1.0, 6.0] {
                app.world_mut()
                    .get_mut::<Transform>(wall)
                    .unwrap()
                    .translation
                    .z = wall_z;
                let image = capture(&mut app, &target);
                let sample = diagnostic(&app, camera, sequence);
                sequence = sample.sequence;
                let occluded_expected = expected(&z, opposite, wall_z);
                let covered_error = region_error(&image, covered_x, occluded_expected);
                let open_error = region_error(&image, open_x, clear_expected);
                assert!(
                    covered_error < 0.01 && open_error < 0.01,
                    "{name} wall={wall_z}: covered={covered_error}, open={open_error}"
                );
                let foreground = if opposite {
                    wall_z == -6.0
                } else {
                    wall_z == 6.0
                };
                let background = if opposite {
                    wall_z == 6.0
                } else {
                    wall_z == -6.0
                };
                assert!(
                    sample.attempted_fragments > 0,
                    "open half must remain visible"
                );
                assert!(sample.attempted_fragments <= baseline.attempted_fragments);
                if foreground {
                    assert!(sample.attempted_fragments < baseline.attempted_fragments);
                }
                if background {
                    assert_eq!(sample.attempted_fragments, baseline.attempted_fragments);
                }
                let wrong_unoccluded_error = region_error(&clear, covered_x, occluded_expected);
                if !background {
                    assert!(
                        wrong_unoccluded_error > 0.1,
                        "oracle must reject missing occlusion"
                    );
                }
                let case_name = format!("{name}-wall-{wall_z}");
                image.save(root.join(format!("{case_name}.png"))).unwrap();
                cases.push(json!({"name":case_name,"wall_z":wall_z,"opposite":opposite,
                    "expected_linear_rgb":occluded_expected,"covered_max_linear_error":covered_error,
                    "open_max_linear_error":open_error,"wrong_unoccluded_error":wrong_unoccluded_error,
                    "attempted_fragments":sample.attempted_fragments,
                    "baseline_attempted_fragments":baseline.attempted_fragments,
                    "dropped_fragments":sample.dropped_fragments,"over_sort_pixels":sample.over_sort_pixels,
                    "invalid_links":sample.invalid_links,"capped_max_depth":sample.capped_max_depth}));
            }
            app.world_mut().entity_mut(wall).insert(Visibility::Hidden);
            let restored = capture(&mut app, &target);
            let restoration = delta(&clear, &restored).1;
            assert!(restoration <= 1, "hidden wall must leave no stale depth");
            assert_eq!(
                diagnostic(&app, camera, sequence).attempted_fragments,
                baseline.attempted_fragments
            );
            restored
                .save(root.join(format!("{name}-restored.png")))
                .unwrap();
            assert_native_transport(&app);
            let adapter = app
                .world()
                .resource::<bevy::render::renderer::RenderAdapterInfo>();
            controls.push(
                json!({"name":name,"wall_calibration_max_linear_error":calibration,
                "clear_calibration_max_linear_error":clear_error,
                "restoration_max_byte_error":restoration,
                "adapter":{"name":adapter.name,"backend":format!("{:?}",adapter.backend)}}),
            );
        }
    }
    write_report(
        &root,
        &json!({"probe_passed":true,"cases":cases,"controls":controls,
        "scope":"Partial opaque StandardMaterial box, default Aestra DepthPrepass, unlit LDR alpha sprites. Not alpha-mask, non-prepass custom materials, HDR/lit, mixed transparent or production smoke acceptance."}),
    );
}
