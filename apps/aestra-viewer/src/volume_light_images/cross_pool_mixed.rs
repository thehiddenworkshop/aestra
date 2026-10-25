//! Expected mixed-pass ordering failures. Passing this probe is NOT renderer acceptance.
use super::hdr::{center, healthy, image_difference, replace_blue};
use super::*;
use crate::volume_light_images::outputs::cross_draw::sheet_configured;
use aestra_bevy::{
    AssetDefinition, AssetId, AssetKind, BlendMode, ExperimentalOitDiagnostics, MaterialDefinition,
    MaterialProperties, RendererProperties, RendererTypeId, SpriteColorSource, TrailEndCap,
    TrailSamplingMode, TrailUvMode,
};

#[derive(Clone, Copy, Debug)]
enum MixedKind {
    AdditiveSprite,
    AdditiveTrail,
    AlphaTrail,
    AlphaMesh,
}

impl MixedKind {
    fn additive(self) -> bool {
        matches!(self, Self::AdditiveSprite | Self::AdditiveTrail)
    }

    fn trail(self) -> bool {
        matches!(self, Self::AdditiveTrail | Self::AlphaTrail)
    }

    fn name(self) -> &'static str {
        match self {
            Self::AdditiveSprite => "additive-sprite",
            Self::AdditiveTrail => "additive-trail",
            Self::AlphaTrail => "alpha-trail",
            Self::AlphaMesh => "alpha-mesh",
        }
    }
}

const MESH: AssetId = AssetId::from_u128(0xa3574a00000040008000000000f81910);

fn fixture(kind: MixedKind) -> Arc<CompiledEffectProject> {
    let mut program = super::hdr::program("0.0,1.0,0.0", false);
    if matches!(kind, MixedKind::AlphaMesh) {
        program.domain = aestra_bevy::material::MaterialDomain::Mesh;
    }
    sheet_configured(1, EmitterShape::Point, 0.0, program, |effect| {
        if !matches!(kind, MixedKind::AlphaMesh) {
            // Deliberately use the legacy path for additive sprites/strips; the mesh
            // exercises a semantic alpha material excluded by renderer kind instead.
            let mut material = MaterialDefinition::sprite(
                "Mixed green draw",
                if kind.additive() {
                    BlendMode::Additive
                } else {
                    BlendMode::Alpha
                },
                0.5,
            );
            material.id = effect.emitters[0].renderers[0].material;
            let MaterialProperties::Sprite { color, .. } = &mut material.properties;
            *color = SpriteColorSource::ParticleColor;
            effect.materials = vec![material];
            effect.material_instances.clear();
        }
        let emitter = &mut effect.emitters[0];
        for module in &mut emitter.modules {
            match &mut module.parameters {
                ModuleParameters::Initialize {
                    speed, direction, ..
                } if kind.trail() => {
                    speed.min = 8.0;
                    speed.max = 8.0;
                    *direction = [0.0, 1.0, 0.0];
                }
                ModuleParameters::Appearance { color, .. } => {
                    for key in &mut color.keys {
                        key.color = [0.0, 1.0, 0.0, 1.0];
                    }
                }
                _ => {}
            }
        }
        let renderer = &mut emitter.renderers[0];
        if kind.trail() {
            // At .5s, the path spans y=-2..2 at one world Z. Its center crosses the
            // oracle patch, so its measured green channel calibrates native age fading.
            emitter.transform.translation[1] = -2.0;
            renderer.renderer_type = RendererTypeId::new(aestra_bevy::RENDERER_TRAIL);
            renderer.properties = RendererProperties::Trail {
                width: 0.5,
                sample_interval: 1.0 / 60.0,
                lifetime: 2.0,
                max_points: 64,
                max_trails: 1,
                sampling: TrailSamplingMode::Time,
                sample_distance: 0.1,
                curve_tolerance: 0.01,
                uv_mode: TrailUvMode::Stretch,
                tile_length: 1.0,
                end_cap: TrailEndCap::Flat,
            };
        } else if matches!(kind, MixedKind::AlphaMesh) {
            effect.assets = vec![AssetDefinition {
                id: MESH,
                name: "In-memory plane".into(),
                kind: AssetKind::Mesh,
                path: "mixed-plane.glb#Mesh0/Primitive0".into(),
            }];
            renderer.renderer_type = RendererTypeId::new(aestra_bevy::RENDERER_MESH);
            renderer.properties = RendererProperties::Mesh { asset: MESH };
        }
    })
}

