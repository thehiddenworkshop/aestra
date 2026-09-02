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
fn remap_semantic_sockets_are_rewireable_and_undoable() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x1300);
    let value = MaterialExpressionId::from_u128(0x1301);
    let alternate_value = MaterialExpressionId::from_u128(0x1302);
    let input_min = MaterialExpressionId::from_u128(0x1303);
    let input_max = MaterialExpressionId::from_u128(0x1304);
    let alternate_scalar = MaterialExpressionId::from_u128(0x1305);
    let output_min = MaterialExpressionId::from_u128(0x1306);
    let output_max = MaterialExpressionId::from_u128(0x1307);
    let alternate_vector = MaterialExpressionId::from_u128(0x1308);
    let remap = MaterialExpressionId::from_u128(0x1309);
    let alpha = MaterialExpressionId::from_u128(0x130A);
    let mut program = MaterialProgram::additive_sprite("Authorable remap");
    program.id = program_id;
    program.expressions.extend([
        MaterialExpression {
            id: value,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: alternate_value,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.25, 0.75])),
        },
        MaterialExpression {
            id: input_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: input_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: alternate_scalar,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(2.0)),
        },
        MaterialExpression {
            id: output_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([-1.0; 2])),
        },
        MaterialExpression {
            id: output_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([1.0; 2])),
        },
        MaterialExpression {
            id: alternate_vector,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([3.0, 4.0])),
        },
        MaterialExpression {
            id: remap,
            kind: MaterialExpressionKind::Remap {
                value,
                input_min,
                input_max,
                output_min,
                output_max,
            },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: remap,
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
                "Rewire remap",
                vec![
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: remap,
                        input: MaterialExpressionInput::Value,
                        source: alternate_value,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: remap,
                        input: MaterialExpressionInput::InputMinimum,
                        source: alternate_scalar,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: remap,
                        input: MaterialExpressionInput::InputMaximum,
                        source: input_min,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: remap,
                        input: MaterialExpressionInput::OutputMinimum,
                        source: alternate_vector,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: remap,
                        input: MaterialExpressionInput::OutputMaximum,
                        source: output_min,
                    },
                ],
            ),
        )
        .unwrap();

    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == remap)
            .unwrap()
            .kind,
        MaterialExpressionKind::Remap {
            value: rewired_value,
            input_min: rewired_input_min,
            input_max: rewired_input_max,
            output_min: rewired_output_min,
            output_max: rewired_output_max,
        } if rewired_value == alternate_value
            && rewired_input_min == alternate_scalar
            && rewired_input_max == input_min
            && rewired_output_min == alternate_vector
            && rewired_output_max == output_min
    ));

    history.undo(&mut document).unwrap().unwrap();
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == remap)
            .unwrap()
            .kind,
        MaterialExpressionKind::Remap {
            value: original_value,
            input_min: original_input_min,
            input_max: original_input_max,
            output_min: original_output_min,
            output_max: original_output_max,
        } if original_value == value
            && original_input_min == input_min
            && original_input_max == input_max
            && original_output_min == output_min
            && original_output_max == output_max
    ));
}

#[test]
fn smoothstep_semantic_sockets_are_rewireable_and_undoable() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x1400);
    let edge_min = MaterialExpressionId::from_u128(0x1401);
    let edge_max = MaterialExpressionId::from_u128(0x1402);
    let value = MaterialExpressionId::from_u128(0x1403);
    let alternate = MaterialExpressionId::from_u128(0x1404);
    let smoothstep = MaterialExpressionId::from_u128(0x1405);
    let alpha = MaterialExpressionId::from_u128(0x1406);
    let mut program = MaterialProgram::additive_sprite("Authorable smoothstep");
    program.id = program_id;
    program.expressions.extend([
        MaterialExpression {
            id: edge_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: edge_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: value,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: alternate,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.25, 0.75])),
        },
        MaterialExpression {
            id: smoothstep,
            kind: MaterialExpressionKind::Smoothstep {
                edge_min,
                edge_max,
                value,
            },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: smoothstep,
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
                "Rewire smoothstep",
                vec![
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: smoothstep,
                        input: MaterialExpressionInput::EdgeMinimum,
                        source: edge_max,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: smoothstep,
                        input: MaterialExpressionInput::EdgeMaximum,
                        source: edge_min,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: smoothstep,
                        input: MaterialExpressionInput::Value,
                        source: alternate,
                    },
                ],
            ),
        )
        .unwrap();

    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == smoothstep)
            .unwrap()
            .kind,
        MaterialExpressionKind::Smoothstep {
            edge_min: rewired_min,
            edge_max: rewired_max,
            value: rewired_value,
        } if rewired_min == edge_max && rewired_max == edge_min && rewired_value == alternate
    ));

    history.undo(&mut document).unwrap().unwrap();
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == smoothstep)
            .unwrap()
            .kind,
        MaterialExpressionKind::Smoothstep {
            edge_min: original_min,
            edge_max: original_max,
            value: original_value,
        } if original_min == edge_min && original_max == edge_max && original_value == value
    ));
}

