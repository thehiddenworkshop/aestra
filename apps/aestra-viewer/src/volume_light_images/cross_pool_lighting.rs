//! Isolated ScenePointIrradiance receiver versus independently ordered single-particle draws.
use super::hdr::{camera_hdr, center, center_difference, healthy, image_difference, replace_blue};
use super::*;
use crate::volume_light_images::outputs::cross_draw::sheet_program;
use aestra_bevy::ExperimentalOitDiagnostics;

fn lit_program() -> MaterialProgram {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    MaterialProgram::from_ron(
        &fs::read_to_string(root.join("materials/fireworks_lit_smoke.aestra.material.ron"))
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn lit_smoke_reference_compiles_the_saved_alpha_material() {
    let program = lit_program();
    assert_eq!(program.name, "Fireworks Lit Particle Smoke");
    assert!(
        program
            .parameters
            .iter()
            .any(|p| p.name == "Scene light gain")
    );
    let project = sheet_program(1, EmitterShape::Point, 0.0, program);
    assert_eq!(project.root.max_particles, 1);
    assert!(project.root.point_lights.is_empty());
}

#[test]
#[ignore = "native lit semantic OIT smoke receiver and split-draw HDR control; run alone"]
fn fragment_list_lighting_preserves_dark_smoke_and_local_relighting() {
    let root = root("fragment-list-lighting");
    let (_, seed, _) = interleaved();
    let receiver = sheet_program(
        2,
        EmitterShape::Box {
            half_extents: [0.001, 0.001, 4.0],
        },
        0.0,
        lit_program(),
    );
    let single = sheet_program(1, EmitterShape::Point, 0.0, lit_program());
    let mut samples = Vec::new();
    receiver.evaluate(0.5, seed, &mut samples);
    let mut positions: Vec<_> = samples
        .iter()
        .map(|sample| {
            Mat4::from_cols_array(&sample.world_from_effect)
                .transform_point3(Vec3::from_array(sample.particle.position))
                + Vec3::Z * INTERLEAVED_HOST_Z
        })
        .collect();
    positions.push(Vec3::ZERO);
    let mut cases = Vec::new();
    let mut controls = Vec::new();
    for reversed in [false, true] {
        for opposite in [false, true] {
            let (mut app, red, blue, target) = pair(
                receiver.clone(),
                seed,
                reversed,
                opposite,
                false,
                INTERLEAVED_HOST_Z,
            );
            replace_blue(&mut app, blue, single.clone(), seed);
            let camera = enable(&mut app);
            app.world_mut()
                .get_mut::<Camera>(camera)
                .unwrap()
                .clear_color = ClearColorConfig::Custom(Color::linear_rgb(0.2, 0.2, 0.2));
            camera_hdr(&mut app, camera, 0.0);
            app.world()
                .resource::<ExperimentalOitDiagnostics>()
                .set_enabled(true);
            forward(&mut app, red, blue, 30);
            for entity in [red, blue] {
                app.world_mut()
                    .entity_mut(entity)
                    .insert(Visibility::Hidden);
            }
            let background = capture(&mut app, &target);
            for entity in [red, blue] {
                app.world_mut()
                    .entity_mut(entity)
                    .insert(Visibility::Visible);
            }
            let light = app
                .world_mut()
                .spawn((
                    PointLight {
                        intensity: 0.0,
                        range: 16.0,
                        radius: 0.5,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(-2.0, 1.0, 3.0),
                ))
                .id();
            let reference: Vec<_> = positions
                .iter()
                .map(|position| {
                    let entity = spawn(&mut app, single.clone(), seed, position.z);
                    app.world_mut()
                        .get_mut::<Transform>(entity)
                        .unwrap()
                        .translation = *position;
                    app.world_mut()
                        .get_mut::<EffectPlayer>(entity)
                        .unwrap()
                        .seek_frame(30);
                    app.world_mut()
                        .entity_mut(entity)
                        .insert(Visibility::Hidden);
                    entity
                })
                .collect();
            let mut off = None;
            let mut lit_blue = None;
            let name_prefix = format!("spawn-{}-view-{}", u8::from(reversed), u8::from(opposite));
            background
                .save(root.join(format!("{name_prefix}-background.png")))
                .unwrap();
            for (phase, color, intensity) in [
                ("dark", Color::WHITE, 0.0),
                ("red", Color::linear_rgb(1.0, 0.01, 0.01), 24000.0),
                ("blue", Color::linear_rgb(0.01, 0.01, 1.0), 24000.0),
            ] {
                {
                    let mut light = app.world_mut().get_mut::<PointLight>(light).unwrap();
                    light.color = color;
                    light.intensity = intensity;
                }
                let image = capture(&mut app, &target);
                let attempts = healthy(&app);
                let measured = center(&image);
                image
                    .save(root.join(format!("{name_prefix}-{phase}-oit.png")))
                    .unwrap();
                if phase == "dark" {
                    assert!(
                        measured[0] < center(&background)[0] - 0.03,
                        "alpha smoke must darken the background"
                    );
                    off = Some(image.clone());
                } else {
                    let c = if phase == "red" { 0 } else { 2 };
                    assert!(
                        measured[c] > center(off.as_ref().unwrap())[c] + 0.05,
                        "{phase} contrast: measured={measured:?}, dark={:?}",
                        center(off.as_ref().unwrap())
                    );
                    assert!(
                        measured[c] > measured[2 - c] + 0.05,
                        "local light must recolor the receiver"
                    );
                }
                // Disable collection and split every particle into a correctly depth-sorted draw.
                app.world_mut()
                    .entity_mut(camera)
                    .remove::<ExperimentalOitSettings>();
                for entity in [red, blue] {
                    app.world_mut()
                        .entity_mut(entity)
                        .insert(Visibility::Hidden);
                }
                for entity in &reference {
                    app.world_mut()
                        .entity_mut(*entity)
                        .insert(Visibility::Visible);
                }
                let split = capture(&mut app, &target);
                let reference_error = center_difference(&image, &split);
                let whole_image_error = image_difference(&image, &split);
                assert!(
                    reference_error < 0.01,
                    "{phase} split-draw error={reference_error}"
                );
                assert!(
                    whole_image_error < 0.01,
                    "{phase} whole-image split-draw error={whole_image_error}"
                );
                for entity in &reference {
                    app.world_mut()
                        .entity_mut(*entity)
                        .insert(Visibility::Hidden);
                }
                for entity in [red, blue] {
                    app.world_mut()
                        .entity_mut(entity)
                        .insert(Visibility::Visible);
                }
                app.world_mut()
                    .entity_mut(camera)
                    .insert(ExperimentalOitSettings::default());
                let restored = capture(&mut app, &target);
                assert!(delta(&image, &restored).1 <= 1);
                healthy(&app);
                let name = format!(
                    "spawn-{}-view-{}-{phase}",
                    u8::from(reversed),
                    u8::from(opposite)
                );
                for (suffix, image) in [("oit", &image), ("split", &split), ("restored", &restored)]
                {
                    image
                        .save(root.join(format!("{name}-{suffix}.png")))
                        .unwrap();
                }
                cases.push(json!({"name":name,"phase":phase,"light_lumens":intensity,"actual_linear_rgb":measured,
                "background_linear_rgb":center(&background),"split_max_linear_error":reference_error,
                "whole_image_split_max_linear_error":whole_image_error,
                "restoration_max_byte_error":delta(&image,&restored).1,"attempted_fragments":attempts}));
                if phase == "blue" {
                    lit_blue = Some(image);
                }
            }
            // Material gain control: leave the blue light active, mute only scattering.
            for entity in [red, blue] {
                let artifact = app
                    .world()
                    .get::<EffectPlayer>(entity)
                    .unwrap()
                    .effect()
                    .clone();
                let emitter = &artifact.emitters[0];
                let material = emitter.renderers[0].material;
                let mut presented = app.world_mut().get_mut::<PresentedEffect>(entity).unwrap();
                let mut binding = presented
                    .material_binding_for_emitter(material, emitter.source)
                    .unwrap()
                    .clone();
                let compiled = binding.program().clone();
                binding
                    .set_value(
                        compiled
                            .reflection
                            .parameters
                            .iter()
                            .find(|p| p.name == "Scene light gain")
                            .unwrap()
                            .id,
                        aestra_bevy::material::MaterialValue::Float(0.0),
                    )
                    .unwrap();
                assert!(Arc::ptr_eq(binding.program(), &compiled));
                presented.bind_material(material, binding);
            }
            let gain_off = capture(&mut app, &target);
            let gain_off_error = delta(off.as_ref().unwrap(), &gain_off).1;
            assert!(
                gain_off_error <= 1,
                "gain zero must restore dark smoke without changing the light"
            );
            gain_off
                .save(root.join(format!(
                    "spawn-{}-view-{}-gain-off.png",
                    u8::from(reversed),
                    u8::from(opposite)
                )))
                .unwrap();
            for entity in [red, blue] {
                let material = app
                    .world()
                    .get::<EffectPlayer>(entity)
                    .unwrap()
                    .effect()
                    .emitters[0]
                    .renderers[0]
                    .material;
                app.world_mut()
                    .get_mut::<PresentedEffect>(entity)
                    .unwrap()
                    .unbind_material(material);
                assert_eq!(app.world().get::<EffectPlayer>(entity).unwrap().frame(), 30);
                assert!(!app.world().get::<EffectPlayer>(entity).unwrap().playing);
            }
            let gain_restored = capture(&mut app, &target);
            let gain_restore_error = delta(lit_blue.as_ref().unwrap(), &gain_restored).1;
            assert!(gain_restore_error <= 1);
            healthy(&app);
            gain_restored
                .save(root.join(format!("{name_prefix}-gain-restored.png")))
                .unwrap();
            let adapter = app
                .world()
                .resource::<bevy::render::renderer::RenderAdapterInfo>();
            controls.push(
                json!({"name":name_prefix,"gain_off_max_byte_error":gain_off_error,
                "gain_restore_max_byte_error":gain_restore_error,"original_player_frame":30,
                "adapter":{"name":adapter.name,"backend":format!("{:?}",adapter.backend)}}),
            );
            assert_native_transport(&app);
        }
    }
    write_report(
        &root,
        &json!({"probe_passed":true,"cases":cases,"controls":controls,
        "scope":"Saved fireworks_lit_smoke material in isolated receiver, synthetic host point light, fixed Reinhard HDR, split-draw and gain controls. Not authored light-route, wispy smoke art, bloom or production-show acceptance."}),
    );
}
