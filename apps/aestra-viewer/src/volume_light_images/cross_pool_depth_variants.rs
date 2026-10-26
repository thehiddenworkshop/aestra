//! Supported depth comparisons/projections and a near-clipped semantic mesh oracle.
use super::hdr::{image_difference, program, replace_blue};
use super::*;
use crate::volume_light_images::outputs::cross_draw::sheet_configured;
use aestra_bevy::{
    AssetDefinition, AssetId, AssetKind, BlendMode, ExperimentalOitDiagnostics, RendererProperties,
    RendererTypeId,
    material::{MaterialDepthTest, MaterialDomain},
};

const PLANE: AssetId = AssetId::from_u128(0xa3574a00000040008000000000f81a10);

fn mesh_project(additive: bool, depth: MaterialDepthTest) -> Arc<CompiledEffectProject> {
    let mut material = program("0.0,1.0,0.0", false);
    material.domain = MaterialDomain::Mesh;
    let mut state = material.render_state_policy.default;
    state.depth_test = depth;
    state.blend = if additive {
        BlendMode::Additive
    } else {
        BlendMode::Alpha
    };
    material.render_state_policy.default = state;
    material.render_state_policy.allowed = vec![state];
    sheet_configured(1, EmitterShape::Point, 0.0, material, |effect| {
        effect.material_instances[0].render_state = state;
        effect.assets = vec![AssetDefinition {
            id: PLANE,
            name: "Depth oracle plane".into(),
            kind: AssetKind::Mesh,
            path: "depth-oracle.glb#Mesh0/Primitive0".into(),
        }];
        let renderer = &mut effect.emitters[0].renderers[0];
        renderer.renderer_type = RendererTypeId::new(aestra_bevy::RENDERER_MESH);
        renderer.properties = RendererProperties::Mesh { asset: PLANE };
    })
}

fn prepare_mesh(app: &mut App, entity: Entity, project: Arc<CompiledEffectProject>) {
    replace_blue(app, entity, project.clone(), 1);
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(1.0, 1.0));
    app.world_mut().entity_mut(entity).insert(
        PresentedEffect::new(project.root.clone())
            .with_mesh_overrides(BTreeMap::from([(PLANE, mesh)])),
    );
}

fn expected(wall: f32, opposite: bool, additive: bool) -> [f32; 3] {
    let mut layers = vec![
        (0.0, [1.0, 0.0, 0.0], false),
        (-3.0, [0.0, 1.0, 0.0], additive),
    ];
    layers.retain(|(z, _, _)| if opposite { *z < wall } else { *z > wall });
    layers.sort_by(|a, b| {
        if opposite {
            b.0.total_cmp(&a.0)
        } else {
            a.0.total_cmp(&b.0)
        }
    });
    layers
        .into_iter()
        .fold([0.0, 0.0, 0.25], |dst, (_, src, add)| {
            std::array::from_fn(|c| src[c] * 0.5 + dst[c] * if add { 1.0 } else { 0.5 })
        })
}

fn sample(app: &App) -> aestra_bevy::ExperimentalOitDiagnosticSample {
    let s = app
        .world()
        .resource::<ExperimentalOitDiagnostics>()
        .snapshot()
        .unwrap();
    assert_eq!(
        (s.dropped_fragments, s.over_sort_pixels, s.invalid_links),
        (0, 0, 0)
    );
    s
}

#[test]
fn depth_variant_reference_and_materials_preserve_operator_and_occlusion() {
    assert_eq!(expected(2.0, false, true), [0.0, 0.0, 0.25]);
    assert_eq!(expected(-1.0, false, true), [0.5, 0.0, 0.125]);
    assert_eq!(expected(-6.0, false, false), [0.5, 0.25, 0.0625]);
    assert_eq!(expected(-1.0, true, true), [0.0, 0.5, 0.25]);
    assert_eq!(expected(2.0, true, false), [0.25, 0.5, 0.0625]);
    for additive in [false, true] {
        for depth in [MaterialDepthTest::Less, MaterialDepthTest::LessEqual] {
            let p = mesh_project(additive, depth);
            assert_eq!(p.root.max_particles, 1);
            assert!(matches!(
                p.root.emitters[0].renderers[0].kind,
                aestra_bevy::RendererPlanKind::Mesh { .. }
            ));
        }
    }
}

