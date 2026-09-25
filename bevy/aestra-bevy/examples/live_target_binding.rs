//! Live host binding: sparks launched along a moving target's velocity (host bindings HB5).
//!
//! The effect declares a `Target` binding slot; the game binds an orbiting entity to it with
//! `AestraBindings`, and writes that entity's velocity into `AestraLinearVelocity` every frame. The
//! Initialize module's `direction` reads `Target`'s velocity (source `HostBinding`), so the spray
//! follows the orbiter — without the game setting any parameter.
//!
//! The emitter is analytic: every live particle is evaluated from the current input, so the whole
//! spray swings with the target rather than each spark keeping its launch direction. Per-spark
//! capture needs a stateful emitter (host bindings roadmap §7.1).
//!
//! ```sh
//! cargo run -p aestra-bevy --example live_target_binding
//! ```

use aestra_bevy::{
    AESTRA_FIELD_LINEAR_VELOCITY, AestraBindings, AestraLinearVelocity, AestraPlugin,
    BindingFieldId, BindingUpdateMode, EffectAsset, EffectBinding, EffectPlaybackMode,
    EffectPlayer, Emitter, EmitterShape, HostFieldRef, MODULE_INITIALIZE, ModuleParameters,
    PropertySource, ScalarRange,
};
use bevy::prelude::*;

const ORBIT_RADIUS: f32 = 180.0;
const ORBIT_SPEED: f32 = 1.2;

#[derive(Component)]
struct Orbiter;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, AestraPlugin))
        .add_systems(Startup, setup)
        .add_systems(Update, orbit)
        .run();
}

/// A continuous point emitter whose launch direction follows `Target`'s velocity.
fn effect() -> EffectAsset {
    let mut effect = EffectAsset::new("Live Target", 4.0);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    target
        .optional_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY));
    let mut emitter = Emitter::basic_sprite("Sparks", 4.0);
    emitter.max_particles = 512;
    for module in &mut emitter.modules {
        match &mut module.parameters {
            ModuleParameters::Emission { spawn_rate, .. } => *spawn_rate = 120.0,
            ModuleParameters::Shape { shape } => *shape = EmitterShape::Point,
            ModuleParameters::Initialize {
                speed,
                spread_degrees,
                ..
            } => {
                *speed = ScalarRange::new(90.0, 140.0);
                *spread_degrees = 12.0;
            }
            ModuleParameters::Motion { gravity, .. } => *gravity = [0.0; 3],
            _ => {}
        }
        if module.module_type.0 == MODULE_INITIALIZE {
            module
                .property_sources
                .insert("direction".into(), PropertySource::HostBinding);
            module.host_bindings.insert(
                "direction".into(),
                HostFieldRef::new(target.id, AESTRA_FIELD_LINEAR_VELOCITY),
            );
        }
    }
    effect.emitters.push(emitter);
    effect.bindings.push(target);
    effect
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let target = commands
        .spawn((
            Orbiter,
            Sprite::from_color(Color::srgb(1.0, 0.45, 0.2), Vec2::splat(14.0)),
            Transform::from_xyz(ORBIT_RADIUS, 0.0, 1.0),
            AestraLinearVelocity::default(),
        ))
        .id();
    // The effect stays at the origin; only its `Target` slot follows the orbiter.
    commands.spawn((
        EffectPlayer::new(&effect()),
        AestraBindings::new().bind("Target", target),
    ));
}

/// Moves the orbiter and reports its velocity, as a physics integration would.
fn orbit(
    time: Res<Time>,
    mut orbiters: Query<(&mut Transform, &mut AestraLinearVelocity), With<Orbiter>>,
) {
    let angle = time.elapsed_secs() * ORBIT_SPEED;
    for (mut transform, mut velocity) in &mut orbiters {
        transform.translation.x = ORBIT_RADIUS * angle.cos();
        transform.translation.y = ORBIT_RADIUS * angle.sin();
        velocity.0 = Vec3::new(-angle.sin(), angle.cos(), 0.0) * ORBIT_RADIUS * ORBIT_SPEED;
    }
}
