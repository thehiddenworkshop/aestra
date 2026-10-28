//! Exact packed source-over identities must not spend global list capacity.
use super::*;
use super::{
    hdr::center,
    overflow::{MESH, sheet_project},
};
use aestra_bevy::ExperimentalOitDiagnostics;

#[test]
#[ignore = "native packed OIT identity, absorption and emission oracle; run alone"]
fn fragment_list_packed_identities_preserve_black_absorption_and_faint_emission() {
    let root = root("fragment-list-packed-identity");
    let mut cases = Vec::new();
    for opposite in [false, true] {
        for (name, color, alpha, additive, count, identity) in [
            ("tiny-alpha-rgb", [1e-9; 3], 1e-5, false, 64, true),
            ("black-additive", [0.0; 3], 0.5, true, 64, true),
            ("tiny-additive", [1e-9; 3], 0.5, true, 64, true),
            ("below-rgb-half-step", [5.8e-8; 3], 0.5, true, 64, true),
            ("above-rgb-half-step", [6.1e-8; 3], 0.5, true, 1, false),
            (
                "below-alpha-half-step",
                [0.0; 3],
                0.49 / 255.0,
                false,
                64,
                true,
            ),
            (
                "above-alpha-half-step",
                [0.0; 3],
                0.51 / 255.0,
                false,
                1,
                false,
            ),
            ("black-absorber", [0.0; 3], 0.5, false, 1, false),
            (
                "sub-alpha-emission",
                [8.0, 0.0, 0.0],
                0.001,
                false,
                1,
                false,
            ),
            ("additive-light", [0.25, 0.0, 0.0], 0.5, true, 1, false),
        ] {
            let background = sheet_project(1, [0.4; 3], 0.5, false, false);
            let foreground = sheet_project(count, color, alpha, additive, false);
            let (mut app, first, target) =
                headless_project("medium", true, true, background.clone());
            app.world_mut().despawn(first);
            let back = spawn(&mut app, background.clone(), 1, 0.0);
            let front = spawn(
                &mut app,
                foreground.clone(),
                1,
                if opposite { -1.0 } else { 1.0 },
            );
            let mesh = app
                .world_mut()
                .resource_mut::<Assets<Mesh>>()
                .add(Rectangle::new(1.0, 1.0));
            for (entity, project) in [(back, background), (front, foreground)] {
                app.world_mut().entity_mut(entity).insert(
                    PresentedEffect::new(project.root.clone())
                        .with_mesh_overrides(BTreeMap::from([(MESH, mesh.clone())])),
                );
                app.world_mut().get_mut::<Transform>(entity).unwrap().scale = Vec3::splat(0.25);
            }
            let camera = enable(&mut app);
            *app.world_mut().get_mut::<Transform>(camera).unwrap() =
                Transform::from_xyz(0.0, 0.0, if opposite { -16.0 } else { 16.0 })
                    .looking_at(Vec3::ZERO, Vec3::Y);
            app.world_mut().entity_mut(camera).insert((
                bevy::camera::Hdr,
                Tonemapping::None,
                bevy::core_pipeline::tonemapping::DebandDither::Disabled,
            ));
            app.world()
                .resource::<ExperimentalOitDiagnostics>()
                .set_enabled(true);
            let _ = capture(&mut app, &target);
            forward(&mut app, back, front, 30);
            app.world_mut().entity_mut(front).insert(Visibility::Hidden);
            let control = capture(&mut app, &target);
            let control_count = app
                .world()
                .resource::<ExperimentalOitDiagnostics>()
                .snapshot()
                .unwrap();
            app.world_mut()
                .entity_mut(front)
                .insert(Visibility::Visible);
            let image = capture(&mut app, &target);
            let sample = app
                .world()
                .resource::<ExperimentalOitDiagnostics>()
                .snapshot()
                .unwrap();
            assert_eq!((sample.dropped_fragments, sample.invalid_links), (0, 0));
            assert!(control_count.attempted_fragments > 0);
            let base = center(&control);
            // Use native rounded 8-bit extinction, not unquantized alpha. RGB
            // quantization is covered by the existing 0.01 native image budget.
            let extinction = if additive {
                0.0
            } else {
                (alpha * 255.0_f32 + 0.5).floor() / 255.0
            };
            let expected: [f32; 3] =
                std::array::from_fn(|c| base[c] * (1.0 - extinction) + color[c] * alpha);
            let observed = center(&image);
            let error = (0..3)
                .map(|c| (observed[c] - expected[c]).abs())
                .fold(0.0_f32, f32::max);
            assert!(error < 0.01, "{name}: {observed:?} != {expected:?}");
            if identity {
                assert_eq!(
                    sample.attempted_fragments, control_count.attempted_fragments,
                    "identity allocated: {name}"
                );
                assert_eq!(image, control, "identity changed the image: {name}");
            } else {
                assert!(
                    sample.attempted_fragments > control_count.attempted_fragments,
                    "lost non-identity: {name}"
                );
                if name == "black-absorber" {
                    assert!(observed[0] < base[0] * 0.6);
                }
                if name == "sub-alpha-emission" {
                    assert!(observed[0] > base[0] + 0.004);
                }
            }
            let prefix = format!("{name}-view-{}", u8::from(opposite));
            image.save(root.join(format!("{prefix}.png"))).unwrap();
            cases.push(json!({"name":prefix,"identity":identity,"particles":count,
                "control_attempts":control_count.attempted_fragments,"attempts":sample.attempted_fragments,
                "dropped":sample.dropped_fragments,"invalid_links":sample.invalid_links,
                "center_linear":observed,"expected_linear":expected,"max_linear_error":error}));
        }
    }
    write_report(
        &root,
        &json!({"passed":true,"cases":cases,
        "scope":"Native RGB9E5 and rounded-alpha packing, 20 opposite-view semantic-mesh cases. Exact identities only; not an epsilon policy or arbitrary-density qualification."}),
    );
}