#[test]
fn radial_mask_semantic_sockets_are_rewireable_and_undoable() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x1500);
    let uv = MaterialExpressionId::from_u128(0x1501);
    let center = MaterialExpressionId::from_u128(0x1502);
    let radius = MaterialExpressionId::from_u128(0x1503);
    let softness = MaterialExpressionId::from_u128(0x1504);
    let invert = MaterialExpressionId::from_u128(0x1505);
    let alternate_uv = MaterialExpressionId::from_u128(0x1506);
    let alternate_center = MaterialExpressionId::from_u128(0x1507);
    let alternate_radius = MaterialExpressionId::from_u128(0x1508);
    let alternate_softness = MaterialExpressionId::from_u128(0x1509);
    let alternate_invert = MaterialExpressionId::from_u128(0x150A);
    let radial_mask = MaterialExpressionId::from_u128(0x150B);
    let mut program = MaterialProgram::additive_sprite("Authorable radial mask");
    program.id = program_id;
    program.expressions.extend([
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: center,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.5; 2])),
        },
        MaterialExpression {
            id: radius,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: softness,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.1)),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: alternate_uv,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.25; 2])),
        },
        MaterialExpression {
            id: alternate_center,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.75; 2])),
        },
        MaterialExpression {
            id: alternate_radius,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
        },
        MaterialExpression {
            id: alternate_softness,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.2)),
        },
        MaterialExpression {
            id: alternate_invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(true)),
        },
        MaterialExpression {
            id: radial_mask,
            kind: MaterialExpressionKind::RadialMask {
                uv,
                center,
                radius,
                softness,
                invert,
            },
        },
    ]);
    program.outputs.alpha = radial_mask;
    document.programs.push(program);
    let mut history = MaterialCommandHistory::default();

    history
        .execute(
            &mut document,
            MaterialTransaction::new(
                "Rewire radial mask",
                vec![
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: radial_mask,
                        input: MaterialExpressionInput::Uv,
                        source: alternate_uv,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: radial_mask,
                        input: MaterialExpressionInput::Center,
                        source: alternate_center,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: radial_mask,
                        input: MaterialExpressionInput::Radius,
                        source: alternate_radius,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: radial_mask,
                        input: MaterialExpressionInput::Softness,
                        source: alternate_softness,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: radial_mask,
                        input: MaterialExpressionInput::Invert,
                        source: alternate_invert,
                    },
                ],
            ),
        )
        .unwrap();

    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == radial_mask)
            .unwrap()
            .kind,
        MaterialExpressionKind::RadialMask {
            uv: rewired_uv,
            center: rewired_center,
            radius: rewired_radius,
            softness: rewired_softness,
            invert: rewired_invert,
        } if rewired_uv == alternate_uv
            && rewired_center == alternate_center
            && rewired_radius == alternate_radius
            && rewired_softness == alternate_softness
            && rewired_invert == alternate_invert
    ));

    history.undo(&mut document).unwrap().unwrap();
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == radial_mask)
            .unwrap()
            .kind,
        MaterialExpressionKind::RadialMask {
            uv: original_uv,
            center: original_center,
            radius: original_radius,
            softness: original_softness,
            invert: original_invert,
        } if original_uv == uv
            && original_center == center
            && original_radius == radius
            && original_softness == softness
            && original_invert == invert
    ));
}

#[test]
fn dissolve_semantic_sockets_are_rewireable_and_undoable() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x1600);
    let source = MaterialExpressionId::from_u128(0x1601);
    let threshold = MaterialExpressionId::from_u128(0x1602);
    let edge_width = MaterialExpressionId::from_u128(0x1603);
    let invert = MaterialExpressionId::from_u128(0x1604);
    let alternate_source = MaterialExpressionId::from_u128(0x1605);
    let alternate_threshold = MaterialExpressionId::from_u128(0x1606);
    let alternate_edge_width = MaterialExpressionId::from_u128(0x1607);
    let alternate_invert = MaterialExpressionId::from_u128(0x1608);
    let dissolve = MaterialExpressionId::from_u128(0x1609);
    let mut program = MaterialProgram::additive_sprite("Authorable dissolve");
    program.id = program_id;
    program.expressions.extend([
        MaterialExpression {
            id: source,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.25)),
        },
        MaterialExpression {
            id: threshold,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: edge_width,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.08)),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: alternate_source,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
        },
        MaterialExpression {
            id: alternate_threshold,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.6)),
        },
        MaterialExpression {
            id: alternate_edge_width,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.12)),
        },
        MaterialExpression {
            id: alternate_invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(true)),
        },
        MaterialExpression {
            id: dissolve,
            kind: MaterialExpressionKind::Dissolve {
                source,
                threshold,
                edge_width,
                invert,
            },
        },
    ]);
    program.outputs.alpha = dissolve;
    document.programs.push(program);
    let mut history = MaterialCommandHistory::default();

    history
        .execute(
            &mut document,
            MaterialTransaction::new(
                "Rewire dissolve",
                vec![
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: dissolve,
                        input: MaterialExpressionInput::Source,
                        source: alternate_source,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: dissolve,
                        input: MaterialExpressionInput::Threshold,
                        source: alternate_threshold,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: dissolve,
                        input: MaterialExpressionInput::EdgeWidth,
                        source: alternate_edge_width,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: dissolve,
                        input: MaterialExpressionInput::Invert,
                        source: alternate_invert,
                    },
                ],
            ),
        )
        .unwrap();

    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == dissolve)
            .unwrap()
            .kind,
        MaterialExpressionKind::Dissolve {
            source: rewired_source,
            threshold: rewired_threshold,
            edge_width: rewired_edge_width,
            invert: rewired_invert,
        } if rewired_source == alternate_source
            && rewired_threshold == alternate_threshold
            && rewired_edge_width == alternate_edge_width
            && rewired_invert == alternate_invert
    ));

    history.undo(&mut document).unwrap().unwrap();
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == dissolve)
            .unwrap()
            .kind,
        MaterialExpressionKind::Dissolve {
            source: original_source,
            threshold: original_threshold,
            edge_width: original_edge_width,
            invert: original_invert,
        } if original_source == source
            && original_threshold == threshold
            && original_edge_width == edge_width
            && original_invert == invert
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
