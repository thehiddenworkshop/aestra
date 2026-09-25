//! Fluid F2: module commands addressed to `EmitterId::EFFECT_SCOPE` edit the effect's own simulation
//! stages, undo exactly, and never touch an emitter.

use aestra_authoring::{CommandExecutor, EffectCommand, EffectTransaction, LockState};
use aestra_core::{
    EffectAsset, EffectSimulationStage, Emitter, EmitterId, ModuleInstance, ModuleParameters,
    StageKind, StageTypeId, Value,
};
use std::collections::BTreeMap;

fn domain_module() -> ModuleInstance {
    let mut module = ModuleInstance::motion([0.0; 3], 0.0, 0.0);
    module.stage = StageKind::Simulation("Fluid".into());
    module.parameters = ModuleParameters::Custom(BTreeMap::new());
    module
}

fn effect() -> EffectAsset {
    let mut effect = EffectAsset::new("Scoped", 2.0);
    effect.simulation_stages.push(EffectSimulationStage::new(
        "Fluid",
        StageTypeId::new("org.x::stage/solver"),
    ));
    effect.emitters.push(Emitter::basic_sprite("Smoke", 2.0));
    effect
}

#[test]
fn effect_scope_module_commands_edit_the_domain_and_undo() {
    let original = effect();
    let mut effect = original.clone();
    let module = domain_module();
    let id = module.id;
    let commands = vec![
        EffectCommand::AddModule {
            emitter: EmitterId::EFFECT_SCOPE,
            module,
            index: 0,
        },
        EffectCommand::SetModuleParameter {
            emitter: EmitterId::EFFECT_SCOPE,
            module: id,
            parameter: "radius".into(),
            value: Value::Scalar(2.5),
        },
        EffectCommand::SetModuleEnabled {
            emitter: EmitterId::EFFECT_SCOPE,
            module: id,
            enabled: false,
        },
    ];
    let outcome = CommandExecutor::execute(
        &mut effect,
        &LockState::default(),
        &EffectTransaction::new("Edit domain", commands),
    )
    .unwrap();
    assert!(!outcome.diff.is_empty(), "the edit is a reported change");
    let stage = &effect.simulation_stages[0];
    assert_eq!(stage.modules.len(), 1);
    assert!(!stage.modules[0].enabled);
    assert_eq!(
        stage.modules[0].parameter_value("radius"),
        Some(Value::Scalar(2.5))
    );
    assert_eq!(effect.emitters, original.emitters, "no emitter was touched");

    CommandExecutor::execute(&mut effect, &LockState::default(), &outcome.inverse).unwrap();
    assert_eq!(effect, original, "undo restores the document exactly");
}
