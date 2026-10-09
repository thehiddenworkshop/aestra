//! Native source-over oracle for separate alpha pools. Readbacks are screenshot tooling only.
use super::*;
use aestra_bevy::{CompiledEffectProject, material::MaterialProgramRef};
use bevy::core_pipeline::tonemapping::Tonemapping;
use serde_json::json;

fn sheet(red: bool) -> Arc<CompiledEffectProject> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let mut effect = EffectAsset::from_ron(
        &fs::read_to_string(root.join("effects/fireworks_particle_smoke_lighting.aestra.ron"))
            .unwrap(),
    )
    .unwrap();
    effect.emitters.truncate(1);
    effect.event_outputs.clear();
    effect.particle_outputs.clear();
    effect.point_lights.clear();
    effect.emitters[0].transform.translation = [0.0; 3];
    for module in &mut effect.emitters[0].modules {
        match &mut module.parameters {
            ModuleParameters::Initialize { speed, .. } => {
                speed.min = 0.0;
                speed.max = 0.0;
            }
            ModuleParameters::Appearance { size, opacity, .. } => {
                for key in &mut size.keys {
                    key.value = 8.0;
                }
                for key in &mut opacity.keys {
                    key.value = 0.5;
                }
            }
            _ => {}
        }
    }
    let color = if red { "1.0,0.0,0.0" } else { "0.0,0.0,1.0" };
    let program = MaterialProgram::from_ron(&format!(
        r#"(id:"a3574a00-0000-4000-8000-000000f81600",schema_version:1,name:"Source-over sheet",domain:Sprite,
        render_state_policy:(default:(blend:Alpha,depth_test:LessEqual,depth_write:false,cull_mode:r#None),allowed:[(blend:Alpha,depth_test:LessEqual,depth_write:false,cull_mode:r#None)]),
        parameters:[],expressions:[
        (id:"a3574a00-0000-4000-8000-000000f81601",kind:Constant(Vec3(({color})))),
        (id:"a3574a00-0000-4000-8000-000000f81602",kind:Input(ParticleOpacity))],
        outputs:(color:"a3574a00-0000-4000-8000-000000f81601",alpha:"a3574a00-0000-4000-8000-000000f81602"))"#
    ))
    .unwrap();
    effect.material_instances[0].program = MaterialProgramRef::Project(program.id);
    let resolved = aestra_project::ResolvedEffectProject {
        root: effect,
        dependencies: BTreeMap::new(),
        material_programs: BTreeMap::from([(program.id, program)]),
        material_functions: BTreeMap::new(),
    };
    Arc::new(
        EffectCompiler::default()
            .compile_resolved_project(&resolved)
            .unwrap(),
    )
}

