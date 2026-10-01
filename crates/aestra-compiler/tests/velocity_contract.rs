use aestra_compiler::EffectCompiler;
use aestra_core::{
    EffectAsset, Emitter, EmitterShape, ModuleInstance, ModuleParameters, ScalarRange,
    VelocityDistribution,
};
use aestra_runtime::{EffectInstance, Instruction};
use std::sync::Arc;

fn shell(mode: VelocityDistribution, shape: EmitterShape) -> EffectAsset {
    let mut effect = EffectAsset::new("Generic shell", 2.0);
    let mut emitter = Emitter::basic_sprite("Shell", 2.0);
    emitter.modules[0] = ModuleInstance::emission(0.0, 128);
    emitter.modules[1] = ModuleInstance::shape(shape);
    emitter.modules[2] = ModuleInstance::initialize_with_distribution(
        ScalarRange::new(2.0, 2.0),
        ScalarRange::new(18.0, 22.0),
        mode,
        [0.0, 1.0, 0.0],
        40.0,
        ScalarRange::new(0.0, 0.0),
    );
    emitter.modules[3] = ModuleInstance::motion([0.0; 3], 0.0, 0.0);
    effect.emitters.push(emitter);
    effect
}

#[test]
fn generic_shells_lower_and_sample_independent_position_direction_and_speed() {
    for mode in VelocityDistribution::ALL {
        let effect = shell(mode, EmitterShape::Point);
        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        assert!(compiled.emitters[0].execution.particle_spawn.iter().any(|instruction|
            matches!(instruction, Instruction::Initialize { velocity_distribution, .. } if *velocity_distribution == mode)));
        let mut instance = EffectInstance::with_seed(compiled.clone(), 42);
        instance.advance(0.5);
        let mut samples = Vec::new();
        instance.evaluate(&mut samples);
        assert_eq!(samples.len(), 128);
        for sample in &samples {
            let radius = sample.position.iter().map(|x| x * x).sum::<f32>().sqrt();
            if mode == VelocityDistribution::Disk {
                assert!(radius <= 11.001);
            } else {
                assert!((8.999..=11.001).contains(&radius), "{mode:?}: {radius}");
            }
            if matches!(
                mode,
                VelocityDistribution::Ring | VelocityDistribution::Disk
            ) {
                assert!(sample.position[1].abs() < 1e-6);
            }
            if mode == VelocityDistribution::Hemisphere {
                assert!(sample.position[1] >= 0.0);
            }
        }
        let mut replay = EffectInstance::with_seed(compiled.clone(), 42);
        replay.advance(0.5);
        let mut again = Vec::new();
        replay.evaluate(&mut again);
        assert_eq!(samples, again, "stable seed and particle identity");
        let mut different_seed = EffectInstance::with_seed(compiled, 43);
        different_seed.advance(0.5);
        different_seed.evaluate(&mut again);
        assert_ne!(samples, again, "instance seed must affect samples");

        let mut volume = effect.clone();
        if let ModuleParameters::Shape { shape } = &mut volume.emitters[0].modules[1].parameters {
            *shape = EmitterShape::Sphere { radius: 3.0 };
        }
        let mut from_volume = EffectInstance::with_seed(
            Arc::new(EffectCompiler::default().compile(&volume).unwrap()),
            42,
        );
        // At birth, isolate position samples. The direction/speed stream must
        // remain identical after changing only the position distribution.
        let mut origins = Vec::new();
        from_volume.evaluate(&mut origins);
        from_volume.advance(0.5);
        from_volume.evaluate(&mut again);
        for ((point, volume), origin) in samples.iter().zip(&again).zip(&origins) {
            assert_eq!(point.particle_index, volume.particle_index);
            for axis in 0..3 {
                assert!(
                    (point.position[axis] - (volume.position[axis] - origin.position[axis])).abs()
                        < 1e-5
                );
            }
        }
    }
}

#[test]
fn velocity_mode_is_not_silently_ignored_when_bound_as_an_effect_parameter() {
    let mut effect = shell(VelocityDistribution::Ring, EmitterShape::Point);
    let parameter = aestra_core::EffectParameter {
        id: aestra_core::ParameterId::new(),
        name: "Mode".into(),
        default: aestra_core::Value::Text("Sphere".into()),
        exposed: true,
    };
    effect.emitters[0].modules[2]
        .bindings
        .insert("velocity_distribution".into(), parameter.id);
    effect.parameters.push(parameter);
    assert!(EffectCompiler::default().compile(&effect).is_err());
}
