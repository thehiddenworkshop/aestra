use aestra_compiler::{
    MaterialCompiler, MaterialStackFallbackReason, MaterialStackModifierKind,
    MaterialStackProjection,
};
use aestra_core::material::{
    MaterialEvaluationDomain, MaterialExpression, MaterialExpressionKind, MaterialInput,
    MaterialParameter, MaterialProgram, MaterialSamplerDescriptor, MaterialTextureColorSpace,
    MaterialTextureDescriptor, MaterialValue, MaterialValueType, MaterialVectorComponent,
};

fn texture_type() -> MaterialValueType {
    MaterialValueType::Texture2D(MaterialTextureDescriptor {
        color_space: MaterialTextureColorSpace::SrgbColor,
        sampler: MaterialSamplerDescriptor::default(),
    })
}
use aestra_core::{AssetId, MaterialExpressionId, MaterialParameterId};

fn linear_stack_program() -> MaterialProgram {
    let uv = MaterialExpressionId::from_u128(0x5101);
    let speed = MaterialExpressionId::from_u128(0x5102);
    let time = MaterialExpressionId::from_u128(0x5103);
    let pan = MaterialExpressionId::from_u128(0x5104);
    let texture_parameter = MaterialParameterId::from_u128(0x5110);
    let texture = MaterialExpressionId::from_u128(0x5105);
    let sample = MaterialExpressionId::from_u128(0x5106);
    let sampled_alpha = MaterialExpressionId::from_u128(0x5107);
    let threshold = MaterialExpressionId::from_u128(0x5108);
    let edge_width = MaterialExpressionId::from_u128(0x5109);
    let dissolve_invert = MaterialExpressionId::from_u128(0x510a);
    let dissolve = MaterialExpressionId::from_u128(0x510b);
    let scene_depth = MaterialExpressionId::from_u128(0x510c);
    let pixel_depth = MaterialExpressionId::from_u128(0x510d);
    let fade_distance = MaterialExpressionId::from_u128(0x510e);
    let fade_invert = MaterialExpressionId::from_u128(0x510f);
    let soft_particle = MaterialExpressionId::from_u128(0x5111);
    let texture_type = texture_type();
    let mut program = MaterialProgram::additive_sprite("Linear stack");
    program.parameters.push(MaterialParameter {
        id: texture_parameter,
        name: "Texture".into(),
        value_type: texture_type,
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x5112))),
    });
    program.expressions = vec![
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: speed,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.1, 0.0])),
        },
        MaterialExpression {
            id: time,
            kind: MaterialExpressionKind::Input(MaterialInput::EffectTime),
        },
        MaterialExpression {
            id: pan,
            kind: MaterialExpressionKind::PanUv { uv, speed, time },
        },
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(texture_parameter),
        },
        MaterialExpression {
            id: sample,
            kind: MaterialExpressionKind::SampleTexture { texture, uv: pan },
        },
        MaterialExpression {
            id: sampled_alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: sample,
                component: MaterialVectorComponent::W,
            },
        },
        MaterialExpression {
            id: threshold,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.4)),
        },
        MaterialExpression {
            id: edge_width,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.1)),
        },
        MaterialExpression {
            id: dissolve_invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: dissolve,
            kind: MaterialExpressionKind::Dissolve {
                source: sampled_alpha,
                threshold,
                edge_width,
                invert: dissolve_invert,
            },
        },
        MaterialExpression {
            id: scene_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::SceneDepth),
        },
        MaterialExpression {
            id: pixel_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::PixelDepth),
        },
        MaterialExpression {
            id: fade_distance,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: fade_invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: soft_particle,
            kind: MaterialExpressionKind::SoftParticle {
                alpha: dissolve,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert: fade_invert,
            },
        },
    ];
    program.outputs.color = sample;
    program.outputs.alpha = soft_particle;
    program
}

#[test]
fn linear_semantic_program_projects_in_source_to_output_order() {
    let program = linear_stack_program();

    let MaterialStackProjection::Stack { entries } =
        MaterialCompiler.project_stack(&program).unwrap()
    else {
        panic!("linear semantic program must project as a stack");
    };
    assert_eq!(
        entries.iter().map(|entry| entry.kind).collect::<Vec<_>>(),
        vec![
            MaterialStackModifierKind::PanUv,
            MaterialStackModifierKind::BaseTexture,
            MaterialStackModifierKind::Dissolve,
            MaterialStackModifierKind::SoftParticle,
        ]
    );
    assert_eq!(
        entries[0].expression,
        MaterialExpressionId::from_u128(0x5104)
    );
    assert_eq!(
        entries[3].expression,
        MaterialExpressionId::from_u128(0x5111)
    );
}

#[test]
fn projection_is_independent_of_authored_expression_order() {
    let program = linear_stack_program();
    let mut reordered = program.clone();
    reordered.expressions.reverse();

    assert_eq!(
        MaterialCompiler.project_stack(&program).unwrap(),
        MaterialCompiler.project_stack(&reordered).unwrap()
    );
}

#[test]
fn shared_modifier_fan_out_requires_the_advanced_representation() {
    let mut program = linear_stack_program();
    let pan = MaterialExpressionId::from_u128(0x5104);
    let texture = MaterialExpressionId::from_u128(0x5105);
    let alternate_sample = MaterialExpressionId::from_u128(0x5113);
    program.expressions.push(MaterialExpression {
        id: alternate_sample,
        kind: MaterialExpressionKind::SampleTexture { texture, uv: pan },
    });
    program.outputs.color = alternate_sample;

    let MaterialStackProjection::Advanced { reason } =
        MaterialCompiler.project_stack(&program).unwrap()
    else {
        panic!("one modifier feeding separate chains must require the advanced representation");
    };
    assert_eq!(
        reason,
        MaterialStackFallbackReason::Branched { expression: pan }
    );
}

#[test]
fn independent_texture_chains_require_the_advanced_representation() {
    let texture_parameter = MaterialParameterId::from_u128(0x5201);
    let texture = MaterialExpressionId::from_u128(0x5202);
    let uv = MaterialExpressionId::from_u128(0x5203);
    let first = MaterialExpressionId::from_u128(0x5204);
    let second = MaterialExpressionId::from_u128(0x5205);
    let alpha = MaterialExpressionId::from_u128(0x5206);
    let texture_type = texture_type();
    let mut program = MaterialProgram::additive_sprite("Branched textures");
    program.parameters.push(MaterialParameter {
        id: texture_parameter,
        name: "Texture".into(),
        value_type: texture_type,
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x5207))),
    });
    program.expressions = vec![
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(texture_parameter),
        },
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: first,
            kind: MaterialExpressionKind::SampleTexture { texture, uv },
        },
        MaterialExpression {
            id: second,
            kind: MaterialExpressionKind::SampleTexture { texture, uv },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: second,
                component: MaterialVectorComponent::W,
            },
        },
    ];
    program.outputs.color = first;
    program.outputs.alpha = alpha;

    let MaterialStackProjection::Advanced { reason } =
        MaterialCompiler.project_stack(&program).unwrap()
    else {
        panic!("independent texture chains must not be represented as one stack");
    };
    assert_eq!(
        reason,
        MaterialStackFallbackReason::MultipleRoots {
            expressions: vec![first, second],
        }
    );
}
