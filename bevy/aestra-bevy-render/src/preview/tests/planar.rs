use super::*;
use bevy::camera::CameraProjection;

#[test]
fn mirror_rays_and_reverse_z_clip_preserve_the_water_plane() {
    let source = GlobalTransform::from(
        Transform::from_xyz(2.0, 6.0, 30.0).looking_at(Vec3::new(0.0, 4.0, 0.0), Vec3::Y),
    );
    let pose = math::reflected_pose(&source, -0.5);
    assert_eq!(pose.translation, Vec3::new(2.0, -7.0, 30.0));
    let world_plane = Vec4::new(0.0, 1.0, 0.0, 0.5);
    let projection = math::MirrorProjection {
        base: PerspectiveProjection::default(),
        plane: pose.to_matrix().transpose() * world_plane,
    };
    let clip = projection.get_clip_from_view() * pose.to_matrix().inverse();
    for point in [Vec3::ZERO.with_y(-0.5), Vec3::new(3.0, -0.5, 10.0)] {
        let p = clip * point.extend(1.0);
        assert!((p.w - p.z).abs() < 0.0001);
    }
    let above = clip * Vec4::new(0.0, 1.0, 0.0, 1.0);
    let below = clip * Vec4::new(0.0, -1.0, 0.0, 1.0);
    assert!(above.w - above.z > 0.0 && below.w - below.z < 0.0);
    assert!(
        pose.to_matrix().determinant() > 0.0,
        "proper camera basis keeps ordinary occluder winding"
    );
}

#[test]
fn reflection_target_budget_scales_pixels_without_effect_owner_limits() {
    assert_eq!(
        math::extent(UVec2::new(1280, 720), 0.5, 16 * 1024 * 1024),
        Some((UVec2::new(640, 360), 8_908_504))
    );
    let (size, bytes) = math::extent(UVec2::new(7680, 4320), 0.5, 16 * 1024 * 1024).unwrap();
    assert!(size.x < 3840 && bytes <= 16 * 1024 * 1024);
    for scale in [0.0, -1.0, f32::NAN, 2.0] {
        assert!(math::extent(UVec2::ONE, scale, 1024).is_none());
    }
}

#[test]
fn reflection_sees_sources_above_main_view_and_skips_an_absent_water_plane() {
    let source = GlobalTransform::from(
        Transform::from_xyz(0.0, 18.0, 120.0).looking_at(Vec3::ZERO, Vec3::Y),
    );
    let base = PerspectiveProjection::default();
    let mirror = math::reflected_pose(&source, -0.5);
    let projection = math::MirrorProjection {
        base: base.clone(),
        plane: mirror.to_matrix().transpose() * Vec4::new(0.0, 1.0, 0.0, 0.48),
    };
    let point = Vec4::new(0.0, 50.0, 0.0, 1.0);
    let main = base.get_clip_from_view() * source.to_matrix().inverse() * point;
    let reflected = projection.get_clip_from_view() * mirror.to_matrix().inverse() * point;
    assert!(main.y / main.w > 1.0);
    assert!(
        (reflected.y / reflected.w).abs() < 1.0 && reflected.z >= 0.0 && reflected.z <= reflected.w
    );
    assert!(math::sees_plane(&source, &base));
    let sky =
        GlobalTransform::from(Transform::from_xyz(0.0, 18.0, 120.0).looking_to(Vec3::Y, Vec3::Z));
    assert!(!math::sees_plane(&sky, &base));
}

