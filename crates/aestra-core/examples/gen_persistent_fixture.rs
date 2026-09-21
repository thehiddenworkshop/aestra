//! One-off generator for the Persistent stateful test fixture. Builds a MULTI-emitter effect whose
//! every emitter carries the Persistent solver, so it exercises the GPU multi-stateful path (one set
//! of persistent buffers + one dispatch per emitter, sharing the effect's particle/alive/indirect
//! buffers). Written through the real `save_ron` so the RON matches the loader exactly.

use aestra_core::{
    EffectAsset, EffectPlaybackMode, Emitter, MODULE_EMISSION, MODULE_INITIALIZE, MODULE_MOTION,
    ModuleInstance, ModuleParameters, ScalarRange,
};

/// A stateful sprite emitter tuned to clean scalar values, since the stateful integrator reads range
/// midpoints. Distinct capacities/dynamics per emitter exercise the per-emitter slot regions.
fn stateful_emitter(
    name: &str,
    duration: f32,
    capacity: u32,
    spawn_rate: f32,
    speed: f32,
    lifetime: f32,
    gravity: [f32; 3],
) -> Emitter {
    let mut emitter = Emitter::basic_sprite(name, duration);
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
                *life = ScalarRange::new(lifetime, lifetime);
                *spd = ScalarRange::new(speed, speed);
                *spread_degrees = 25.0;
            }
            ModuleParameters::Motion { gravity: g, .. }
                if module.module_type.0 == MODULE_MOTION =>
            {
                *g = gravity;
            }
            _ => {}
        }
    }
    // The marker that promotes this emitter to the stateful GPU path.
    emitter.modules.push(ModuleInstance::persistent());
    emitter
}

fn main() {
    let mut effect = EffectAsset::new("Persistent Lab", 3.0);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;

    // Three stateful emitters with distinct capacities, spawn rates, speeds, lifetimes, and gravity,
    // so their per-emitter state regions, spawn ordinals, and dynamics are all independently driven.
    effect.emitters.push(stateful_emitter(
        "Tall Fountain",
        3.0,
        256,
        48.0,
        45.0,
        1.5,
        [0.0, -30.0, 0.0],
    ));
    effect.emitters.push(stateful_emitter(
        "Fast Spray",
        3.0,
        128,
        30.0,
        70.0,
        1.0,
        [0.0, -50.0, 0.0],
    ));
    effect.emitters.push(stateful_emitter(
        "Slow Drift",
        3.0,
        192,
        60.0,
        20.0,
        2.0,
        [0.0, -8.0, 0.0],
    ));

    effect.validate().expect("the persistent fixture is valid");
    let path = "sample-project/effects/persistent_lab.aestra.ron";
    effect.save_ron(path).expect("write persistent fixture");

    // Round-trip: the loader must parse the file we just wrote, and every reloaded emitter must still
    // carry the Persistent marker.
    let reloaded = EffectAsset::load_ron(path).expect("reload persistent fixture");
    reloaded.validate().expect("reloaded fixture is valid");
    let all_stateful = reloaded.emitters.iter().all(|emitter| {
        emitter
            .modules
            .iter()
            .any(|module| matches!(module.parameters, ModuleParameters::Persistent {}))
    });
    assert!(
        all_stateful,
        "every reloaded emitter keeps the Persistent module"
    );
    println!(
        "wrote and round-tripped {path} ({} stateful emitters)",
        reloaded.emitters.len()
    );
}
