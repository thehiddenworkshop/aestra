use super::*;

#[test]
fn review_views_are_explicit_and_motion_cannot_be_used_as_an_ordinary_cost_lane() {
    for name in ["host-default", "audience", "close", "wide", "side", "near"] {
        assert_eq!(
            ReviewCamera::parse(name, false),
            ReviewCamera::parse(name, true)
        );
    }
    assert!(ReviewCamera::parse("orbit", false).is_err());
    assert!(ReviewCamera::parse("orbit", true).is_ok());
    assert!(ReviewCamera::parse("unexpected", true).is_err());
}

#[test]
fn orbit_is_continuous_periodic_and_keeps_the_same_view_distance() {
    let mut previous = ReviewCamera::Orbit.pose(0.0).unwrap();
    for frame in 1..=960 {
        let pose = ReviewCamera::Orbit.pose(frame as f32 / 60.0).unwrap();
        assert!(pose.translation.is_finite() && pose.rotation.is_finite());
        assert!((Vec2::new(pose.translation.x, pose.translation.z).length() - 180.0).abs() < 0.001);
        assert!(pose.translation.distance(previous.translation) < 1.6);
        let toward = (Vec3::new(0.0, 28.0, 0.0) - pose.translation).normalize();
        assert!(pose.forward().dot(toward) > 0.99999);
        previous = pose;
    }
    assert!(
        previous
            .translation
            .distance(ReviewCamera::Orbit.pose(0.0).unwrap().translation)
            < 0.001
    );
}

#[test]
fn review_motion_reads_the_clock_without_advancing_or_seeking_the_effect() {
    let mut app = App::new();
    app.init_resource::<Readiness>().add_systems(Update, motion);
    let mut player = EffectPlayer::new(&aestra_bevy::EffectAsset::new("Clock", 40.0));
    player.set_seed(123);
    player.seek(4.0);
    let revision = player.instance().history_revision();
    let root = app.world_mut().spawn((ShowRoot, player)).id();
    let camera = app
        .world_mut()
        .spawn((AudienceCamera, ReviewCamera::Orbit, Transform::IDENTITY))
        .id();
    app.update();
    assert_eq!(
        *app.world().get::<Transform>(camera).unwrap(),
        Transform::IDENTITY
    );
    app.world_mut().resource_mut::<Readiness>().started = true;
    app.update();
    assert_eq!(
        *app.world().get::<Transform>(camera).unwrap(),
        ReviewCamera::Orbit.pose(4.0).unwrap()
    );
    let player = app.world().get::<EffectPlayer>(root).unwrap();
    assert_eq!(player.elapsed(), 4.0);
    assert_eq!(player.instance().seed(), 123);
    assert_eq!(player.instance().history_revision(), revision);
    let global = app.world().get::<GlobalTransform>(camera).unwrap();
    assert_eq!(
        global.translation(),
        ReviewCamera::Orbit.pose(4.0).unwrap().translation
    );
}
