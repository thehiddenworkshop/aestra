use aestra_compiler::{EffectCompiler, ModuleRegistry};
use aestra_core::{
    DiagnosticCode, EffectAsset, EffectParameter, Emitter, MODULE_EMISSION, ModuleInstance,
    ModuleParameters, ModuleTypeId, ParameterId, ScalarRange, StageKind, Value,
};
use aestra_runtime::{
    EffectInstance, Expression, Instruction, ParameterError, ParticleAttribute, RuntimeStage,
};
use std::{collections::BTreeMap, sync::Arc};

const SAMPLE: &str = include_str!("../../../assets/effects/prism_bloom.aestra.ron");

#[test]
fn builtin_registry_exposes_authoring_and_runtime_metadata() {
    let registry = ModuleRegistry::builtin();
    assert_eq!(registry.len(), 5);

    let motion = registry
        .iter()
        .find(|metadata| metadata.type_id.0 == "aestra.update.motion")
        .expect("motion metadata must be registered");
    assert_eq!(motion.category, "Forces");
    assert_eq!(motion.stages, [StageKind::ParticleUpdate]);
    assert!(motion.reads.contains(&ParticleAttribute::Velocity));
    assert!(motion.writes.contains(&ParticleAttribute::Position));
    assert!(motion.approximate_cost > 0);
    let gravity = &motion.inputs[0];
    assert_eq!(gravity.display_name, "Gravity");
    assert_eq!(gravity.unit, Some("units/s²"));
    assert_eq!(gravity.default_value, Value::Vec2([0.0, -18.0]));
    assert!(!gravity.description.is_empty());
}

#[test]
fn builtin_registry_instantiates_every_catalog_module() {
    let registry = ModuleRegistry::builtin();
    for metadata in registry.iter() {
        let instance = registry
            .instantiate(&metadata.type_id)
            .expect("built-in catalog entries must be authorable");
        assert_eq!(instance.module_type, metadata.type_id);
        assert!(metadata.stages.contains(&instance.stage));
        for input in &metadata.inputs {
            assert_eq!(instance.parameter_type(input.name), Some(input.value_type));
        }
    }
}

#[test]
fn complex_metadata_defaults_receive_fresh_semantic_ids() {
    let registry = ModuleRegistry::builtin();
    let appearance = registry
        .iter()
        .find(|metadata| metadata.type_id.0.ends_with("appearance"))
        .unwrap();
    let Value::Curve(first) = appearance.inputs[0].instantiate_default() else {
        panic!("size default must be a curve");
    };
    let Value::Curve(second) = appearance.inputs[0].instantiate_default() else {
        panic!("size default must be a curve");
    };
    assert!(!first.id.is_nil());
    assert_ne!(first.id, second.id);
}

#[test]
fn compiler_lowers_ordered_stages_and_records_source_locations() {
    let asset = EffectAsset::from_ron(SAMPLE).unwrap();
    let compiled = EffectCompiler::default().compile(&asset).unwrap();

    assert_eq!(compiled.source, asset.id);
    assert_eq!(compiled.emitters.len(), asset.emitters.len());
    assert_eq!(compiled.source_map.len(), 20);
    assert!(
        compiled
            .particle_layout
            .attributes
            .contains(&ParticleAttribute::Color)
    );
    assert!(
        !compiled
            .particle_layout
            .attributes
            .contains(&ParticleAttribute::NormalizedAge)
    );
    assert!(
        compiled
            .particle_layout
            .transient_attributes
            .contains(&ParticleAttribute::NormalizedAge)
    );
    assert!(compiled.optimizations.constant_expressions > 0);
    assert!(compiled.optimizations.eliminated_attributes > 0);

    let motion = &asset.emitters[0].modules[3];
    let location = compiled.source_map.get(&motion.id).unwrap();
    assert_eq!(location.stage, RuntimeStage::ParticleUpdate);
    assert_eq!(location.instruction_index, 0);
}

