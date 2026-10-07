//! Fireworks (host bindings HB9b): particle event links chaining three emitters.
//!
//! Rockets rise and die; each rocket's death (`OnDeath`) bursts into 48 sparks where it died, the
//! sparks keeping a little of the rocket's velocity. Sparks that reach the ground (`OnCollision` with
//! their ground plane) scatter two glints each where they land. The burst and the glints are
//! *sub-emitters*: they spawn only from their links, never on their own. Everything runs on the GPU,
//! in lockstep, and a rerun reproduces every spark.
//!
//! ```sh
//! cargo run -p aestra-bevy --example fireworks_event_chain --release
//! ```
//!
//! With `AESTRA_EXAMPLE_CAPTURE=<file.png>` it saves a screenshot after five seconds and exits.

use aestra_bevy::{
    AestraPlugin, ColliderShape, ColorKey, Curve, CurveKey, EffectAsset, EffectPlaybackMode,
    EffectPlayer, Emitter, EmitterShape, EventLink, EventTrigger, Gradient, ModuleInstance,
    ScalarRange,
};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

/// Frames since the screenshot was taken, when it was.
#[derive(Resource, Default)]
struct Frames(Option<u32>);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, AestraPlugin))
        .init_resource::<Frames>()
        .add_systems(Startup, setup)
        .add_systems(Update, capture)
        .run();
}

/// An emitter with the usual modules: rate, shape, launch, forces, look.
#[allow(clippy::too_many_arguments)]
fn emitter(
    name: &str,
    capacity: u32,
    rate: f32,
    lifetime: (f32, f32),
    speed: (f32, f32),
    spread: f32,
    motion: ModuleInstance,
    size: f32,
    colors: [[f32; 4]; 3],
) -> Emitter {
    let mut emitter = Emitter::basic_sprite(name, 6.0);
    emitter.max_particles = capacity;
    emitter.modules = vec![
        ModuleInstance::emission(rate, 0),
        ModuleInstance::shape(EmitterShape::Point),
        ModuleInstance::initialize(
            ScalarRange::new(lifetime.0, lifetime.1),
            ScalarRange::new(speed.0, speed.1),
            [0.0, 1.0, 0.0],
            spread,
            ScalarRange::new(0.0, 0.0),
        ),
        motion,
        ModuleInstance::appearance(
            Curve::new(vec![
                CurveKey::new(0.0, size),
                CurveKey::new(1.0, size * 0.4),
            ]),
            Curve::new(vec![
                CurveKey::new(0.0, 1.0),
                CurveKey::new(0.7, 0.8),
                CurveKey::new(1.0, 0.0),
            ]),
            Gradient::new(vec![
                ColorKey::new(0.0, colors[0]),
                ColorKey::new(0.4, colors[1]),
                ColorKey::new(1.0, colors[2]),
            ]),
        ),
    ];
    emitter
}

fn fireworks() -> EffectAsset {
    let mut effect = EffectAsset::new("Fireworks", 6.0);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    let rockets = emitter(
        "Rockets",
        64,
        3.0,
        (1.1, 1.5),
        (38.0, 46.0),
        10.0,
        ModuleInstance::motion([0.0, -25.0, 0.0], 0.0, 0.0),
        1.4,
        [
            [1.0, 0.95, 0.8, 1.0],
            [1.0, 0.7, 0.3, 1.0],
            [1.0, 0.4, 0.1, 1.0],
        ],
    );
    let mut burst = emitter(
        "Burst",
        4096,
        0.0,
        (2.2, 2.8),
        (10.0, 18.0),
        180.0,
        ModuleInstance::motion([0.0, -20.0, 0.0], 0.4, 1.5),
        1.1,
        [
            [1.0, 1.0, 0.9, 1.0],
            [0.3, 0.8, 1.0, 1.0],
            [0.7, 0.2, 1.0, 1.0],
        ],
    );
    // Sparks reaching the ground bounce off it — and raise `OnCollision`.
    burst
        .modules
        .push(ModuleInstance::collision(vec![aestra_bevy::Collider {
            shape: ColliderShape::Plane {
                normal: [0.0, 1.0, 0.0],
                distance: 0.0,
            },
            restitution: 0.3,
            friction: 0.5,
            kill: false,
        }]));
    let glints = emitter(
        "Glints",
        2048,
        0.0,
        (0.3, 0.6),
        (2.0, 5.0),
        60.0,
        ModuleInstance::motion([0.0, -6.0, 0.0], 1.0, 0.0),
        0.6,
        [
            [1.0, 1.0, 1.0, 1.0],
            [1.0, 0.9, 0.5, 1.0],
            [1.0, 0.6, 0.2, 1.0],
        ],
    );
    let mut on_death = EventLink::new(rockets.id, EventTrigger::OnDeath, burst.id);
    on_death.count = 48;
    on_death.inherit_velocity = 0.2;
    let mut on_ground = EventLink::new(burst.id, EventTrigger::OnCollision, glints.id);
    on_ground.count = 2;
    effect.events = vec![on_death, on_ground];
    effect.emitters = vec![rockets, burst, glints];
    effect
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 22.0, 85.0).looking_at(Vec3::new(0.0, 22.0, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 1_500.0,
            ..default()
        },
        Transform::from_xyz(-30.0, 80.0, 60.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(200.0, 200.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.08, 0.09, 0.12))),
    ));
    commands.spawn((EffectPlayer::new(&fireworks()), Transform::default()));
}

/// `AESTRA_EXAMPLE_CAPTURE`: a screenshot after five seconds, then exit.
fn capture(
    time: Res<Time>,
    mut commands: Commands,
    mut frames: ResMut<Frames>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(path) = std::env::var("AESTRA_EXAMPLE_CAPTURE") else {
        return;
    };
    match frames.0.as_mut() {
        None if time.elapsed_secs() >= 5.0 => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
            frames.0 = Some(0);
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
