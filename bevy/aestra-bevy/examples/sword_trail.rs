//! A sword trail (host bindings HB7b): an emitter attached to a moving blade leaves a wake.
//!
//! The effect entity stays put; its emitter is *attached* to the effect's `Blade` binding, which the
//! game binds to an entity at the sword's tip with `AestraBindings`. Every frame the emitter follows
//! the tip — position and rotation — so each spark is born where the blade is at that moment and keeps
//! its own motion afterwards: the swing leaves a glowing arc behind it. The game never moves the
//! effect nor writes a parameter; it only animates its sword.
//!
//! ```sh
//! cargo run -p aestra-bevy --example sword_trail --release
//! ```
//!
//! With `AESTRA_EXAMPLE_CAPTURE=<file.png>` it saves a screenshot after four seconds and exits.

use aestra_bevy::{
    AESTRA_FIELD_ROTATION, AestraBindings, AestraPlugin, AttachmentInherit, BindingFieldId,
    BindingUpdateMode, ColorKey, Curve, CurveKey, EffectAsset, EffectBinding, EffectPlaybackMode,
    EffectPlayer, Emitter, EmitterAttachment, EmitterShape, Gradient, ModuleInstance, ScalarRange,
};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

/// The sword's length from the hand to the tip.
const BLADE: f32 = 14.0;

/// The pivot the sword swings around: the knight's hand.
#[derive(Component)]
struct Hand;

#[derive(Resource, Default)]
struct Frames(u32);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, AestraPlugin))
        .init_resource::<Frames>()
        .add_systems(Startup, setup)
        .add_systems(Update, (swing, capture))
        .run();
}

/// Sparks shed by the blade: born along a short segment at the tip, drifting slowly outward along
/// the blade, fading within a second.
fn blade_wake() -> EffectAsset {
    let mut effect = EffectAsset::new("Blade Wake", 4.0);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut blade = EffectBinding::spatial("Blade", BindingUpdateMode::Live);
    blade
        .optional_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_ROTATION));

    let mut emitter = Emitter::basic_sprite("Wake", 4.0);
    emitter.max_particles = 2048;
    // Along the blade's last few units: the tip entity's local -y points back toward the hand.
    emitter.transform.translation = [0.0, -2.0, 0.0];
    emitter.attachment = Some(EmitterAttachment {
        binding: blade.id,
        inherit: AttachmentInherit::PositionAndRotation,
    });
    emitter.modules = vec![
        ModuleInstance::emission(600.0, 0),
        ModuleInstance::shape(EmitterShape::Box {
            half_extents: [0.2, 2.0, 0.2],
        }),
        ModuleInstance::initialize(
            ScalarRange::new(0.7, 0.9),
            ScalarRange::new(0.5, 2.0),
            [0.0, 1.0, 0.0],
            40.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0, 2.0, 0.0], 1.5, 0.0),
        ModuleInstance::appearance(
            Curve::new(vec![CurveKey::new(0.0, 1.4), CurveKey::new(1.0, 0.4)]),
            Curve::new(vec![
                CurveKey::new(0.0, 1.0),
                CurveKey::new(0.6, 0.7),
                CurveKey::new(1.0, 0.0),
            ]),
            Gradient::new(vec![
                ColorKey::new(0.0, [0.85, 0.95, 1.0, 1.0]),
                ColorKey::new(0.3, [0.35, 0.65, 1.0, 1.0]),
                ColorKey::new(1.0, [0.2, 0.1, 0.8, 1.0]),
            ]),
        ),
    ];
    effect.emitters.push(emitter);
    effect.bindings.push(blade);
    effect
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 30.0, 60.0).looking_at(Vec3::new(0.0, 12.0, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 5_000.0,
            ..default()
        },
        Transform::from_xyz(-30.0, 80.0, 60.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(120.0, 120.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.18, 0.2, 0.22))),
    ));
    // The knight.
    commands.spawn((
        Mesh3d(meshes.add(Capsule3d::new(3.0, 8.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.5, 0.5, 0.55))),
        Transform::from_xyz(0.0, 7.0, 0.0),
    ));
    // The hand, the sword along its +y, and the tip entity bound to the effect's Blade.
    let tip = commands.spawn(Transform::from_xyz(0.0, BLADE, 0.0)).id();
    commands
        .spawn((
            Hand,
            Transform::from_xyz(3.5, 10.0, 2.0),
            Visibility::default(),
        ))
        .with_children(|hand| {
            hand.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.5, BLADE, 0.15))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.85, 0.88, 0.95),
                    metallic: 0.9,
                    perceptual_roughness: 0.2,
                    ..default()
                })),
                Transform::from_xyz(0.0, BLADE * 0.5, 0.0),
            ));
        })
        .add_child(tip);
    // The effect itself never moves.
    commands.spawn((
        EffectPlayer::new(&blade_wake()),
        AestraBindings::new().bind("Blade", tip),
        Transform::default(),
    ));
}

/// Wide horizontal sweeps back and forth, the blade held out to the side and rising and falling.
fn swing(time: Res<Time>, mut hands: Query<&mut Transform, With<Hand>>) {
    let t = time.elapsed_secs();
    let sweep = 1.6 * (2.2 * t).sin();
    let tilt = -1.2 + 0.3 * (4.4 * t).cos();
    for mut transform in &mut hands {
        transform.rotation = Quat::from_rotation_y(sweep) * Quat::from_rotation_z(tilt);
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
