//! Fluid F2: simulation stages the effect itself owns — authored beside the emitters, round-tripped
//! through format v4, addressed by the reserved `EmitterId::EFFECT_SCOPE` owner, and validated.

use aestra_core::{
    DiagnosticCode, EffectAsset, EffectSimulationStage, Emitter, EmitterId, ExtensionId,
    ModuleInstance, ModuleParameters, ModuleTypeId, StageKind, StageTypeId,
};
use std::collections::BTreeMap;

fn plugin_module(stage: &str) -> ModuleInstance {
    let mut module = ModuleInstance::motion([0.0; 3], 0.0, 0.0);
    module.module_type = ModuleTypeId::new("org.x::module/grid");
    module.stage = StageKind::Simulation(stage.into());
    module.parameters = ModuleParameters::Custom(BTreeMap::new());
    module
}

fn effect_with_domain() -> EffectAsset {
    let mut effect = EffectAsset::new("Domain", 2.0);
    let mut domain = EffectSimulationStage::new("Fluid", StageTypeId::new("org.x::stage/solver"));
    domain.modules.push(plugin_module("Fluid"));
    effect.simulation_stages.push(domain);
    effect.emitters.push(Emitter::basic_sprite("Smoke", 2.0));
    effect
}

#[test]
fn an_effect_level_stage_round_trips_through_format_v4() {
    let effect = effect_with_domain();
    let ron = effect.to_pretty_ron().unwrap();
    assert!(ron.contains("simulation_stages"));
    assert_eq!(EffectAsset::from_ron(&ron).unwrap(), effect);
    assert!(
        effect
            .referenced_plugins()
            .contains(&ExtensionId::new("org.x")),
        "the stage type and its modules name their plugin"
    );
}

#[test]
fn effect_scope_modules_resolve_by_owner() {
    let mut effect = effect_with_domain();
    let module = effect.simulation_stages[0].modules[0].id;
    assert!(effect.module(EmitterId::EFFECT_SCOPE, module).is_some());
    let emitter = effect.emitters[0].id;
    assert!(
        effect.module(emitter, module).is_none(),
        "not an emitter's module"
    );
    effect
        .module_mut(EmitterId::EFFECT_SCOPE, module)
        .unwrap()
        .enabled = false;
    assert!(!effect.simulation_stages[0].modules[0].enabled);
}

#[test]
fn effect_level_stages_are_validated() {
    let codes = |effect: &EffectAsset| -> Vec<DiagnosticCode> {
        effect
            .validation_report()
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    };
    let mut duplicate = effect_with_domain();
    let copy = EffectSimulationStage::new("Fluid", StageTypeId::new("org.x::stage/solver"));
    duplicate.simulation_stages.push(copy);
    assert!(codes(&duplicate).contains(&DiagnosticCode::DuplicateId));

    let mut misplaced = effect_with_domain();
    misplaced.simulation_stages[0].modules[0].stage = StageKind::ParticleUpdate;
    assert!(codes(&misplaced).contains(&DiagnosticCode::StageMismatch));

    let mut reserved = effect_with_domain();
    reserved.emitters[0].id = EmitterId::EFFECT_SCOPE;
    assert!(codes(&reserved).contains(&DiagnosticCode::InvalidValue));
}
