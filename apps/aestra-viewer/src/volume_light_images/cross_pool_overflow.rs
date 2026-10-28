//! Dense premultiplied-color and interleaved overflow ordering oracle.
use super::hdr::{center, center_difference, program};
use super::*;
use crate::volume_light_images::outputs::cross_draw::sheet_configured;
use aestra_bevy::{
    AssetDefinition, AssetId, AssetKind, BlendMode, ExperimentalOitDiagnostics, MaterialProgramId,
    RendererProperties, RendererTypeId,
};

pub(super) const MESH: AssetId = AssetId::from_u128(0xa3574a00000040008000000000f81b80);

#[derive(Clone, Copy, Debug)]
enum Scene {
    Smoke,
    Additive,
    Interleaved,
}
impl Scene {
    fn name(self) -> &'static str {
        match self {
            Self::Smoke => "homogeneous-smoke",
            Self::Additive => "additive-only",
            Self::Interleaved => "interleaved-alpha-emission",
        }
    }
}

#[derive(Clone, Copy)]
struct Layer {
    position: Vec3,
    color: [f32; 3],
    alpha: f32,
    additive: bool,
}

pub(super) fn sheet_project(
    count: u32,
    color: [f32; 3],
    alpha: f32,
    additive: bool,
    volume: bool,
) -> Arc<CompiledEffectProject> {
    let mut material = program(&format!("{},{},{}", color[0], color[1], color[2]), false);
    material.domain = aestra_bevy::material::MaterialDomain::Mesh;
    material.id = MaterialProgramId::from_u128(
        0xa3574a00000040008000000000f81b00
            + u128::from(color[0].to_bits())
            + (u128::from(color[1].to_bits()) << 32),
    );
    if additive {
        material.render_state_policy.default.blend = BlendMode::Additive;
        material.render_state_policy.allowed = vec![material.render_state_policy.default];
    }
    let state = material.render_state_policy.default;
    sheet_configured(
        count,
        if volume {
            EmitterShape::Box {
                half_extents: [0.001, 0.001, 4.0],
            }
        } else {
            EmitterShape::Point
        },
        0.0,
        material,
        |effect| {
            effect.material_instances[0].render_state = state;
            effect.assets = vec![AssetDefinition {
                id: MESH,
                name: "Flat overflow oracle plane".into(),
                kind: AssetKind::Mesh,
                path: "overflow-plane.glb#Mesh0/Primitive0".into(),
            }];
            let renderer = &mut effect.emitters[0].renderers[0];
            renderer.renderer_type = RendererTypeId::new(aestra_bevy::RENDERER_MESH);
            renderer.properties = RendererProperties::Mesh { asset: MESH };
            for m in &mut effect.emitters[0].modules {
                if let ModuleParameters::Appearance { opacity, size, .. } = &mut m.parameters {
                    for key in &mut opacity.keys {
                        key.value = alpha;
                    }
                    // Flat mesh coverage makes the analytic patch's alpha
                    // explicit; this is not a smoke-texture art fixture.
                    for key in &mut size.keys {
                        key.value = 32.0;
                    }
                }
            }
        },
    )
}

type Source = (Arc<CompiledEffectProject>, Vec3, [f32; 3], f32, bool);

fn sources(scene: Scene) -> Vec<Source> {
    match scene {
        Scene::Smoke => vec![(
            sheet_project(40, [1.0, 0.0, 0.0], 0.125, false, true),
            Vec3::ZERO,
            [1.0, 0.0, 0.0],
            0.125,
            false,
        )],
        Scene::Additive => vec![(
            sheet_project(40, [0.025, 0.0125, 0.0], 0.5, true, true),
            Vec3::ZERO,
            [0.025, 0.0125, 0.0],
            0.5,
            true,
        )],
        Scene::Interleaved => vec![
            (
                sheet_project(20, [1.0, 0.0, 0.0], 0.125, false, true),
                Vec3::new(0.0, 0.0, 0.25),
                [1.0, 0.0, 0.0],
                0.125,
                false,
            ),
            (
                sheet_project(20, [0.0, 0.0, 1.0], 0.125, false, true),
                Vec3::ZERO,
                [0.0, 0.0, 1.0],
                0.125,
                false,
            ),
            (
                sheet_project(1, [0.0, 0.6, 0.0], 0.5, true, false),
                Vec3::ZERO,
                [0.0, 0.6, 0.0],
                0.5,
                true,
            ),
        ],
    }
}

fn analytic(layers: &[Layer], opposite: bool) -> [f32; 3] {
    let mut layers = layers.to_vec();
    layers.sort_by(|a, b| {
        if opposite {
            b.position.z.total_cmp(&a.position.z)
        } else {
            a.position.z.total_cmp(&b.position.z)
        }
    });
    layers.iter().fold([0.0; 3], |dst, l| {
        std::array::from_fn(|c| {
            l.color[c] * l.alpha + dst[c] * if l.additive { 1.0 } else { 1.0 - l.alpha }
        })
    })
}

