use aestra_compiler::{
    EffectCompiler, InputEvaluationDomain, InputSourceKind, ModuleRegistry, ProjectCompileError,
};
use aestra_core::{
    ChoreographyEvent, ChoreographyEventPayload, Curve, CurveKey, DiagnosticCode, EffectAsset,
    EffectClip, EffectClipSeed, EffectParameter, Emitter, EmitterShape, MODULE_EMISSION,
    MODULE_INITIALIZE, MODULE_MOTION, MODULE_SHAPE, MaterialInput, MaterialProperties,
    ModuleInstance, ModuleParameters, ModuleTypeId, ParameterId, PropertySourceValue, ScalarRange,
    StageKind, Value, Vec3Curve, Vec3Range,
};
use aestra_project::ProjectAssetIndex;
use aestra_runtime::{
    EffectInstance, Expression, Instruction, ParameterError, ParticleAttribute, RendererPlanKind,
    RuntimeStage, ScalarSource, SimulationSeekMode, VectorSource,
};
use std::{collections::BTreeMap, sync::Arc};

const SAMPLE: &str = include_str!("../../../assets/effects/prism_bloom.aestra.ron");
const TEXTURED_SAMPLE: &str = include_str!("../../../assets/effects/ember_sigil.aestra.ron");
const FLIPBOOK_SAMPLE: &str = include_str!("../../../assets/effects/plasma_burst.aestra.ron");

#[test]
fn builtin_registry_exposes_authoring_and_runtime_metadata() {
    let registry = ModuleRegistry::builtin();
    assert_eq!(registry.len(), 5);

    let motion = registry
        .iter()
        .find(|metadata| metadata.type_id.0 == "aestra.update.motion")
        .expect("motion metadata must be registered");
    assert_eq!(motion.category, "Forces");
    assert!(motion.description.contains("gravity"));
    assert_eq!(motion.stages, [StageKind::ParticleUpdate]);
    assert!(motion.reads.contains(&ParticleAttribute::Velocity));
    assert!(motion.writes.contains(&ParticleAttribute::Position));
    assert!(motion.approximate_cost > 0);
    let gravity = &motion.inputs[0];
    assert_eq!(gravity.display_name, "Gravity");
    assert_eq!(gravity.unit, Some("units/s²"));
    assert_eq!(gravity.default_value, Value::Vec3([0.0, -18.0, 0.0]));
    assert!(!gravity.description.is_empty());
    assert_eq!(
        gravity.sources,
        [
            InputSourceKind::Constant,
            InputSourceKind::RandomRange,
            InputSourceKind::Curve(InputEvaluationDomain::ParticleLife),
        ]
    );
    let drag = motion
        .inputs
        .iter()
        .find(|input| input.name == "drag")
        .expect("motion drag metadata must be registered");
    assert_eq!(
        drag.sources,
        [
            InputSourceKind::Constant,
            InputSourceKind::RandomRange,
            InputSourceKind::Curve(InputEvaluationDomain::ParticleLife),
        ]
    );
    let turbulence = motion
        .inputs
        .iter()
        .find(|input| input.name == "turbulence")
        .expect("motion turbulence metadata must be registered");
    assert_eq!(turbulence.sources, drag.sources);

    let initialize = registry
        .iter()
        .find(|metadata| metadata.type_id.0 == "aestra.spawn.initialize")
        .expect("initialize metadata must be registered");
    assert_eq!(
        initialize.inputs[0].sources,
        [InputSourceKind::Constant, InputSourceKind::RandomRange]
    );

    let appearance = registry
        .iter()
        .find(|metadata| metadata.type_id.0 == "aestra.update.appearance")
        .expect("appearance metadata must be registered");
    assert_eq!(appearance.display_name, "Appearance");
    assert_eq!(appearance.inputs[0].display_name, "Size");
    assert_eq!(
        appearance.inputs[0].sources,
        [
            InputSourceKind::Constant,
            InputSourceKind::Curve(InputEvaluationDomain::ParticleLife),
        ]
    );
    assert_eq!(
        appearance.inputs[2].sources,
        [
            InputSourceKind::Constant,
            InputSourceKind::Gradient(InputEvaluationDomain::ParticleLife),
        ]
    );
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
    assert_eq!(compiled.seek_mode, SimulationSeekMode::StatelessDirect);
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
fn explicit_constant_source_controls_lowering_independently_of_curve_key_count() {
    let mut asset = EffectAsset::from_ron(SAMPLE).unwrap();
    let appearance = asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == aestra_core::MODULE_APPEARANCE)
        .unwrap();
    let Value::Curve(authored) = appearance.parameter_value("size").unwrap() else {
        panic!("size must retain its authored curve data");
    };
    assert!(authored.keys.len() > 1);
    appearance
        .property_sources
        .insert("size".into(), InputSourceKind::Constant);

    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let Instruction::Appearance { size, .. } = compiled.emitters[0]
        .execution
        .particle_update
        .iter()
        .find(|instruction| matches!(instruction, Instruction::Appearance { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    let Expression::Constant(size) = size else {
        panic!("unbound size should lower to a constant expression");
    };
    assert_eq!(size.sample(0.0), size.sample(1.0));
}

#[test]
fn spawn_rate_emitter_curve_is_preserved_and_lowered_as_a_scalar_source() {
    let mut asset = EffectAsset::from_ron(SAMPLE).unwrap();
    let emission = asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_EMISSION)
        .unwrap();
    let source = InputSourceKind::Curve(InputEvaluationDomain::EmitterTime);
    let curve = Curve::normalized(
        vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)],
        ScalarRange::new(0.0, 20.0),
    );
    emission.property_source_values.insert(
        "spawn_rate".into(),
        vec![PropertySourceValue::new(
            source,
            Value::Curve(curve.clone()),
        )],
    );
    emission
        .property_sources
        .insert("spawn_rate".into(), source);

    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let Instruction::Emit { spawn_rate, .. } = &compiled.emitters[0].execution.emitter_update[0]
    else {
        panic!("first instruction must emit particles");
    };
    let ScalarSource::Curve { value, domain } = spawn_rate else {
        panic!("spawn rate should lower as a curve source");
    };
    assert_eq!(*domain, InputEvaluationDomain::EmitterTime);
    let Expression::Constant(compiled_curve) = value else {
        panic!("unbound curve should be constant-folded");
    };
    assert_eq!(compiled_curve.sample(0.0), curve.sample(0.0));
    assert_eq!(compiled_curve.sample(1.0), curve.sample(1.0));
    assert!((compiled_curve.integral(1.0) - 10.0).abs() < 0.0001);
}

