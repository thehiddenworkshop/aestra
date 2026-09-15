//! The GPU artifact reserves persistent simulation-state storage for stateful emitters and none for
//! analytic ones (hybrid roadmap M4/M6, real-backend increment). This is the engine-neutral sizing
//! the render backend allocates its state buffer from.

use aestra_compiler::{
    EffectCompiler, ExtensionRegistry, SimulationRequirements, TemporalRequirement,
};
use aestra_core::{EffectAsset, Emitter, MODULE_MOTION, ModuleTypeId};
use aestra_gpu::{GpuEffectArtifact, GpuEffectDynamics, GpuSimulationState};
use aestra_runtime::EffectInstance;
use std::sync::Arc;

fn dynamics(compiler: &EffectCompiler, asset: &EffectAsset) -> GpuEffectDynamics {
    let compiled = compiler.compile(asset).expect("effect compiles");
    let instance = EffectInstance::new(Arc::new(compiled));
    GpuEffectArtifact::dynamics_from_instance(&instance).expect("artifact builds")
}

/// A registry in which the built-in Motion module is stateful, so a normal sprite emitter compiles
/// to a stateful island without any bespoke module.
fn stateful_compiler() -> EffectCompiler {
    let mut registry = ExtensionRegistry::builtin();
    let mut motion = registry
        .modules
        .get(&ModuleTypeId::new(MODULE_MOTION))
        .unwrap()
        .clone();
    motion.simulation = SimulationRequirements {
        temporal: TemporalRequirement::PreviousState,
        ..Default::default()
    };
    registry.modules.register(motion);
    EffectCompiler::with_extensions(registry)
}

#[test]
fn analytic_effects_reserve_no_simulation_state() {
    let mut asset = EffectAsset::new("Analytic", 2.0);
    asset.emitters.push(Emitter::basic_sprite("Sparks", 2.0));

    let built = dynamics(&EffectCompiler::default(), &asset);
    assert_eq!(
        built.simulation_state,
        GpuSimulationState::default(),
        "analytic effects allocate no persistent simulation state"
    );
    assert_eq!(built.simulation_state.records, 0);
}

#[test]
fn stateful_emitters_reserve_persistent_state_sized_by_capacity() {
    let emitter = Emitter::basic_sprite("Debris", 2.0);
    let capacity = emitter.max_particles;
    let mut asset = EffectAsset::new("Debris", 2.0);
    asset.emitters.push(emitter);

    let built = dynamics(&stateful_compiler(), &asset);
    assert_eq!(
        built.simulation_state.stride, 8,
        "prototype state stride is position + velocity + age + lifetime = 8 floats"
    );
    assert_eq!(
        built.simulation_state.records, capacity,
        "one persistent-state record per stateful slot"
    );
}

#[test]
fn disabled_stateful_emitters_reserve_no_state() {
    let mut emitter = Emitter::basic_sprite("Debris", 2.0);
    emitter.enabled = false;
    let mut asset = EffectAsset::new("Disabled", 2.0);
    // A second, enabled analytic emitter keeps the effect compilable.
    asset.emitters.push(Emitter::basic_sprite("Sparks", 2.0));
    asset.emitters.push(emitter);

    let built = dynamics(&stateful_compiler(), &asset);
    // Only the enabled emitter counts, and it is analytic in the stock modules... but this compiler
    // makes Motion stateful, so the enabled "Sparks" is stateful and the disabled "Debris" is not.
    assert_eq!(
        built.simulation_state.records, asset.emitters[0].max_particles,
        "disabled emitters reserve no persistent state"
    );
}
