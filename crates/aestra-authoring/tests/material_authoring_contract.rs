use aestra_authoring::{
    MaterialAuthoringDocument, MaterialChangeKind, MaterialCommand, MaterialCommandError,
    MaterialCommandExecutor, MaterialCommandHistory, MaterialExpressionInput, MaterialOutputSocket,
    MaterialSemanticTarget, MaterialTransaction,
};
use aestra_core::{
    BlendMode, EffectAsset, Emitter, MaterialExpressionId, MaterialId, MaterialParameterId,
    MaterialProgramId, RendererId,
    material::{
        MaterialEvaluationDomain, MaterialExpression, MaterialExpressionKind, MaterialInput,
        MaterialInstance, MaterialParameter, MaterialParameterValue, MaterialProgram,
        MaterialProgramRef, MaterialRenderState, MaterialValue, MaterialValueType,
        MaterialVectorComponent,
    },
};
use std::collections::BTreeMap;

fn authoring_document() -> MaterialAuthoringDocument {
    let mut effect = EffectAsset::new("Material authoring", 2.0);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
    MaterialAuthoringDocument::new(effect, Vec::new())
}

fn parameterized_program(id: MaterialProgramId) -> (MaterialProgram, MaterialParameterId) {
    let parameter = MaterialParameterId::from_u128(0x1100);
    let mut program = MaterialProgram::additive_sprite("Parameterized");
    program.id = id;
    program.parameters.push(MaterialParameter {
        id: parameter,
        name: "intensity".into(),
        value_type: MaterialValueType::Float,
        evaluation_domain: MaterialEvaluationDomain::Effect,
        default: Some(MaterialValue::Float(1.0)),
    });
    (program, parameter)
}

#[test]
fn program_and_expression_commands_are_transactional_and_reversible() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x1000);
    let (program, _) = parameterized_program(program_id);
    let original_alpha = program.outputs.alpha;
    let multiply = MaterialExpressionId::from_u128(0x1001);
    let alternate = MaterialExpressionId::from_u128(0x1002);
    let mut history = MaterialCommandHistory::default();

    let diff = history
        .execute(
            &mut document,
            MaterialTransaction::single(
                "Add material program",
                MaterialCommand::AddMaterialProgram { program, index: 0 },
            ),
        )
        .unwrap();
    assert!(diff.changes.iter().any(|change| {
        change.kind == MaterialChangeKind::Added
            && change.target == MaterialSemanticTarget::Program(program_id)
    }));

    let mut replacement = document.programs[0].clone();
    replacement.name = "Renamed program".into();
    history
        .execute(
            &mut document,
            MaterialTransaction::single(
                "Replace material program",
                MaterialCommand::ReplaceMaterialProgram {
                    id: program_id,
                    program: replacement,
                },
            ),
        )
        .unwrap();

    history
        .execute(
            &mut document,
            MaterialTransaction::new(
                "Build alpha expression",
                vec![
                    MaterialCommand::AddMaterialExpression {
                        program: program_id,
                        expression: MaterialExpression {
                            id: multiply,
                            kind: MaterialExpressionKind::Multiply(original_alpha, original_alpha),
                        },
                        index: 2,
                    },
                    MaterialCommand::SetMaterialOutput {
                        program: program_id,
                        output: MaterialOutputSocket::Alpha,
                        expression: multiply,
                    },
                    MaterialCommand::AddMaterialExpression {
                        program: program_id,
                        expression: MaterialExpression {
                            id: alternate,
                            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
                        },
                        index: 3,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: multiply,
                        input: MaterialExpressionInput::Right,
                        source: alternate,
                    },
                ],
            ),
        )
        .unwrap();
    assert_eq!(document.programs[0].outputs.alpha, multiply);
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == multiply)
            .unwrap()
            .kind,
        MaterialExpressionKind::Multiply(_, right) if right == alternate
    ));

    history
        .execute(
            &mut document,
            MaterialTransaction::single(
                "Replace alpha constant",
                MaterialCommand::ReplaceMaterialExpression {
                    program: program_id,
                    expression: alternate,
                    replacement: MaterialExpression {
                        id: alternate,
                        kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
                    },
                },
            ),
        )
        .unwrap();

    history
        .execute(
            &mut document,
            MaterialTransaction::new(
                "Remove alpha branch",
                vec![
                    MaterialCommand::SetMaterialOutput {
                        program: program_id,
                        output: MaterialOutputSocket::Alpha,
                        expression: original_alpha,
                    },
                    MaterialCommand::RemoveMaterialExpression {
                        program: program_id,
                        expression: multiply,
                    },
                    MaterialCommand::RemoveMaterialExpression {
                        program: program_id,
                        expression: alternate,
                    },
                ],
            ),
        )
        .unwrap();
    assert_eq!(document.programs[0].expressions.len(), 2);

    history
        .execute(
            &mut document,
            MaterialTransaction::single(
                "Remove material program",
                MaterialCommand::RemoveMaterialProgram { id: program_id },
            ),
        )
        .unwrap();
    assert!(document.programs.is_empty());

    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document.programs[0].id, program_id);
    history.redo(&mut document).unwrap().unwrap();
    assert!(document.programs.is_empty());
}

