//! Expected native transparency failures, not approval of a global compositor.
use super::cross_draw::{linear, sheet, sheet_with};
use super::*;
use aestra_bevy::{CompiledEffectProject, EmitterShape, TransparentOrderMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use serde_json::{Value, json};
use std::path::Path;

#[path = "cross_pool_oit.rs"]
mod oit;

// Keep pool centers distinct: equal centers have no unique whole-draw ordering.
// The particles still straddle blue at z=0, so either pool order remains wrong.
const INTERLEAVED_HOST_Z: f32 = 0.25;

fn depths(project: &CompiledEffectProject, time: f32, seed: u64) -> Vec<f32> {
    let mut samples = Vec::new();
    project.evaluate(time, seed, &mut samples);
    samples
        .into_iter()
        .map(|s| {
            assert!((s.particle.color[3] - 0.5).abs() < 0.0001);
            assert_eq!(s.particle.size, 8.0);
            Mat4::from_cols_array(&s.world_from_effect)
                .transform_point3(Vec3::from_array(s.particle.position))
                .z
        })
        .collect()
}

fn interleaved() -> (Arc<CompiledEffectProject>, u64, Vec<f32>) {
    let project = sheet_with(
        true,
        2,
        EmitterShape::Box {
            half_extents: [0.001, 0.001, 4.0],
        },
        0.0,
    );
    for seed in 1..1000 {
        let z: Vec<_> = depths(&project, 0.5, seed)
            .into_iter()
            .map(|z| z + INTERLEAVED_HOST_Z)
            .collect();
        if z.len() == 2 && z.iter().any(|v| *v < -1.0) && z.iter().any(|v| *v > 1.0) {
            return (project, seed, z);
        }
    }
    panic!("no deterministic straddling fixture");
}

fn source_over(red_depths: &[f32], opposite: bool) -> [f32; 2] {
    let mut layers: Vec<_> = red_depths.iter().map(|z| (*z, [1.0, 0.0])).collect();
    layers.push((0.0, [0.0, 1.0]));
    layers.sort_by(|a, b| {
        if opposite {
            b.0.total_cmp(&a.0)
        } else {
            a.0.total_cmp(&b.0)
        }
    });
    layers.into_iter().fold([0.0; 2], |dst, (_, src)| {
        [src[0] * 0.5 + dst[0] * 0.5, src[1] * 0.5 + dst[1] * 0.5]
    })
}

fn error(image: &RgbaImage, expected: [f32; 2]) -> f32 {
    let mut maximum = 0.0_f32;
    for y in 176..184 {
        for x in 236..244 {
            let p = image.get_pixel(x, y);
            maximum = maximum
                .max((linear(p[0]) - expected[0]).abs())
                .max((linear(p[2]) - expected[1]).abs())
                .max(linear(p[1]));
        }
    }
    maximum
}

fn spawn(app: &mut App, project: Arc<CompiledEffectProject>, seed: u64, z: f32) -> Entity {
    let mut player = EffectPlayer::from_project(project)
        .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    player.set_seed(seed);
    player.playing = false;
    app.world_mut()
        .spawn((
            player,
            Transform::from_xyz(0.0, 0.0, z),
            Visibility::Visible,
        ))
        .id()
}

fn pair(
    project: Arc<CompiledEffectProject>,
    seed: u64,
    reversed: bool,
    opposite: bool,
    sorted: bool,
    red_z: f32,
) -> (App, Entity, Entity, Handle<Image>) {
    let (mut app, first, target) = headless_project(
        "medium",
        true,
        true,
        if reversed {
            sheet(false)
        } else {
            project.clone()
        },
    );
    app.world_mut()
        .resource_mut::<AestraSettings>()
        .transparent_order = if sorted {
        TransparentOrderMode::DepthBackToFront
    } else {
        TransparentOrderMode::Fast
    };
    let other = spawn(
        &mut app,
        if reversed { project } else { sheet(false) },
        seed,
        0.0,
    );
    let (red, blue) = if reversed {
        (other, first)
    } else {
        (first, other)
    };
    for (entity, z) in [(red, red_z), (blue, 0.0)] {
        app.world_mut()
            .get_mut::<EffectPlayer>(entity)
            .unwrap()
            .set_seed(seed);
        *app.world_mut().get_mut::<Transform>(entity).unwrap() = Transform::from_xyz(0.0, 0.0, z);
        app.world_mut()
            .entity_mut(entity)
            .insert(Visibility::Visible);
    }
    let mut cameras = app
        .world_mut()
        .query_filtered::<(Entity, &mut Transform), With<Camera3d>>();
    let camera = cameras
        .iter_mut(app.world_mut())
        .map(|(entity, mut t)| {
            *t = Transform::from_xyz(0.0, 0.0, if opposite { -16.0 } else { 16.0 })
                .looking_at(Vec3::ZERO, Vec3::Y);
            entity
        })
        .next()
        .unwrap();
    app.world_mut().entity_mut(camera).insert(Tonemapping::None);
    (app, red, blue, target)
}

fn forward(app: &mut App, red: Entity, blue: Entity, frame: u64) {
    app.world_mut()
        .get_mut::<EffectPlayer>(blue)
        .unwrap()
        .playing = true;
    advance(app, red, frame);
    app.world_mut()
        .get_mut::<EffectPlayer>(blue)
        .unwrap()
        .playing = false;
    assert_eq!(app.world().get::<EffectPlayer>(red).unwrap().frame(), frame);
    assert_eq!(
        app.world().get::<EffectPlayer>(blue).unwrap().frame(),
        frame
    );
}

fn observe(
    app: &mut App,
    target: &Handle<Image>,
    players: (Entity, Entity),
    root: &Path,
    name: &str,
    expected: [f32; 2],
    count: u32,
) -> Value {
    let (red, blue) = players;
    let both = capture(app, target);
    app.world_mut().entity_mut(blue).insert(Visibility::Hidden);
    let red_only = capture(app, target);
    app.world_mut().entity_mut(blue).insert(Visibility::Visible);
    app.world_mut().entity_mut(red).insert(Visibility::Hidden);
    let blue_only = capture(app, target);
    app.world_mut().entity_mut(red).insert(Visibility::Visible);
    let restored = capture(app, target);
    let measured = [
        linear(both.get_pixel(240, 180)[0]),
        linear(both.get_pixel(240, 180)[2]),
    ];
    let red_coverage = red_only.pixels().filter(|p| p[0] > 100).count();
    let a = 1.0 - 0.5_f32.powi(count as i32);
    let calibration = error(&red_only, [a, 0.0]).max(error(&blue_only, [0.0, 0.5]));
    let exact_error = error(&both, expected);
    let whole_draw_error = error(&both, [a, 0.5 * (1.0 - a)]).min(error(&both, [a * 0.5, 0.5]));
    let repeat = delta(&both, &restored).1;
    for (suffix, image) in [
        ("both", both),
        ("red", red_only),
        ("blue", blue_only),
        ("restored", restored),
    ] {
        image
            .save(root.join(format!("{name}-{suffix}.png")))
            .unwrap();
    }
    assert_native_transport(app);
    let sort = app
        .world()
        .resource::<aestra_bevy::gpu::GpuAlphaSortStatistics>()
        .snapshot();
    let adapter = app
        .world()
        .resource::<bevy::render::renderer::RenderAdapterInfo>();
    json!({"case":name,"expected_linear_rb":expected,"actual_linear_rb":measured,
        "adapter":{"name":adapter.name,"backend":format!("{:?}",adapter.backend)},"red_coverage_pixels":red_coverage,
        "source_over_max_linear_error":exact_error,"whole_draw_max_linear_error":whole_draw_error,
        "calibration_max_linear_error":calibration,"restoration_max_byte_error":repeat,
        "sort":{"pairs":sort.pairs,"dispatched":sort.dispatched,"owned_buffer_bytes":sort.owned_buffer_bytes}})
}

fn root(kind: &str) -> PathBuf {
    let root = PathBuf::from(
        std::env::var_os("AESTRA_CROSS_POOL_IMAGES").expect("fresh absolute directory"),
    )
    .join(kind);
    assert!(
        root.is_absolute() && !root.exists(),
        "use a fresh capture directory: {root:?}"
    );
    fs::create_dir_all(&root).unwrap();
    write_report(&root, &json!({"probe_passed":false}));
    root
}

fn write_report(root: &Path, report: &Value) {
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(report).unwrap(),
    )
    .unwrap();
}