#[test]
#[ignore = "native perspective/orthographic Less/LessEqual mixed-depth oracle; run alone"]
fn fragment_list_depth_variants_match_opaque_occlusion_and_sorted_reference() {
    let root = root("fragment-list-depth-variants");
    let mut cases = Vec::new();
    for ortho in [false, true] {
        for opposite in [false, true] {
            for additive in [false, true] {
                for depth in [MaterialDepthTest::Less, MaterialDepthTest::LessEqual] {
                    let (mut app, red, green, target) =
                        pair(sheet(true), 1, false, opposite, true, 0.0);
                    prepare_mesh(&mut app, green, mesh_project(additive, depth));
                    app.world_mut()
                        .get_mut::<Transform>(green)
                        .unwrap()
                        .translation
                        .z = -3.0;
                    let camera = enable(&mut app);
                    if ortho {
                        app.world_mut()
                            .entity_mut(camera)
                            .insert(Projection::Orthographic(OrthographicProjection {
                                scaling_mode: bevy::camera::ScalingMode::FixedVertical {
                                    viewport_height: 16.0,
                                },
                                ..OrthographicProjection::default_3d()
                            }));
                    }
                    app.world()
                        .resource::<ExperimentalOitDiagnostics>()
                        .set_enabled(true);
                    let _ = capture(&mut app, &target);
                    forward(&mut app, red, green, 30);
                    let mesh = app
                        .world_mut()
                        .resource_mut::<Assets<Mesh>>()
                        .add(Rectangle::new(64.0, 64.0));
                    let material = app
                        .world_mut()
                        .resource_mut::<Assets<StandardMaterial>>()
                        .add(StandardMaterial {
                            base_color: Color::linear_rgb(0.0, 0.0, 0.25),
                            unlit: true,
                            cull_mode: None,
                            ..default()
                        });
                    let wall = app
                        .world_mut()
                        .spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::IDENTITY))
                        .id();
                    for wall_z in [-6.0, -1.0, 2.0] {
                        app.world_mut()
                            .get_mut::<Transform>(wall)
                            .unwrap()
                            .translation
                            .z = wall_z;
                        let prefix = format!(
                            "ortho-{}-view-{}-add-{}-{depth:?}-wall-{wall_z}",
                            u8::from(ortho),
                            u8::from(opposite),
                            u8::from(additive)
                        );
                        let image = capture(&mut app, &target);
                        let s = sample(&app);
                        let wanted = expected(wall_z, opposite, additive);
                        let error = super::cameras::patch_error(&image, wanted);
                        assert!(error < 0.01, "{prefix} analytic error {error}");
                        if wanted == [0.0, 0.0, 0.25] {
                            assert_eq!(s.attempted_fragments, 0);
                        } else {
                            assert!(s.attempted_fragments > 0);
                        }
                        app.world_mut()
                            .entity_mut(camera)
                            .remove::<ExperimentalOitSettings>();
                        let normal = capture(&mut app, &target);
                        let reference_error = super::cameras::patch_error(&normal, wanted);
                        assert!(
                            reference_error < 0.01,
                            "{prefix} invalid reference {reference_error}"
                        );
                        let whole = image_difference(&image, &normal);
                        assert!(whole < 0.01, "{prefix} whole reference error {whole}");
                        app.world_mut()
                            .entity_mut(camera)
                            .insert(ExperimentalOitSettings::default());
                        let restored = capture(&mut app, &target);
                        assert_eq!(delta(&image, &restored).1, 0);
                        sample(&app);
                        for (suffix, img) in [
                            ("oit", &image),
                            ("reference", &normal),
                            ("restored", &restored),
                        ] {
                            img.save(root.join(format!("{prefix}-{suffix}.png")))
                                .unwrap();
                        }
                        cases.push(json!({"name":prefix,"orthographic":ortho,"opposite":opposite,"additive":additive,
                            "depth_test":format!("{depth:?}"),"wall_z":wall_z,"expected":wanted,"patch_error":error,
                            "reference_patch_error":reference_error,"whole_reference_error":whole,"restore_bytes":0,
                            "attempted_fragments":s.attempted_fragments,"dropped":s.dropped_fragments,"oversort":s.over_sort_pixels,"invalid":s.invalid_links}));
                    }
                    assert_native_transport(&app);
                }
            }
        }
    }
    write_report(
        &root,
        &json!({"probe_passed":true,"cases":cases,"scope":"DepthPrepass, separated planes only; no equal-depth tie or no-prepass qualification."}),
    );
}