#[test]
fn uv_transform_semantic_sockets_are_rewireable_and_undoable() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x1200);
    let uv = MaterialExpressionId::from_u128(0x1201);
    let alternate_uv = MaterialExpressionId::from_u128(0x1202);
    let speed = MaterialExpressionId::from_u128(0x1203);
    let time = MaterialExpressionId::from_u128(0x1204);
    let alternate_time = MaterialExpressionId::from_u128(0x1205);
    let pan = MaterialExpressionId::from_u128(0x1206);
    let center = MaterialExpressionId::from_u128(0x1207);
    let alternate_center = MaterialExpressionId::from_u128(0x1208);
    let angle = MaterialExpressionId::from_u128(0x1209);
    let alternate_angle = MaterialExpressionId::from_u128(0x120A);
    let rotate = MaterialExpressionId::from_u128(0x120B);
    let scale_value = MaterialExpressionId::from_u128(0x120C);
    let alternate_scale = MaterialExpressionId::from_u128(0x120D);
    let scale = MaterialExpressionId::from_u128(0x120E);
    let alpha = MaterialExpressionId::from_u128(0x120F);
    let mut program = MaterialProgram::additive_sprite("Authorable UV transforms");
    program.id = program_id;
    program.expressions.extend([
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: alternate_uv,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.5, 0.5])),
        },
        MaterialExpression {
            id: speed,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.1, -0.2])),
        },
        MaterialExpression {
            id: time,
            kind: MaterialExpressionKind::Input(MaterialInput::EffectTime),
        },
        MaterialExpression {
            id: alternate_time,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(2.0)),
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
            id: alternate_center,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.25, 0.75])),
        },
        MaterialExpression {
            id: angle,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: alternate_angle,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
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
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([1.0, 1.0])),
        },
        MaterialExpression {
            id: alternate_scale,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([2.0, 0.5])),
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
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: scale,
                component: MaterialVectorComponent::X,
            },
        },
    ]);
    program.outputs.alpha = alpha;
    document.programs.push(program);
    let mut history = MaterialCommandHistory::default();

    history
        .execute(
            &mut document,
            MaterialTransaction::new(
                "Rewire panning UV",
                vec![
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: pan,
                        input: MaterialExpressionInput::Uv,
                        source: alternate_uv,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: pan,
                        input: MaterialExpressionInput::Speed,
                        source: uv,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: pan,
                        input: MaterialExpressionInput::Time,
                        source: alternate_time,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: rotate,
                        input: MaterialExpressionInput::Uv,
                        source: uv,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: rotate,
                        input: MaterialExpressionInput::Center,
                        source: alternate_center,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: rotate,
                        input: MaterialExpressionInput::Angle,
                        source: alternate_angle,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: scale,
                        input: MaterialExpressionInput::Uv,
                        source: pan,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: scale,
                        input: MaterialExpressionInput::Center,
                        source: alternate_center,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: scale,
                        input: MaterialExpressionInput::Scale,
                        source: alternate_scale,
                    },
                ],
            ),
        )
        .unwrap();

    let expression = document.programs[0]
        .expressions
        .iter()
        .find(|expression| expression.id == pan)
        .unwrap();
    assert!(matches!(
        expression.kind,
        MaterialExpressionKind::PanUv {
            uv: rewired_uv,
            speed: rewired_speed,
            time: rewired_time,
        } if rewired_uv == alternate_uv && rewired_speed == uv && rewired_time == alternate_time
    ));
    let expression = document.programs[0]
        .expressions
        .iter()
        .find(|expression| expression.id == rotate)
        .unwrap();
    assert!(matches!(
        expression.kind,
        MaterialExpressionKind::RotateUv {
            uv: rewired_uv,
            center: rewired_center,
            angle: rewired_angle,
        } if rewired_uv == uv
            && rewired_center == alternate_center
            && rewired_angle == alternate_angle
    ));
    let expression = document.programs[0]
        .expressions
        .iter()
        .find(|expression| expression.id == scale)
        .unwrap();
    assert!(matches!(
        expression.kind,
        MaterialExpressionKind::ScaleUv {
            uv: rewired_uv,
            center: rewired_center,
            scale: rewired_scale,
        } if rewired_uv == pan
            && rewired_center == alternate_center
            && rewired_scale == alternate_scale
    ));

    history.undo(&mut document).unwrap().unwrap();
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == pan)
            .unwrap()
            .kind,
        MaterialExpressionKind::PanUv {
            uv: original_uv,
            speed: original_speed,
            time: original_time,
        } if original_uv == uv && original_speed == speed && original_time == time
    ));
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == rotate)
            .unwrap()
            .kind,
        MaterialExpressionKind::RotateUv {
            uv: original_uv,
            center: original_center,
            angle: original_angle,
        } if original_uv == pan && original_center == center && original_angle == angle
    ));
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == scale)
            .unwrap()
            .kind,
        MaterialExpressionKind::ScaleUv {
            uv: original_uv,
            center: original_center,
            scale: original_scale,
        } if original_uv == rotate
            && original_center == center
            && original_scale == scale_value
    ));
}

