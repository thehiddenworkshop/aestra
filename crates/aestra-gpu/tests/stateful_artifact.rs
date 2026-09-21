//! The GPU artifact side of mixed analytic + stateful effects (hybrid roadmap M6): a mixed effect
//! must mark exactly its stateful emitters (so the analytic `simulate` skips their slots) and size the
//! persistent state from only those emitters. This is the headless data path the render orchestration
//! consumes.

use std::sync::Arc;

use aestra_compiler::EffectCompiler;
use aestra_core::{EffectAsset, Emitter, ModuleInstance};
use aestra_gpu::GpuEffectArtifact;
use aestra_runtime::EffectInstance;

fn artifact(asset: &EffectAsset) -> GpuEffectArtifact {
    let compiled = Arc::new(EffectCompiler::default().compile(asset).unwrap());
    GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap()
}

#[test]
fn a_mixed_effect_marks_only_its_stateful_emitters() {
    // One analytic emitter, then two stateful emitters (Persistent marker), in one effect.
    let mut asset = EffectAsset::new("Mixed", 3.0);
    asset
        .emitters
        .push(Emitter::basic_sprite("Analytic Sparks", 3.0));
    for (name, capacity) in [("Stateful Fountain", 200u32), ("Stateful Drift", 96)] {
        let mut emitter = Emitter::basic_sprite(name, 3.0);
        emitter.max_particles = capacity;
        emitter.modules.push(ModuleInstance::persistent());
        asset.emitters.push(emitter);
    }

    let artifact = artifact(&asset);

    // The analytic `simulate` keys off this flag to skip stateful slots: only the Persistent emitters
    // are marked.
    assert_eq!(
        artifact.emitters[0].stateful, 0,
        "the analytic emitter is not marked stateful"
    );
    assert_eq!(
        artifact.emitters[1].stateful, 1,
        "the first Persistent emitter is marked stateful"
    );
    assert_eq!(
        artifact.emitters[2].stateful, 1,
        "the second Persistent emitter is marked stateful"
    );

    // The persistent state is sized from only the stateful emitters' capacities.
    assert_eq!(
        artifact.simulation_state.records, 296,
        "state records = 200 + 96 (the stateful emitters), not the analytic emitter's slots"
    );
    assert!(artifact.simulation_state.stride > 0);
}

#[test]
fn a_fully_analytic_effect_marks_no_emitter_and_needs_no_state() {
    let mut asset = EffectAsset::new("Analytic", 3.0);
    asset.emitters.push(Emitter::basic_sprite("Sparks", 3.0));
    asset.emitters.push(Emitter::basic_sprite("Embers", 3.0));

    let artifact = artifact(&asset);

    assert!(
        artifact
            .emitters
            .iter()
            .all(|emitter| emitter.stateful == 0),
        "no emitter is marked stateful in a fully analytic effect"
    );
    assert_eq!(
        artifact.simulation_state.records, 0,
        "an analytic effect allocates no persistent state"
    );
}