#[test]
#[ignore = "native near-plane clipping of alpha/additive semantic meshes; run alone"]
fn fragment_list_near_plane_clipping_matches_visible_geometry_and_reference() {
    let root = root("fragment-list-near-plane");
    let mut cases = Vec::new();
    for additive in [false, true] {
        let (mut app, red, green, target) = pair(sheet(true), 1, false, false, true, 14.0);
        prepare_mesh(
            &mut app,
            green,
            mesh_project(additive, MaterialDepthTest::LessEqual),
        );
        app.world_mut().get_mut::<Transform>(red).unwrap().scale = Vec3::splat(0.15);
        let camera = enable(&mut app);
        app.world_mut()
            .entity_mut(camera)
            .insert(Projection::Perspective(PerspectiveProjection {
                near: 1.0,
                ..default()
            }));
        app.world()
            .resource::<ExperimentalOitDiagnostics>()
            .set_enabled(true);
        let _ = capture(&mut app, &target);
        forward(&mut app, red, green, 30);
        let mut partial = 0;
        let mut visited = BTreeMap::<u32, RgbaImage>::new();
        for (step, distance) in [-0.5_f32, 0.7, 1.0, 1.4, 1.0, 0.7, -0.5]
            .into_iter()
            .enumerate()
        {
            *app.world_mut().get_mut::<Transform>(green).unwrap() =
                Transform::from_xyz(0.0, 0.0, 16.0 - distance)
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_4))
                    .with_scale(Vec3::splat(0.1));
            let prefix = format!("add-{}-step-{step}-distance-{distance}", u8::from(additive));
            let image = capture(&mut app, &target);
            let clipping_restore = visited.get(&distance.to_bits()).map(|previous| {
                let bytes = delta(previous, &image).1;
                assert_eq!(
                    bytes, 0,
                    "near-plane crossing must restore without stale fragments"
                );
                bytes
            });
            visited.insert(distance.to_bits(), image.clone());
            sample(&app);
            app.world_mut().entity_mut(red).insert(Visibility::Hidden);
            let green_only = capture(&mut app, &target);
            let isolated = sample(&app);
            let covered = green_only.pixels().filter(|p| p[1] > 10).count();
            if distance < 1.0 {
                assert_eq!(covered, 0);
                assert_eq!(isolated.attempted_fragments, 0);
            } else {
                assert!(covered > 100);
                assert!(isolated.attempted_fragments > 0);
            }
            if distance == 1.0 {
                partial = covered;
            }
            if distance == 1.4 {
                assert!(
                    covered > partial,
                    "full geometry must exceed its clipped half"
                );
            }
            app.world_mut().entity_mut(red).insert(Visibility::Visible);
            app.world_mut().entity_mut(green).insert(Visibility::Hidden);
            let red_only = capture(&mut app, &target);
            let mut analytic = 0.0_f32;
            for ((actual, r), g) in image
                .pixels()
                .zip(red_only.pixels())
                .zip(green_only.pixels())
            {
                let red_value = linear(r[0]);
                let green_value = linear(g[1]);
                let wanted = [
                    red_value * if additive { 1.0 } else { 1.0 - green_value },
                    green_value,
                    0.0,
                ];
                for c in 0..3 {
                    analytic = analytic.max((linear(actual[c]) - wanted[c]).abs());
                }
            }
            assert!(
                analytic < 0.01,
                "{prefix} isolated-source blend error {analytic}"
            );
            app.world_mut()
                .entity_mut(green)
                .insert(Visibility::Visible);
            app.world_mut()
                .entity_mut(camera)
                .remove::<ExperimentalOitSettings>();
            let normal = capture(&mut app, &target);
            let whole = image_difference(&image, &normal);
            assert!(
                whole < 0.01,
                "{prefix} whole near-plane reference error {whole}"
            );
            app.world_mut()
                .entity_mut(camera)
                .insert(ExperimentalOitSettings::default());
            let restored = capture(&mut app, &target);
            assert_eq!(delta(&image, &restored).1, 0);
            for (suffix, img) in [
                ("oit", &image),
                ("green", &green_only),
                ("red", &red_only),
                ("reference", &normal),
                ("restored", &restored),
            ] {
                img.save(root.join(format!("{prefix}-{suffix}.png")))
                    .unwrap();
            }
            cases.push(json!({"name":prefix,"additive":additive,"step":step,"distance":distance,"near":1.0,
                "green_pixels":covered,"isolated_attempts":isolated.attempted_fragments,"analytic_error":analytic,
                "whole_reference_error":whole,"restore_bytes":0,"clipping_restore_bytes":clipping_restore}));
        }
        assert_native_transport(&app);
    }
    write_report(
        &root,
        &json!({"probe_passed":true,"cases":cases,"scope":"Perspective near=1, rotated plane; not arbitrary mesh/depth material qualification."}),
    );
}
