//! Isolated visual A/B: continuous cooling filaments versus bright sprite clusters.
use aestra_core::material::MaterialInput;
use aestra_core::{material::*, *};
use std::path::{Path, PathBuf};
const BASE: u128 = 0xa3574a00_0000_4000_8000_000000fb0200;

fn material() -> Result<(MaterialFunction, MaterialProgram), String> {
    let input = |n| MaterialFunctionInputId::from_u128(BASE + n);
    let expression = |n| MaterialExpressionId::from_u128(BASE + 0x100 + n);
    let output = MaterialFunctionOutputId::from_u128(BASE + 1);
    let specs = [
        ("World position", MaterialValueType::Vec3),
        ("Strand seed", MaterialValueType::Float),
        ("Normalized age", MaterialValueType::Float),
        ("Grain amount", MaterialValueType::Float),
        ("Spatial frequency", MaterialValueType::Float),
        ("Color cooling", MaterialValueType::Float),
    ];
    let function = MaterialFunction {
        id: MaterialFunctionId::from_u128(BASE),
        schema_version: MaterialSchemaVersion::default(),
        name: "Emission Grain".into(),
        inputs: specs
            .into_iter()
            .enumerate()
            .map(|(i, (name, value_type))| MaterialFunctionInput {
                id: input(0x10 + i as u128),
                name: name.into(),
                value_type,
                default: None,
            })
            .collect(),
        outputs: vec![MaterialFunctionOutput {
            id: output,
            name: "Radiance modulation".into(),
            value_type: MaterialValueType::Color,
            expression: expression(0),
        }],
        expressions: vec![],
        custom_wesl: Some(MaterialCustomWeslImplementation {
            evaluation_domain: MaterialExpressionDomain::Fragment,
            source: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../crates/aestra-gpu/src/shaders/emission_grain.wesl"
            ))
            .into(),
            entry_points: [(output, "emission_grain".into())].into(),
        }),
    };
    let mut program = MaterialProgram::from_ron(
        &std::fs::read_to_string(root().join("materials/fireworks_hero_trail.aestra.material.ron"))
            .map_err(|e| e.to_string())?
            .replace("0000000f74", "000000fb05"),
    )
    .map_err(|e| e.to_string())?;
    program.name = "Grained Cooling Filament".into();
    for (i, value) in [
        MaterialInput::WorldPosition,
        MaterialInput::ParticleRandom,
        MaterialInput::ParticleNormalizedAge,
    ]
    .into_iter()
    .enumerate()
    {
        program.expressions.push(MaterialExpression {
            id: expression(i as u128),
            kind: MaterialExpressionKind::Input(value),
        });
    }
    for (i, (name, default)) in [
        ("Grain amount", 0.85),
        ("Spatial frequency", 1.8),
        ("Color cooling", 0.55),
    ]
    .into_iter()
    .enumerate()
    {
        let parameter = MaterialParameterId::from_u128(BASE + 0x20 + i as u128);
        program.parameters.push(MaterialParameter {
            id: parameter,
            name: name.into(),
            value_type: MaterialValueType::Float,
            evaluation_domain: MaterialEvaluationDomain::Effect,
            default: Some(MaterialValue::Float(default)),
        });
        program.expressions.push(MaterialExpression {
            id: expression(3 + i as u128),
            kind: MaterialExpressionKind::Parameter(parameter),
        });
    }
    program.expressions.extend([
        MaterialExpression {
            id: expression(6),
            kind: MaterialExpressionKind::FunctionCall {
                function: MaterialFunctionRef::Project(function.id),
                arguments: (0..6).map(|i| (input(0x10 + i), expression(i))).collect(),
                output,
            },
        },
        MaterialExpression {
            id: expression(7),
            kind: MaterialExpressionKind::Multiply(program.outputs.color, expression(6)),
        },
    ]);
    program.outputs.color = expression(7);
    Ok((function, program))
}
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test")
}
fn author() -> Result<[EffectAsset; 2], String> {
    let mut control = EffectAsset::from_ron(
        &std::fs::read_to_string(
            root().join("effects/fireworks_gold_tail_reference_smoke.aestra.ron"),
        )
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    control.id = EffectId::from_u128(0xa3574a00_0000_4000_8000_000000fb0000);
    control.name = "Gold filament study — control".into();
    let mut candidate = control.clone();
    candidate.id = EffectId::from_u128(0xa3574a00_0000_4000_8000_000000fb0100);
    candidate.name = "Gold filament study — connected cooling spokes".into();
    for p in &mut candidate.parameters {
        match p.name.as_str() {
            "Gold head radiance" => p.default = Value::Scalar(18.0),
            "Gold cooling tail radiance" => p.default = Value::Scalar(52.0),
            "Gold ember radiance" => p.default = Value::Scalar(3.0),
            _ => {}
        }
    }
    for emitter in &mut candidate.emitters {
        if emitter.name == "Main stars" {
            for module in &mut emitter.modules {
                if let ModuleParameters::Appearance { size, opacity, .. } = &mut module.parameters {
                    size.keys = vec![
                        CurveKey::new(0.0, 0.22),
                        CurveKey::new(0.4, 0.16),
                        CurveKey::new(1.0, 0.035),
                    ];
                    opacity.keys = vec![
                        CurveKey::new(0.0, 1.0),
                        CurveKey::new(0.25, 0.9),
                        CurveKey::new(0.6, 0.38),
                        CurveKey::new(0.85, 0.08),
                        CurveKey::new(1.0, 0.0),
                    ];
                }
            }
            for renderer in &mut emitter.renderers {
                if let RendererProperties::Trail {
                    width,
                    lifetime,
                    max_points,
                    sample_interval,
                    ..
                } = &mut renderer.properties
                {
                    // Width is also multiplied by recorded particle size. Keep
                    // the effective strip narrow, but give its grain enough
                    // pixel energy to read over the residual ember sprites.
                    *width = 0.24;
                    *lifetime = 2.4;
                    *max_points = 64;
                    *sample_interval = 1.0 / 24.0;
                }
            }
        }
        if emitter.name == "Gold tail embers" {
            for module in &mut emitter.modules {
                match &mut module.parameters {
                    ModuleParameters::Initialize { speed, .. } => {
                        *speed = ScalarRange::new(0.15, 0.8)
                    }
                    ModuleParameters::Motion { turbulence, .. } => *turbulence = 0.08,
                    ModuleParameters::Appearance { size, .. } => {
                        size.keys = vec![
                            CurveKey::new(0.0, 0.18),
                            CurveKey::new(0.6, 0.09),
                            CurveKey::new(1.0, 0.02),
                        ]
                    }
                    _ => {}
                }
            }
        }
    }
    let (_, program) = material()?;
    let trail = candidate
        .emitters
        .iter_mut()
        .find(|e| e.name == "Main stars")
        .unwrap()
        .renderers
        .iter_mut()
        .find(|r| matches!(r.properties, RendererProperties::Trail { .. }))
        .unwrap();
    let mut instance = candidate
        .material_instances
        .iter()
        .find(|m| m.id == trail.material)
        .unwrap()
        .clone();
    instance.id = MaterialId::from_u128(BASE + 0x200);
    instance.program = MaterialProgramRef::Project(program.id);
    let radiance = instance
        .values
        .remove(&MaterialParameterId::from_u128(
            0xa3574a00_0000_4000_8000_0000000f74a6,
        ))
        .ok_or("Trail radiance missing")?;
    instance.values.insert(
        MaterialParameterId::from_u128(0xa3574a00_0000_4000_8000_000000fb05a6),
        radiance,
    );
    for (i, (name, value)) in [
        ("Tail burn grain", 0.85),
        ("Tail grain frequency", 1.8),
        ("Tail color cooling", 0.55),
    ]
    .into_iter()
    .enumerate()
    {
        let id = ParameterId::from_u128(BASE + 0x210 + i as u128);
        candidate.parameters.push(EffectParameter {
            id,
            name: name.into(),
            default: Value::Scalar(value),
            exposed: true,
        });
        instance.values.insert(
            MaterialParameterId::from_u128(BASE + 0x20 + i as u128),
            MaterialParameterValue::EffectParameter(id),
        );
    }
    trail.material = instance.id;
    candidate.material_instances.push(instance);
    candidate.metadata.insert("notes".into(),"Single-shell reference study: connected histories with stationary, independently seeded emission grain, smaller heads and fine residual embers. Tail burn grain / frequency / color cooling are exposed editor controls. Recorded sample age drives color cooling; no extra clock or particle births. Parent trajectories, event demand, smoke and lights retain the control. Visual approval and overlap costs remain open.".into());
    Ok([control, candidate])
}
fn main() -> Result<(), String> {
    let (function, program) = material()?;
    std::fs::write(
        root().join("materials/emission_grain.aestra.material-function.ron"),
        function.to_pretty_ron().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        root().join("materials/grained_filament.aestra.material.ron"),
        program.to_pretty_ron().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let [control, candidate] = author()?;
    for (name, asset) in [
        ("control", control),
        ("uniform", uniform(&candidate)),
        ("candidate", candidate),
    ] {
        std::fs::write(
            root().join(format!("effects/fireworks_gold_filament_{name}.aestra.ron")),
            asset.to_pretty_ron().map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn uniform(candidate: &EffectAsset) -> EffectAsset {
    let mut asset = candidate.clone();
    asset.id = EffectId::from_u128(BASE + 0x400);
    asset.name = "Gold filament study — uniform control".into();
    for parameter in &mut asset.parameters {
        if matches!(
            parameter.name.as_str(),
            "Tail burn grain" | "Tail color cooling"
        ) {
            parameter.default = Value::Scalar(0.0);
        }
    }
    asset
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filament_study_keeps_event_demand_and_compiles_editable_trails_at_all_tiers() {
        let [control, candidate] = author().unwrap();
        assert_eq!(control.events, candidate.events);
        assert_eq!(control.particle_budgets, candidate.particle_budgets);
        assert_eq!(control.particle_outputs, candidate.particle_outputs);
        assert_eq!(control.point_lights, candidate.point_lights);
        let (function, program) = material().unwrap();
        assert_eq!(
            MaterialFunction::load_ron(
                root().join("materials/emission_grain.aestra.material-function.ron")
            )
            .unwrap(),
            function.normalized()
        );
        assert_eq!(
            MaterialProgram::load_ron(
                root().join("materials/grained_filament.aestra.material.ron")
            )
            .unwrap(),
            program.normalized()
        );
        let uniform = uniform(&candidate);
        for (name, asset) in [
            ("control", &control),
            ("uniform", &uniform),
            ("candidate", &candidate),
        ] {
            let saved = EffectAsset::from_ron(
                &std::fs::read_to_string(
                    root().join(format!("effects/fireworks_gold_filament_{name}.aestra.ron")),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(&saved, asset, "regenerate {name}");
        }
        for (a, b) in control.emitters.iter().zip(&candidate.emitters) {
            assert_eq!(a.max_particles, b.max_particles);
            if a.name == "Main stars" {
                for (a, b) in a.modules.iter().zip(&b.modules) {
                    if !matches!(a.parameters, ModuleParameters::Appearance { .. }) {
                        assert_eq!(a, b);
                    }
                }
            }
        }
        let index = aestra_project::ProjectAssetIndex::scan(root());
        for tier in ["high", "medium", "low"] {
            let resolved = index.resolve_effect_project(&candidate).unwrap();
            aestra_compiler::EffectCompiler::default()
                .with_tier(aestra_runtime::QualityTier::preset(tier).unwrap())
                .compile_resolved_project(&resolved)
                .unwrap();
        }
    }
}
