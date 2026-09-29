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

#[test]
fn the_colliders_around_an_effect_become_its_proxies() {
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
    for _ in 0..8 {
        app.update();
    }
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