/// Calibrate each source independently on black, then apply the blend operators in
/// actual world-depth order. Green is unit radiance, so measured G is effective alpha
/// (or additive contribution). No fragment/particle readback is used by the runtime.
fn compose(red: f32, green: f32, additive: bool, green_front: bool) -> [f32; 3] {
    if green_front {
        [if additive { red } else { red * (1.0 - green) }, green, 0.0]
    } else {
        [red, green * (1.0 - red), 0.0]
    }
}

fn patch_error(
    image: &RgbaImage,
    red: &RgbaImage,
    green: &RgbaImage,
    additive: bool,
    front: bool,
) -> f32 {
    let mut maximum = 0.0_f32;
    for y in 176..184 {
        for x in 236..244 {
            let expected = compose(
                linear(red.get_pixel(x, y)[0]),
                linear(green.get_pixel(x, y)[1]),
                additive,
                front,
            );
            for (c, expected) in expected.into_iter().enumerate() {
                maximum = maximum.max((linear(image.get_pixel(x, y)[c]) - expected).abs());
            }
        }
    }
    maximum
}

#[test]
fn mixed_pass_fixture_and_oracle_distinguish_front_additive_and_alpha_draws() {
    assert_eq!(compose(0.5, 0.5, true, true), [0.5, 0.5, 0.0]);
    assert_eq!(compose(0.5, 0.5, false, true), [0.25, 0.5, 0.0]);
    assert_eq!(compose(0.5, 0.5, true, false), [0.5, 0.25, 0.0]);
    assert_eq!(compose(0.5, 0.5, false, false), [0.5, 0.25, 0.0]);
    for kind in [
        MixedKind::AdditiveSprite,
        MixedKind::AdditiveTrail,
        MixedKind::AlphaTrail,
        MixedKind::AlphaMesh,
    ] {
        let project = fixture(kind);
        assert_eq!(project.root.max_particles, 1);
        assert_eq!(
            project.root.material_instances.is_empty(),
            !matches!(kind, MixedKind::AlphaMesh)
        );
        let renderer = &project.root.emitters[0].renderers[0];
        assert_eq!(
            matches!(renderer.kind, aestra_bevy::RendererPlanKind::Trail { .. }),
            kind.trail()
        );
        assert!(project.root.point_lights.is_empty());
    }
}

