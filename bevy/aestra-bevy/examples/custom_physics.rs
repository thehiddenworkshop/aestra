//! Particles colliding with a game's own physics (host bindings HB10), no physics crate involved.
//!
//! Aestra ships adapters for Rapier (`aestra-bevy-rapier`) and Avian (`aestra-bevy-avian`). A game
//! with its own physics — or none — describes its colliders itself: one system writes the
//! effect's `AestraPhysicsColliders` every frame from whatever the game simulates, as `PhysicsProxy`
//! primitives in world space (`PhysicsPose` places them). Here two balls the game moves on circles,
//! and the ground, deflect a fountain of sparks whose emitter carries a `Physics` collider.
//!
//! ```sh
//! cargo run -p aestra-bevy --example custom_physics --release
//! ```
//!
//! With `AESTRA_EXAMPLE_CAPTURE=<file.png>` it saves a screenshot after four seconds and exits.

use aestra_bevy::{
    AestraPhysicsColliders, AestraPlugin, Collider, ColliderShape, ColorKey, Curve, CurveKey,
    EffectAsset, EffectPlaybackMode, EffectPlayer, Emitter, EmitterShape, Gradient, ModuleInstance,
    PhysicsPose, PhysicsProxy, ScalarRange,
};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

/// A ball the game moves on a circle: its own "physics".
#[derive(Component)]
struct Orbiting {
    radius: f32,
    speed: f32,
    phase: f32,
    size: f32,
}

#[derive(Component)]
struct Fountain;

/// Frames since the screenshot was taken, when it was.
#[derive(Resource, Default)]
struct Captured(Option<u32>);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, AestraPlugin))
        .init_resource::<Captured>()
        .add_systems(Startup, setup)
        .add_systems(Update, (orbit, describe_colliders, capture).chain())
        .run();
}

fn fountain() -> EffectAsset {
    let mut effect = EffectAsset::new("Deflected Sparks", 6.0);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut emitter = Emitter::basic_sprite("Sparks", 6.0);
    emitter.max_particles = 2048;
    emitter.modules = vec![
        ModuleInstance::emission(200.0, 0),
        ModuleInstance::shape(EmitterShape::Sphere { radius: 0.3 }),
        ModuleInstance::initialize(
            ScalarRange::new(2.5, 3.0),
            ScalarRange::new(12.0, 16.0),
            [0.0, 1.0, 0.0],
            12.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0, -14.0, 0.0], 0.05, 0.0),
        ModuleInstance::collision(vec![Collider {
            shape: ColliderShape::Physics { radius: 0.15 },
            restitution: 0.5,
            friction: 0.1,
            kill: false,
        }]),
        ModuleInstance::appearance(
            Curve::new(vec![CurveKey::new(0.0, 0.5), CurveKey::new(1.0, 0.25)]),
            Curve::new(vec![CurveKey::new(0.0, 1.0), CurveKey::new(1.0, 0.0)]),
            Gradient::new(vec![
                ColorKey::new(0.0, [0.7, 1.0, 0.9, 1.0]),
                ColorKey::new(1.0, [0.1, 0.5, 1.0, 1.0]),
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
        Transform::from_xyz(0.0, 12.0, 30.0).looking_at(Vec3::new(0.0, 5.0, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 4_000.0,
            ..default()
        },
        Transform::from_xyz(-20.0, 40.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 40.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.18, 0.2, 0.22))),
    ));
    let material = materials.add(Color::srgb(0.8, 0.45, 0.2));
    for (index, (radius, speed, size)) in [(3.0f32, 1.3f32, 1.2f32), (5.0, -0.8, 1.6)]
        .into_iter()
        .enumerate()
    {
        commands.spawn((
            Orbiting {
                radius,
                speed,
                phase: index as f32 * 2.0,
                size,
            },
            Mesh3d(meshes.add(Sphere::new(size))),
            MeshMaterial3d(material.clone()),
            Transform::default(),
        ));
    }
    commands.spawn((
        Fountain,
        EffectPlayer::new(&fountain()),
        Transform::default(),
    ));
}

/// The game's own motion: the balls circle above the fountain.
fn orbit(time: Res<Time>, mut balls: Query<(&Orbiting, &mut Transform)>) {
    for (orbiting, mut transform) in &mut balls {
        let angle = orbiting.phase + orbiting.speed * time.elapsed_secs();
        transform.translation = Vec3::new(
            orbiting.radius * angle.cos(),
            8.0,
            orbiting.radius * angle.sin(),
        );
    }
}

/// Aestra's side of the game's physics: its colliders as proxies, every frame.
fn describe_colliders(
    mut commands: Commands,
    balls: Query<(&Orbiting, &GlobalTransform)>,
    fountains: Query<Entity, With<Fountain>>,
) {
    let mut proxies: Vec<PhysicsProxy> = balls
        .iter()
        .map(|(orbiting, transform)| PhysicsPose::of(transform).sphere(Vec3::ZERO, orbiting.size))
        .collect();
    proxies.push(PhysicsProxy::HalfSpace {
        normal: [0.0, 1.0, 0.0],
        distance: 0.0,
    });
    for fountain in &fountains {
        commands
            .entity(fountain)
            .insert(AestraPhysicsColliders::nearest(
                Vec3::ZERO,
                proxies.clone(),
                16,
            ));
    }
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
