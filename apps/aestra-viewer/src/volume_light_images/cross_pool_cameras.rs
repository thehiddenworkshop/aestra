//! Shared-list offscreen target/camera lifecycle; diagnostics stay unknown with two cameras.
use super::*;
use aestra_bevy::ExperimentalOitDiagnostics;
use bevy::render::render_resource::Extent3d;

pub(super) fn patch_error(image: &RgbaImage, expected: [f32; 3]) -> f32 {
    let (cx, cy) = (image.width() / 2, image.height() / 2);
    let mut max = 0.0_f32;
    for y in cy - 4..cy + 4 {
        for x in cx - 4..cx + 4 {
            for (c, expected) in expected.iter().enumerate() {
                max = max.max((linear(image.get_pixel(x, y)[c]) - expected).abs());
            }
        }
    }
    max
}

fn target(app: &mut App, width: u32, height: u32) -> Handle<Image> {
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            width,
            height,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ))
}

fn allocation(app: &App, cameras: &[Entity], extent: UVec2) -> Value {
    let a = app
        .world()
        .resource::<ExperimentalOitStatistics>()
        .snapshot();
    let pixels = extent.x as usize * extent.y as usize;
    assert_eq!((a.head_capacity, a.node_capacity), (pixels, pixels * 4));
    assert_eq!(a.storage_bytes, pixels as u64 * 52);
    for camera in cameras {
        let s = app.world().get::<ExperimentalOitStatus>(*camera).unwrap();
        assert_eq!(s.rejection, None);
        assert_eq!(s.estimated_storage_bytes, a.storage_bytes);
    }
    json!({"extent":extent.to_array(),"heads":a.head_capacity,"nodes":a.node_capacity,"bytes":a.storage_bytes})
}

fn healthy_single(app: &App, camera: Entity, size: UVec2) {
    let s = app
        .world()
        .resource::<ExperimentalOitDiagnostics>()
        .snapshot()
        .unwrap();
    assert_eq!(s.camera, camera);
    assert_eq!(s.target_size, size);
    assert!(s.attempted_fragments > 0);
    assert_eq!(
        (s.dropped_fragments, s.over_sort_pixels, s.invalid_links),
        (0, 0, 0)
    );
}

