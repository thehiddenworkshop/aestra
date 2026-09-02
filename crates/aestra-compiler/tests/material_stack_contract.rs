use aestra_compiler::{
    MaterialCompiler, MaterialStackFallbackReason, MaterialStackModifierKind,
    MaterialStackMoveError, MaterialStackMoveTarget, MaterialStackProjection,
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

fn reorderable_uv_stack_program() -> MaterialProgram {
    let uv = MaterialExpressionId::from_u128(0x5301);
    let speed = MaterialExpressionId::from_u128(0x5302);
    let time = MaterialExpressionId::from_u128(0x5303);
    let pan = MaterialExpressionId::from_u128(0x5304);
    let center = MaterialExpressionId::from_u128(0x5305);
    let angle = MaterialExpressionId::from_u128(0x5306);
    let rotate = MaterialExpressionId::from_u128(0x5307);
    let scale_value = MaterialExpressionId::from_u128(0x5308);
    let scale = MaterialExpressionId::from_u128(0x5309);
    let texture_parameter = MaterialParameterId::from_u128(0x530a);
    let texture = MaterialExpressionId::from_u128(0x530b);
    let sample = MaterialExpressionId::from_u128(0x530c);
    let alpha = MaterialExpressionId::from_u128(0x530d);
    let texture_type = texture_type();
    let mut program = MaterialProgram::additive_sprite("Reorderable UV stack");
    program.parameters.push(MaterialParameter {
        id: texture_parameter,
        name: "Texture".into(),
        value_type: texture_type,
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x530e))),
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
            id: center,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.5, 0.5])),
        },
        MaterialExpression {
            id: angle,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: rotate,
            kind: MaterialExpressionKind::RotateUv {
                uv: pan,
                center,
                angle,
            },
        },
        MaterialExpression {
            id: scale_value,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([2.0, 2.0])),
        },
        MaterialExpression {
            id: scale,
            kind: MaterialExpressionKind::ScaleUv {
                uv: rotate,
                center,
                scale: scale_value,
            },
        },
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(texture_parameter),
        },
        MaterialExpression {
            id: sample,
            kind: MaterialExpressionKind::SampleTexture { texture, uv: scale },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: sample,
                component: MaterialVectorComponent::W,
            },
        },
    ];
    program.outputs.color = sample;
    program.outputs.alpha = alpha;
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

#[test]
fn move_targets_only_include_valid_positions_in_a_direct_typed_chain() {
    let program = reorderable_uv_stack_program();
    let rotate = MaterialExpressionId::from_u128(0x5307);
    let sample = MaterialExpressionId::from_u128(0x530c);

    assert_eq!(
        MaterialCompiler
            .stack_move_targets(&program, rotate)
            .unwrap(),
        vec![
            MaterialStackMoveTarget { index: 0 },
            MaterialStackMoveTarget { index: 2 },
        ]
    );
    assert!(
        MaterialCompiler
            .stack_move_targets(&program, sample)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn stack_move_preserves_ids_and_rewires_the_terminal_consumer() {
    let program = reorderable_uv_stack_program();
    let pan = MaterialExpressionId::from_u128(0x5304);
    let rotate = MaterialExpressionId::from_u128(0x5307);
    let scale = MaterialExpressionId::from_u128(0x5309);
    let sample = MaterialExpressionId::from_u128(0x530c);
    let original_ids = program
        .expressions
        .iter()
        .map(|expression| expression.id)
        .collect::<Vec<_>>();

    let plan = MaterialCompiler.plan_stack_move(&program, pan, 2).unwrap();
    assert_eq!(plan.from_index, 0);
    assert_eq!(plan.to_index, 2);
    assert_eq!(
        plan.replacement
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        original_ids
    );
    let MaterialStackProjection::Stack { entries } =
        MaterialCompiler.project_stack(&plan.replacement).unwrap()
    else {
        panic!("moved program must remain a stack");
    };
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.expression)
            .collect::<Vec<_>>(),
        vec![rotate, scale, pan, sample]
    );
    assert!(matches!(
        plan.replacement
            .expressions
            .iter()
            .find(|expression| expression.id == sample)
            .unwrap()
            .kind,
        MaterialExpressionKind::SampleTexture { uv, .. } if uv == pan
    ));
}

#[test]
fn incompatible_and_advanced_moves_are_rejected_without_a_replacement() {
    let program = reorderable_uv_stack_program();
    let pan = MaterialExpressionId::from_u128(0x5304);
    assert!(matches!(
        MaterialCompiler.plan_stack_move(&program, pan, 3),
        Err(MaterialStackMoveError::IncompatibleTarget { index: 3 })
    ));

    let mut advanced = linear_stack_program();
    let source = MaterialExpressionId::from_u128(0x5104);
    let texture = MaterialExpressionId::from_u128(0x5105);
    let alternate = MaterialExpressionId::from_u128(0x5310);
    advanced.expressions.push(MaterialExpression {
        id: alternate,
        kind: MaterialExpressionKind::SampleTexture {
            texture,
            uv: source,
        },
    });
    advanced.outputs.color = alternate;
    assert!(matches!(
        MaterialCompiler.plan_stack_move(&advanced, source, 1),
        Err(MaterialStackMoveError::Advanced)
    ));
}
