//! One-off generator for the Persistent stateful test fixtures. Writes effects through the real
//! `save_ron` (so the RON matches the loader exactly):
//! - `persistent_lab`: a multi-emitter effect where *every* emitter carries the Persistent solver,
//!   exercising the pure multi-stateful GPU path.
//! - `mixed_lab`: one analytic emitter and two stateful emitters in one effect, exercising the mixed
//!   path (analytic reset+simulate skipping stateful slots, then the stateful dispatches filling them).
//! - `collision_lab`: a fountain whose Collision module (which auto-promotes it to stateful) bounces
//!   particles off a ground plane, a sphere obstacle, and a box obstacle (hybrid roadmap M10).

use aestra_core::{
    Collider, ColliderShape, EffectAsset, EffectPlaybackMode, Emitter, EmitterShape,
    MODULE_EMISSION, MODULE_INITIALIZE, MODULE_MOTION, MODULE_SHAPE, ModuleInstance,
    ModuleParameters, ScalarRange,
};

/// A sprite emitter tuned to clean scalar values, since the stateful integrator reads range midpoints.
/// Distinct capacities/dynamics per emitter exercise the per-emitter slot regions. When `persistent`
/// is true the Persistent marker is added, promoting the emitter to the stateful GPU path; otherwise
/// it stays analytic.
fn tuned_emitter(
    name: &str,
    capacity: u32,
    spawn_rate: f32,
    speed: f32,
    lifetime: f32,
    gravity: [f32; 3],
    persistent: bool,
) -> Emitter {
    let mut emitter = Emitter::basic_sprite(name, 3.0);
    emitter.max_particles = capacity;
    for module in &mut emitter.modules {
        match &mut module.parameters {
            ModuleParameters::Emission {
                spawn_rate: rate, ..
            } if module.module_type.0 == MODULE_EMISSION => {
                *rate = spawn_rate;
            }
            ModuleParameters::Initialize {
                lifetime: life,
                speed: spd,
                spread_degrees,
                ..
            } if module.module_type.0 == MODULE_INITIALIZE => {
                // Real ranges so speed and lifetime vary per particle (the richer stateful dynamics),
                // plus a spread cone.
                *life = ScalarRange::new(lifetime * 0.8, lifetime * 1.2);
                *spd = ScalarRange::new(speed * 0.7, speed * 1.3);
                *spread_degrees = 30.0;
            }
            ModuleParameters::Motion {
                gravity: g,
                turbulence,
                ..
            } if module.module_type.0 == MODULE_MOTION => {
                *g = gravity;
                *turbulence = 6.0; // value-noise turbulence in the stateful path
            }
            ModuleParameters::Shape { shape } if module.module_type.0 == MODULE_SHAPE => {
                // A sphere spawn volume, so particles emit from a filled sphere (the stateful path
                // maps Sphere/Box; other shapes fall back to a point).
                *shape = EmitterShape::Sphere { radius: 6.0 };
            }
            _ => {}
        }
    }
    if persistent {
        emitter.modules.push(ModuleInstance::persistent());
    }
    emitter
}

fn write(effect: &EffectAsset, path: &str) {
    effect.validate().expect("fixture is valid");
    effect.save_ron(path).expect("write fixture");
    let reloaded = EffectAsset::load_ron(path).expect("reload fixture");
    reloaded.validate().expect("reloaded fixture is valid");
    let stateful = reloaded
        .emitters
        .iter()
        .filter(|emitter| {
            emitter.modules.iter().any(|module| {
                matches!(
                    module.parameters,
                    ModuleParameters::Persistent {} | ModuleParameters::Collision { .. }
                )
            })
        })
        .count();
    println!(
        "wrote and round-tripped {path} ({} emitters, {stateful} stateful)",
        reloaded.emitters.len()
    );
}

fn main() {
    // persistent_lab: three stateful emitters with distinct capacities/dynamics — the pure
    // multi-stateful path (per-emitter state regions, ordinals, and dynamics all independently driven).
    let mut persistent = EffectAsset::new("Persistent Lab", 3.0);
    persistent.playback_mode = EffectPlaybackMode::LoopContinuous;
    persistent.emitters.push(tuned_emitter(
        "Tall Fountain",
        256,
        48.0,
        45.0,
        1.5,
        [0.0, -30.0, 0.0],
        true,
    ));
    persistent.emitters.push(tuned_emitter(
        "Fast Spray",
        128,
        30.0,
        70.0,
        1.0,
        [0.0, -50.0, 0.0],
        true,
    ));
    persistent.emitters.push(tuned_emitter(
        "Slow Drift",
        192,
        60.0,
        20.0,
        2.0,
        [0.0, -8.0, 0.0],
        true,
    ));
    write(
        &persistent,
        "sample-project/effects/persistent_lab.aestra.ron",
    );

    // mixed_lab: one analytic emitter plus two stateful emitters in one effect — the mixed path. The
    // analytic emitter simulates the usual way (with drag/turbulence); the stateful emitters are the
    // reference integrator. All three share the effect's particle/alive/indirect buffers.
    let mut mixed = EffectAsset::new("Mixed Lab", 3.0);
    mixed.playback_mode = EffectPlaybackMode::LoopContinuous;
    mixed.emitters.push(tuned_emitter(
        "Analytic Sparks",
        192,
        40.0,
        60.0,
        1.2,
        [0.0, -20.0, 0.0],
        false,
    ));
    mixed.emitters.push(tuned_emitter(
        "Stateful Fountain",
        256,
        48.0,
        45.0,
        1.5,
        [0.0, -30.0, 0.0],
        true,
    ));
    mixed.emitters.push(tuned_emitter(
        "Stateful Drift",
        128,
        30.0,
        22.0,
        2.0,
        [0.0, -8.0, 0.0],
        true,
    ));
    write(&mixed, "sample-project/effects/mixed_lab.aestra.ron");

    // collision_lab: a single upward fountain with a Collision module (M10). Adding the module — with
    // no manual stateful toggle — promotes the emitter to the stateful GPU path, and the integrator
    // bounces particles off a ground plane, a sphere obstacle, and a box obstacle each fixed tick.
    let mut collision = EffectAsset::new("Collision Lab", 3.0);
    collision.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut fountain = tuned_emitter(
        "Bouncing Fountain",
        512,
        60.0,
        45.0,
        2.5,
        [0.0, -30.0, 0.0],
        false, // the Collision module below is what promotes it — no Persistent marker needed
    );
    fountain.modules.push(ModuleInstance::collision(vec![
        // Ground plane below the spawn sphere (radius 6 at the origin) so fresh spawns start above it.
        Collider {
            shape: ColliderShape::Plane {
                normal: [0.0, 1.0, 0.0],
                distance: -8.0,
            },
            restitution: 0.6,
            friction: 0.3,
            kill: false,
        },
        // A sphere obstacle up in the fountain's arc.
        Collider {
            shape: ColliderShape::Sphere {
                center: [0.0, 16.0, 0.0],
                radius: 5.0,
            },
            restitution: 0.85,
            friction: 0.1,
            kill: false,
        },
        // A box obstacle the particles pass through on the way up and down.
        Collider {
            shape: ColliderShape::Aabb {
                min: [-6.0, 4.0, -6.0],
                max: [6.0, 6.0, 6.0],
            },
            restitution: 0.4,
            friction: 0.5,
            kill: false,
        },
    ]));
    collision.emitters.push(fountain);
    write(
        &collision,
        "sample-project/effects/collision_lab.aestra.ron",
    );
}