fn calibrated(c: &Value) -> bool {
    c["calibration_max_linear_error"].as_f64().unwrap() < 0.01
        && c["restoration_max_byte_error"].as_u64().unwrap() <= 1
}

#[test]
fn cross_pool_fixtures_and_source_over_oracle() {
    let (_, _, z) = interleaved();
    for opposite in [false, true] {
        assert_eq!(source_over(&z, opposite), [0.625, 0.25]);
    }
    let moving = sheet_with(true, 1, EmitterShape::Point, 2.0);
    for (time, z) in [(0.5, -1.0), (1.5, 1.0), (2.5, 3.0)] {
        assert!((depths(&moving, time, 1)[0] - 2.0 - z).abs() < 0.0001);
    }
    let black = RgbaImage::from_pixel(480, 360, Rgba([0, 0, 0, 255]));
    assert!(error(&black, [0.625, 0.25]) > 0.6);
}

#[test]
#[ignore = "native interleaved-pool expected failure with split-draw positive control; run alone"]
fn cross_pool_interleaving_exposes_whole_draw_limit() {
    let root = root("interleaved");
    let (project, seed, z) = interleaved();
    let mut cases = Vec::new();
    for sorted in [false, true] {
        for reversed in [false, true] {
            for opposite in [false, true] {
                let (mut app, red, blue, target) = pair(
                    project.clone(),
                    seed,
                    reversed,
                    opposite,
                    sorted,
                    INTERLEAVED_HOST_Z,
                );
                forward(&mut app, red, blue, 30);
                let name = format!(
                    "sorted-{}-spawn-{}-view-{}",
                    u8::from(sorted),
                    u8::from(reversed),
                    u8::from(opposite)
                );
                let mut c = observe(
                    &mut app,
                    &target,
                    (red, blue),
                    &root,
                    &name,
                    source_over(&z, opposite),
                    2,
                );
                c["sorted"] = json!(sorted);
                c["opposite"] = json!(opposite);
                if !reversed && !opposite {
                    c["frozen_two_draw_gpu_costs"] = costs(&mut app);
                }
                app.world_mut().entity_mut(red).insert(Visibility::Hidden);
                let controls: Vec<_> = z
                    .iter()
                    .map(|depth| {
                        let entity = spawn(&mut app, sheet(true), seed, *depth);
                        app.world_mut()
                            .get_mut::<EffectPlayer>(entity)
                            .unwrap()
                            .seek_frame(30);
                        entity
                    })
                    .collect();
                let control = capture(&mut app, &target);
                c["split_draw_control_max_linear_error"] =
                    json!(error(&control, source_over(&z, opposite)));
                control
                    .save(root.join(format!("{name}-split-control.png")))
                    .unwrap();
                for entity in controls {
                    app.world_mut()
                        .entity_mut(entity)
                        .insert(Visibility::Hidden);
                }
                app.world_mut().entity_mut(red).insert(Visibility::Visible);
                let restored = capture(&mut app, &target);
                c["split_control_restoration_max_linear_error"] = json!(error(
                    &restored,
                    [
                        c["actual_linear_rb"][0].as_f64().unwrap() as f32,
                        c["actual_linear_rb"][1].as_f64().unwrap() as f32
                    ]
                ));
                cases.push(c);
            }
        }
    }
    let passed = cases.iter().all(|c| {
        calibrated(c)
            && sort_verified(c)
            && c["source_over_max_linear_error"].as_f64().unwrap() > 0.1
            && c["whole_draw_max_linear_error"].as_f64().unwrap() < 0.01
            && c["split_draw_control_max_linear_error"].as_f64().unwrap() < 0.01
            && c["split_control_restoration_max_linear_error"]
                .as_f64()
                .unwrap()
                < 0.01
    });
    let report = json!({"probe_passed":passed,"renderer_correct":false,"seed":seed,"red_host_z":INTERLEAVED_HOST_Z,"red_depths":z,"cases":cases,
        "scope":"Expected whole-draw compositing failure, not transparency acceptance. Native ordinary GPU players, linear alpha oracle, split-draw positive control. Frozen tiny-pool timestamps are not show or alternative-compositor costs."});
    write_report(&root, &report);
    assert!(passed, "{report}");
}