#[test]
fn spawn_rate_emitter_curve_controls_accumulated_particle_count() {
    let mut asset = EffectAsset::new("Emitter-time rate", 2.0);
    asset.looping = false;
    asset
        .emitters
        .push(Emitter::basic_sprite("Emitter", asset.duration));
    let emitter = &mut asset.emitters[0];
    let emission = emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_EMISSION)
        .unwrap();
    let ModuleParameters::Emission { burst_count, .. } = &mut emission.parameters else {
        unreachable!()
    };
    *burst_count = 0;
    let source = InputSourceKind::Curve(InputEvaluationDomain::EmitterTime);
    emission
        .property_sources
        .insert("spawn_rate".into(), source);
    emission.property_source_values.insert(
        "spawn_rate".into(),
        vec![PropertySourceValue::new(
            source,
            Value::Curve(Curve::new(vec![
                CurveKey::new(0.0, 0.0),
                CurveKey::new(1.0, 20.0),
            ])),
        )],
    );
    let initialize = emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_INITIALIZE)
        .unwrap();
    initialize.bindings.remove("lifetime");
    let ModuleParameters::Initialize { lifetime, .. } = &mut initialize.parameters else {
        unreachable!()
    };
    *lifetime = ScalarRange::new(10.0, 10.0);

    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let mut particles = Vec::new();
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut particles);
    assert_eq!(particles.len(), 3);
    aestra_runtime::evaluate(&compiled, 2.0, 42, &mut particles);
    assert_eq!(particles.len(), 20);
}

