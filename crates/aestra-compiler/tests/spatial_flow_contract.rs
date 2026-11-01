use aestra_compiler::EffectCompiler;
use aestra_core::{spatial_flow::*, *};
#[test]
fn spatial_flow_is_registered_editable_roundtrips_and_requires_stateful_simulation() {
    let compiler = EffectCompiler::default();
    let mut effect = EffectAsset::new("Shared flow", 5.0);
    let mut emitter = Emitter::basic_sprite("Smoke", 5.0);
    let flow = ModuleInstance::spatial_flow(SpatialFlow::default());
    emitter.modules.push(flow.clone());
    effect.emitters.push(emitter);
    let saved = EffectAsset::from_ron(&effect.to_pretty_ron().unwrap()).unwrap();
    let compiled = compiler.compile(&saved).unwrap();
    assert_eq!(
        compiled.emitters[0].spatial_flow,
        Some(SpatialFlow::default())
    );
    assert_eq!(
        compiled.emitters[0].simulation_class,
        aestra_runtime::SimulationClass::Stateful
    );
    let registry = aestra_extension::ModuleRegistry::builtin();
    let instance = registry.instantiate(&flow.module_type).unwrap();
    assert_eq!(
        instance.spatial_flow_settings(),
        flow.spatial_flow_settings()
    );
    let mut invalid = effect.clone();
    if let ModuleParameters::Custom(values) =
        &mut invalid.emitters[0].modules.last_mut().unwrap().parameters
    {
        values.insert("octaves".into(), Value::U32(100));
    }
    assert!(compiler.compile(&invalid).is_err());
    let mut disabled = effect;
    disabled.emitters[0].modules.last_mut().unwrap().enabled = false;
    let compiled = compiler.compile(&disabled).unwrap();
    assert!(compiled.emitters[0].spatial_flow.is_none());
    assert_eq!(
        compiled.emitters[0].simulation_class,
        aestra_runtime::SimulationClass::Analytic
    );
}