fn linear(byte: u8) -> f32 {
    let s = f32::from(byte) / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

#[test]
fn separate_alpha_sheet_fixture_is_ordinary_compiled_playback() {
    for red in [true, false] {
        let project = sheet(red);
        assert_eq!(project.root.emitters.len(), 1);
        assert_eq!(project.root.max_particles, 1);
        assert!(project.root.point_lights.is_empty());
    }
}

#[test]
fn whole_draw_sorting_cannot_resolve_depth_interleaved_pools() {
    // Far red, middle blue, near red, each alpha 0.5: exact source-over is R=.625/B=.25.
    // Combining both reds into one indirect draw gives alpha .75. Neither whole-draw
    // ordering equals the particle-interleaved reference, even with perfect pool centers.
    let reference = [0.625_f32, 0.25];
    for whole_draw in [[0.75_f32, 0.125], [0.375, 0.5]] {
        assert!(
            reference
                .into_iter()
                .zip(whole_draw)
                .any(|(a, b)| (a - b).abs() >= 0.125)
        );
    }
}

#[test]
#[ignore = "native separate-pool alpha compositing oracle; run alone"]
fn separate_alpha_pools_match_source_over_from_both_views_and_spawn_orders() {
    let root = PathBuf::from(
        std::env::var_os("AESTRA_CROSS_DRAW_IMAGES").expect("fresh absolute directory"),
    );
    assert!(root.is_absolute());
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("report.json"), b"{\"accepted\":false}").unwrap();
    let mut cases = Vec::new();
    for reversed in [false, true] {
        for opposite in [false, true] {
            let (mut app, first, target) = headless_project("medium", true, true, sheet(!reversed));
            let mut second = EffectPlayer::from_project(sheet(reversed))
                .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
            second.playing = false;
            second.seek_frame(30);
            let other = app
                .world_mut()
                .spawn((second, Transform::IDENTITY, Visibility::Visible))
                .id();
            let (red, blue) = if reversed {
                (other, first)
            } else {
                (first, other)
            };
            for (entity, z) in [(red, 2.0), (blue, -2.0)] {
                let mut player = app.world_mut().get_mut::<EffectPlayer>(entity).unwrap();
                player.seek_frame(30);
                *app.world_mut().get_mut::<Transform>(entity).unwrap() =
                    Transform::from_xyz(0.0, 0.0, z);
                app.world_mut()
                    .entity_mut(entity)
                    .insert(Visibility::Visible);
            }
            let mut cameras = app
                .world_mut()
                .query_filtered::<(Entity, &mut Transform), With<Camera3d>>();
            let camera = cameras
                .iter_mut(app.world_mut())
                .map(|(e, mut t)| {
                    *t = Transform::from_xyz(0.0, 0.0, if opposite { -16.0 } else { 16.0 })
                        .looking_at(Vec3::ZERO, Vec3::Y);
                    e
                })
                .next()
                .unwrap();
            app.world_mut().entity_mut(camera).insert(Tonemapping::None);
            let both = capture(&mut app, &target);
            app.world_mut().entity_mut(blue).insert(Visibility::Hidden);
            let red_only = capture(&mut app, &target);
            app.world_mut().entity_mut(blue).insert(Visibility::Visible);
            app.world_mut().entity_mut(red).insert(Visibility::Hidden);
            let blue_only = capture(&mut app, &target);
            app.world_mut().entity_mut(red).insert(Visibility::Visible);
            let restored = capture(&mut app, &target);
            let mut max_error = 0.0_f32;
            let mut wrong_order_error = 0.0_f32;
            for y in 176..184 {
                for x in 236..244 {
                    let a = linear(red_only.get_pixel(x, y)[0]);
                    let b = linear(blue_only.get_pixel(x, y)[2]);
                    assert!((a - 0.5).abs() < 0.01 && (b - 0.5).abs() < 0.01);
                    let expected = if opposite {
                        [a * (1.0 - b), b]
                    } else {
                        [a, b * (1.0 - a)]
                    };
                    let wrong = if opposite {
                        [a, b * (1.0 - a)]
                    } else {
                        [a * (1.0 - b), b]
                    };
                    let actual = [
                        linear(both.get_pixel(x, y)[0]),
                        linear(both.get_pixel(x, y)[2]),
                    ];
                    for channel in 0..2 {
                        max_error = max_error.max((expected[channel] - actual[channel]).abs());
                        wrong_order_error =
                            wrong_order_error.max((wrong[channel] - actual[channel]).abs());
                    }
                }
            }
            let repeat = delta(&both, &restored);
            let name = format!("spawn-{}-view-{}", u8::from(reversed), u8::from(opposite));
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
            assert_native_transport(&app);
            cases.push(json!({"case":name,"source_over_max_linear_error":max_error,"wrong_order_max_linear_error":wrong_order_error,"restoration":repeat}));
        }
    }
    let accepted = cases.iter().all(|c| {
        c["source_over_max_linear_error"].as_f64().unwrap() < 0.01
            && c["wrong_order_max_linear_error"].as_f64().unwrap() > 0.2
            && c["restoration"][1].as_u64().unwrap() <= 1
    });
    let report = json!({"accepted":accepted,"cases":cases,"scope":"separate nonintersecting single-particle sheets; analytic linear source-over oracle; does not certify interleaved smoke pools or moving particle depth bounds"});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(accepted, "{report}");
}