#[test]
fn spawn_rate_random_range_is_deterministic_for_an_effect_seed() {
    let mut asset = EffectAsset::new("Random rate", 2.0);
    asset.looping = false;
    asset
        .emitters
        .push(Emitter::basic_sprite("Emitter", asset.duration));
    let emitter = &mut asset.emitters[0];
    let emission = emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_EMISSION)
        .unwrap();
    let ModuleParameters::Emission { burst_count, .. } = &mut emission.parameters else {
        unreachable!()
    };
    *burst_count = 0;
    emission
        .property_sources
        .insert("spawn_rate".into(), InputSourceKind::RandomRange);
    emission.property_source_values.insert(
        "spawn_rate".into(),
        vec![PropertySourceValue::new(
            InputSourceKind::RandomRange,
            Value::Range(ScalarRange::new(10.0, 30.0)),
        )],
    );
    let initialize = emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_INITIALIZE)
        .unwrap();
    let ModuleParameters::Initialize { lifetime, .. } = &mut initialize.parameters else {
        unreachable!()
    };
    *lifetime = ScalarRange::new(10.0, 10.0);

    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let Instruction::Emit { spawn_rate, .. } = &compiled.emitters[0].execution.emitter_update[0]
    else {
        unreachable!()
    };
    assert_eq!(
        spawn_rate,
        &ScalarSource::RandomRange(Expression::Constant(ScalarRange::new(10.0, 30.0)))
    );
    let mut first = Vec::new();
    let mut repeated = Vec::new();
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut first);
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut repeated);
    assert_eq!(first, repeated);
    assert!((10..=30).contains(&first.len()));
}

