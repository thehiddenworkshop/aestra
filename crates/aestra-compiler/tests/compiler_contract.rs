use aestra_compiler::{EffectCompiler, ModuleRegistry};
use aestra_core::{
    DiagnosticCode, EffectAsset, Emitter, ModuleInstance, ModuleParameters, ModuleTypeId, StageKind,
};
use aestra_runtime::{EffectInstance, ParticleAttribute, RuntimeStage};
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
