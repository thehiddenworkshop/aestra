//! One-off generator for the Persistent stateful test fixture. Builds a single-emitter effect whose
//! only non-analytic module is the Persistent solver, so it exercises the GPU stateful path, and
//! writes it through the real `save_ron` so the RON exactly matches what the loader expects.

use aestra_core::{
    EffectAsset, EffectPlaybackMode, Emitter, ModuleInstance, ModuleParameters, ScalarRange,
    MODULE_EMISSION, MODULE_INITIALIZE, MODULE_MOTION,
};

fn main() {
    let mut effect = EffectAsset::new("Persistent Lab", 3.0);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;

    let mut emitter = Emitter::basic_sprite("Persistent Fountain", 3.0);
    emitter.max_particles = 256;

    // Tune the sibling modules to clean values so the stateful integrator (which reads scalar
    // midpoints) makes an obvious fountain: steady spawn, upward launch, downward gravity, ~1.5s life.
    for module in &mut emitter.modules {
        match &mut module.parameters {
            ModuleParameters::Emission { spawn_rate, .. } if module.module_type.0 == MODULE_EMISSION => {
                *spawn_rate = 48.0;
            }
            ModuleParameters::Initialize { lifetime, speed, spread_degrees, .. }
                if module.module_type.0 == MODULE_INITIALIZE =>
            {
                *lifetime = ScalarRange::new(1.5, 1.5);
                *speed = ScalarRange::new(45.0, 45.0);
                *spread_degrees = 25.0;
            }
            ModuleParameters::Motion { gravity, .. } if module.module_type.0 == MODULE_MOTION => {
                *gravity = [0.0, -30.0, 0.0];
            }
            _ => {}
        }
    }

    // The marker that promotes this emitter to the stateful GPU path.
    emitter.modules.push(ModuleInstance::persistent());
    effect.emitters.push(emitter);

    effect.validate().expect("the persistent fixture is valid");
    let path = "sample-project/effects/persistent_lab.aestra.ron";
    effect.save_ron(path).expect("write persistent fixture");

    // Round-trip: the loader must parse the file we just wrote, and the reloaded emitter must still
    // carry the Persistent marker.
    let reloaded = EffectAsset::load_ron(path).expect("reload persistent fixture");
    reloaded.validate().expect("reloaded fixture is valid");
    let has_marker = reloaded.emitters[0]
        .modules
        .iter()
        .any(|module| matches!(module.parameters, ModuleParameters::Persistent {}));
    assert!(has_marker, "reloaded fixture keeps the Persistent module");
    println!("wrote and round-tripped {path}");
}
