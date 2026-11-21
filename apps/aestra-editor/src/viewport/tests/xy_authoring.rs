use super::*;

#[test]
fn xy_handles_match_the_screen_axes_and_keep_the_planar_drag() {
    let mut settings = TransformGizmoSettings::default();
    let start = Transform::from_xyz(10.0, 20.0, 4.25)
        .with_rotation(Quat::from_rotation_x(0.25))
        .with_scale(Vec3::new(2.0, 3.0, 4.0));
    let center = start.translation.truncate();
    assert_eq!(hit_handle(&settings, start, Vec2::ZERO), Some(Axis::View));
    assert_eq!(
        hit_handle(&settings, start, Vec2::NEG_Y * 36.0),
        Some(Axis::Y)
    );
    let moved = drag(
        &settings,
        Axis::View,
        start,
        center,
        center + Vec2::new(4.0, 6.0),
    );
    assert_eq!(moved.translation, Vec3::new(14.0, 26.0, 4.25));

    settings.mode = Mode::Rotate;
    assert_eq!(
        hit_handle(&settings, start, Vec2::X * RING_RADIUS),
        Some(Axis::Z)
    );
    assert_eq!(hit_handle(&settings, start, Vec2::ZERO), None);
    let rotated = drag(
        &settings,
        Axis::Z,
        start,
        center + Vec2::X,
        center + Vec2::Y,
    );
    let expected = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) * start.rotation;
    assert!(rotated.rotation.abs_diff_eq(expected, 0.0001));

    settings.mode = Mode::Scale;
    assert_eq!(hit_handle(&settings, start, Vec2::ZERO), None);
    let scaled = drag(
        &settings,
        Axis::Y,
        start,
        center + Vec2::Y,
        center + Vec2::Y * 2.0,
    );
    assert_eq!(scaled.scale, Vec3::new(2.0, 6.0, 4.0));
}