#[test]
fn presentation_view_reuses_targets_resizes_and_retires_without_touching_instances() {
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
    let mut app = App::new();
    app.init_resource::<Captures>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<WaterMaterial>>()
        .add_systems(Update, sync);
    let material =
        app.world_mut()
            .resource_mut::<Assets<WaterMaterial>>()
            .add(super::super::water::material(
                super::super::water::SeaState::Calm,
            ));
    let receiver = app
        .world_mut()
        .spawn((
            MeshMaterial3d(material.clone()),
            GlobalTransform::IDENTITY,
            Visibility::Inherited,
            InheritedVisibility::VISIBLE,
        ))
        .id();
    let player = app
        .world_mut()
        .spawn(crate::PresentedEffect::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&aestra_core::EffectAsset::new("Reflection ownership", 20.0))
                .unwrap()
                .into(),
        ))
        .id();
    let source = app
        .world_mut()
        .spawn((
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(1280, 720),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                ..default()
            },
            Projection::default(),
            GlobalTransform::from(Transform::from_xyz(0.0, 6.0, 30.0)),
            PlanarReflection::new(receiver),
        ))
        .id();
    app.update();
    let capture = &app.world().resource::<Captures>().0[&source];
    let image = capture.image.clone();
    let filtered = capture.filtered.clone();
    let mirror = capture.camera;
    assert_eq!(capture.size, UVec2::new(640, 360));
    app.world_mut()
        .get_mut::<crate::PresentedEffect>(player)
        .unwrap()
        .instance
        .seek(3.5);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        app.world()
            .get::<crate::PresentedEffect>(player)
            .unwrap()
            .instance
            .time(),
        3.5
    );
    app.world_mut()
        .get_mut::<crate::PresentedEffect>(player)
        .unwrap()
        .instance
        .restart();
    assert_eq!(app.world().resource::<Captures>().0[&source].camera, mirror);
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 2);
    app.world_mut()
        .get_mut::<Camera>(source)
        .unwrap()
        .computed
        .target_info
        .as_mut()
        .unwrap()
        .physical_size = UVec2::new(640, 360);
    app.update();
    assert_eq!(app.world().resource::<Captures>().0[&source].image, image);
    assert_eq!(
        app.world()
            .resource::<Assets<Image>>()
            .get(&image)
            .unwrap()
            .size(),
        UVec2::new(320, 180)
    );
    assert_eq!(
        app.world().resource::<Captures>().0[&source].filtered,
        filtered
    );
    let clip = app
        .world()
        .get::<Projection>(mirror)
        .unwrap()
        .get_clip_from_view()
        * app
            .world()
            .get::<GlobalTransform>(mirror)
            .unwrap()
            .to_matrix()
            .inverse();
    assert!(
        app.world()
            .resource::<Assets<WaterMaterial>>()
            .get(&material)
            .unwrap()
            .extension
            .reflection_matches(&filtered, clip, UVec2::new(320, 180))
    );
    // Invalid edits fail closed instead of retaining an old reflected frame.
    app.world_mut()
        .get_mut::<PlanarReflection>(source)
        .unwrap()
        .resolution_scale = f32::NAN;
    app.update();
    assert!(app.world().resource::<Captures>().0.is_empty());
    assert!(app.world().resource::<Assets<Image>>().is_empty());
    assert!(app.world().get_entity(mirror).is_err());
    assert!(
        app.world()
            .get::<PlanarReflectionStatus>(source)
            .unwrap()
            .snapshot()
            .rejection
            .is_some()
    );
    assert!(
        !app.world()
            .resource::<Assets<WaterMaterial>>()
            .get(&material)
            .unwrap()
            .extension
            .reflection_matches(&filtered, clip, UVec2::new(320, 180))
    );
    app.world_mut()
        .get_mut::<PlanarReflection>(source)
        .unwrap()
        .resolution_scale = 0.5;
    app.update();
    app.world_mut()
        .entity_mut(source)
        .remove::<PlanarReflection>();
    app.update();
    assert!(app.world().resource::<Captures>().0.is_empty());
    assert!(app.world().resource::<Assets<Image>>().is_empty());
    assert_eq!(
        app.world_mut()
            .query::<&crate::PresentedEffect>()
            .iter(app.world())
            .count(),
        1
    );
    assert_eq!(
        app.world()
            .get::<crate::PresentedEffect>(player)
            .unwrap()
            .instance
            .time(),
        0.0
    );
}
