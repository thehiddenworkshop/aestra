use aestra_compiler::EffectCompiler;
use aestra_core::{particle_sampling::*, *};

fn source() -> EffectAsset {
    let mut source = EffectAsset::new("Sampling", 5.0);
    let mut emitter = Emitter::basic_sprite("Shell", 5.0);
    for m in &mut emitter.modules {
        if let ModuleParameters::Initialize {
            velocity_distribution,
            ..
        } = &mut m.parameters
        {
            *velocity_distribution = VelocityDistribution::Sphere;
        }
    }
    emitter
        .modules
        .push(ModuleInstance::particle_sampling(ParticleSampling {
            directions: SamplingMode::Stratified,
            seed: u32::MAX,
            ..Default::default()
        }));
    source.emitters.push(emitter);
    source
}
#[test]
fn sampling_is_registered_saved_baked_and_keeps_analytic_execution() {
    let compiler = EffectCompiler::default();
    let source = source();
    let restored = EffectAsset::from_ron(&source.to_pretty_ron().unwrap()).unwrap();
    let compiled = compiler.compile(&restored).unwrap();
    assert_eq!(
        compiled.emitters[0].particle_sampling,
        source.emitters[0]
            .modules
            .last()
            .unwrap()
            .particle_sampling_settings()
    );
    assert_eq!(
        compiled.emitters[0].simulation_class,
        aestra_runtime::SimulationClass::Analytic
    );
    let registry = aestra_extension::ModuleRegistry::builtin();
    let module = registry
        .instantiate(&ModuleTypeId::new(MODULE_PARTICLE_SAMPLING))
        .unwrap();
    assert_eq!(
        module.particle_sampling_settings(),
        Some(ParticleSampling::default())
    );
    let mut disabled = source;
    disabled.emitters[0].modules.last_mut().unwrap().enabled = false;
    assert!(
        compiler.compile(&disabled).unwrap().emitters[0]
            .particle_sampling
            .is_none()
    );
}
#[test]
fn invalid_dynamic_legacy_and_unsupported_stateful_settings_are_diagnosed() {
    let compiler = EffectCompiler::default();
    for value in [Value::U32(0), Value::U32(16_777_217), Value::Scalar(1.0)] {
        let mut source = source();
        let ModuleParameters::Custom(v) =
            &mut source.emitters[0].modules.last_mut().unwrap().parameters
        else {
            unreachable!()
        };
        v.insert("sequence_count".into(), value);
        assert!(compiler.compile(&source).is_err());
    }
    let mut legacy = source();
    for m in &mut legacy.emitters[0].modules {
        if let ModuleParameters::Initialize {
            velocity_distribution,
            ..
        } = &mut m.parameters
        {
            *velocity_distribution = VelocityDistribution::LegacyCone;
        }
    }
    assert!(compiler.compile(&legacy).is_err());
    let mut dynamic = source();
    dynamic.emitters[0]
        .modules
        .last_mut()
        .unwrap()
        .bindings
        .insert("jitter".into(), ParameterId::new());
    assert!(compiler.compile(&dynamic).is_err());
    let mut unsupported = source();
    unsupported.emitters[0]
        .modules
        .push(ModuleInstance::persistent());
    for m in &mut unsupported.emitters[0].modules {
        if let ModuleParameters::Shape { shape } = &mut m.parameters {
            *shape = EmitterShape::Circle { radius: 2.0 };
        }
    }
    assert!(compiler.compile(&unsupported).is_err());
    let mut duplicate = source();
    duplicate.emitters[0]
        .modules
        .push(ModuleInstance::particle_sampling(Default::default()));
    assert!(compiler.compile(&duplicate).is_err());
}
