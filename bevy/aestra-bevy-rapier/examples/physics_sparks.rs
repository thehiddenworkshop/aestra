//! Sparks bouncing off Rapier's rigid bodies (host bindings HB10).
//!
//! A fountain throws sparks over a ramp; balls roll down it. The effect carries an
//! `AestraPhysicsQuery`, so `AestraRapierPlugin` hands it the Rapier colliders around it every frame,
//! and the sparks' `Physics` collider bounces them off the ground, the ramp and the rolling balls —
//! on the GPU, whatever the balls do.
//!
//! ```sh
//! cargo run -p aestra-bevy-rapier --example physics_sparks --release
//! ```
//!
//! With `AESTRA_EXAMPLE_CAPTURE=<file.png>` it saves a screenshot after four seconds and exits.

use aestra_bevy::{
    AestraPhysicsQuery, AestraPlugin, Collider as AestraCollider, ColliderShape, ColorKey, Curve,
    CurveKey, EffectAsset, EffectPlaybackMode, EffectPlayer, Emitter, EmitterShape, Gradient,
    ModuleInstance, ScalarRange,
};
use aestra_bevy_rapier::AestraRapierPlugin;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy_rapier3d::prelude::{Collider, NoUserData, RapierPhysicsPlugin, RigidBody};

/// Frames since the screenshot was taken, when it was.
#[derive(Resource, Default)]
struct Captured(Option<u32>);

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            RapierPhysicsPlugin::<NoUserData>::default(),
            AestraPlugin,
            AestraRapierPlugin,
        ))
        .init_resource::<Captured>()
        .add_systems(Startup, setup)
        .add_systems(Update, capture)
        .run();
}

/// Bright sparks that bounce off whatever the host's physics has around them.
pub fn sparks() -> EffectAsset {
    let mut effect = EffectAsset::new("Physics Sparks", 6.0);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut emitter = Emitter::basic_sprite("Sparks", 6.0);
    emitter.max_particles = 2048;
    emitter.modules = vec![
        ModuleInstance::emission(160.0, 0),
        ModuleInstance::shape(EmitterShape::Sphere { radius: 0.3 }),
        ModuleInstance::initialize(
            ScalarRange::new(2.5, 3.5),
            ScalarRange::new(8.0, 12.0),
            [1.0, 0.7, 0.0],
            18.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0, -12.0, 0.0], 0.1, 0.0),
        ModuleInstance::collision(vec![AestraCollider {
            shape: ColliderShape::Physics { radius: 0.15 },
            restitution: 0.45,
            friction: 0.2,
            kill: false,
        }]),
        ModuleInstance::appearance(
            Curve::new(vec![CurveKey::new(0.0, 0.5), CurveKey::new(1.0, 0.25)]),
            Curve::new(vec![
                CurveKey::new(0.0, 1.0),
                CurveKey::new(0.8, 0.8),
                CurveKey::new(1.0, 0.0),
            ]),
            Gradient::new(vec![
                ColorKey::new(0.0, [1.0, 0.95, 0.7, 1.0]),
                ColorKey::new(0.5, [1.0, 0.6, 0.2, 1.0]),
                ColorKey::new(1.0, [0.8, 0.2, 0.05, 1.0]),
            ]),
        ),
    ];
    effect.emitters.push(emitter);
    effect
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(2.0, 14.0, 34.0).looking_at(Vec3::new(2.0, 3.0, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 4_000.0,
            ..default()
        },
        Transform::from_xyz(-20.0, 40.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // The ground.
    commands.spawn((
        RigidBody::Fixed,
        Collider::halfspace(Vec3::Y).expect("a unit normal"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(60.0, 40.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.18, 0.2, 0.22))),
    ));
    // A ramp the sparks land on and the balls roll down.
    commands.spawn((
        RigidBody::Fixed,
        Collider::cuboid(6.0, 0.3, 4.0),
        Mesh3d(meshes.add(Cuboid::new(12.0, 0.6, 8.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.4, 0.42, 0.45))),
        Transform::from_xyz(4.0, 3.0, 0.0).with_rotation(Quat::from_rotation_z(0.35)),
    ));
    // Balls dropped onto the ramp.
    let ball = meshes.add(Sphere::new(1.0));
    let ball_material = materials.add(Color::srgb(0.25, 0.45, 0.8));
    for (index, x) in [5.0f32, 7.0, 9.0].into_iter().enumerate() {
        commands.spawn((
            RigidBody::Dynamic,
            Collider::ball(1.0),
            Mesh3d(ball.clone()),
            MeshMaterial3d(ball_material.clone()),
            Transform::from_xyz(x, 9.0 + 3.0 * index as f32, (index as f32 - 1.0) * 2.0),
        ));
    }
    // The fountain.
    commands.spawn((
        EffectPlayer::new(&sparks()),
        AestraPhysicsQuery::within(40.0),
        Transform::from_xyz(-8.0, 6.0, 0.0),
    ));
}

/// `AESTRA_EXAMPLE_CAPTURE`: a screenshot after four seconds, then exit.
fn capture(
    time: Res<Time>,
    mut commands: Commands,
    mut captured: ResMut<Captured>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(path) = std::env::var("AESTRA_EXAMPLE_CAPTURE") else {
        return;
    };
    match captured.0.as_mut() {
        None if time.elapsed_secs() >= 4.0 => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
            captured.0 = Some(0);
        }
        Some(since) => {
            *since += 1;
            if *since == 10 {
                exit.write(AppExit::Success);
            }
        }
        None => {}
    }
}