#[test]
fn unregistered_modules_produce_targeted_diagnostics() {
    let mut asset = EffectAsset::new("Unknown module", 1.0);
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    emitter.modules.push(ModuleInstance {
        id: aestra_core::ModuleId::new(),
        module_type: ModuleTypeId::new("example.custom.warp"),
        stage: StageKind::ParticleUpdate,
        enabled: true,
        parameters: ModuleParameters::Custom(BTreeMap::new()),
        bindings: BTreeMap::new(),
    });
    asset.emitters.push(emitter);

    let error = EffectCompiler::default().compile(&asset).unwrap_err();
    assert!(error.report().diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::UnknownModule
            && diagnostic.path.ends_with("modules[5].module_type")
    }));
}

#[test]
fn runtime_instances_are_deterministic_per_seed() {
    let asset = EffectAsset::from_ron(SAMPLE).unwrap();
    let compiled = Arc::new(EffectCompiler::default().compile(&asset).unwrap());
    let mut first = EffectInstance::with_seed(compiled.clone(), 42);
    let mut second = EffectInstance::with_seed(compiled.clone(), 42);
    let mut other = EffectInstance::with_seed(compiled, 7);
    first.seek(0.75);
    second.seek(0.75);
    other.seek(0.75);

    let mut first_samples = Vec::new();
    let mut second_samples = Vec::new();
    let mut other_samples = Vec::new();
    first.evaluate(&mut first_samples);
    second.evaluate(&mut second_samples);
    other.evaluate(&mut other_samples);

    assert_eq!(first_samples, second_samples);
    assert_ne!(first_samples, other_samples);
}

#[test]
fn exposed_bindings_update_instances_without_recompiling() {
    let (asset, parameter_id) = parameterized_effect(true);
    let compiled = Arc::new(EffectCompiler::default().compile(&asset).unwrap());
    assert_eq!(compiled.parameters.len(), 1);
    assert_eq!(compiled.optimizations.runtime_parameter_reads, 1);
    let emission = &compiled.emitters[0].execution.emitter_update[0];
    assert!(matches!(
        emission,
        Instruction::Emit {
            spawn_rate: Expression::Parameter(_),
            ..
        }
    ));

    let mut instance = EffectInstance::new(compiled);
    instance.seek(0.5);
    let mut default_samples = Vec::new();
    instance.evaluate(&mut default_samples);

    instance
        .set_parameter(parameter_id, Value::Scalar(20.0))
        .unwrap();
    let mut overridden_samples = Vec::new();
    instance.evaluate(&mut overridden_samples);
    assert_eq!(default_samples.len(), 2);
    assert_eq!(overridden_samples.len(), 10);
    assert_eq!(instance.overridden_parameters().count(), 1);

    let error = instance
        .set_parameter(parameter_id, Value::Vec2([1.0, 2.0]))
        .unwrap_err();
    assert!(matches!(error, ParameterError::TypeMismatch { .. }));

    instance.clear_parameter(parameter_id).unwrap();
    let mut restored_samples = Vec::new();
    instance.evaluate(&mut restored_samples);
    assert_eq!(restored_samples, default_samples);
}

#[test]
fn non_exposed_bindings_are_constant_folded() {
    let (asset, _) = parameterized_effect(false);
    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    assert!(compiled.parameters.is_empty());
    assert_eq!(compiled.optimizations.runtime_parameter_reads, 0);
    let Instruction::Emit { spawn_rate, .. } = &compiled.emitters[0].execution.emitter_update[0]
    else {
        panic!("first instruction must emit particles");
    };
    assert_eq!(spawn_rate.constant_value(), Some(&4.0));
}

fn parameterized_effect(exposed: bool) -> (EffectAsset, ParameterId) {
    let mut asset = EffectAsset::new("Parameterized", 2.0);
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: "Spawn Rate".into(),
        default: Value::Scalar(4.0),
        exposed,
    };
    let parameter_id = parameter.id;
    let mut emitter = Emitter::basic_sprite("Emitter", 2.0);
    *emitter.burst_count_mut() = 0;
    *emitter.lifetime_mut() = ScalarRange::new(2.0, 2.0);
    emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_EMISSION)
        .unwrap()
        .bindings
        .insert("spawn_rate".into(), parameter_id);
    asset.parameters.push(parameter);
    asset.emitters.push(emitter);
    (asset, parameter_id)
}
