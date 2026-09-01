use aestra_compiler::{
    MaterialCompileError, MaterialCompiler, MaterialIrConstant, MaterialIrInstruction,
    MaterialIrValueId,
};
use aestra_core::{
    AssetId, DiagnosticCode, MaterialExpressionId, MaterialParameterId, MaterialProgramId,
    material::{
        MaterialAddressMode, MaterialEvaluationDomain, MaterialExpression, MaterialExpressionKind,
        MaterialFilterMode, MaterialInput, MaterialMipFilterMode, MaterialParameter,
        MaterialProgram, MaterialSamplerDescriptor, MaterialTextureColorSpace,
        MaterialTextureDescriptor, MaterialValue, MaterialValueType,
    },
};

fn texture_descriptor(color_space: MaterialTextureColorSpace) -> MaterialTextureDescriptor {
    MaterialTextureDescriptor {
        color_space,
        sampler: MaterialSamplerDescriptor {
            filter: MaterialFilterMode::Linear,
            mip_filter: MaterialMipFilterMode::Linear,
            address_u: MaterialAddressMode::Repeat,
            address_v: MaterialAddressMode::Repeat,
        },
    }
}

fn two_texture_flame_program() -> MaterialProgram {
    let main_parameter = MaterialParameterId::from_u128(0x1001);
    let noise_parameter = MaterialParameterId::from_u128(0x1002);
    let tint_parameter = MaterialParameterId::from_u128(0x1003);
    let intensity_parameter = MaterialParameterId::from_u128(0x1004);
    let main = MaterialExpressionId::from_u128(0x2001);
    let noise = MaterialExpressionId::from_u128(0x2002);
    let tint = MaterialExpressionId::from_u128(0x2003);
    let intensity = MaterialExpressionId::from_u128(0x2004);
    let uv = MaterialExpressionId::from_u128(0x2005);
    let main_sample = MaterialExpressionId::from_u128(0x2006);
    let noise_sample = MaterialExpressionId::from_u128(0x2007);
    let combined = MaterialExpressionId::from_u128(0x2008);
    let tinted = MaterialExpressionId::from_u128(0x2009);
    let bright = MaterialExpressionId::from_u128(0x200A);
    let particle_color = MaterialExpressionId::from_u128(0x200B);
    let color = MaterialExpressionId::from_u128(0x200C);
    let opacity = MaterialExpressionId::from_u128(0x200D);
    let alpha = MaterialExpressionId::from_u128(0x200E);
    let mut program = MaterialProgram::additive_sprite("Two Texture Flame");
    program.id = MaterialProgramId::from_u128(0x1000);
    program.parameters = vec![
        MaterialParameter {
            id: main_parameter,
            name: "main_texture".into(),
            value_type: MaterialValueType::Texture2D(texture_descriptor(
                MaterialTextureColorSpace::SrgbColor,
            )),
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x3001))),
        },
        MaterialParameter {
            id: noise_parameter,
            name: "noise_texture".into(),
            value_type: MaterialValueType::Texture2D(texture_descriptor(
                MaterialTextureColorSpace::LinearData,
            )),
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x3002))),
        },
        MaterialParameter {
            id: tint_parameter,
            name: "tint".into(),
            value_type: MaterialValueType::Color,
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::ColorSrgb([1.0, 0.5, 0.25, 1.0])),
        },
        MaterialParameter {
            id: intensity_parameter,
            name: "intensity".into(),
            value_type: MaterialValueType::Float,
            evaluation_domain: MaterialEvaluationDomain::Effect,
            default: Some(MaterialValue::Float(2.0)),
        },
    ];
    program.expressions = vec![
        MaterialExpression {
            id: main,
            kind: MaterialExpressionKind::Parameter(main_parameter),
        },
        MaterialExpression {
            id: noise,
            kind: MaterialExpressionKind::Parameter(noise_parameter),
        },
        MaterialExpression {
            id: tint,
            kind: MaterialExpressionKind::Parameter(tint_parameter),
        },
        MaterialExpression {
            id: intensity,
            kind: MaterialExpressionKind::Parameter(intensity_parameter),
        },
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: main_sample,
            kind: MaterialExpressionKind::SampleTexture { texture: main, uv },
        },
        MaterialExpression {
            id: noise_sample,
            kind: MaterialExpressionKind::SampleTexture { texture: noise, uv },
        },
        MaterialExpression {
            id: combined,
            kind: MaterialExpressionKind::Multiply(main_sample, noise_sample),
        },
        MaterialExpression {
            id: tinted,
            kind: MaterialExpressionKind::Multiply(combined, tint),
        },
        MaterialExpression {
            id: bright,
            kind: MaterialExpressionKind::Multiply(tinted, intensity),
        },
        MaterialExpression {
            id: particle_color,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleColor),
        },
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Multiply(bright, particle_color),
        },
        MaterialExpression {
            id: opacity,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleOpacity),
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Multiply(opacity, intensity),
        },
    ];
    program.outputs.color = color;
    program.outputs.alpha = alpha;
    program
}