#[test]
#[ignore = "native moving local-depth pools expected failure; run alone"]
fn cross_pool_motion_exposes_stale_whole_draw_order() {
    let root = root("moving");
    let project = sheet_with(true, 1, EmitterShape::Point, 2.0);
    let mut cases = Vec::new();
    for sorted in [false, true] {
        for reversed in [false, true] {
            for opposite in [false, true] {
                let (mut app, red, blue, target) =
                    pair(project.clone(), 1, reversed, opposite, sorted, -2.0);
                for frame in [30, 90, 150] {
                    forward(&mut app, red, blue, frame);
                    assert_eq!(
                        app.world().get::<Transform>(red).unwrap().translation.z,
                        -2.0
                    );
                    let z: Vec<_> = depths(&project, frame as f32 / 60.0, 1)
                        .iter()
                        .map(|z| z - 2.0)
                        .collect();
                    let name = format!(
                        "sorted-{}-spawn-{}-view-{}-frame-{frame}",
                        u8::from(sorted),
                        u8::from(reversed),
                        u8::from(opposite)
                    );
                    let mut c = observe(
                        &mut app,
                        &target,
                        (red, blue),
                        &root,
                        &name,
                        source_over(&z, opposite),
                        1,
                    );
                    c["frame"] = json!(frame);
                    c["sorted"] = json!(sorted);
                    c["opposite"] = json!(opposite);
                    c["red_world_depth"] = json!(z);
                    cases.push(c);
                }
            }
        }
    }
    let mismatches = cases
        .iter()
        .filter(|c| c["source_over_max_linear_error"].as_f64().unwrap() > 0.2)
        .count();
    let passed = mismatches == 16
        && cases.iter().all(|c| {
            calibrated(c)
                && sort_verified(c)
                && if c["frame"] == 30 {
                    c["source_over_max_linear_error"].as_f64().unwrap() < 0.01
                } else {
                    c["source_over_max_linear_error"].as_f64().unwrap() > 0.2
                }
        })
        && cases.len() == 24
        && cases.as_chunks::<3>().0.iter().all(|group| {
            let areas: Vec<_> = group
                .iter()
                .map(|c| c["red_coverage_pixels"].as_u64().unwrap())
                .collect();
            if group[0]["opposite"] == true {
                areas[0] > areas[1] && areas[1] > areas[2]
            } else {
                areas[0] < areas[1] && areas[1] < areas[2]
            }
        });
    let report = json!({"probe_passed":passed,"renderer_correct":mismatches == 0,"mismatches":mismatches,"cases":cases,
        "scope":"Fixed hosts, +Z particle motion through another pool, forward PlaybackOnly at frames 30/90/150, both cameras/spawn orders and Fast/DepthBackToFront. Expected limitation; not production smoke art or performance acceptance."});
    write_report(&root, &report);
    assert!(passed, "{report}");
}

fn sort_verified(c: &Value) -> bool {
    if c["sorted"] == true {
        c["sort"]["dispatched"] == true && c["sort"]["pairs"] == 2
    } else {
        c["sort"]["dispatched"] == false && c["sort"]["pairs"] == 0
    }
}
