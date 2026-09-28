//! A homing projectile (host bindings HB7): a wizard's staff looses fire at an enemy that dodges.
//!
//! The effect's sparks carry a Homing module whose target is the effect's `Target` binding — its
//! position and velocity (to lead it). The game binds the enemy to `Target` with `AestraBindings` and
//! reports the enemy's velocity in `AestraLinearVelocity`, as a physics integration would. It never
//! writes an effect parameter: the sparks steer toward wherever the enemy is, every tick, and retire
//! when they reach it. This is the *Aestra-owned visual projectile* pattern — decorative; a game whose
//! hits matter owns the projectile itself (see `docs/ARCHITECTURE.md`, "Gameplay authority").
//!
//! ```sh
//! cargo run -p aestra-bevy --example homing_projectile --release
//! ```
//!
//! With `AESTRA_EXAMPLE_CAPTURE=<file.png>` it saves a screenshot after four seconds and exits.

use aestra_bevy::{
    AESTRA_FIELD_LINEAR_VELOCITY, AESTRA_FIELD_POSITION, AestraBindings, AestraLinearVelocity,
    AestraPlugin, BindingFieldId, BindingUpdateMode, ColorKey, Curve, CurveKey, EffectAsset,
    EffectBinding, EffectPlaybackMode, EffectPlayer, Emitter, EmitterShape, Gradient, HostFieldRef,
    ModuleInstance, PropertySource, ScalarRange,
};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

/// The staff's tip, where the fire leaves.
const STAFF_TIP: Vec3 = Vec3::new(-40.0, 14.0, 0.0);

#[derive(Component)]
struct Enemy;

#[derive(Resource, Default)]
struct Frames(u32);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, AestraPlugin))
        .init_resource::<Frames>()
        .add_systems(Startup, setup)
        .add_systems(Update, (dodge, capture))
        .run();
}

/// Homing fire: sparks leave the staff upward and outward, then turn toward the target, leading it,
/// at up to 70 units/s; they retire within 3 units of it.
fn homing_fire() -> EffectAsset {
    let mut effect = EffectAsset::new("Homing Fire", 4.0);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    target
        .optional_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY));

    let mut homing = ModuleInstance::homing([0.0; 3], 70.0);
    if let aestra_bevy::ModuleParameters::Homing {
        acceleration,
        turn_rate,
        arrival_radius,
        ..
    } = &mut homing.parameters
    {
        *acceleration = 60.0;
        *turn_rate = 2.5;
        *arrival_radius = 3.0;
    }
    for (input, field) in [
        ("target", AESTRA_FIELD_POSITION),
        ("target_velocity", AESTRA_FIELD_LINEAR_VELOCITY),
    ] {
        homing
            .property_sources
            .insert(input.into(), PropertySource::HostBinding);
        homing
            .host_bindings
            .insert(input.into(), HostFieldRef::new(target.id, field));
    }

    let mut emitter = Emitter::basic_sprite("Sparks", 4.0);
    emitter.max_particles = 1024;
    emitter.modules = vec![
        ModuleInstance::emission(40.0, 0),
        ModuleInstance::shape(EmitterShape::Sphere { radius: 1.0 }),
        ModuleInstance::initialize(
            ScalarRange::new(2.5, 3.5),
            ScalarRange::new(25.0, 40.0),
            [0.3, 1.0, 0.0],
            60.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0; 3], 0.0, 0.0),
        homing,
        ModuleInstance::appearance(
            Curve::new(vec![CurveKey::new(0.0, 3.0), CurveKey::new(1.0, 1.5)]),
            Curve::new(vec![
                CurveKey::new(0.0, 1.0),
                CurveKey::new(0.8, 0.9),
                CurveKey::new(1.0, 0.0),
            ]),
            Gradient::new(vec![
                ColorKey::new(0.0, [1.0, 0.95, 0.6, 1.0]),
                ColorKey::new(0.4, [1.0, 0.5, 0.1, 1.0]),
                ColorKey::new(1.0, [0.6, 0.1, 0.05, 1.0]),
            ]),
        ),
    ];
    effect.emitters.push(emitter);
    effect.bindings.push(target);
    effect
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 45.0, 140.0).looking_at(Vec3::new(0.0, 12.0, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 6_000.0,
            ..default()
        },
        Transform::from_xyz(-30.0, 80.0, 60.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(200.0, 120.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.22, 0.24, 0.2))),
    ));
    // The wizard and the staff.
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(3.0, 8.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.25, 0.2, 0.5))),
        Transform::from_xyz(-44.0, 7.0, 0.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.4, 16.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.4, 0.28, 0.15))),
        Transform::from_translation(STAFF_TIP - Vec3::Y * 7.0),
    ));
    // The enemy, dodging; bound to the effect's Target.
    let enemy = commands
        .spawn((
            Enemy,
            Mesh3d(meshes.add(Cuboid::new(6.0, 6.0, 6.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.7, 0.15, 0.15),
                ..default()
            })),
            Transform::from_xyz(35.0, 8.0, 0.0),
            AestraLinearVelocity::default(),
        ))
        .id();
    commands.spawn((
        EffectPlayer::new(&homing_fire()),
        AestraBindings::new().bind("Target", enemy),
        Transform::from_translation(STAFF_TIP),
    ));
}

/// The enemy weaves across the field, changing direction, and reports its velocity.
fn dodge(
    time: Res<Time>,
    mut enemies: Query<(&mut Transform, &mut AestraLinearVelocity), With<Enemy>>,
) {
    let t = time.elapsed_secs();
    let position = |t: f32| {
        Vec3::new(
            35.0 + 12.0 * (0.9 * t).sin(),
            8.0 + 5.0 * (1.7 * t).sin().abs(),
            28.0 * (0.7 * t).sin() * (0.35 * t).cos(),
        )
    };
    for (mut transform, mut velocity) in &mut enemies {
        transform.translation = position(t);
        velocity.0 = (position(t + 0.01) - position(t)) / 0.01;
    }
}

/// `AESTRA_EXAMPLE_CAPTURE`: a screenshot after four seconds, then exit.
fn capture(mut commands: Commands, mut frames: ResMut<Frames>, mut exit: MessageWriter<AppExit>) {
    let Ok(path) = std::env::var("AESTRA_EXAMPLE_CAPTURE") else {
        return;
    };
    frames.0 += 1;
    if frames.0 == 240 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if frames.0 == 250 {
        exit.write(AppExit::Success);
    }
}
