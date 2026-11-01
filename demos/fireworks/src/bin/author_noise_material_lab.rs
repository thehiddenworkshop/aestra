//! Reproducible opt-in material study: ordinary graph nodes, including nested built-in noise.
use aestra_core::material::MaterialInput;
use aestra_core::{
    material::*,
    noise::{self, NoiseBasis, NoiseFunction},
    *,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
const BASE: u128 = 0xa3574a00_0000_4000_8000_00000000e000;
fn parameter(i: u128) -> MaterialParameterId {
    MaterialParameterId::from_u128(BASE + 0x100 + i)
}
fn input(i: u128) -> MaterialFunctionInputId {
    MaterialFunctionInputId::from_u128(BASE + 0x200 + i)
}
fn function_id() -> MaterialFunctionId {
    MaterialFunctionId::from_u128(BASE)
}
fn output() -> MaterialFunctionOutputId {
    MaterialFunctionOutputId::from_u128(BASE + 1)
}
fn control_type(value: &MaterialValue) -> MaterialValueType {
    match value {
        MaterialValue::Vec3(_) => MaterialValueType::Vec3,
        MaterialValue::Float(_) => MaterialValueType::Float,
        _ => unreachable!(),
    }
}
#[derive(Default)]
struct Graph(Vec<MaterialExpression>);
impl Graph {
    fn add(&mut self, kind: MaterialExpressionKind) -> MaterialExpressionId {
        let id = MaterialExpressionId::from_u128(BASE + 0x300 + self.0.len() as u128);
        self.0.push(MaterialExpression { id, kind });
        id
    }
    fn scalar(&mut self, v: f32) -> MaterialExpressionId {
        self.add(MaterialExpressionKind::Constant(MaterialValue::Float(v)))
    }
}
fn controls() -> Vec<(&'static str, MaterialValue)> {
    vec![
        ("Scale", MaterialValue::Float(2.2)),
        ("Seed", MaterialValue::Float(73.0)),
        ("Octaves", MaterialValue::Float(4.0)),
        ("Flow", MaterialValue::Vec3([0.6, 0.12, 0.08])),
        ("Footprint", MaterialValue::Float(0.0)),
        ("Density", MaterialValue::Float(0.85)),
        ("Ambient", MaterialValue::Vec3([0.65, 0.7, 0.75])),
    ]
}
fn material() -> (MaterialProgram, MaterialFunction) {
    use MaterialExpressionKind as E;
    let mut g = Graph::default();
    let uv = g.add(E::FunctionInput(input(0)));
    let world = g.add(E::FunctionInput(input(1)));
    let time = g.add(E::FunctionInput(input(2)));
    let scale = g.add(E::FunctionInput(input(3)));
    let seed = g.add(E::FunctionInput(input(4)));
    let octaves = g.add(E::FunctionInput(input(5)));
    let flow = g.add(E::FunctionInput(input(6)));
    let footprint = g.add(E::FunctionInput(input(7)));
    let travel = g.add(E::Multiply(flow, time));
    let position = g.add(E::Subtract(world, travel));
    let field = g.add(E::FunctionCall {
        function: MaterialFunctionRef::BuiltIn(
            NoiseFunction {
                basis: NoiseBasis::Value,
                dimensions: 3,
                fractal: true,
            }
            .id(),
        ),
        arguments: BTreeMap::from([
            (noise::input(0), position),
            (noise::input(1), scale),
            (noise::input(4), seed),
            (noise::input(5), octaves),
            (noise::input(8), footprint),
        ]),
        output: noise::OUTPUT,
    });
    let lower = g.scalar(0.24);
    let upper = g.scalar(0.76);
    let density = g.add(E::Smoothstep {
        edge_min: lower,
        edge_max: upper,
        value: field,
    });
    let center = g.add(E::Constant(MaterialValue::Vec2([0.5; 2])));
    let radius = g.scalar(0.48);
    let softness = g.scalar(0.8);
    let invert = g.add(E::Constant(MaterialValue::Bool(false)));
    let radial = g.add(E::RadialMask {
        uv,
        center,
        radius,
        softness,
        invert,
    });
    // A separate UV field breaks up billboard edges while world noise remains coherent.
    let uv_scale = g.scalar(4.5);
    let uv_field = g.add(E::FunctionCall {
        function: MaterialFunctionRef::BuiltIn(
            NoiseFunction {
                basis: NoiseBasis::Gradient,
                dimensions: 2,
                fractal: true,
            }
            .id(),
        ),
        arguments: BTreeMap::from([
            (noise::POSITION, uv),
            (noise::input(1), uv_scale),
            (noise::input(4), seed),
        ]),
        output: noise::OUTPUT,
    });
    let a = g.scalar(0.4);
    let b = g.scalar(1.25);
    let breakup = g.add(E::Lerp {
        start: a,
        end: b,
        factor: uv_field,
    });
    let mask = g.add(E::Multiply(radial, breakup));
    let alpha = g.add(E::Multiply(mask, density));
    let mut inputs = vec![
        MaterialFunctionInput {
            id: input(0),
            name: "UV".into(),
            value_type: MaterialValueType::Vec2,
            default: None,
        },
        MaterialFunctionInput {
            id: input(1),
            name: "World position".into(),
            value_type: MaterialValueType::Vec3,
            default: None,
        },
        MaterialFunctionInput {
            id: input(2),
            name: "Effect time".into(),
            value_type: MaterialValueType::Float,
            default: None,
        },
    ];
    inputs.extend(
        controls()
            .into_iter()
            .take(5)
            .enumerate()
            .map(|(i, (name, value))| MaterialFunctionInput {
                id: input(i as u128 + 3),
                name: name.into(),
                value_type: control_type(&value),
                default: Some(value),
            }),
    );
    let function = MaterialFunction {
        id: function_id(),
        schema_version: MaterialSchemaVersion::CURRENT,
        name: "Noise Smoke Density".into(),
        inputs,
        outputs: vec![MaterialFunctionOutput {
            id: output(),
            name: "Density".into(),
            value_type: MaterialValueType::Float,
            expression: alpha,
        }],
        expressions: g.0,
        custom_wesl: None,
    };
    let mut p = MaterialProgram::additive_sprite("Noise Smoke");
    p.id = MaterialProgramId::from_u128(BASE + 2);
    let state = MaterialRenderState {
        blend: BlendMode::Alpha,
        depth_test: MaterialDepthTest::LessEqual,
        depth_write: false,
        cull_mode: MaterialCullMode::None,
    };
    p.render_state_policy = MaterialRenderStatePolicy::fixed(state);
    p.parameters = controls()
        .into_iter()
        .enumerate()
        .map(|(i, (name, value))| MaterialParameter {
            id: parameter(i as u128),
            name: name.into(),
            value_type: control_type(&value),
            evaluation_domain: MaterialEvaluationDomain::Effect,
            default: Some(value),
        })
        .collect();
    let mut g = Graph::default();
    let uv = g.add(E::Input(MaterialInput::Uv0));
    let world = g.add(E::Input(MaterialInput::WorldPosition));
    let time = g.add(E::Input(MaterialInput::EffectTime));
    let mut arguments = BTreeMap::from([(input(0), uv), (input(1), world), (input(2), time)]);
    for i in 0..5 {
        let e = g.add(E::Parameter(parameter(i)));
        arguments.insert(input(i + 3), e);
    }
    let field = g.add(E::FunctionCall {
        function: MaterialFunctionRef::Project(function.id),
        arguments,
        output: output(),
    });
    let opacity = g.add(E::Input(MaterialInput::ParticleOpacity));
    let coverage = g.add(E::Multiply(field, opacity));
    let density = g.add(E::Parameter(parameter(5)));
    p.outputs.alpha = g.add(E::Multiply(coverage, density));
    let light = g.add(E::Input(MaterialInput::ScenePointIrradiance));
    let gain = g.scalar(0.035);
    let direct = g.add(E::Multiply(light, gain));
    let ambient = g.add(E::Parameter(parameter(6)));
    p.outputs.color = g.add(E::Add(direct, ambient));
    p.expressions = g.0;
    (p.normalized(), function.normalized())
}
fn sample_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-project")
}
fn test_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test")
}
fn lab(program: &MaterialProgram) -> Result<EffectAsset, String> {
    let mut effect =
        EffectAsset::load_ron(sample_root().join("effects/smoke_material_lab.aestra.ron"))
            .map_err(|e| e.to_string())?;
    effect.id = EffectId::from_u128(BASE + 3);
    effect.name = "Noise Smoke Lab".into();
    effect.parameters.clear();
    for instance in &mut effect.material_instances {
        instance.program = MaterialProgramRef::Project(program.id);
        instance.values.clear();
        for (i, (name, value)) in controls().into_iter().enumerate() {
            let id = ParameterId::from_u128(BASE + 0x500 + i as u128);
            let default = match value {
                MaterialValue::Float(v) => Value::Scalar(v),
                MaterialValue::Vec3(v) => Value::Vec3(v),
                _ => unreachable!(),
            };
            effect.parameters.push(EffectParameter {
                id,
                name: format!("Noise {name}"),
                default,
                exposed: true,
            });
            instance.values.insert(
                parameter(i as u128),
                MaterialParameterValue::EffectParameter(id),
            );
        }
    }
    Ok(effect)
}
fn shell(program: &MaterialProgram) -> Result<EffectAsset, String> {
    let mut effect = EffectAsset::load_ron(
        test_root().join("effects/fireworks_gold_tail_reference_smoke.aestra.ron"),
    )
    .map_err(|e| e.to_string())?;
    effect.id = EffectId::from_u128(BASE + 4);
    effect.name = "Fireworks — Shared Noise Smoke Study".into();
    for instance in &mut effect.material_instances {
        if instance.program
            == MaterialProgramRef::Project(MaterialProgramId::from_u128(
                0xa3574a00_0000_4000_8000_000000f8d700,
            ))
        {
            instance.program = MaterialProgramRef::Project(program.id);
            instance.values.clear();
            // Night-time world scale/ambient differ from the unit-size editor swatch.
            instance.values.insert(
                parameter(0),
                MaterialParameterValue::Constant(MaterialValue::Float(0.22)),
            );
            instance.values.insert(
                parameter(6),
                MaterialParameterValue::Constant(MaterialValue::Vec3([0.25, 0.28, 0.32])),
            );
        }
    }
    Ok(effect)
}
fn main() -> Result<(), String> {
    let (program, function) = material();
    let lab = lab(&program)?;
    let shell = shell(&program)?;
    for root in [sample_root(), test_root()] {
        std::fs::write(
            root.join("materials/noise_smoke.aestra.material.ron"),
            program.to_pretty_ron().map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::write(
            root.join("materials/noise_smoke_density.aestra.material-function.ron"),
            function.to_pretty_ron().map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    std::fs::write(
        sample_root().join("effects/noise_smoke_lab.aestra.ron"),
        lab.to_pretty_ron().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        test_root().join("effects/fireworks_noise_smoke.aestra.ron"),
        shell.to_pretty_ron().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_noise_studies_preserve_particle_workload_and_compile_at_every_tier() {
        let (program, function) = material();
        assert_eq!(
            MaterialProgram::load_ron(
                sample_root().join("materials/noise_smoke.aestra.material.ron")
            )
            .unwrap(),
            program
        );
        assert_eq!(
            MaterialFunction::load_ron(
                sample_root().join("materials/noise_smoke_density.aestra.material-function.ron")
            )
            .unwrap(),
            function
        );
        let shell = shell(&program).unwrap();
        assert_eq!(
            EffectAsset::load_ron(test_root().join("effects/fireworks_noise_smoke.aestra.ron"))
                .unwrap(),
            shell
        );
        assert_eq!(
            EffectAsset::load_ron(sample_root().join("effects/noise_smoke_lab.aestra.ron"))
                .unwrap(),
            lab(&program).unwrap()
        );
        let original = EffectAsset::load_ron(
            test_root().join("effects/fireworks_gold_tail_reference_smoke.aestra.ron"),
        )
        .unwrap();
        assert_eq!(original.emitters, shell.emitters);
        assert_eq!(original.events, shell.events);
        assert_eq!(original.point_lights, shell.point_lights);
        assert_eq!(original.particle_outputs, shell.particle_outputs);
        for (root, effect) in [
            (sample_root(), lab(&program).unwrap()),
            (test_root(), shell),
        ] {
            let project = aestra_project::ProjectAssetIndex::scan(root)
                .resolve_effect_project(&effect)
                .unwrap();
            for tier in ["high", "medium", "low"] {
                aestra_compiler::EffectCompiler::default()
                    .with_tier(aestra_runtime::QualityTier::preset(tier).unwrap())
                    .compile_resolved_project(&project)
                    .unwrap();
            }
        }
    }
}