#[test]
#[ignore = "native shared-list target resizing, camera isolation, rejection and removal; run alone"]
fn fragment_list_camera_lifecycle_preserves_independent_views_and_releases_storage() {
    let root = root("fragment-list-camera-lifecycle");
    let (mut app, red, blue, first_target) = pair(sheet(true), 1, false, false, false, -3.0);
    let first = enable(&mut app);
    app.world()
        .resource::<ExperimentalOitDiagnostics>()
        .set_enabled(true);
    let _ = capture(&mut app, &first_target);
    forward(&mut app, red, blue, 30);
    let original = capture(&mut app, &first_target);
    assert!(patch_error(&original, [0.25, 0.0, 0.5]) < 0.01);
    healthy_single(&app, first, UVec2::new(480, 360));
    original.save(root.join("single-original.png")).unwrap();
    let mut stages = vec![
        json!({"stage":"single","allocation":allocation(&app,&[first],UVec2::new(480,360)),"diagnostics":"healthy single camera"}),
    ];
    let second_target = target(&mut app, 320, 480);
    let second = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Camera {
                order: 1,
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            RenderTarget::Image(second_target.clone().into()),
            Transform::from_xyz(0.0, 0.0, -16.0).looking_at(Vec3::ZERO, Vec3::Y),
            Msaa::Off,
            Tonemapping::None,
            ExperimentalOitSettings::default(),
        ))
        .id();
    for (stage, size, order) in [
        ("two-original", UVec2::new(480, 360), 1),
        ("wide-first", UVec2::new(640, 240), 1),
        ("tall-first", UVec2::new(240, 640), -1),
        ("two-restored", UVec2::new(480, 360), 1),
    ] {
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .get_mut(&first_target)
            .unwrap()
            .resize(Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            });
        app.world_mut().get_mut::<Camera>(second).unwrap().order = order;
        let a = capture(&mut app, &first_target);
        let b = capture(&mut app, &second_target);
        assert_eq!(a.dimensions(), (size.x, size.y));
        let ea = patch_error(&a, [0.25, 0.0, 0.5]);
        let eb = patch_error(&b, [0.5, 0.0, 0.25]);
        assert!(
            ea < 0.01 && eb < 0.01,
            "{stage} wrong camera colors {ea}/{eb}"
        );
        assert!(
            app.world()
                .resource::<ExperimentalOitDiagnostics>()
                .snapshot()
                .is_none(),
            "multiple cameras must not expose a stale single-camera success"
        );
        if size == UVec2::new(480, 360) {
            assert_eq!(delta(&original, &a).1, 0);
        }
        // Whole-image isolation controls: render each target without the other
        // active camera, then restore both. Center colors alone cannot detect
        // residual list heads elsewhere in the shared buffer.
        app.world_mut().get_mut::<Camera>(second).unwrap().is_active = false;
        let solo_a = capture(&mut app, &first_target);
        app.world_mut().get_mut::<Camera>(first).unwrap().is_active = false;
        app.world_mut().get_mut::<Camera>(second).unwrap().is_active = true;
        let solo_b = capture(&mut app, &second_target);
        app.world_mut().get_mut::<Camera>(first).unwrap().is_active = true;
        let restored_a = capture(&mut app, &first_target);
        let restored_b = capture(&mut app, &second_target);
        for (paired, solo, restored) in [(&a, &solo_a, &restored_a), (&b, &solo_b, &restored_b)] {
            assert_eq!(delta(paired, solo).1, 0, "{stage} camera isolation");
            assert_eq!(delta(paired, restored).1, 0, "{stage} camera reactivation");
        }
        assert!(
            app.world()
                .resource::<ExperimentalOitDiagnostics>()
                .snapshot()
                .is_none()
        );
        for (suffix, image) in [
            ("solo-first", &solo_a),
            ("solo-second", &solo_b),
            ("reactivated-first", &restored_a),
            ("reactivated-second", &restored_b),
        ] {
            image
                .save(root.join(format!("{stage}-{suffix}.png")))
                .unwrap();
        }
        a.save(root.join(format!("{stage}-first.png"))).unwrap();
        b.save(root.join(format!("{stage}-second.png"))).unwrap();
        stages.push(json!({"stage":stage,"first_size":size.to_array(),"second_order":order,
            "first_patch_error":ea,"second_patch_error":eb,"diagnostics":"unavailable: multiple cameras",
            "whole_image_isolation_bytes":0,"reactivation_bytes":0,
            "allocation":allocation(&app,&[first,second],size.max(UVec2::new(320,480)))}));
    }
    // The strictest participating budget must reject the whole shared allocation.
    app.world_mut()
        .entity_mut(second)
        .insert(ExperimentalOitSettings {
            max_storage_bytes: 1,
            ..default()
        });
    let rejected = capture(&mut app, &first_target);
    for camera in [first, second] {
        assert!(
            app.world()
                .get::<ExperimentalOitStatus>(camera)
                .unwrap()
                .rejection
                .is_some()
        );
    }
    assert!(
        app.world()
            .resource::<ExperimentalOitDiagnostics>()
            .snapshot()
            .is_none()
    );
    let released = app
        .world()
        .resource::<ExperimentalOitStatistics>()
        .snapshot()
        .storage_bytes;
    assert!(released <= 16);
    rejected
        .save(root.join("shared-budget-rejected.png"))
        .unwrap();
    stages.push(json!({"stage":"shared-budget-rejected","storage_bytes":released,"diagnostics":"unavailable: rejected"}));
    app.world_mut()
        .entity_mut(second)
        .insert(ExperimentalOitSettings::default());
    let restored = capture(&mut app, &first_target);
    assert_eq!(delta(&original, &restored).1, 0);
    restored
        .save(root.join("shared-budget-restored.png"))
        .unwrap();
    stages.push(json!({"stage":"shared-budget-restored","allocation":allocation(&app,&[first,second],UVec2::new(480,480)),"restore_bytes":0}));

    app.world_mut().entity_mut(second).despawn();
    let solo = capture(&mut app, &first_target);
    healthy_single(&app, first, UVec2::new(480, 360));
    assert_eq!(delta(&original, &solo).1, 0);
    solo.save(root.join("second-removed.png")).unwrap();
    stages.push(json!({"stage":"second-removed","allocation":allocation(&app,&[first],UVec2::new(480,360)),"diagnostics":"healthy fresh single-camera sample","restore_bytes":0}));
    for size in [
        UVec2::splat(256),
        UVec2::new(800, 360),
        UVec2::new(480, 360),
    ] {
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .get_mut(&first_target)
            .unwrap()
            .resize(Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            });
        let image = capture(&mut app, &first_target);
        assert_eq!(image.dimensions(), (size.x, size.y));
        healthy_single(&app, first, size);
        let error = patch_error(&image, [0.25, 0.0, 0.5]);
        assert!(error < 0.01);
        if size == UVec2::new(480, 360) {
            assert_eq!(delta(&original, &image).1, 0);
        }
        image
            .save(root.join(format!("single-resize-{}-{}.png", size.x, size.y)))
            .unwrap();
        stages.push(json!({"stage":"single-resize","size":size.to_array(),"patch_error":error,
            "allocation":allocation(&app,&[first],size),"diagnostics":"fresh sample with current target size"}));
    }
    // Rebind to a different image with the same dimensions, then restore the old target.
    let replacement = target(&mut app, 480, 360);
    app.world_mut()
        .entity_mut(first)
        .insert(RenderTarget::Image(replacement.clone().into()));
    let rebound = capture(&mut app, &replacement);
    healthy_single(&app, first, UVec2::new(480, 360));
    assert_eq!(delta(&original, &rebound).1, 0);
    rebound.save(root.join("target-rebound.png")).unwrap();
    app.world_mut()
        .entity_mut(first)
        .insert(RenderTarget::Image(first_target.clone().into()));
    let rebound = capture(&mut app, &first_target);
    assert_eq!(delta(&original, &rebound).1, 0);
    rebound.save(root.join("target-restored.png")).unwrap();
    stages.push(json!({"stage":"target-rebind-restore","restore_bytes":0,"allocation":allocation(&app,&[first],UVec2::new(480,360))}));
    app.world_mut()
        .entity_mut(first)
        .remove::<ExperimentalOitSettings>();
    let _ = capture(&mut app, &first_target);
    assert!(app.world().get::<ExperimentalOitStatus>(first).is_none());
    assert!(
        app.world()
            .resource::<ExperimentalOitDiagnostics>()
            .snapshot()
            .is_none()
    );
    let released = app
        .world()
        .resource::<ExperimentalOitStatistics>()
        .snapshot()
        .storage_bytes;
    assert!(released <= 16);
    stages.push(
        json!({"stage":"last-opt-in-removed","storage_bytes":released,"diagnostics":"unavailable"}),
    );
    assert_native_transport(&app);
    for owner in [red, blue] {
        assert_eq!(app.world().get::<EffectPlayer>(owner).unwrap().frame(), 30);
    }
    write_report(
        &root,
        &json!({"probe_passed":true,"stages":stages,"scope":"Full-target offscreen images; frozen alpha sheets, not multi-camera overflow diagnostics or live show costs."}),
    );
}