#[test]
fn drag_sources_lower_without_losing_their_authored_values() {
    let mut asset = EffectAsset::from_ron(SAMPLE).unwrap();
    let motion = asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    let source = InputSourceKind::Curve(InputEvaluationDomain::ParticleLife);
    let curve = Curve::normalized(
        vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)],
        ScalarRange::new(0.0, 4.0),
    );
    motion.property_sources.insert("drag".into(), source);
    motion.property_source_values.insert(
        "drag".into(),
        vec![PropertySourceValue::new(
            source,
            Value::Curve(curve.clone()),
        )],
    );

    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let Instruction::Motion { drag, .. } = compiled.emitters[0]
        .execution
        .particle_update
        .iter()
        .find(|instruction| matches!(instruction, Instruction::Motion { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    let ScalarSource::Curve { value, domain } = drag else {
        panic!("drag should lower as a curve source");
    };
    assert_eq!(*domain, InputEvaluationDomain::ParticleLife);
    let Expression::Constant(compiled_curve) = value else {
        panic!("unbound drag curve should be constant-folded");
    };
    assert_eq!(compiled_curve.sample(0.0), curve.sample(0.0));
    assert_eq!(compiled_curve.sample(1.0), curve.sample(1.0));

    let mut asset = EffectAsset::from_ron(SAMPLE).unwrap();
    let motion = asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    motion
        .property_sources
        .insert("drag".into(), InputSourceKind::RandomRange);
    motion.property_source_values.insert(
        "drag".into(),
        vec![PropertySourceValue::new(
            InputSourceKind::RandomRange,
            Value::Range(ScalarRange::new(0.25, 1.5)),
        )],
    );
    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let Instruction::Motion { drag, .. } = compiled.emitters[0]
        .execution
        .particle_update
        .iter()
        .find(|instruction| matches!(instruction, Instruction::Motion { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(
        drag,
        &ScalarSource::RandomRange(Expression::Constant(ScalarRange::new(0.25, 1.5)))
    );
}

#[test]
fn gravity_sources_lower_without_losing_their_authored_channels() {
    let mut asset = EffectAsset::from_ron(SAMPLE).unwrap();
    let motion = asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    let source = InputSourceKind::Curve(InputEvaluationDomain::ParticleLife);
    let curves = Vec3Curve {
        curves: [
            Curve::normalized(
                vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)],
                ScalarRange::new(-2.0, 4.0),
            ),
            Curve::normalized(
                vec![CurveKey::new(0.0, 1.0), CurveKey::new(1.0, 0.0)],
                ScalarRange::new(-8.0, 2.0),
            ),
            Curve::normalized(
                vec![CurveKey::new(0.0, 0.25), CurveKey::new(1.0, 0.75)],
                ScalarRange::new(0.0, 12.0),
            ),
        ],
    };
    motion.property_sources.insert("gravity".into(), source);
    motion.property_source_values.insert(
        "gravity".into(),
        vec![PropertySourceValue::new(
            source,
            Value::Vec3Curve(curves.clone()),
        )],
    );

    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let Instruction::Motion { gravity, .. } = compiled.emitters[0]
        .execution
        .particle_update
        .iter()
        .find(|instruction| matches!(instruction, Instruction::Motion { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    let VectorSource::Curve { value, domain } = gravity else {
        panic!("gravity should lower as an XYZ curve source");
    };
    assert_eq!(*domain, InputEvaluationDomain::ParticleLife);
    let Expression::Constant(compiled_curves) = value else {
        panic!("unbound gravity curves should be constant-folded");
    };
    assert_eq!(compiled_curves.sample(0.5), curves.sample(0.5));

    let mut asset = EffectAsset::from_ron(SAMPLE).unwrap();
    let motion = asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    let range = Vec3Range::new([-4.0, -8.0, 1.0], [5.0, 2.0, 9.0]);
    motion
        .property_sources
        .insert("gravity".into(), InputSourceKind::RandomRange);
    motion.property_source_values.insert(
        "gravity".into(),
        vec![PropertySourceValue::new(
            InputSourceKind::RandomRange,
            Value::Vec3Range(range),
        )],
    );
    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let Instruction::Motion { gravity, .. } = compiled.emitters[0]
        .execution
        .particle_update
        .iter()
        .find(|instruction| matches!(instruction, Instruction::Motion { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(
        gravity,
        &VectorSource::RandomRange(Expression::Constant(range))
    );
}

#[test]
fn turbulence_sources_lower_without_losing_their_authored_values() {
    let mut asset = EffectAsset::from_ron(SAMPLE).unwrap();
    let motion = asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    let source = InputSourceKind::Curve(InputEvaluationDomain::ParticleLife);
    let curve = Curve::normalized(
        vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)],
        ScalarRange::new(0.0, 6.0),
    );
    motion.property_sources.insert("turbulence".into(), source);
    motion.property_source_values.insert(
        "turbulence".into(),
        vec![PropertySourceValue::new(
            source,
            Value::Curve(curve.clone()),
        )],
    );

    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let Instruction::Motion { turbulence, .. } = compiled.emitters[0]
        .execution
        .particle_update
        .iter()
        .find(|instruction| matches!(instruction, Instruction::Motion { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    let ScalarSource::Curve { value, domain } = turbulence else {
        panic!("turbulence should lower as a curve source");
    };
    assert_eq!(*domain, InputEvaluationDomain::ParticleLife);
    let Expression::Constant(compiled_curve) = value else {
        panic!("unbound turbulence curve should be constant-folded");
    };
    assert_eq!(compiled_curve.sample(0.0), curve.sample(0.0));
    assert_eq!(compiled_curve.sample(1.0), curve.sample(1.0));

    let mut asset = EffectAsset::from_ron(SAMPLE).unwrap();
    let motion = asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    motion
        .property_sources
        .insert("turbulence".into(), InputSourceKind::RandomRange);
    motion.property_source_values.insert(
        "turbulence".into(),
        vec![PropertySourceValue::new(
            InputSourceKind::RandomRange,
            Value::Range(ScalarRange::new(1.0, 5.0)),
        )],
    );
    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let Instruction::Motion { turbulence, .. } = compiled.emitters[0]
        .execution
        .particle_update
        .iter()
        .find(|instruction| matches!(instruction, Instruction::Motion { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(
        turbulence,
        &ScalarSource::RandomRange(Expression::Constant(ScalarRange::new(1.0, 5.0)))
    );
}

fn configured_motion_effect() -> EffectAsset {
    let mut asset = EffectAsset::new("Variable motion", 2.0);
    asset.looping = false;
    asset
        .emitters
        .push(Emitter::basic_sprite("Emitter", asset.duration));
    let emitter = &mut asset.emitters[0];
    emitter.max_particles = 8;
    for module in &mut emitter.modules {
        module.bindings.clear();
    }
    let emission = emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_EMISSION)
        .unwrap();
    let ModuleParameters::Emission {
        spawn_rate,
        burst_count,
    } = &mut emission.parameters
    else {
        unreachable!()
    };
    *spawn_rate = 0.0;
    *burst_count = 2;
    let shape = emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_SHAPE)
        .unwrap();
    shape.parameters = ModuleParameters::Shape {
        shape: EmitterShape::Point,
    };
    let initialize = emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_INITIALIZE)
        .unwrap();
    initialize.parameters = ModuleParameters::Initialize {
        lifetime: ScalarRange::new(2.0, 2.0),
        speed: ScalarRange::new(10.0, 10.0),
        direction: [1.0, 0.0, 0.0],
        spread_degrees: 0.0,
        angular_velocity: ScalarRange::new(0.0, 0.0),
    };
    let motion = emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    motion.parameters = ModuleParameters::Motion {
        gravity: [0.0; 3],
        drag: 0.0,
        turbulence: 0.0,
    };
    asset
}

#[test]
fn gravity_curve_and_random_range_control_xyz_motion_deterministically() {
    let curve_value = [2.0, -4.0, 6.0];
    let mut curve_asset = configured_motion_effect();
    let motion = curve_asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    let curve_source = InputSourceKind::Curve(InputEvaluationDomain::ParticleLife);
    motion
        .property_sources
        .insert("gravity".into(), curve_source);
    motion.property_source_values.insert(
        "gravity".into(),
        vec![PropertySourceValue::new(
            curve_source,
            Value::Vec3Curve(Vec3Curve::constant(curve_value)),
        )],
    );
    let compiled = EffectCompiler::default().compile(&curve_asset).unwrap();
    let mut curve_particles = Vec::new();
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut curve_particles);

    let mut constant_asset = configured_motion_effect();
    let motion = constant_asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    let ModuleParameters::Motion { gravity, .. } = &mut motion.parameters else {
        unreachable!()
    };
    *gravity = curve_value;
    let compiled = EffectCompiler::default().compile(&constant_asset).unwrap();
    let mut constant_particles = Vec::new();
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut constant_particles);
    assert_eq!(curve_particles, constant_particles);

    let mut random_asset = configured_motion_effect();
    let motion = random_asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    motion
        .property_sources
        .insert("gravity".into(), InputSourceKind::RandomRange);
    motion.property_source_values.insert(
        "gravity".into(),
        vec![PropertySourceValue::new(
            InputSourceKind::RandomRange,
            Value::Vec3Range(Vec3Range::new([-8.0, -6.0, -4.0], [8.0, 6.0, 4.0])),
        )],
    );
    let compiled = EffectCompiler::default().compile(&random_asset).unwrap();
    let mut first = Vec::new();
    let mut repeated = Vec::new();
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut first);
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut repeated);
    assert_eq!(first, repeated);
    assert_eq!(first.len(), 2);
    assert_ne!(first[0].position, first[1].position);
}

#[test]
fn drag_curve_and_random_range_control_particle_motion_deterministically() {
    let mut curve_asset = configured_motion_effect();
    let motion = curve_asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    let curve_source = InputSourceKind::Curve(InputEvaluationDomain::ParticleLife);
    motion.property_sources.insert("drag".into(), curve_source);
    motion.property_source_values.insert(
        "drag".into(),
        vec![PropertySourceValue::new(
            curve_source,
            Value::Curve(Curve::normalized(
                vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)],
                ScalarRange::new(0.0, 4.0),
            )),
        )],
    );
    let compiled = EffectCompiler::default().compile(&curve_asset).unwrap();
    let mut curve_particles = Vec::new();
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut curve_particles);
    assert_eq!(curve_particles.len(), 2);
    let expected_travel = 10.0 * (1.0 - (-2.0_f32).exp()) / 2.0;
    assert!((curve_particles[0].position[0] - expected_travel).abs() < 0.0001);

    let mut random_asset = configured_motion_effect();
    let motion = random_asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    motion
        .property_sources
        .insert("drag".into(), InputSourceKind::RandomRange);
    motion.property_source_values.insert(
        "drag".into(),
        vec![PropertySourceValue::new(
            InputSourceKind::RandomRange,
            Value::Range(ScalarRange::new(0.25, 4.0)),
        )],
    );
    let compiled = EffectCompiler::default().compile(&random_asset).unwrap();
    let mut first = Vec::new();
    let mut repeated = Vec::new();
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut first);
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut repeated);
    assert_eq!(first, repeated);
    assert_eq!(first.len(), 2);
    assert_ne!(first[0].position[0], first[1].position[0]);
}

#[test]
fn turbulence_curve_and_random_range_control_particle_motion_deterministically() {
    let mut curve_asset = configured_motion_effect();
    let motion = curve_asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    let curve_source = InputSourceKind::Curve(InputEvaluationDomain::ParticleLife);
    motion
        .property_sources
        .insert("turbulence".into(), curve_source);
    motion.property_source_values.insert(
        "turbulence".into(),
        vec![PropertySourceValue::new(
            curve_source,
            Value::Curve(Curve::normalized(
                vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)],
                ScalarRange::new(0.0, 4.0),
            )),
        )],
    );
    let compiled = EffectCompiler::default().compile(&curve_asset).unwrap();
    let mut curve_particles = Vec::new();
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut curve_particles);

    let mut constant_asset = configured_motion_effect();
    let motion = constant_asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    let ModuleParameters::Motion { turbulence, .. } = &mut motion.parameters else {
        unreachable!()
    };
    *turbulence = 2.0;
    let compiled = EffectCompiler::default().compile(&constant_asset).unwrap();
    let mut constant_particles = Vec::new();
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut constant_particles);
    assert_eq!(curve_particles, constant_particles);

    let mut random_asset = configured_motion_effect();
    let motion = random_asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    motion
        .property_sources
        .insert("turbulence".into(), InputSourceKind::RandomRange);
    motion.property_source_values.insert(
        "turbulence".into(),
        vec![PropertySourceValue::new(
            InputSourceKind::RandomRange,
            Value::Range(ScalarRange::new(1.0, 5.0)),
        )],
    );
    let compiled = EffectCompiler::default().compile(&random_asset).unwrap();
    let mut first = Vec::new();
    let mut repeated = Vec::new();
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut first);
    aestra_runtime::evaluate(&compiled, 1.0, 42, &mut repeated);
    assert_eq!(first, repeated);
    assert_eq!(first.len(), 2);
}

