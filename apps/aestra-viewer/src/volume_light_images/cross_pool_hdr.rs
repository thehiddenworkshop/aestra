//! Analytic HDR radiance bridge; screenshots are display-referred SDR, not raw HDR.
use super::*;
use crate::volume_light_images::outputs::cross_draw::sheet_program;
use aestra_bevy::{
    ExperimentalOitDiagnostics,
    preview::{DisplayTransform, PhotographicPreview},
};

pub(super) fn camera_hdr(app: &mut App, camera: Entity, stops: f32) {
    PhotographicPreview {
        exposure_stops: stops,
        tonemapping: DisplayTransform::Reinhard,
        bloom_intensity: 0.0,
    }
    .apply(&mut app.world_mut().commands().entity(camera));
    app.world_mut().flush();
}

fn display(radiance: f32, stops: f32) -> f32 {
    let c = radiance * 2.0_f32.powf(stops);
    c / (1.0 + c)
}

pub(super) fn center(image: &RgbaImage) -> [f32; 3] {
    std::array::from_fn(|c| {
        (176..184)
            .flat_map(|y| (236..244).map(move |x| linear(image.get_pixel(x, y)[c])))
            .sum::<f32>()
            / 64.0
    })
}

pub(super) fn center_difference(a: &RgbaImage, b: &RgbaImage) -> f32 {
    let mut maximum = 0.0_f32;
    for y in 176..184 {
        for x in 236..244 {
            for c in 0..3 {
                maximum = maximum
                    .max((linear(a.get_pixel(x, y)[c]) - linear(b.get_pixel(x, y)[c])).abs());
            }
        }
    }
    maximum
}

pub(super) fn image_difference(a: &RgbaImage, b: &RgbaImage) -> f32 {
    assert_eq!(a.dimensions(), b.dimensions());
    a.pixels()
        .zip(b.pixels())
        .flat_map(|(a, b)| (0..3).map(move |c| (linear(a[c]) - linear(b[c])).abs()))
        .fold(0.0, f32::max)
}

pub(super) fn healthy(app: &App) -> u32 {
    let sample = app
        .world()
        .resource::<ExperimentalOitDiagnostics>()
        .snapshot()
        .expect("missing GPU observation is not HDR acceptance");
    assert_eq!(sample.dropped_fragments, 0);
    assert_eq!(sample.over_sort_pixels, 0);
    assert_eq!(sample.invalid_links, 0);
    assert!(sample.attempted_fragments > 0);
    sample.attempted_fragments
}