#[test]
fn valid_two_texture_flame_lowers_to_typed_backend_neutral_ir() {
    let program = two_texture_flame_program();

    let ir = MaterialCompiler.compile(&program).unwrap();

    assert_eq!(ir.source, program.id);
    assert_eq!(ir.parameters.len(), 4);
    assert_eq!(
        ir.values
            .iter()
            .filter(|value| matches!(
                value.instruction,
                MaterialIrInstruction::SampleTexture { .. }
            ))
            .count(),
        2
    );
    assert_eq!(
        ir.value(ir.outputs.color).unwrap().value_type,
        MaterialValueType::Color
    );
    assert_eq!(
        ir.value(ir.outputs.alpha).unwrap().value_type,
        MaterialValueType::Float
    );
    assert!(
        ir.values
            .iter()
            .enumerate()
            .all(|(index, value)| value.id == MaterialIrValueId(index as u32))
    );
    assert_eq!(ir.source_map.eliminated.len(), 0);
}

#[test]
fn lowering_is_deterministic_independent_of_authored_vector_order() {
    let program = two_texture_flame_program();
    let mut reordered = program.clone();
    reordered.parameters.reverse();
    reordered.expressions.reverse();

    let first = MaterialCompiler.compile(&program).unwrap();
    let second = MaterialCompiler.compile(&reordered).unwrap();

    assert_eq!(first, second);
}

#[test]
fn folding_simplification_and_dead_elimination_preserve_source_mapping() {
    let color_a = MaterialExpressionId::from_u128(0x4001);
    let color_b = MaterialExpressionId::from_u128(0x4002);
    let color = MaterialExpressionId::from_u128(0x4003);
    let opacity = MaterialExpressionId::from_u128(0x4004);
    let zero = MaterialExpressionId::from_u128(0x4005);
    let alpha = MaterialExpressionId::from_u128(0x4006);
    let unreachable = MaterialExpressionId::from_u128(0x4007);
    let mut program = MaterialProgram::additive_sprite("Optimized");
    program.expressions = vec![
        MaterialExpression {
            id: color_a,
            kind: MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([0.5, 0.25, 0.0, 1.0])),
        },
        MaterialExpression {
            id: color_b,
            kind: MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([0.5, 1.0, 1.0, 1.0])),
        },
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Multiply(color_a, color_b),
        },
        MaterialExpression {
            id: opacity,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleOpacity),
        },
        MaterialExpression {
            id: zero,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Add(opacity, zero),
        },
        MaterialExpression {
            id: unreachable,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(42.0)),
        },
    ];
    program.outputs.color = color;
    program.outputs.alpha = alpha;

    let ir = MaterialCompiler.compile(&program).unwrap();

    assert_eq!(ir.values.len(), 2);
    assert_eq!(ir.optimizations.constant_folds, 1);
    assert_eq!(ir.optimizations.trivial_simplifications, 1);
    assert!(ir.optimizations.eliminated_values >= 3);
    assert!(ir.source_map.eliminated.contains(&color_a));
    assert!(ir.source_map.eliminated.contains(&color_b));
    assert!(ir.source_map.eliminated.contains(&zero));
    assert!(ir.source_map.eliminated.contains(&unreachable));
    assert_eq!(ir.source_map.values[&opacity], ir.source_map.values[&alpha]);
    assert!(matches!(
        ir.value(ir.outputs.color).unwrap().instruction,
        MaterialIrInstruction::Constant(MaterialIrConstant::ColorLinear(_))
    ));
}

#[test]
fn authored_srgb_constants_are_lowered_to_linear_color() {
    let mut program = MaterialProgram::additive_sprite("Linearized");
    program.expressions[0].kind =
        MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([0.5, 0.5, 0.5, 0.25]));

    let ir = MaterialCompiler.compile(&program).unwrap();
    let MaterialIrInstruction::Constant(MaterialIrConstant::ColorLinear(color)) =
        ir.value(ir.outputs.color).unwrap().instruction
    else {
        panic!("color output should be a linear IR constant");
    };

    assert!((color[0] - 0.214_041_14).abs() < 0.000_001);
    assert_eq!(color[3], 0.25);
}

#[test]
fn invalid_materials_never_reach_ir_lowering() {
    let mut program = MaterialProgram::additive_sprite("Invalid output");
    program.outputs.alpha = program.outputs.color;

    let error = MaterialCompiler.compile(&program).unwrap_err();

    assert!(matches!(error, MaterialCompileError::Validation(_)));
    assert!(
        error
            .report()
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::MaterialTypeMismatch)
    );
}
