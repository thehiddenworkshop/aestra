//! Scrubbing a recorded target (host bindings HB8).
//!
//! Homing sparks chase a weaving enemy for three seconds while an `AestraBindingRecorder` records what
//! the effect's `Target` binding resolves to, tick by tick. Then the player pauses, the recording
//! drives it (`AestraBindingTrace`), and the timeline is scrubbed back and forth: every scrubbed frame
//! replays the sparks exactly as they flew, because each past tick reads its own recorded target.
//! Without the recording, a backward seek would replay the past with the enemy's *present* position —
//! `EffectPlayer::supports_exact_backward_seek` reports it, printed before and after.
//!
//! ```sh
//! cargo run -p aestra-bevy --example scrub_recorded --release
//! ```
//!
//! With `AESTRA_EXAMPLE_CAPTURE=<file.png>` it saves a screenshot after five seconds and exits.

use aestra_bevy::{
    AESTRA_FIELD_POSITION, AestraBindingRecorder, AestraBindingTrace, AestraBindings, AestraPlugin,
    BindingUpdateMode, ColorKey, Curve, CurveKey, EffectAsset, EffectBinding, EffectPlaybackMode,
    EffectPlayer, Emitter, EmitterShape, Gradient, HostFieldRef, ModuleInstance, PropertySource,
    ScalarRange,
};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use std::sync::Arc;

/// How long the live run records before scrubbing starts.
const RECORD_SECONDS: f32 = 3.0;

#[derive(Component)]
struct Enemy;

#[derive(Component)]
struct Sparks;

/// Frames since the screenshot was taken, when it was.
#[derive(Resource, Default)]
struct Captured(Option<u32>);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, AestraPlugin))
        .init_resource::<Captured>()
        .add_systems(Startup, setup)
        .add_systems(Update, (dodge, scrub, capture))
        .run();
}

fn homing_sparks() -> EffectAsset {
    let mut effect = EffectAsset::new("Recorded Homing", 6.0);
    effect.playback_mode = EffectPlaybackMode::Once;
    let target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    let mut homing = ModuleInstance::homing([0.0; 3], 40.0);
    if let aestra_bevy::ModuleParameters::Homing {
        acceleration,
        turn_rate,
        arrival_radius,
        ..
    } = &mut homing.parameters
    {
        *acceleration = 50.0;
        *turn_rate = 6.0;
        *arrival_radius = 2.5;
    }
    homing
        .property_sources
        .insert("target".into(), PropertySource::HostBinding);
    homing.host_bindings.insert(
        "target".into(),
        HostFieldRef::new(target.id, AESTRA_FIELD_POSITION),
    );
    let mut emitter = Emitter::basic_sprite("Sparks", 6.0);
    emitter.max_particles = 1024;
    emitter.modules = vec![
        ModuleInstance::emission(60.0, 0),
        ModuleInstance::shape(EmitterShape::Sphere { radius: 1.0 }),
        ModuleInstance::initialize(
            ScalarRange::new(2.0, 3.0),
            ScalarRange::new(15.0, 25.0),
            [0.0, 1.0, 0.0],
            70.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0; 3], 0.0, 0.0),
        homing,
        ModuleInstance::appearance(
            Curve::new(vec![CurveKey::new(0.0, 2.0), CurveKey::new(1.0, 1.0)]),
            Curve::new(vec![CurveKey::new(0.0, 1.0), CurveKey::new(1.0, 0.0)]),
            Gradient::new(vec![
                ColorKey::new(0.0, [0.6, 1.0, 0.8, 1.0]),
                ColorKey::new(1.0, [0.1, 0.4, 1.0, 1.0]),
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
        Transform::from_xyz(0.0, 40.0, 90.0).looking_at(Vec3::new(0.0, 5.0, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 5_000.0,
            ..default()
        },
        Transform::from_xyz(-30.0, 80.0, 60.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(160.0, 120.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.2, 0.22, 0.24))),
    ));
    let enemy = commands
        .spawn((
            Enemy,
            Mesh3d(meshes.add(Cuboid::new(4.0, 4.0, 4.0))),
            MeshMaterial3d(materials.add(Color::srgb(0.7, 0.15, 0.15))),
            Transform::from_xyz(25.0, 6.0, 0.0),
        ))
        .id();
    commands.spawn((
        Sparks,
        EffectPlayer::new(&homing_sparks()),
        AestraBindings::new().bind("Target", enemy),
        AestraBindingRecorder::new(),
        Transform::from_xyz(-30.0, 2.0, 0.0),
    ));
}

/// The enemy weaves while the run is recorded, then keeps moving — which no longer matters.
fn dodge(time: Res<Time>, mut enemies: Query<&mut Transform, With<Enemy>>) {
    let t = time.elapsed_secs();
    for mut transform in &mut enemies {
        transform.translation = Vec3::new(
            25.0 + 10.0 * (1.1 * t).sin(),
            6.0 + 4.0 * (2.3 * t).sin().abs(),
            20.0 * (0.8 * t).sin(),
        );
    }
}

/// The sparks' player, recording or replaying.
type SparkPlayer = (
    Entity,
    &'static mut EffectPlayer,
    Option<&'static AestraBindingRecorder>,
    Option<&'static AestraBindingTrace>,
);

/// After the recording: pause, replay the recording, and scrub between one and three seconds.
fn scrub(time: Res<Time>, mut commands: Commands, mut sparks: Query<SparkPlayer, With<Sparks>>) {
    let t = time.elapsed_secs();
    for (entity, mut player, recorder, trace) in &mut sparks {
        if t < RECORD_SECONDS {
            continue;
        }
        if let Some(recorder) = recorder {
            println!(
                "live target: exact backward seek {} ({:?})",
                player.supports_exact_backward_seek(),
                player.host_input_availability()
            );
            player.playing = false;
            commands
                .entity(entity)
                .remove::<AestraBindingRecorder>()
                .insert(AestraBindingTrace(Arc::new(recorder.trace())));
            continue;
        }
        if trace.is_some() {
            let phase = ((t - RECORD_SECONDS) * 1.2).sin() * 0.5 + 0.5;
            player.seek(1.0 + phase * (RECORD_SECONDS - 1.0));
        }
    }
}

/// `AESTRA_EXAMPLE_CAPTURE`: a screenshot after five seconds, then exit.
fn capture(
    time: Res<Time>,
    mut commands: Commands,
    mut captured: ResMut<Captured>,
    sparks: Query<&EffectPlayer, With<Sparks>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(path) = std::env::var("AESTRA_EXAMPLE_CAPTURE") else {
        return;
    };
    match captured.0.as_mut() {
        None if time.elapsed_secs() >= 5.0 => {
            for player in &sparks {
                println!(
                    "recorded target: exact backward seek {} ({:?})",
                    player.supports_exact_backward_seek(),
                    player.host_input_availability()
                );
            }
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
