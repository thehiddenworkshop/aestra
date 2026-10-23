//! The Avian adapter (host bindings HB10): the colliders around an effect become its physics proxies.

use aestra_bevy::{AestraPhysicsColliders, AestraPhysicsQuery, PhysicsProxy};
use aestra_bevy_avian::AestraAvianPlugin;
use avian3d::prelude::{Collider, PhysicsPlugins, RigidBody};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::diagnostic::DiagnosticsPlugin,
        PhysicsPlugins::default(),
        AestraAvianPlugin,
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    app
}

fn settle(app: &mut App) {
    for _ in 0..8 {
        app.update();
    }
}

#[test]
fn the_colliders_around_an_effect_become_its_proxies() {
    let mut app = app();
    let spawn = |app: &mut App, collider: Collider, transform: Transform| {
        app.world_mut()
            .spawn((RigidBody::Static, collider, transform));
    };
    spawn(
        &mut app,
        Collider::sphere(1.0),
        Transform::from_xyz(3.0, 0.0, 0.0),
    );
    spawn(
        &mut app,
        Collider::cuboid(2.0, 1.0, 4.0),
        Transform::from_xyz(-3.0, 1.0, 0.0).with_rotation(Quat::from_rotation_y(0.5)),
    );
    spawn(
        &mut app,
        Collider::capsule(0.5, 2.0),
        Transform::from_xyz(0.0, 0.0, 4.0),
    );
    spawn(
        &mut app,
        Collider::half_space(Vec3::Y),
        Transform::from_xyz(0.0, -2.0, 0.0),
    );
    spawn(
        &mut app,
        Collider::compound(vec![
            (
                Vec3::new(1.0, 0.0, 0.0),
                Quat::IDENTITY,
                Collider::sphere(0.5),
            ),
            (
                Vec3::new(-1.0, 0.0, 0.0),
                Quat::IDENTITY,
                Collider::sphere(0.5),
            ),
        ]),
        Transform::from_xyz(0.0, 5.0, 0.0),
    );
    // Out of reach.
    spawn(
        &mut app,
        Collider::sphere(1.0),
        Transform::from_xyz(40.0, 0.0, 0.0),
    );
    let effect = app
        .world_mut()
        .spawn((Transform::default(), AestraPhysicsQuery::within(10.0)))
        .id();
    app.finish();
    app.cleanup();
    settle(&mut app);
    let proxies = &app
        .world()
        .get::<AestraPhysicsColliders>(effect)
        .expect("the adapter filled the effect's colliders")
        .0
        .proxies;
    assert_eq!(proxies.len(), 6, "{proxies:#?}");
    assert!(proxies.iter().any(|proxy| matches!(
        proxy,
        PhysicsProxy::Sphere { center, radius } if near(*center, [3.0, 0.0, 0.0]) && *radius == 1.0
    )));
    assert!(proxies.iter().any(|proxy| matches!(
        proxy,
        PhysicsProxy::Box { center, half_extents, rotation }
            if near(*center, [-3.0, 1.0, 0.0])
                && near(*half_extents, [1.0, 0.5, 2.0])
                && Quat::from_array(*rotation).angle_between(Quat::from_rotation_y(0.5)) < 1e-4
    )));
    assert!(proxies.iter().any(|proxy| matches!(
        proxy,
        PhysicsProxy::Capsule { a, b, radius }
            if *radius == 0.5
                && ((near(*a, [0.0, -1.0, 4.0]) && near(*b, [0.0, 1.0, 4.0]))
                    || (near(*a, [0.0, 1.0, 4.0]) && near(*b, [0.0, -1.0, 4.0])))
    )));
    assert!(proxies.iter().any(|proxy| matches!(
        proxy,
        PhysicsProxy::HalfSpace { normal, distance } if near(*normal, [0.0, 1.0, 0.0]) && (*distance + 2.0).abs() < 1e-4
    )));
    let compound_parts = proxies
        .iter()
        .filter(|proxy| matches!(
            proxy,
            PhysicsProxy::Sphere { center, radius } if (center[1] - 5.0).abs() < 1e-4 && *radius == 0.5
        ))
        .count();
    assert_eq!(compound_parts, 2, "a compound becomes its parts");
}

#[test]
fn moving_scaled_colliders_refresh_and_removed_or_distant_colliders_clear() {
    let mut app = app();
    let body = app
        .world_mut()
        .spawn((
            RigidBody::Kinematic,
            Collider::sphere(0.5),
            Transform::from_xyz(2.0, 0.0, 0.0),
        ))
        .id();
    let effect = app
        .world_mut()
        .spawn((Transform::default(), AestraPhysicsQuery::within(10.0)))
        .id();
    app.finish();
    app.cleanup();
    settle(&mut app);
    let proxies = &app
        .world()
        .get::<AestraPhysicsColliders>(effect)
        .unwrap()
        .0
        .proxies;
    assert!(
        matches!(proxies.as_slice(), [PhysicsProxy::Sphere { center, radius }] if near(*center, [2.0, 0.0, 0.0]) && *radius == 0.5)
    );

    *app.world_mut().get_mut::<Transform>(body).unwrap() =
        Transform::from_xyz(5.0, 1.0, 0.0).with_scale(Vec3::splat(2.0));
    settle(&mut app);
    let proxies = &app
        .world()
        .get::<AestraPhysicsColliders>(effect)
        .unwrap()
        .0
        .proxies;
    assert!(
        matches!(proxies.as_slice(), [PhysicsProxy::Sphere { center, radius }] if near(*center, [5.0, 1.0, 0.0]) && *radius == 1.0),
        "{proxies:?}"
    );

    app.world_mut()
        .entity_mut(effect)
        .insert(AestraPhysicsQuery::within(1.0));
    settle(&mut app);
    assert!(
        app.world()
            .get::<AestraPhysicsColliders>(effect)
            .unwrap()
            .0
            .proxies
            .is_empty()
    );
    app.world_mut()
        .entity_mut(effect)
        .insert(AestraPhysicsQuery::within(10.0));
    settle(&mut app);
    assert_eq!(
        app.world()
            .get::<AestraPhysicsColliders>(effect)
            .unwrap()
            .0
            .proxies
            .len(),
        1
    );
    app.world_mut().despawn(body);
    settle(&mut app);
    assert!(
        app.world()
            .get::<AestraPhysicsColliders>(effect)
            .unwrap()
            .0
            .proxies
            .is_empty()
    );
}

#[test]
fn query_budget_keeps_nearest_to_the_current_effect_pose() {
    let mut app = app();
    for x in [2.0, 4.0] {
        app.world_mut().spawn((
            RigidBody::Static,
            Collider::sphere(0.5),
            Transform::from_xyz(x, 0.0, 0.0),
        ));
    }
    let mut query = AestraPhysicsQuery::within(10.0);
    query.max = 1;
    let effect = app.world_mut().spawn((Transform::default(), query)).id();
    app.finish();
    app.cleanup();
    for (host_x, expected_x) in [(0.0, 2.0), (5.0, 4.0)] {
        app.world_mut()
            .get_mut::<Transform>(effect)
            .unwrap()
            .translation
            .x = host_x;
        settle(&mut app);
        let proxies = &app
            .world()
            .get::<AestraPhysicsColliders>(effect)
            .unwrap()
            .0
            .proxies;
        assert!(
            matches!(proxies.as_slice(), [PhysicsProxy::Sphere { center, .. }] if near(*center, [expected_x, 0.0, 0.0])),
            "{proxies:?}"
        );
    }
}