#[test]
fn compiler_uses_the_active_source_specific_range_and_curve_values() {
    let mut asset = EffectAsset::from_ron(SAMPLE).unwrap();
    let emitter = &mut asset.emitters[0];
    let initialize = emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_INITIALIZE)
        .unwrap();
    initialize.bindings.remove("lifetime");
    initialize
        .property_sources
        .insert("lifetime".into(), InputSourceKind::RandomRange);
    initialize.property_source_values.insert(
        "lifetime".into(),
        vec![PropertySourceValue::new(
            InputSourceKind::RandomRange,
            Value::Range(ScalarRange::new(9.0, 10.0)),
        )],
    );
    let appearance = emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == aestra_core::MODULE_APPEARANCE)
        .unwrap();
    appearance.bindings.remove("size");
    let curve_source = InputSourceKind::Curve(InputEvaluationDomain::ParticleLife);
    appearance
        .property_sources
        .insert("size".into(), curve_source);
    appearance.property_source_values.insert(
        "size".into(),
        vec![PropertySourceValue::new(
            curve_source,
            Value::Curve(Curve::new(vec![
                CurveKey::new(0.0, 7.0),
                CurveKey::new(1.0, 8.0),
            ])),
        )],
    );

    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let Instruction::Initialize { lifetime, .. } = compiled.emitters[0]
        .execution
        .particle_spawn
        .iter()
        .find(|instruction| matches!(instruction, Instruction::Initialize { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(lifetime, &Expression::Constant(ScalarRange::new(9.0, 10.0)));
    let Instruction::Appearance { size, .. } = compiled.emitters[0]
        .execution
        .particle_update
        .iter()
        .find(|instruction| matches!(instruction, Instruction::Appearance { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    let Expression::Constant(size) = size else {
        unreachable!()
    };
    assert_eq!(size.sample(0.0), 7.0);
    assert_eq!(size.sample(1.0), 8.0);
}

#[test]
fn compiler_resolves_texture_assets_into_renderer_plans() {
    let asset = EffectAsset::from_ron(TEXTURED_SAMPLE).unwrap();
    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let renderer = compiled.emitters[0]
        .renderers
        .iter()
        .find(|renderer| {
            compiled
                .material(renderer.material)
                .is_some_and(|material| material.texture.is_some())
        })
        .unwrap();
    let material = compiled.material(renderer.material).unwrap();
    let texture = material.texture.expect("example material must be textured");
    let registered = compiled
        .assets
        .iter()
        .find(|asset| asset.source == texture)
        .unwrap();

    assert_eq!(registered.path, "textures/ember_spark.png");
    assert_eq!(material.uv, aestra_core::UvRect::FULL);
}

#[test]
fn compiler_lowers_imported_flipbook_renderer_metadata() {
    let asset = EffectAsset::from_ron(FLIPBOOK_SAMPLE).unwrap();
    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    let renderer = &compiled.emitters[0].renderers[0];
    let RendererPlanKind::Flipbook {
        flipbook,
        time_source,
        playback,
        random_start,
    } = renderer.kind
    else {
        panic!("example must compile to a flipbook renderer");
    };
    let definition = compiled.flipbook(flipbook).unwrap();
    assert_eq!(definition.frames.len(), 4);
    assert_eq!(definition.frame_rate, 8.0);
    assert_eq!(time_source, aestra_core::FlipbookTimeSource::ParticleAge);
    assert_eq!(playback, aestra_core::FlipbookPlaybackMode::Forward);
    assert!(random_start);
    assert_eq!(compiled.assets[0].source, definition.texture);
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
        property_sources: BTreeMap::new(),
        property_source_values: BTreeMap::new(),
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
fn choreography_events_compile_and_dispatch_deterministically_across_loop_boundaries() {
    let mut asset = EffectAsset::new("Event dispatch", 2.0);
    asset.looping = true;
    asset.choreography_events = vec![
        ChoreographyEvent::new(
            "Begin",
            0.0,
            ChoreographyEventPayload::GameplayNotify {
                topic: "effect.begin".into(),
            },
        ),
        ChoreographyEvent::new(
            "Shake",
            0.5,
            ChoreographyEventPayload::CameraShake { intensity: 0.75 },
        ),
        ChoreographyEvent::new(
            "Sound",
            1.5,
            ChoreographyEventPayload::PlaySound {
                cue: "impact".into(),
            },
        ),
    ];
    let compiled = Arc::new(EffectCompiler::default().compile(&asset).unwrap());
    assert_eq!(compiled.choreography_events.len(), 3);

    let mut instance = EffectInstance::new(compiled);
    let mut dispatched = Vec::new();
    instance.advance_with_choreography_events(0.75, &mut dispatched);
    assert_eq!(
        dispatched
            .iter()
            .map(|event| event.name.as_str())
            .collect::<Vec<_>>(),
        ["Begin", "Shake"]
    );

    instance.advance_with_choreography_events(1.5, &mut dispatched);
    assert_eq!(
        dispatched
            .iter()
            .map(|event| event.name.as_str())
            .collect::<Vec<_>>(),
        ["Sound", "Begin"]
    );
    assert!((instance.time() - 0.25).abs() < 0.000_1);
    assert!(matches!(
        dispatched[1].payload,
        ChoreographyEventPayload::GameplayNotify { ref topic } if topic == "effect.begin"
    ));
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
            spawn_rate: ScalarSource::Constant(Expression::Parameter(_)),
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
    let ScalarSource::Constant(spawn_rate) = spawn_rate else {
        panic!("spawn rate should remain constant");
    };
    assert_eq!(spawn_rate.constant_value(), Some(&4.0));
}

#[test]
fn material_inputs_bind_to_runtime_parameters() {
    let mut asset = EffectAsset::new("Parameterized Material", 1.0);
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: "Edge Softness".into(),
        default: Value::Scalar(0.25),
        exposed: true,
    };
    let parameter_id = parameter.id;
    let MaterialProperties::Sprite { softness, .. } = &mut asset.materials[0].properties;
    *softness = MaterialInput::Parameter(parameter_id);
    asset.parameters.push(parameter);
    asset.emitters.push(Emitter::basic_sprite("Emitter", 1.0));

    let compiled = Arc::new(EffectCompiler::default().compile(&asset).unwrap());
    let material_id = asset.materials[0].id;
    assert!(matches!(
        compiled.material(material_id).unwrap().softness,
        Expression::Parameter(_)
    ));
    assert_eq!(compiled.optimizations.runtime_parameter_reads, 1);

    let mut instance = EffectInstance::new(compiled);
    assert_eq!(
        *instance
            .effect()
            .material(material_id)
            .unwrap()
            .softness
            .resolve(instance.parameter_values()),
        0.25
    );
    instance
        .set_parameter(parameter_id, Value::Scalar(0.8))
        .unwrap();
    assert_eq!(
        *instance
            .effect()
            .material(material_id)
            .unwrap()
            .softness
            .resolve(instance.parameter_values()),
        0.8
    );
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

#[test]
fn project_compilation_resolves_and_executes_timed_child_effects() {
    let temporary = tempfile::tempdir().unwrap();
    let mut child = EffectAsset::new("Child", 1.0);
    child
        .emitters
        .push(Emitter::basic_sprite("Child emitter", 1.0));
    child
        .save_ron(temporary.path().join("child.aestra.ron"))
        .unwrap();

    let mut root = EffectAsset::new("Root", 2.0);
    let mut clip = EffectClip::new(child.id, 0.5, 1.0);
    clip.source_offset = 0.1;
    clip.seed = EffectClipSeed::Fixed(77);
    let clip_id = clip.id;
    root.effect_clips.push(clip);

    let project = EffectCompiler::default()
        .compile_project(&root, &ProjectAssetIndex::scan(temporary.path()))
        .unwrap();
    assert_eq!(project.dependencies.len(), 1);
    assert_eq!(project.root.effect_clips[0].source.id, child.id);

    let mut before = Vec::new();
    project.evaluate(0.25, 1, &mut before);
    assert!(before.is_empty());

    let mut first = Vec::new();
    let mut second = Vec::new();
    project.evaluate(0.75, 1, &mut first);
    project.evaluate(0.75, 999, &mut second);
    assert!(!first.is_empty());
    assert_eq!(first, second);
    assert!(first.iter().all(|sample| {
        sample.effect == child.id && sample.instance_path.as_slice() == [clip_id]
    }));
}

#[test]
fn project_compilation_applies_exposed_clip_parameter_overrides() {
    let temporary = tempfile::tempdir().unwrap();
    let (child, parameter) = parameterized_effect(true);
    child
        .save_ron(temporary.path().join("child.aestra.ron"))
        .unwrap();

    let mut root = EffectAsset::new("Root", 2.0);
    let mut clip = EffectClip::new(child.id, 0.0, 2.0);
    clip.parameter_overrides
        .insert(parameter, Value::Scalar(20.0));
    root.effect_clips.push(clip);

    let project = EffectCompiler::default()
        .compile_project(&root, &ProjectAssetIndex::scan(temporary.path()))
        .unwrap();
    let compiled_override = &project.root.effect_clips[0].parameter_overrides[0];
    assert_eq!(compiled_override.source, parameter);

    let mut samples = Vec::new();
    project.evaluate(0.5, 42, &mut samples);
    assert_eq!(samples.len(), 10);
    assert!(samples.iter().all(|sample| sample.effect == child.id));
}

#[test]
fn project_compilation_diagnoses_orphaned_and_type_changed_clip_overrides() {
    let temporary = tempfile::tempdir().unwrap();
    let (child, parameter) = parameterized_effect(true);
    child
        .save_ron(temporary.path().join("child.aestra.ron"))
        .unwrap();
    let index = ProjectAssetIndex::scan(temporary.path());

    let mut root = EffectAsset::new("Root", 2.0);
    let mut clip = EffectClip::new(child.id, 0.0, 2.0);
    let missing = ParameterId::new();
    clip.parameter_overrides
        .insert(missing, Value::Scalar(10.0));
    root.effect_clips.push(clip);

    let ProjectCompileError::Effect { source, .. } = EffectCompiler::default()
        .compile_project(&root, &index)
        .unwrap_err()
    else {
        panic!("orphaned overrides must be compiler diagnostics");
    };
    assert!(source.report().diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::UnknownParameter
            && diagnostic.path.contains(&missing.to_string())
    }));

    root.effect_clips[0].parameter_overrides.clear();
    root.effect_clips[0]
        .parameter_overrides
        .insert(parameter, Value::Vec2([1.0, 2.0]));
    let ProjectCompileError::Effect { source, .. } = EffectCompiler::default()
        .compile_project(&root, &index)
        .unwrap_err()
    else {
        panic!("type-changed overrides must be compiler diagnostics");
    };
    assert!(source.report().diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ParameterTypeMismatch
            && diagnostic.path.contains(&parameter.to_string())
    }));
}