#[test]
#[ignore = "native expected mixed OIT/ordinary-pass ordering failure; run alone"]
fn fragment_list_mixed_draws_expose_foreground_ordering_failure_without_overflow() {
    let root = root("fragment-list-mixed");
    let mut cases = Vec::new();
    for kind in [
        MixedKind::AdditiveSprite,
        MixedKind::AdditiveTrail,
        MixedKind::AlphaTrail,
        MixedKind::AlphaMesh,
    ] {
        let green_project = fixture(kind);
        for sorted in [false, true] {
            for reversed in [false, true] {
                for opposite in [false, true] {
                    let (mut app, red, green, target) =
                        pair(sheet(true), 1, reversed, opposite, sorted, 0.0);
                    replace_blue(&mut app, green, green_project.clone(), 1);
                    // Trail samples are world-space history, not a rigid mesh attached
                    // to its owner's current pose. Fix placement BEFORE recording.
                    app.world_mut()
                        .get_mut::<Transform>(green)
                        .unwrap()
                        .translation
                        .z = -3.0;
                    if matches!(kind, MixedKind::AlphaMesh) {
                        let mesh = app
                            .world_mut()
                            .resource_mut::<Assets<Mesh>>()
                            .add(Rectangle::new(1.0, 1.0));
                        app.world_mut().entity_mut(green).insert(
                            PresentedEffect::new(green_project.root.clone())
                                .with_mesh_overrides(BTreeMap::from([(MESH, mesh)])),
                        );
                    }
                    let camera = enable(&mut app);
                    app.world()
                        .resource::<ExperimentalOitDiagnostics>()
                        .set_enabled(true);
                    // Warm lazily queued render/simulation/trail pipelines at frame zero.
                    // Warming only after advancing can leave different early trail histories
                    // across cold runs, even when a central compositing patch looks identical.
                    let _ = capture(&mut app, &target);
                    for entity in [red, green] {
                        assert_eq!(app.world().get::<EffectPlayer>(entity).unwrap().frame(), 0);
                    }
                    forward(&mut app, red, green, 30);
                    let mut original = None;
                    for (step, view_opposite) in
                        [opposite, !opposite, opposite].into_iter().enumerate()
                    {
                        let camera_z = if view_opposite { -16.0 } else { 16.0 };
                        *app.world_mut().get_mut::<Transform>(camera).unwrap() =
                            Transform::from_xyz(0.0, 0.0, camera_z).looking_at(Vec3::ZERO, Vec3::Y);
                        let front = view_opposite;
                        let prefix = format!(
                            "{}-sort-{}-spawn-{}-view-{}-step-{step}",
                            kind.name(),
                            u8::from(sorted),
                            u8::from(reversed),
                            u8::from(opposite)
                        );
                        let image = capture(&mut app, &target);
                        image.save(root.join(format!("{prefix}-oit.png"))).unwrap();
                        let attempts = healthy(&app);
                        let diagnostic = app
                            .world()
                            .resource::<ExperimentalOitDiagnostics>()
                            .snapshot()
                            .unwrap();
                        assert_eq!(
                            app.world()
                                .get::<ExperimentalOitStatus>(camera)
                                .unwrap()
                                .rejection,
                            None
                        );
                        app.world_mut().entity_mut(green).insert(Visibility::Hidden);
                        let red_only = capture(&mut app, &target);
                        app.world_mut().entity_mut(red).insert(Visibility::Hidden);
                        app.world_mut()
                            .entity_mut(green)
                            .insert(Visibility::Visible);
                        let green_only = capture(&mut app, &target);
                        let green_attempts = app
                            .world()
                            .resource::<ExperimentalOitDiagnostics>()
                            .snapshot()
                            .unwrap();
                        assert_eq!(
                            green_attempts.attempted_fragments, 0,
                            "mixed source unexpectedly entered fragment lists"
                        );
                        assert!(green_attempts.sequence > diagnostic.sequence);
                        assert_eq!(green_attempts.dropped_fragments, 0);
                        assert_eq!(green_attempts.over_sort_pixels, 0);
                        assert_eq!(green_attempts.invalid_links, 0);
                        let r = center(&red_only);
                        let g = center(&green_only);
                        assert!((r[0] - 0.5).abs() < 0.01 && r[1] < 0.001 && r[2] < 0.001);
                        assert!(
                            g[1] > 0.15 && g[1] < 0.6 && g[0] < 0.001 && g[2] < 0.001,
                            "{} invisible/miscalibrated mixed draw: {g:?}",
                            kind.name()
                        );
                        // Both ordinary draws have distinct, planar depths. Their sorted
                        // normal pass is a valid positive control, unlike an interleaved pool.
                        app.world_mut()
                            .entity_mut(camera)
                            .remove::<ExperimentalOitSettings>();
                        app.world_mut()
                            .resource_mut::<AestraSettings>()
                            .transparent_order = TransparentOrderMode::DepthBackToFront;
                        let green_normal = capture(&mut app, &target);
                        assert!(
                            delta(&green_only, &green_normal).1 <= 1,
                            "OIT changed the isolated non-OIT source"
                        );
                        app.world_mut().entity_mut(red).insert(Visibility::Visible);
                        let reference = capture(&mut app, &target);
                        reference
                            .save(root.join(format!("{prefix}-reference.png")))
                            .unwrap();
                        let reference_error =
                            patch_error(&reference, &red_only, &green_only, kind.additive(), front);
                        assert!(
                            reference_error < 0.01,
                            "{prefix} invalid positive control {reference_error}"
                        );
                        let actual_error =
                            patch_error(&image, &red_only, &green_only, kind.additive(), front);
                        let forced_background_error =
                            patch_error(&image, &red_only, &green_only, kind.additive(), false);
                        assert!(
                            forced_background_error < 0.01,
                            "{prefix} unexpected resolve behavior {forced_background_error}"
                        );
                        if front {
                            assert!(
                                actual_error > 0.07,
                                "must expose wrong foreground ordering, not accept it"
                            );
                        } else {
                            assert!(actual_error < 0.01);
                            assert!(image_difference(&image, &reference) < 0.01);
                        }
                        app.world_mut()
                            .resource_mut::<AestraSettings>()
                            .transparent_order = if sorted {
                            TransparentOrderMode::DepthBackToFront
                        } else {
                            TransparentOrderMode::Fast
                        };
                        app.world_mut()
                            .entity_mut(camera)
                            .insert(ExperimentalOitSettings::default());
                        let restored = capture(&mut app, &target);
                        healthy(&app);
                        let restoration = delta(&image, &restored).1;
                        assert!(restoration <= 1);
                        if step == 0 {
                            original = Some(image.clone());
                        }
                        let camera_restore = if step == 2 {
                            let error = delta(original.as_ref().unwrap(), &image).1;
                            assert!(error <= 1);
                            Some(error)
                        } else {
                            None
                        };
                        for (suffix, img) in [
                            ("red-only", &red_only),
                            ("green-only", &green_only),
                            ("restored", &restored),
                        ] {
                            img.save(root.join(format!("{prefix}-{suffix}.png")))
                                .unwrap();
                        }
                        cases.push(json!({"name":prefix,"path":kind.name(),"sort":if sorted {"DepthBackToFront"} else {"Fast"},
                            "green_world_z":-3.0,"camera_world_z":camera_z,"green_in_front":front,"actual_linear_rgb":center(&image),
                            "source_red_linear_rgb":r,"source_green_linear_rgb":g,
                            "expected_linear_rgb":compose(r[0],g[1],kind.additive(),front),
                            "reference_patch_max_error":reference_error,"oit_patch_max_error":actual_error,
                            "forced_background_patch_max_error":forced_background_error,
                            "whole_image_reference_max_error":image_difference(&image,&reference),
                            "oit_restore_max_byte_error":restoration,"camera_restore_max_byte_error":camera_restore,
                            "attempted_fragments":attempts,"isolated_mixed_attempted_fragments":green_attempts.attempted_fragments,
                            "allocation_rejection":null,"dropped_fragments":diagnostic.dropped_fragments,
                            "over_sort_pixels":diagnostic.over_sort_pixels,"visited_invalid_links":diagnostic.invalid_links,
                            "renderer_correct":!front}));
                        assert_eq!(app.world().get::<EffectPlayer>(green).unwrap().frame(), 30);
                        assert!(!app.world().get::<EffectPlayer>(green).unwrap().playing);
                        assert_eq!(
                            app.world().get::<Transform>(green).unwrap().translation.z,
                            -3.0
                        );
                        assert_native_transport(&app);
                    }
                }
            }
        }
    }
    write_report(
        &root,
        &json!({"probe_passed":true,"renderer_correct":false,"cases":cases,
        "scope":"Expected mixed-pass foreground failure, not compositor adoption. Healthy counters and pool sorting do not repair resolve-after-transparent ordering."}),
    );
}