fn patch_error(image: &RgbaImage, expected: [f32; 3]) -> f32 {
    (176..184)
        .flat_map(|y| (236..244).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            (0..3).map(move |c| (linear(image.get_pixel(x, y)[c]) - expected[c]).abs())
        })
        .fold(0.0, f32::max)
}

#[test]
fn dense_overflow_reference_requires_more_than_32_real_layers() {
    for scene in [Scene::Smoke, Scene::Additive, Scene::Interleaved] {
        let count: usize = sources(scene).iter().map(|s| s.0.root.max_particles).sum();
        assert!(count > 32);
    }
    let l = Layer {
        position: Vec3::ZERO,
        color: [1.0, 0.0, 0.0],
        alpha: 0.125,
        additive: false,
    };
    assert!((analytic(&[l; 40], false)[0] - (1.0 - 0.875_f32.powi(40))).abs() < 0.00001);
    let l = Layer {
        color: [0.025, 0.0125, 0.0],
        alpha: 0.5,
        additive: true,
        ..l
    };
    assert!((analytic(&[l; 40], false)[0] - 0.5).abs() < 0.00001);
}

#[test]
#[ignore = "native dense overflow color and interleaved-order image oracle; run alone"]
fn oit_dense_overflow_matches_interleaved_depth_order_and_preserves_energy() {
    let root = root("fragment-list-dense-overflow");
    let mut cases = Vec::new();
    let mut math_ok = true;
    let mut residual_order_errors = 0;
    let mut ownership_controls = Vec::new();
    for scene in [Scene::Smoke, Scene::Additive, Scene::Interleaved] {
        for opposite in [false, true] {
            for reversed in [false, true] {
                let inputs = sources(scene);
                let (mut app, first, target) =
                    headless_project("medium", true, true, inputs[0].0.clone());
                app.world_mut()
                    .resource_mut::<AestraSettings>()
                    .transparent_order = TransparentOrderMode::DepthBackToFront;
                app.world_mut().despawn(first);
                let mut roots = Vec::new();
                let mut refs = Vec::new();
                let mut layers = Vec::new();
                let mesh = app
                    .world_mut()
                    .resource_mut::<Assets<Mesh>>()
                    .add(Rectangle::new(1.0, 1.0));
                let order: Vec<_> = if reversed {
                    (0..inputs.len()).rev().collect()
                } else {
                    (0..inputs.len()).collect()
                };
                for i in order {
                    let (project, offset, color, alpha, additive) = &inputs[i];
                    let entity = spawn(&mut app, project.clone(), 1, offset.z);
                    app.world_mut().entity_mut(entity).insert(
                        PresentedEffect::new(project.root.clone())
                            .with_mesh_overrides(BTreeMap::from([(MESH, mesh.clone())])),
                    );
                    app.world_mut().get_mut::<Transform>(entity).unwrap().scale = Vec3::splat(0.25);
                    roots.push(entity);
                    let mut samples = Vec::new();
                    project.evaluate(0.5, 1, &mut samples);
                    let single = sheet_project(1, *color, *alpha, *additive, false);
                    for sample in samples {
                        let position = Vec3::from_array(sample.particle.position) * 0.25 + *offset;
                        layers.push(Layer {
                            position,
                            color: *color,
                            alpha: *alpha,
                            additive: *additive,
                        });
                        let entity = spawn(&mut app, single.clone(), 1, position.z);
                        app.world_mut().entity_mut(entity).insert(
                            PresentedEffect::new(single.root.clone())
                                .with_mesh_overrides(BTreeMap::from([(MESH, mesh.clone())])),
                        );
                        *app.world_mut().get_mut::<Transform>(entity).unwrap() =
                            Transform::from_translation(position).with_scale(Vec3::splat(0.25));
                        app.world_mut()
                            .entity_mut(entity)
                            .insert(Visibility::Hidden);
                        refs.push(entity);
                    }
                }
                assert!(layers.len() > 32);
                let camera = enable(&mut app);
                *app.world_mut().get_mut::<Transform>(camera).unwrap() =
                    Transform::from_xyz(0.0, 0.0, if opposite { -16.0 } else { 16.0 })
                        .looking_at(Vec3::ZERO, Vec3::Y);
                // Repeated 8-bit blending loses precision over 40 layers. Both
                // paths use a linear float intermediate, then sRGB capture.
                app.world_mut().entity_mut(camera).insert((
                    bevy::camera::Hdr,
                    Tonemapping::None,
                    bevy::core_pipeline::tonemapping::DebandDither::Disabled,
                ));
                app.world()
                    .resource::<ExperimentalOitDiagnostics>()
                    .set_enabled(true);
                let _ = capture(&mut app, &target);
                for entity in roots.iter().chain(&refs) {
                    app.world_mut()
                        .get_mut::<EffectPlayer>(*entity)
                        .unwrap()
                        .playing = true;
                }
                advance(&mut app, roots[0], 30);
                for entity in roots.iter().chain(&refs) {
                    app.world_mut()
                        .get_mut::<EffectPlayer>(*entity)
                        .unwrap()
                        .playing = false;
                    assert_eq!(
                        app.world().get::<EffectPlayer>(*entity).unwrap().frame(),
                        30
                    );
                }
                for entity in &roots {
                    app.world_mut()
                        .entity_mut(*entity)
                        .insert(Visibility::Hidden);
                }
                for entity in &refs {
                    app.world_mut()
                        .entity_mut(*entity)
                        .insert(Visibility::Visible);
                }
                app.world_mut()
                    .entity_mut(camera)
                    .remove::<ExperimentalOitSettings>();
                let reference = capture(&mut app, &target);
                let expected = analytic(&layers, opposite);
                let reference_error = patch_error(&reference, expected);
                assert!(
                    reference_error < 0.01,
                    "{} invalid ordered reference: {reference_error} / {:?} / {expected:?}",
                    scene.name(),
                    center(&reference)
                );
                for entity in &refs {
                    app.world_mut()
                        .entity_mut(*entity)
                        .insert(Visibility::Hidden);
                }
                for entity in &roots {
                    app.world_mut()
                        .entity_mut(*entity)
                        .insert(Visibility::Visible);
                }
                let prefix = format!(
                    "{}-view-{}-spawn-{}",
                    scene.name(),
                    u8::from(opposite),
                    u8::from(reversed)
                );
                reference
                    .save(root.join(format!("{prefix}-reference.png")))
                    .unwrap();
                for sorted_max in [1, 8, 32] {
                    app.world_mut()
                        .entity_mut(camera)
                        .insert(ExperimentalOitSettings {
                            sorted_fragment_max_count: sorted_max,
                            fragments_per_pixel_average: 16.0,
                            max_storage_bytes: 64 * 1024 * 1024,
                        });
                    let image = capture(&mut app, &target);
                    let s = app
                        .world()
                        .resource::<ExperimentalOitDiagnostics>()
                        .snapshot()
                        .unwrap();
                    assert_eq!((s.dropped_fragments, s.invalid_links), (0, 0));
                    assert!(s.over_sort_pixels > 0);
                    assert_eq!(s.capped_max_depth, sorted_max + 1);
                    let error = patch_error(&image, expected);
                    let reference_difference = center_difference(&image, &reference);
                    math_ok &= error < 0.01 && reference_difference < 0.01;
                    if matches!(scene, Scene::Interleaved) {
                        residual_order_errors += usize::from(error > 0.01);
                    }
                    let repeated = capture(&mut app, &target);
                    let repeat_error = patch_error(&repeated, expected);
                    math_ok &= repeat_error < 0.01;
                    // Canonical link sorting must remove arrival-order variation;
                    // independent analytic/reference checks still establish color.
                    let repeat_bytes = delta(&image, &repeated).1;
                    assert_eq!(repeat_bytes, 0, "canonical overflow sort changed on repeat");
                    image
                        .save(root.join(format!("{prefix}-sort-{sorted_max}.png")))
                        .unwrap();
                    cases.push(json!({"name":prefix,"scene":scene.name(),"opposite":opposite,"reversed":reversed,"layers":layers.len(),
                        "sorted_max":sorted_max,"average":16,"node_capacity":s.node_capacity,"quad_size":32,"root_scale":0.25,
                        "expected_linear_rgb":expected,"actual_linear_rgb":center(&image),"patch_error":error,
                        "reference_patch_error":reference_error,"reference_difference":reference_difference,"attempts":s.attempted_fragments,
                        "global_drops":s.dropped_fragments,"oversort_pixels":s.over_sort_pixels,"invalid_links":s.invalid_links,
                        "capped_depth":s.capped_max_depth,"repeat_bytes":repeat_bytes,"repeat_patch_error":repeat_error,
                        "homogeneous_or_additive_math_gate":matches!(scene,Scene::Smoke|Scene::Additive)}));
                }
                if matches!(scene, Scene::Smoke) && !opposite && !reversed && math_ok {
                    let original_exposure =
                        app.world().get::<bevy::camera::Exposure>(camera).copied();
                    for entity in &roots {
                        app.world_mut()
                            .entity_mut(*entity)
                            .insert(Visibility::Hidden);
                    }
                    app.world_mut()
                        .entity_mut(camera)
                        .remove::<ExperimentalOitSettings>();
                    let _ = capture(&mut app, &target);
                    // Aestra draws intentionally do not collect on an unmanaged
                    // camera. Use native StandardMaterial planes to prove the
                    // native resolve itself remains unchanged outside our opt-in.
                    let mesh = app
                        .world_mut()
                        .resource_mut::<Assets<Mesh>>()
                        .add(Rectangle::new(8.0, 8.0));
                    let material = app
                        .world_mut()
                        .resource_mut::<Assets<StandardMaterial>>()
                        .add(StandardMaterial {
                            base_color: Color::linear_rgba(1.0, 0.0, 0.0, 0.125),
                            unlit: true,
                            alpha_mode: AlphaMode::Blend,
                            cull_mode: None,
                            ..default()
                        });
                    let native: Vec<_> = layers
                        .iter()
                        .map(|layer| {
                            app.world_mut()
                                .spawn((
                                    Mesh3d(mesh.clone()),
                                    MeshMaterial3d(material.clone()),
                                    Transform::from_translation(layer.position),
                                ))
                                .id()
                        })
                        .collect();
                    app.world_mut()
                        .entity_mut(camera)
                        .insert(bevy::camera::Exposure { ev100: 0.0 });
                    app.world_mut().entity_mut(camera).insert(
                        bevy::core_pipeline::oit::OrderIndependentTransparencySettings {
                            fragments_per_pixel_average: 16.0,
                            sorted_fragment_max_count: 64,
                            alpha_threshold: 0.0,
                        },
                    );
                    let unmanaged_reference = capture(&mut app, &target);
                    assert!(
                        center(&unmanaged_reference)[0] > 0.2,
                        "native ownership reference must be visible"
                    );
                    app.world_mut().entity_mut(camera).insert(
                        bevy::core_pipeline::oit::OrderIndependentTransparencySettings {
                            fragments_per_pixel_average: 16.0,
                            sorted_fragment_max_count: 1,
                            alpha_threshold: 0.0,
                        },
                    );
                    let unmanaged = capture(&mut app, &target);
                    let unmanaged_error = center_difference(&unmanaged, &unmanaged_reference);
                    assert!(
                        unmanaged_error > 0.1,
                        "unmanaged native resolve must not be silently replaced"
                    );
                    assert!(app.world().get::<ExperimentalOitStatus>(camera).is_none());
                    assert!(
                        app.world()
                            .resource::<ExperimentalOitDiagnostics>()
                            .snapshot()
                            .is_none()
                    );
                    app.world_mut()
                        .entity_mut(camera)
                        .remove::<bevy::core_pipeline::oit::OrderIndependentTransparencySettings>();
                    for entity in native {
                        app.world_mut().despawn(entity);
                    }
                    for entity in &roots {
                        app.world_mut()
                            .entity_mut(*entity)
                            .insert(Visibility::Visible);
                    }
                    if let Some(exposure) = original_exposure {
                        app.world_mut().entity_mut(camera).insert(exposure);
                    }
                    app.world_mut()
                        .entity_mut(camera)
                        .insert(ExperimentalOitSettings {
                            fragments_per_pixel_average: 16.0,
                            sorted_fragment_max_count: 1,
                            max_storage_bytes: 64 * 1024 * 1024,
                        });
                    let managed = capture(&mut app, &target);
                    let managed_error = patch_error(&managed, expected);
                    managed
                        .save(root.join("ownership-managed-diagnostic.png"))
                        .unwrap();
                    assert!(
                        managed_error < 0.01,
                        "managed restore error {managed_error}, center {:?}, expected {expected:?}, status {:?}",
                        center(&managed),
                        app.world().get::<ExperimentalOitStatus>(camera)
                    );
                    unmanaged
                        .save(root.join("ownership-unmanaged-native.png"))
                        .unwrap();
                    unmanaged_reference
                        .save(root.join("ownership-unmanaged-reference.png"))
                        .unwrap();
                    managed
                        .save(root.join("ownership-restored-managed.png"))
                        .unwrap();
                    ownership_controls.push(json!({"unmanaged_native_error":unmanaged_error,"restored_managed_error":managed_error,
                        "unmanaged_status":"none","unmanaged_diagnostics":"unavailable"}));
                }
                assert_native_transport(&app);
            }
        }
    }
    write_report(
        &root,
        &json!({"probe_passed":math_ok && residual_order_errors==0,"math_correct":math_ok,"renderer_correct":math_ok,
        "residual_order_errors":residual_order_errors,"cases":cases,"ownership_controls":ownership_controls,
        "scope":"Positive bounded dense smoke/additive/interleaved image gate; not general host participation, global exhaustion, performance or art acceptance."}),
    );
    assert!(
        math_ok,
        "dense overflow disagreed with the analytic/split-draw oracle"
    );
    assert_eq!(
        ownership_controls.len(),
        1,
        "positive math gate requires ownership restoration control"
    );
    assert_eq!(residual_order_errors, 0, "interleaved ordering must pass");
}