#[test]
fn instance_parameter_render_state_and_assignment_commands_are_undoable() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x2000);
    let (mut program, parameter) = parameterized_program(program_id);
    let alpha_state = MaterialRenderState {
        blend: BlendMode::Alpha,
        ..MaterialRenderState::additive_sprite()
    };
    program.render_state_policy.allowed.push(alpha_state);
    document.programs.push(program);
    let instance_id = MaterialId::from_u128(0x2001);
    let emitter = document.effect.emitters[0].id;
    let renderer = document.effect.emitters[0].renderers[0].id;
    let legacy_material = document.effect.emitters[0].renderers[0].material;
    let instance = MaterialInstance {
        id: instance_id,
        program: MaterialProgramRef::Project(program_id),
        values: BTreeMap::new(),
        render_state: MaterialRenderState::additive_sprite(),
    };
    let mut history = MaterialCommandHistory::default();

    history
        .execute(
            &mut document,
            MaterialTransaction::new(
                "Add and assign material instance",
                vec![
                    MaterialCommand::AddMaterialInstance { instance, index: 0 },
                    MaterialCommand::AssignRendererMaterial {
                        emitter,
                        renderer,
                        material: instance_id,
                    },
                ],
            ),
        )
        .unwrap();

    let diff = history
        .execute(
            &mut document,
            MaterialTransaction::new(
                "Edit material instance",
                vec![
                    MaterialCommand::SetMaterialInstanceParameter {
                        instance: instance_id,
                        parameter,
                        value: Some(MaterialParameterValue::Constant(MaterialValue::Float(2.0))),
                    },
                    MaterialCommand::SetMaterialInstanceRenderState {
                        instance: instance_id,
                        render_state: alpha_state,
                    },
                ],
            ),
        )
        .unwrap();
    assert!(diff.changes.iter().any(|change| {
        change.target == MaterialSemanticTarget::Instance(instance_id)
            && change.kind == MaterialChangeKind::Modified
    }));
    assert_eq!(
        document.effect.material_instances[0].values[&parameter],
        MaterialParameterValue::Constant(MaterialValue::Float(2.0))
    );

    let mut replacement = document.effect.material_instances[0].clone();
    replacement.values.clear();
    history
        .execute(
            &mut document,
            MaterialTransaction::single(
                "Replace material instance",
                MaterialCommand::ReplaceMaterialInstance {
                    id: instance_id,
                    instance: replacement,
                },
            ),
        )
        .unwrap();

    history
        .execute(
            &mut document,
            MaterialTransaction::new(
                "Remove material instance",
                vec![
                    MaterialCommand::AssignRendererMaterial {
                        emitter,
                        renderer,
                        material: legacy_material,
                    },
                    MaterialCommand::RemoveMaterialInstance { id: instance_id },
                ],
            ),
        )
        .unwrap();
    assert!(document.effect.material_instances.is_empty());

    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document.effect.material_instances[0].id, instance_id);
    assert_eq!(
        document.effect.emitters[0].renderers[0].material,
        instance_id
    );
}

#[test]
fn invalid_material_transactions_are_atomic() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x3000);
    let (program, _) = parameterized_program(program_id);
    document.programs.push(program);
    let before = document.clone();
    let color = document.programs[0].outputs.color;

    let error = MaterialCommandExecutor::execute(
        &mut document,
        &MaterialTransaction::single(
            "Break alpha output",
            MaterialCommand::SetMaterialOutput {
                program: program_id,
                output: MaterialOutputSocket::Alpha,
                expression: color,
            },
        ),
    )
    .unwrap_err();

    assert!(matches!(error, MaterialCommandError::Validation(_)));
    assert_eq!(document, before);
}

#[test]
fn replacement_commands_preserve_stable_identity() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x4000);
    let (program, _) = parameterized_program(program_id);
    document.programs.push(program);
    let mut replacement = document.programs[0].clone();
    replacement.id = MaterialProgramId::from_u128(0x4001);

    let error = MaterialCommandExecutor::execute(
        &mut document,
        &MaterialTransaction::single(
            "Invalid replacement",
            MaterialCommand::ReplaceMaterialProgram {
                id: program_id,
                program: replacement,
            },
        ),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        MaterialCommandError::IdentityChanged { .. }
    ));
}

#[test]
fn renderer_lookup_is_scoped_to_the_authored_emitter() {
    let mut document = authoring_document();
    let renderer = RendererId::from_u128(0x5000);
    let emitter = document.effect.emitters[0].id;
    let material = document.effect.materials[0].id;
    let before = document.clone();

    let error = MaterialCommandExecutor::execute(
        &mut document,
        &MaterialTransaction::single(
            "Assign missing renderer",
            MaterialCommand::AssignRendererMaterial {
                emitter,
                renderer,
                material,
            },
        ),
    )
    .unwrap_err();

    assert!(matches!(error, MaterialCommandError::NotFound { .. }));
    assert_eq!(document, before);
}