pub(super) fn program(color: &str, lit: bool) -> MaterialProgram {
    let (parameters, expressions, output) = if lit {
        (
            r#"[(id:"a3574a00-0000-4000-8000-000000f81705",name:"Scene light gain",value_type:Float,evaluation_domain:Instance,default:Some(Float(0.02)))]"#,
            r#"(id:"a3574a00-0000-4000-8000-000000f81702",kind:Input(ScenePointIrradiance)),
        (id:"a3574a00-0000-4000-8000-000000f81703",kind:Parameter("a3574a00-0000-4000-8000-000000f81705")),
        (id:"a3574a00-0000-4000-8000-000000f81704",kind:Multiply("a3574a00-0000-4000-8000-000000f81702","a3574a00-0000-4000-8000-000000f81703")),
        (id:"a3574a00-0000-4000-8000-000000f81706",kind:Add("a3574a00-0000-4000-8000-000000f81701","a3574a00-0000-4000-8000-000000f81704")),"#,
            "a3574a00-0000-4000-8000-000000f81706",
        )
    } else {
        ("[]", "", "a3574a00-0000-4000-8000-000000f81701")
    };
    MaterialProgram::from_ron(&format!(r#"(
        id:"a3574a00-0000-4000-8000-000000f81700",schema_version:1,name:"HDR receiver oracle",domain:Sprite,
        render_state_policy:(default:(blend:Alpha,depth_test:LessEqual,depth_write:false,cull_mode:r#None),
        allowed:[(blend:Alpha,depth_test:LessEqual,depth_write:false,cull_mode:r#None)]),
        parameters:{parameters},expressions:[
        (id:"a3574a00-0000-4000-8000-000000f81701",kind:Constant(Vec3(({color})))),
        {expressions}
        (id:"a3574a00-0000-4000-8000-000000f81707",kind:Input(ParticleOpacity))],
        outputs:(color:"{output}",alpha:"a3574a00-0000-4000-8000-000000f81707"))"#)).unwrap()
}

pub(super) fn replace_blue(
    app: &mut App,
    blue: Entity,
    project: Arc<CompiledEffectProject>,
    seed: u64,
) {
    let mut player = EffectPlayer::from_project(project)
        .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    player.set_seed(seed);
    player.playing = false;
    app.world_mut().entity_mut(blue).insert(player);
}

#[test]
fn hdr_reference_rejects_clamping_and_mapping_before_compositing() {
    assert_eq!(display(5.0, 0.0), 5.0 / 6.0);
    assert!((display(5.0, -2.0) - 1.25 / 2.25).abs() < 0.00001);
    assert!((display(5.0, 0.0) - display(0.625, 0.0)).abs() > 0.1);
    assert!((display(5.0, 0.0) - 0.625 * display(8.0, 0.0)).abs() > 0.1);
    for lit in [false, true] {
        let p = sheet_program(1, EmitterShape::Point, 0.0, program("0.04,0.04,0.04", lit));
        assert_eq!(p.root.max_particles, 1);
    }
}

#[test]
#[ignore = "native HDR OIT radiance/exposure analytic oracle; run alone"]
fn fragment_list_hdr_preserves_radiance_before_display_transform() {
    let root = root("fragment-list-hdr");
    let (_, seed, z) = interleaved();
    let red = sheet_program(
        2,
        EmitterShape::Box {
            half_extents: [0.001, 0.001, 4.0],
        },
        0.0,
        program("8.0,0.0,0.0", false),
    );
    let blue = sheet_program(1, EmitterShape::Point, 0.0, program("0.0,0.0,4.0", false));
    let mut cases = Vec::new();
    for reversed in [false, true] {
        for opposite in [false, true] {
            let (mut app, red_entity, blue_entity, target) = pair(
                red.clone(),
                seed,
                reversed,
                opposite,
                false,
                INTERLEAVED_HOST_Z,
            );
            replace_blue(&mut app, blue_entity, blue.clone(), seed);
            let camera = enable(&mut app);
            app.world()
                .resource::<ExperimentalOitDiagnostics>()
                .set_enabled(true);
            forward(&mut app, red_entity, blue_entity, 30);
            let rb = source_over(&z, opposite);
            let mut base = None;
            for stops in [-2.0, 0.0, 2.0, 0.0] {
                camera_hdr(&mut app, camera, stops);
                let image = capture(&mut app, &target);
                let attempts = healthy(&app);
                let expected = [display(rb[0] * 8.0, stops), display(rb[1] * 4.0, stops)];
                let exact_error = error(&image, expected);
                assert!(
                    exact_error < 0.01,
                    "HDR exposure {stops}: error={exact_error}"
                );
                let clipped_error = error(&image, [display(rb[0], stops), display(rb[1], stops)]);
                assert!(clipped_error > 0.1, "HDR clamp control must fail");
                let mapped_first_error = error(
                    &image,
                    [rb[0] * display(8.0, stops), rb[1] * display(4.0, stops)],
                );
                assert!(
                    mapped_first_error > 0.1,
                    "mapping individual layers before blending must fail"
                );
                let restored = if stops == 0.0 {
                    base.as_ref().map(|old| delta(old, &image).1)
                } else {
                    None
                };
                if let Some(restored) = restored {
                    assert!(restored <= 1);
                }
                let name = format!(
                    "spawn-{}-view-{}-stops-{stops}-{}",
                    u8::from(reversed),
                    u8::from(opposite),
                    cases.len()
                );
                image.save(root.join(format!("{name}.png"))).unwrap();
                cases.push(
                    json!({"name":name,"stops":stops,"expected_linear_rb":expected,
                "actual_linear_rgb":center(&image),"max_linear_error":exact_error,
                "wrong_clipped_error":clipped_error,"wrong_mapped_first_error":mapped_first_error,
                "restoration_max_byte_error":restored,"attempted_fragments":attempts}),
                );
                if stops == 0.0 && base.is_none() {
                    base = Some(image);
                }
            }
            assert_native_transport(&app);
        }
    }
    write_report(
        &root,
        &json!({"probe_passed":true,"cases":cases,
        "scope":"Known HDR radiance composited before fixed Reinhard exposure, display-referred screenshots, not raw HDR/bloom/full-show acceptance."}),
    );
}
