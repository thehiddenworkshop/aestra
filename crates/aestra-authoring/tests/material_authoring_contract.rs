use aestra_authoring::{
    MaterialAuthoringDocument, MaterialChangeKind, MaterialCommand, MaterialCommandError,
    MaterialCommandExecutor, MaterialCommandHistory, MaterialConnectionTarget,
    MaterialExpressionInput, MaterialInsertionPoint, MaterialOutputSocket,
    MaterialParameterBinding, MaterialSemanticTarget, MaterialToolCommand, MaterialToolError,
    MaterialToolPlanner, MaterialTransaction,
};
use aestra_compiler::{
    MATERIAL_PRESET_SOFT_DISSOLVE, MATERIAL_PRESET_UV_DRIFT, MaterialCompiler,
    MaterialGraphCreateKind, MaterialGraphFunction, MaterialPresetCatalog, MaterialPresetCategory,
    MaterialPresetDescriptor, MaterialPresetGraphNode, MaterialPresetGraphNodeKind,
    MaterialPresetGraphRecipe, MaterialPresetRecipe, MaterialPresetValueRef,
    MaterialStackModifierKind, MaterialStackProjection,
};
use aestra_core::{
    AssetId, BlendMode, EffectAsset, EffectParameter, Emitter, MaterialExpressionId,
    MaterialFunctionId, MaterialId, MaterialParameterId, MaterialPresetId, MaterialProgramId,
    ParameterId, RendererId, Value,
    material::{
        MaterialEvaluationDomain, MaterialExpression, MaterialExpressionKind, MaterialFunction,
        MaterialFunctionRef, MaterialInput, MaterialInstance, MaterialParameter,
        MaterialParameterValue, MaterialPresetSchemaVersion, MaterialProgram, MaterialProgramRef,
        MaterialRenderState, MaterialSamplerDescriptor, MaterialTextureColorSpace,
        MaterialTextureDescriptor, MaterialValue, MaterialValueType, MaterialVectorComponent,
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

fn reorderable_material_program(id: MaterialProgramId) -> (MaterialProgram, MaterialExpressionId) {
    let uv = MaterialExpressionId::from_u128(0x1180);
    let speed = MaterialExpressionId::from_u128(0x1181);
    let time = MaterialExpressionId::from_u128(0x1182);
    let pan = MaterialExpressionId::from_u128(0x1183);
    let center = MaterialExpressionId::from_u128(0x1184);
    let angle = MaterialExpressionId::from_u128(0x1185);
    let rotate = MaterialExpressionId::from_u128(0x1186);
    let texture_parameter = MaterialParameterId::from_u128(0x1187);
    let texture = MaterialExpressionId::from_u128(0x1188);
    let sample = MaterialExpressionId::from_u128(0x1189);
    let alpha = MaterialExpressionId::from_u128(0x118a);
    let texture_type = MaterialValueType::Texture2D(MaterialTextureDescriptor {
        color_space: MaterialTextureColorSpace::SrgbColor,
        sampler: MaterialSamplerDescriptor::default(),
    });
    let mut program = MaterialProgram::additive_sprite("Reorder transaction");
    program.id = id;
    program.parameters.push(MaterialParameter {
        id: texture_parameter,
        name: "Texture".into(),
        value_type: texture_type,
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x118b))),
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
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.25)),
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
            id: texture,
            kind: MaterialExpressionKind::Parameter(texture_parameter),
        },
        MaterialExpression {
            id: sample,
            kind: MaterialExpressionKind::SampleTexture {
                texture,
                uv: rotate,
            },
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
    (program, pan)
}

fn preset_host_material_program(id: MaterialProgramId) -> MaterialProgram {
    let mut program = MaterialProgram::additive_sprite("Preset transaction host");
    program.id = id;
    let alpha = program.outputs.alpha;
    let lower = MaterialExpressionId::from_u128(0xa357_b101);
    let upper = MaterialExpressionId::from_u128(0xa357_b102);
    let shaped = MaterialExpressionId::from_u128(0xa357_b103);
    program.expressions.extend([
        MaterialExpression {
            id: lower,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: upper,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: shaped,
            kind: MaterialExpressionKind::Smoothstep {
                edge_min: lower,
                edge_max: upper,
                value: alpha,
            },
        },
    ]);
    program.outputs.alpha = shaped;
    program
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
fn planned_stack_move_is_one_transaction_with_exact_undo_and_redo() {
    let program_id = MaterialProgramId::from_u128(0x117f);
    let (program, pan) = reorderable_material_program(program_id);
    let original = program.clone();
    let plan = aestra_compiler::MaterialCompiler
        .plan_stack_move(&program, pan, 1)
        .unwrap();
    let mut document = authoring_document();
    document.programs.push(program);
    let mut history = MaterialCommandHistory::default();

    history
        .execute(
            &mut document,
            MaterialTransaction::single(
                "Move material modifier",
                MaterialCommand::ReplaceMaterialProgram {
                    id: program_id,
                    program: plan.replacement.clone(),
                },
            ),
        )
        .unwrap();
    assert_eq!(document.programs[0], plan.replacement);

    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document.programs[0], original);

    history.redo(&mut document).unwrap().unwrap();
    assert_eq!(document.programs[0], plan.replacement);
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
fn dissolve_edge_semantic_sockets_are_rewireable_and_undoable() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x1700);
    let source = MaterialExpressionId::from_u128(0x1701);
    let threshold = MaterialExpressionId::from_u128(0x1702);
    let edge_width = MaterialExpressionId::from_u128(0x1703);
    let invert = MaterialExpressionId::from_u128(0x1704);
    let alternate_source = MaterialExpressionId::from_u128(0x1705);
    let alternate_threshold = MaterialExpressionId::from_u128(0x1706);
    let alternate_edge_width = MaterialExpressionId::from_u128(0x1707);
    let alternate_invert = MaterialExpressionId::from_u128(0x1708);
    let dissolve_edge = MaterialExpressionId::from_u128(0x1709);
    let mut program = MaterialProgram::additive_sprite("Authorable dissolve edge");
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
            id: dissolve_edge,
            kind: MaterialExpressionKind::DissolveEdge {
                source,
                threshold,
                edge_width,
                invert,
            },
        },
    ]);
    program.outputs.alpha = dissolve_edge;
    document.programs.push(program);
    let mut history = MaterialCommandHistory::default();

    history
        .execute(
            &mut document,
            MaterialTransaction::new(
                "Rewire dissolve edge",
                vec![
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: dissolve_edge,
                        input: MaterialExpressionInput::Source,
                        source: alternate_source,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: dissolve_edge,
                        input: MaterialExpressionInput::Threshold,
                        source: alternate_threshold,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: dissolve_edge,
                        input: MaterialExpressionInput::EdgeWidth,
                        source: alternate_edge_width,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: dissolve_edge,
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
            .find(|expression| expression.id == dissolve_edge)
            .unwrap()
            .kind,
        MaterialExpressionKind::DissolveEdge {
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
            .find(|expression| expression.id == dissolve_edge)
            .unwrap()
            .kind,
        MaterialExpressionKind::DissolveEdge {
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
fn depth_fade_sockets_are_rewired_transactionally() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x1d00);
    let scene_depth = MaterialExpressionId::from_u128(0x1d01);
    let alternate_scene_depth = MaterialExpressionId::from_u128(0x1d02);
    let pixel_depth = MaterialExpressionId::from_u128(0x1d03);
    let fade_distance = MaterialExpressionId::from_u128(0x1d04);
    let invert = MaterialExpressionId::from_u128(0x1d05);
    let depth_fade = MaterialExpressionId::from_u128(0x1d06);
    let mut program = MaterialProgram::additive_sprite("Authorable depth fade");
    program.id = program_id;
    program.expressions.extend([
        MaterialExpression {
            id: scene_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::SceneDepth),
        },
        MaterialExpression {
            id: alternate_scene_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::PixelDepth),
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
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: depth_fade,
            kind: MaterialExpressionKind::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            },
        },
    ]);
    program.outputs.alpha = depth_fade;
    document.programs.push(program);
    let mut history = MaterialCommandHistory::default();

    history
        .execute(
            &mut document,
            MaterialTransaction::single(
                "Rewire depth fade",
                MaterialCommand::RewireMaterialExpressionInput {
                    program: program_id,
                    expression: depth_fade,
                    input: MaterialExpressionInput::SceneDepth,
                    source: alternate_scene_depth,
                },
            ),
        )
        .unwrap();

    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == depth_fade)
            .unwrap()
            .kind,
        MaterialExpressionKind::DepthFade {
            scene_depth: rewired,
            ..
        } if rewired == alternate_scene_depth
    ));
    history.undo(&mut document).unwrap().unwrap();
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == depth_fade)
            .unwrap()
            .kind,
        MaterialExpressionKind::DepthFade {
            scene_depth: restored,
            ..
        } if restored == scene_depth
    ));
}

#[test]
fn soft_particle_sockets_are_rewired_transactionally() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x1e00);
    let alpha = MaterialExpressionId::from_u128(0x1e01);
    let scene_depth = MaterialExpressionId::from_u128(0x1e02);
    let pixel_depth = MaterialExpressionId::from_u128(0x1e03);
    let fade_distance = MaterialExpressionId::from_u128(0x1e04);
    let invert = MaterialExpressionId::from_u128(0x1e05);
    let alternate_alpha = MaterialExpressionId::from_u128(0x1e06);
    let alternate_scene_depth = MaterialExpressionId::from_u128(0x1e07);
    let alternate_pixel_depth = MaterialExpressionId::from_u128(0x1e08);
    let alternate_fade_distance = MaterialExpressionId::from_u128(0x1e09);
    let alternate_invert = MaterialExpressionId::from_u128(0x1e0a);
    let soft_particle = MaterialExpressionId::from_u128(0x1e0b);
    let mut program = MaterialProgram::additive_sprite("Authorable soft particle");
    program.id = program_id;
    program.expressions.extend([
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleOpacity),
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
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: alternate_alpha,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
        },
        MaterialExpression {
            id: alternate_scene_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::SceneDepth),
        },
        MaterialExpression {
            id: alternate_pixel_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::PixelDepth),
        },
        MaterialExpression {
            id: alternate_fade_distance,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: alternate_invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(true)),
        },
        MaterialExpression {
            id: soft_particle,
            kind: MaterialExpressionKind::SoftParticle {
                alpha,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            },
        },
    ]);
    program.outputs.alpha = soft_particle;
    document.programs.push(program);
    let mut history = MaterialCommandHistory::default();

    history
        .execute(
            &mut document,
            MaterialTransaction::new(
                "Rewire soft particle",
                vec![
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: soft_particle,
                        input: MaterialExpressionInput::SourceAlpha,
                        source: alternate_alpha,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: soft_particle,
                        input: MaterialExpressionInput::SceneDepth,
                        source: alternate_scene_depth,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: soft_particle,
                        input: MaterialExpressionInput::PixelDepth,
                        source: alternate_pixel_depth,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: soft_particle,
                        input: MaterialExpressionInput::FadeDistance,
                        source: alternate_fade_distance,
                    },
                    MaterialCommand::RewireMaterialExpressionInput {
                        program: program_id,
                        expression: soft_particle,
                        input: MaterialExpressionInput::Invert,
                        source: alternate_invert,
                    },
                ],
            ),
        )
        .unwrap();

    let expression = &document.programs[0]
        .expressions
        .iter()
        .find(|expression| expression.id == soft_particle)
        .unwrap()
        .kind;
    assert!(matches!(expression,
        MaterialExpressionKind::SoftParticle { alpha, scene_depth, pixel_depth, fade_distance, invert }
            if *alpha == alternate_alpha
                && *scene_depth == alternate_scene_depth
                && *pixel_depth == alternate_pixel_depth
                && *fade_distance == alternate_fade_distance
                && *invert == alternate_invert
    ));

    history.undo(&mut document).unwrap().unwrap();
    let expression = &document.programs[0]
        .expressions
        .iter()
        .find(|expression| expression.id == soft_particle)
        .unwrap()
        .kind;
    assert!(matches!(expression,
        MaterialExpressionKind::SoftParticle { alpha: restored_alpha, scene_depth: restored_scene_depth, pixel_depth: restored_pixel_depth, fade_distance: restored_fade_distance, invert: restored_invert }
            if *restored_alpha == alpha
                && *restored_scene_depth == scene_depth
                && *restored_pixel_depth == pixel_depth
                && *restored_fade_distance == fade_distance
                && *restored_invert == invert
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

#[test]
fn material_binding_tool_plans_a_stable_effect_binding_and_exact_undo() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x5800);
    let (program, material_parameter) = parameterized_program(program_id);
    document.programs.push(program);
    let instance_id = MaterialId::from_u128(0x5801);
    document.effect.material_instances.push(MaterialInstance {
        id: instance_id,
        program: MaterialProgramRef::Project(program_id),
        values: BTreeMap::new(),
        render_state: MaterialRenderState::additive_sprite(),
    });
    let effect_parameter = ParameterId::from_u128(0x5802);
    document.effect.parameters.push(EffectParameter {
        id: effect_parameter,
        name: "Effect intensity".into(),
        default: Value::Scalar(0.75),
        exposed: true,
    });
    let before = document.clone();
    let command = MaterialToolCommand::BindMaterialParameter {
        instance: instance_id,
        parameter: material_parameter,
        binding: MaterialParameterBinding::EffectParameter(effect_parameter),
    };

    let encoded = ron::to_string(&command).unwrap();
    assert_eq!(
        ron::from_str::<MaterialToolCommand>(&encoded).unwrap(),
        command
    );
    let plan = MaterialToolPlanner::plan(&document, command).unwrap();

    assert_eq!(document, before, "planning must not mutate its input");
    assert!(plan.created_expressions.is_empty());
    assert!(matches!(
        &plan.transaction.commands[..],
        [MaterialCommand::SetMaterialInstanceParameter {
            instance,
            parameter,
            value: Some(MaterialParameterValue::EffectParameter(binding)),
        }] if *instance == instance_id
            && *parameter == material_parameter
            && *binding == effect_parameter
    ));
    let parameter_change = plan
        .diff
        .changes
        .iter()
        .find(|change| {
            change
                .path
                .ends_with(&format!(".values[{material_parameter}]"))
        })
        .expect("binding diff must identify the exact instance parameter");
    assert_eq!(parameter_change.kind, MaterialChangeKind::Added);
    assert_eq!(
        parameter_change.target,
        MaterialSemanticTarget::Instance(instance_id)
    );

    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, plan.transaction).unwrap();
    assert_eq!(
        document.effect.material_instances[0].values[&material_parameter],
        MaterialParameterValue::EffectParameter(effect_parameter)
    );
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
}

#[test]
fn material_binding_tool_uses_an_explicit_program_default_source() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x5900);
    let (program, material_parameter) = parameterized_program(program_id);
    document.programs.push(program);
    let instance_id = MaterialId::from_u128(0x5901);
    document.effect.material_instances.push(MaterialInstance {
        id: instance_id,
        program: MaterialProgramRef::Project(program_id),
        values: BTreeMap::new(),
        render_state: MaterialRenderState::additive_sprite(),
    });
    let before = document.clone();
    let random_binding = MaterialParameterBinding::RandomRange {
        min: MaterialValue::Float(0.25),
        max: MaterialValue::Float(1.5),
        domain: MaterialEvaluationDomain::Effect,
    };
    let random_plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::BindMaterialParameter {
            instance: instance_id,
            parameter: material_parameter,
            binding: random_binding,
        },
    )
    .unwrap();
    let mut history = MaterialCommandHistory::default();
    history
        .execute(&mut document, random_plan.transaction)
        .unwrap();
    assert!(matches!(
        document.effect.material_instances[0]
            .values
            .get(&material_parameter),
        Some(MaterialParameterValue::RandomRange { min, max, domain })
            if *min == MaterialValue::Float(0.25)
                && *max == MaterialValue::Float(1.5)
                && *domain == MaterialEvaluationDomain::Effect
    ));
    let with_random_override = document.clone();

    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::BindMaterialParameter {
            instance: instance_id,
            parameter: material_parameter,
            binding: MaterialParameterBinding::ProgramDefault,
        },
    )
    .unwrap();

    assert!(matches!(
        &plan.transaction.commands[..],
        [MaterialCommand::SetMaterialInstanceParameter {
            instance,
            parameter,
            value: None,
        }] if *instance == instance_id && *parameter == material_parameter
    ));
    assert!(plan.diff.changes.iter().any(|change| {
        change.kind == MaterialChangeKind::Removed
            && change.target == MaterialSemanticTarget::Instance(instance_id)
            && change
                .path
                .ends_with(&format!(".values[{material_parameter}]"))
    }));

    history.execute(&mut document, plan.transaction).unwrap();
    assert!(
        !document.effect.material_instances[0]
            .values
            .contains_key(&material_parameter)
    );
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, with_random_override);
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
}

#[test]
fn material_binding_tool_rejects_stale_and_incompatible_bindings_atomically() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x5a00);
    let (program, material_parameter) = parameterized_program(program_id);
    document.programs.push(program);
    let instance_id = MaterialId::from_u128(0x5a01);
    document.effect.material_instances.push(MaterialInstance {
        id: instance_id,
        program: MaterialProgramRef::Project(program_id),
        values: BTreeMap::new(),
        render_state: MaterialRenderState::additive_sprite(),
    });
    let vector_parameter = ParameterId::from_u128(0x5a02);
    document.effect.parameters.push(EffectParameter {
        id: vector_parameter,
        name: "Wrong type".into(),
        default: Value::Vec2([1.0, 2.0]),
        exposed: true,
    });
    let scalar_parameter = ParameterId::from_u128(0x5a04);
    document.effect.parameters.push(EffectParameter {
        id: scalar_parameter,
        name: "Wrong source domain".into(),
        default: Value::Scalar(1.0),
        exposed: true,
    });
    let hidden_parameter = ParameterId::from_u128(0x5a03);
    document.effect.parameters.push(EffectParameter {
        id: hidden_parameter,
        name: "Internal scalar".into(),
        default: Value::Scalar(1.0),
        exposed: false,
    });
    let before = document.clone();
    let missing_instance = MaterialId::from_u128(0x5aff);
    let missing_material_parameter = MaterialParameterId::from_u128(0x5afe);
    let missing_binding = ParameterId::from_u128(0x5afd);

    let stale_instance = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::BindMaterialParameter {
            instance: missing_instance,
            parameter: material_parameter,
            binding: MaterialParameterBinding::Constant(MaterialValue::Float(1.0)),
        },
    )
    .unwrap_err();
    assert!(matches!(
        stale_instance,
        MaterialToolError::InstanceNotFound(instance) if instance == missing_instance
    ));

    let stale_material_parameter = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::BindMaterialParameter {
            instance: instance_id,
            parameter: missing_material_parameter,
            binding: MaterialParameterBinding::Constant(MaterialValue::Float(1.0)),
        },
    )
    .unwrap_err();
    assert!(matches!(
        stale_material_parameter,
        MaterialToolError::ParameterNotFound { program, parameter }
            if program == program_id && parameter == missing_material_parameter
    ));

    let stale_binding = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::BindMaterialParameter {
            instance: instance_id,
            parameter: material_parameter,
            binding: MaterialParameterBinding::EffectParameter(missing_binding),
        },
    )
    .unwrap_err();
    assert!(matches!(
        stale_binding,
        MaterialToolError::BindingParameterNotFound(parameter) if parameter == missing_binding
    ));

    let hidden_binding = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::BindMaterialParameter {
            instance: instance_id,
            parameter: material_parameter,
            binding: MaterialParameterBinding::EffectParameter(hidden_parameter),
        },
    )
    .unwrap_err();
    assert!(matches!(
        hidden_binding,
        MaterialToolError::BindingParameterNotExposed(parameter) if parameter == hidden_parameter
    ));

    for binding in [
        MaterialParameterBinding::EffectParameter(vector_parameter),
        MaterialParameterBinding::EmitterParameter(scalar_parameter),
        MaterialParameterBinding::Constant(MaterialValue::Vec2([1.0, 2.0])),
    ] {
        let incompatible = MaterialToolPlanner::plan(
            &document,
            MaterialToolCommand::BindMaterialParameter {
                instance: instance_id,
                parameter: material_parameter,
                binding,
            },
        )
        .unwrap_err();
        assert!(matches!(
            incompatible,
            MaterialToolError::Transaction(MaterialCommandError::Validation(_))
        ));
    }
    assert_eq!(document, before);
}

#[test]
fn material_preset_tool_plans_a_valid_reversible_semantic_transaction() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6000);
    let (program, _) = reorderable_material_program(program_id);
    document.programs.push(program);
    let before = document.clone();

    let command = MaterialToolCommand::ApplyMaterialPreset {
        program: program_id,
        preset: MATERIAL_PRESET_UV_DRIFT,
        placement: MaterialInsertionPoint::Start,
    };
    assert_eq!(
        ron::from_str::<MaterialToolCommand>(&ron::to_string(&command).unwrap()).unwrap(),
        command
    );
    let plan = MaterialToolPlanner::plan(&document, command).unwrap();

    assert_eq!(document, before, "planning must not mutate its input");
    assert_eq!(plan.created_expressions.len(), 2);
    assert!(plan.diff.changes.iter().any(|change| {
        change.kind == MaterialChangeKind::Modified
            && change.target == MaterialSemanticTarget::Program(program_id)
    }));
    assert!(plan.created_expressions.iter().all(|expression| {
        plan.diff.changes.iter().any(|change| {
            change.kind == MaterialChangeKind::Added
                && change.target == MaterialSemanticTarget::Expression(*expression)
        })
    }));
    let replacement = plan.replacement_program(program_id).unwrap().clone();

    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, plan.transaction).unwrap();
    assert_eq!(document.programs[0], replacement);
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
}

#[test]
fn material_preset_tool_rejects_incompatible_requests_without_mutation() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6100);
    let (program, _) = reorderable_material_program(program_id);
    document.programs.push(program);
    let before = document.clone();

    let error = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ApplyMaterialPreset {
            program: program_id,
            preset: MATERIAL_PRESET_SOFT_DISSOLVE,
            placement: MaterialInsertionPoint::Start,
        },
    )
    .unwrap_err();

    assert!(matches!(error, MaterialToolError::StackEdit(_)));
    assert_eq!(document, before);
}

#[test]
fn material_preset_tool_applies_a_project_catalog_recipe_transactionally() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6150);
    let (program, _) = reorderable_material_program(program_id);
    document.programs.push(program);
    let preset = MaterialPresetId::from_u128(0xA357_A301);
    let catalog = MaterialPresetCatalog::with_project_presets([MaterialPresetDescriptor {
        schema_version: MaterialPresetSchemaVersion::CURRENT,
        id: preset,
        display_name: "Hologram".into(),
        description: "Adds a graph-authored widened holographic UV band.".into(),
        category: MaterialPresetCategory::Shaping,
        tags: vec!["hologram".into()],
        recipe: MaterialPresetRecipe::Graph(MaterialPresetGraphRecipe {
            nodes: vec![
                MaterialPresetGraphNode {
                    name: "width".into(),
                    kind: MaterialPresetGraphNodeKind::Constant(MaterialValue::Vec2([1.8, 1.0])),
                    inputs: BTreeMap::new(),
                },
                MaterialPresetGraphNode {
                    name: "scaled".into(),
                    kind: MaterialPresetGraphNodeKind::Function(MaterialGraphFunction::Multiply),
                    inputs: BTreeMap::from([
                        ("A".into(), MaterialPresetValueRef::Source),
                        ("B".into(), MaterialPresetValueRef::Node("width".into())),
                    ]),
                },
            ],
            output: MaterialPresetValueRef::Node("scaled".into()),
            program_outputs: BTreeMap::new(),
        }),
    }])
    .unwrap();

    let plan = MaterialToolPlanner::plan_with_preset_catalog(
        &document,
        MaterialToolCommand::ApplyMaterialPreset {
            program: program_id,
            preset,
            placement: MaterialInsertionPoint::Start,
        },
        &catalog,
    )
    .unwrap();

    assert_eq!(plan.created_expressions.len(), 2);
    let replacement = plan.replacement_program(program_id).unwrap().clone();
    assert!(replacement.analyze().is_ok());
    assert!(replacement.expressions.iter().any(|expression| {
        expression.id == plan.created_expressions[1]
            && matches!(expression.kind, MaterialExpressionKind::Multiply(_, _))
    }));

    let before = document.clone();
    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, plan.transaction).unwrap();
    assert!(history.undo(&mut document).unwrap().is_some());
    assert_eq!(document, before);
}

#[test]
fn curated_project_material_presets_apply_and_undo_as_one_transaction() {
    let presets = [
        include_str!("../../../assets/materials/additive_flame.aestra.material-preset.ron"),
        include_str!("../../../assets/materials/soft_smoke.aestra.material-preset.ron"),
        include_str!("../../../assets/materials/energy_beam.aestra.material-preset.ron"),
        include_str!("../../../assets/materials/magic_shield.aestra.material-preset.ron"),
        include_str!("../../../assets/materials/hologram.aestra.material-preset.ron"),
        include_str!("../../../assets/materials/ghost.aestra.material-preset.ron"),
        include_str!("../../../assets/materials/portal.aestra.material-preset.ron"),
        include_str!("../../../assets/materials/impact_flash.aestra.material-preset.ron"),
    ]
    .map(|source| MaterialPresetDescriptor::from_ron(source).unwrap());
    let preset_ids = presets.each_ref().map(|preset| preset.id);
    let catalog = MaterialPresetCatalog::with_project_presets(presets).unwrap();

    for (index, preset) in preset_ids.into_iter().enumerate() {
        let mut document = authoring_document();
        let program_id = MaterialProgramId::from_u128(0xA357_B000 + index as u128);
        document
            .programs
            .push(preset_host_material_program(program_id));
        let before = document.clone();
        let plan = MaterialToolPlanner::plan_with_preset_catalog(
            &document,
            MaterialToolCommand::ApplyMaterialPreset {
                program: program_id,
                preset,
                placement: MaterialInsertionPoint::End,
            },
            &catalog,
        )
        .unwrap_or_else(|error| {
            panic!(
                "{} should produce one valid authoring transaction: {error}",
                catalog.get(preset).unwrap().display_name
            )
        });
        let replacement = plan.replacement_program(program_id).unwrap().clone();
        let mut history = MaterialCommandHistory::default();

        history.execute(&mut document, plan.transaction).unwrap();
        assert_eq!(document.programs[0], replacement);
        history.undo(&mut document).unwrap().unwrap();
        assert_eq!(document, before);
        history.redo(&mut document).unwrap().unwrap();
        assert_eq!(document.programs[0], replacement);
    }
}

#[test]
fn material_operation_tool_uses_stable_placement_and_exact_undo() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6200);
    let (program, pan) = reorderable_material_program(program_id);
    document.programs.push(program);
    let before = document.clone();
    let command = MaterialToolCommand::InsertMaterialOperation {
        program: program_id,
        kind: MaterialStackModifierKind::ScaleUv,
        placement: MaterialInsertionPoint::After(pan),
    };

    let encoded = ron::to_string(&command).unwrap();
    assert_eq!(
        ron::from_str::<MaterialToolCommand>(&encoded).unwrap(),
        command
    );
    let plan = MaterialToolPlanner::plan(&document, command).unwrap();

    assert_eq!(document, before, "planning must not mutate its input");
    assert_eq!(plan.created_expressions.len(), 1);
    let replacement = plan.replacement_program(program_id).unwrap().clone();
    let MaterialStackProjection::Stack { entries } =
        MaterialCompiler.project_stack(&replacement).unwrap()
    else {
        panic!("inserted operation must remain an editable stack");
    };
    assert_eq!(entries[0].expression, pan);
    assert_eq!(entries[1].expression, plan.created_expressions[0]);
    assert_eq!(entries[1].kind, MaterialStackModifierKind::ScaleUv);
    assert!(plan.diff.changes.iter().any(|change| {
        change.kind == MaterialChangeKind::Added
            && change.target == MaterialSemanticTarget::Expression(plan.created_expressions[0])
    }));

    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, plan.transaction).unwrap();
    assert_eq!(document.programs[0], replacement);
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
}

#[test]
fn material_operation_tool_rejects_a_stale_insertion_anchor() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6300);
    let (program, _) = reorderable_material_program(program_id);
    document.programs.push(program);
    let before = document.clone();
    let missing = MaterialExpressionId::from_u128(0x63ff);

    let error = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::InsertMaterialOperation {
            program: program_id,
            kind: MaterialStackModifierKind::ScaleUv,
            placement: MaterialInsertionPoint::Before(missing),
        },
    )
    .unwrap_err();

    assert!(matches!(
        error,
        MaterialToolError::InsertionAnchorNotFound(expression) if expression == missing
    ));
    assert_eq!(document, before);
}

#[test]
fn material_connection_tool_rewires_a_stable_input_and_undoes_exactly() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6400);
    let (program, _) = reorderable_material_program(program_id);
    document.programs.push(program);
    let before = document.clone();
    let time = MaterialExpressionId::from_u128(0x1182);
    let rotate = MaterialExpressionId::from_u128(0x1186);
    let command = MaterialToolCommand::ConnectMaterialExpression {
        program: program_id,
        source: time,
        target: MaterialConnectionTarget::ExpressionInput {
            expression: rotate,
            input: MaterialExpressionInput::Angle,
        },
    };

    let encoded = ron::to_string(&command).unwrap();
    assert_eq!(
        ron::from_str::<MaterialToolCommand>(&encoded).unwrap(),
        command
    );
    let plan = MaterialToolPlanner::plan(&document, command).unwrap();

    assert_eq!(document, before, "planning must not mutate its input");
    assert!(plan.created_expressions.is_empty());
    assert!(plan.diff.changes.iter().any(|change| {
        change.kind == MaterialChangeKind::Modified
            && change.target == MaterialSemanticTarget::Expression(rotate)
    }));
    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, plan.transaction).unwrap();
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == rotate)
            .unwrap()
            .kind,
        MaterialExpressionKind::RotateUv { angle, .. } if angle == time
    ));
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
}

#[test]
fn material_connection_tool_reports_the_exact_program_output_diff() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6500);
    let (program, _) = reorderable_material_program(program_id);
    document.programs.push(program);
    let before = document.clone();
    let previous = document.programs[0].outputs.alpha;
    let angle = MaterialExpressionId::from_u128(0x1185);

    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ConnectMaterialExpression {
            program: program_id,
            source: angle,
            target: MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha),
        },
    )
    .unwrap();

    let output_change = plan
        .diff
        .changes
        .iter()
        .find(|change| change.path.ends_with(".outputs.alpha"))
        .expect("output connection must identify its exact semantic path");
    assert_eq!(
        output_change.target,
        MaterialSemanticTarget::Program(program_id)
    );
    assert_eq!(
        output_change.before.as_deref(),
        Some(previous.to_string().as_str())
    );
    assert_eq!(
        output_change.after.as_deref(),
        Some(angle.to_string().as_str())
    );
    assert_eq!(document, before);
}

#[test]
fn material_connection_tool_rejects_stale_and_incompatible_sockets_atomically() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6600);
    let (program, pan) = reorderable_material_program(program_id);
    document.programs.push(program);
    let before = document.clone();
    let rotate = MaterialExpressionId::from_u128(0x1186);
    let angle = MaterialExpressionId::from_u128(0x1185);
    let missing = MaterialExpressionId::from_u128(0x66ff);

    let stale_source = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ConnectMaterialExpression {
            program: program_id,
            source: missing,
            target: MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha),
        },
    )
    .unwrap_err();
    assert!(matches!(
        stale_source,
        MaterialToolError::SourceExpressionNotFound(expression) if expression == missing
    ));

    let stale_destination = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ConnectMaterialExpression {
            program: program_id,
            source: angle,
            target: MaterialConnectionTarget::ExpressionInput {
                expression: missing,
                input: MaterialExpressionInput::Angle,
            },
        },
    )
    .unwrap_err();
    assert!(matches!(
        stale_destination,
        MaterialToolError::DestinationExpressionNotFound(expression) if expression == missing
    ));

    let missing_socket = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ConnectMaterialExpression {
            program: program_id,
            source: angle,
            target: MaterialConnectionTarget::ExpressionInput {
                expression: pan,
                input: MaterialExpressionInput::Angle,
            },
        },
    )
    .unwrap_err();
    assert!(matches!(
        missing_socket,
        MaterialToolError::Transaction(MaterialCommandError::InvalidExpressionInput {
            input: MaterialExpressionInput::Angle
        })
    ));

    let wrong_type = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ConnectMaterialExpression {
            program: program_id,
            source: pan,
            target: MaterialConnectionTarget::ExpressionInput {
                expression: rotate,
                input: MaterialExpressionInput::Angle,
            },
        },
    )
    .unwrap_err();
    assert!(matches!(
        wrong_type,
        MaterialToolError::Transaction(MaterialCommandError::Validation(_))
    ));
    assert_eq!(document, before);
}

#[test]
fn material_replace_tool_preserves_identity_connections_and_exact_undo() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6680);
    let (program, pan) = reorderable_material_program(program_id);
    document.programs.push(program);
    let before = document.clone();
    let uv = MaterialExpressionId::from_u128(0x1180);
    let center = MaterialExpressionId::from_u128(0x1184);
    let angle = MaterialExpressionId::from_u128(0x1185);
    let rotate = MaterialExpressionId::from_u128(0x1186);
    let replacement = MaterialExpressionKind::RotateUv { uv, center, angle };
    let command = MaterialToolCommand::ReplaceMaterialExpression {
        program: program_id,
        expression: pan,
        replacement: replacement.clone(),
    };

    let encoded = ron::to_string(&command).unwrap();
    assert_eq!(
        ron::from_str::<MaterialToolCommand>(&encoded).unwrap(),
        command
    );
    let plan = MaterialToolPlanner::plan(&document, command).unwrap();

    assert_eq!(document, before, "planning must not mutate its input");
    assert!(plan.created_expressions.is_empty());
    assert_eq!(plan.transaction.commands.len(), 1);
    assert!(matches!(
        &plan.transaction.commands[0],
        MaterialCommand::ReplaceMaterialExpression {
            program,
            expression,
            replacement: MaterialExpression { id, kind },
        } if *program == program_id
            && *expression == pan
            && *id == pan
            && *kind == replacement
    ));
    assert!(plan.diff.changes.iter().any(|change| {
        change.kind == MaterialChangeKind::Modified
            && change.target == MaterialSemanticTarget::Expression(pan)
            && change.path.ends_with(&format!(".expressions[{pan}]"))
    }));

    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, plan.transaction).unwrap();
    let program = &document.programs[0];
    assert!(matches!(
        program
            .expressions
            .iter()
            .find(|expression| expression.id == pan)
            .unwrap()
            .kind,
        MaterialExpressionKind::RotateUv {
            uv: source,
            center: replacement_center,
            angle: replacement_angle,
        } if source == uv && replacement_center == center && replacement_angle == angle
    ));
    assert!(matches!(
        program
            .expressions
            .iter()
            .find(|expression| expression.id == rotate)
            .unwrap()
            .kind,
        MaterialExpressionKind::RotateUv { uv: source, .. } if source == pan
    ));
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
    history.redo(&mut document).unwrap().unwrap();
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == pan)
            .unwrap()
            .kind,
        MaterialExpressionKind::RotateUv { .. }
    ));
}

#[test]
fn material_replace_tool_rejects_stale_and_incompatible_replacements_atomically() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6690);
    let (program, pan) = reorderable_material_program(program_id);
    document.programs.push(program);
    let before = document.clone();
    let missing = MaterialExpressionId::from_u128(0x669f);

    let stale = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ReplaceMaterialExpression {
            program: program_id,
            expression: missing,
            replacement: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
    )
    .unwrap_err();
    assert!(matches!(
        stale,
        MaterialToolError::DestinationExpressionNotFound(expression) if expression == missing
    ));

    let wrong_type = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ReplaceMaterialExpression {
            program: program_id,
            expression: pan,
            replacement: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
    )
    .unwrap_err();
    assert!(matches!(
        wrong_type,
        MaterialToolError::Transaction(MaterialCommandError::Validation(_))
    ));

    let stale_dependency = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ReplaceMaterialExpression {
            program: program_id,
            expression: pan,
            replacement: MaterialExpressionKind::Add(missing, missing),
        },
    )
    .unwrap_err();
    assert!(matches!(
        stale_dependency,
        MaterialToolError::Transaction(MaterialCommandError::Validation(_))
    ));
    assert_eq!(document, before);
}

#[test]
fn material_wrap_tool_wraps_an_exact_input_edge_and_undoes_as_one_edit() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6700);
    let (program, _) = reorderable_material_program(program_id);
    document.programs.push(program);
    let before = document.clone();
    let rotate = MaterialExpressionId::from_u128(0x1186);
    let sample = MaterialExpressionId::from_u128(0x1189);
    let target = MaterialConnectionTarget::ExpressionInput {
        expression: sample,
        input: MaterialExpressionInput::Uv,
    };
    let command = MaterialToolCommand::WrapMaterialExpression {
        program: program_id,
        target,
        kind: MaterialStackModifierKind::ScaleUv,
    };

    let encoded = ron::to_string(&command).unwrap();
    assert_eq!(
        ron::from_str::<MaterialToolCommand>(&encoded).unwrap(),
        command
    );
    let plan = MaterialToolPlanner::plan(&document, command).unwrap();

    assert_eq!(document, before, "planning must not mutate its input");
    assert_eq!(plan.created_expressions.len(), 1);
    let wrapper = plan.created_expressions[0];
    assert!(plan.transaction.commands.len() > 1);
    assert!(
        !plan
            .transaction
            .commands
            .iter()
            .any(|command| matches!(command, MaterialCommand::ReplaceMaterialProgram { .. }))
    );
    let mut preview = document.clone();
    MaterialCommandExecutor::execute(&mut preview, &plan.transaction).unwrap();
    let replacement = preview.programs[0].clone();
    assert!(matches!(
        replacement
            .expressions
            .iter()
            .find(|expression| expression.id == wrapper)
            .unwrap()
            .kind,
        MaterialExpressionKind::ScaleUv { uv, .. } if uv == rotate
    ));
    assert!(matches!(
        replacement
            .expressions
            .iter()
            .find(|expression| expression.id == sample)
            .unwrap()
            .kind,
        MaterialExpressionKind::SampleTexture { uv, .. } if uv == wrapper
    ));
    assert!(plan.diff.changes.iter().any(|change| {
        change.kind == MaterialChangeKind::Added
            && change.target == MaterialSemanticTarget::Expression(wrapper)
    }));
    assert!(plan.diff.changes.iter().any(|change| {
        change.kind == MaterialChangeKind::Modified
            && change.target == MaterialSemanticTarget::Expression(sample)
    }));

    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, plan.transaction).unwrap();
    assert_eq!(document.programs[0], replacement);
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
}

#[test]
fn material_wrap_tool_wraps_an_exact_program_output() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6800);
    let (mut program, _) = reorderable_material_program(program_id);
    let alpha = MaterialExpressionId::from_u128(0x118a);
    let angle = MaterialExpressionId::from_u128(0x1185);
    let sample = MaterialExpressionId::from_u128(0x1189);
    program.outputs.alpha = angle;
    program
        .expressions
        .retain(|expression| expression.id != alpha);
    document.programs.push(program);
    let before = document.clone();

    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::WrapMaterialExpression {
            program: program_id,
            target: MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color),
            kind: MaterialStackModifierKind::Remap,
        },
    )
    .unwrap();

    let wrapper = plan.created_expressions[0];
    let mut preview = document.clone();
    MaterialCommandExecutor::execute(&mut preview, &plan.transaction).unwrap();
    let replacement = &preview.programs[0];
    assert_eq!(replacement.outputs.color, wrapper);
    assert!(matches!(
        replacement
            .expressions
            .iter()
            .find(|expression| expression.id == wrapper)
            .unwrap()
            .kind,
        MaterialExpressionKind::Remap { value, .. } if value == sample
    ));
    assert!(
        plan.diff
            .changes
            .iter()
            .any(|change| change.path.ends_with(".outputs.color"))
    );
    assert_eq!(document, before);
}

#[test]
fn material_create_expression_tool_builds_a_typed_downstream_node_without_rewiring() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6850);
    let (program, _) = reorderable_material_program(program_id);
    let source = program.outputs.color;
    let original_outputs = program.outputs;
    document.programs.push(program);
    let before = document.clone();
    let command = MaterialToolCommand::CreateMaterialExpression {
        program: program_id,
        source,
        kind: MaterialStackModifierKind::Remap,
    };

    let encoded = ron::to_string(&command).unwrap();
    assert_eq!(
        ron::from_str::<MaterialToolCommand>(&encoded).unwrap(),
        command
    );
    let plan = MaterialToolPlanner::plan(&document, command).unwrap();
    assert_eq!(document, before, "planning must not mutate its input");
    assert_eq!(plan.created_expressions.len(), 1);
    assert!(
        !plan
            .transaction
            .commands
            .iter()
            .any(|command| matches!(command, MaterialCommand::ReplaceMaterialProgram { .. }))
    );

    let created = plan.created_expressions[0];
    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, plan.transaction).unwrap();
    let replacement = &document.programs[0];
    assert_eq!(replacement.outputs, original_outputs);
    assert!(matches!(
        replacement
            .expressions
            .iter()
            .find(|expression| expression.id == created)
            .unwrap()
            .kind,
        MaterialExpressionKind::Remap { value, .. } if value == source
    ));
    assert!(
        replacement
            .validation_report()
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == aestra_core::DiagnosticCode::UnreachableExpression)
    );
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
}

#[test]
fn material_graph_node_tool_creates_connects_and_undoes_one_semantic_edit() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6860);
    let (program, _) = reorderable_material_program(program_id);
    let source = program.outputs.alpha;
    document.programs.push(program);
    let before = document.clone();
    let target = MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha);
    let command = MaterialToolCommand::CreateMaterialGraphNode {
        program: program_id,
        kind: MaterialGraphCreateKind::Function(MaterialGraphFunction::Multiply),
        source: Some(source),
        target: Some(target),
    };

    let encoded = ron::to_string(&command).unwrap();
    assert_eq!(
        ron::from_str::<MaterialToolCommand>(&encoded).unwrap(),
        command
    );
    let plan = MaterialToolPlanner::plan(&document, command).unwrap();
    assert_eq!(document, before);
    assert_eq!(plan.created_expressions.len(), 1);
    assert!(plan.transaction.commands.iter().any(|command| matches!(
        command,
        MaterialCommand::SetMaterialOutput {
            output: MaterialOutputSocket::Alpha,
            ..
        }
    )));
    assert!(plan.transaction.commands.iter().any(|command| matches!(
        command,
        MaterialCommand::SetMaterialExpressionInline { inline: true, .. }
    )));

    let created = plan.created_expressions[0];
    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, plan.transaction).unwrap();
    assert_eq!(document.programs[0].outputs.alpha, created);
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == created)
            .map(|expression| &expression.kind),
        Some(MaterialExpressionKind::Multiply(left, _)) if *left == source
    ));
    let inline = match document.programs[0]
        .expressions
        .iter()
        .find(|expression| expression.id == created)
        .map(|expression| &expression.kind)
    {
        Some(MaterialExpressionKind::Multiply(_, right)) => *right,
        _ => unreachable!("created expression was asserted as multiply"),
    };
    assert!(document.programs[0].inline_constants.contains(&inline));
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
}

#[test]
fn project_function_call_creation_and_signature_rewiring_are_transactional() {
    let function = MaterialFunction::from_ron(include_str!(
        "../../../assets/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap();
    let mut document = authoring_document();
    let mut program = MaterialProgram::additive_sprite("Function authoring");
    program.id = MaterialProgramId::from_u128(0x68f0);
    let source = program.outputs.alpha;
    let output = function.outputs[0].id;
    let source_input = function.inputs[0].id;
    let threshold_input = function.inputs[1].id;
    document.programs.push(program);
    document.material_functions.push(function.clone());

    let create = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::CreateMaterialGraphNode {
            program: MaterialProgramId::from_u128(0x68f0),
            kind: MaterialGraphCreateKind::FunctionCall {
                function: MaterialFunctionRef::Project(function.id),
                output,
            },
            source: Some(source),
            target: Some(MaterialConnectionTarget::ProgramOutput(
                MaterialOutputSocket::Alpha,
            )),
        },
    )
    .unwrap();
    let call = create.created_expressions[0];
    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, create.transaction).unwrap();
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == call)
            .map(|expression| &expression.kind),
        Some(MaterialExpressionKind::FunctionCall { arguments, .. })
            if arguments[&source_input] == source
    ));

    let rewire = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ConnectMaterialExpression {
            program: MaterialProgramId::from_u128(0x68f0),
            source,
            target: MaterialConnectionTarget::ExpressionInput {
                expression: call,
                input: MaterialExpressionInput::FunctionArgument(threshold_input),
            },
        },
    )
    .unwrap();
    let before_rewire = document.clone();
    history.execute(&mut document, rewire.transaction).unwrap();
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == call)
            .map(|expression| &expression.kind),
        Some(MaterialExpressionKind::FunctionCall { arguments, .. })
            if arguments[&threshold_input] == source
    ));
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before_rewire);
}

#[test]
fn connected_subgraph_extraction_creates_a_function_and_replaces_it_atomically() {
    let mut document = authoring_document();
    let mut program = MaterialProgram::additive_sprite("Extract function");
    program.id = MaterialProgramId::from_u128(0x68f1);
    let original_alpha = program.outputs.alpha;
    let lower = MaterialExpressionId::from_u128(0x68f2);
    let upper = MaterialExpressionId::from_u128(0x68f3);
    let smoothstep = MaterialExpressionId::from_u128(0x68f4);
    program.expressions.extend([
        MaterialExpression {
            id: lower,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.1)),
        },
        MaterialExpression {
            id: upper,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.9)),
        },
        MaterialExpression {
            id: smoothstep,
            kind: MaterialExpressionKind::Smoothstep {
                edge_min: lower,
                edge_max: upper,
                value: original_alpha,
            },
        },
    ]);
    program.inline_constants.extend([lower, upper]);
    program.outputs.alpha = smoothstep;
    document.programs.push(program);
    let before = document.clone();
    let function_id = MaterialFunctionId::from_u128(0x68f5);
    let command = MaterialToolCommand::ExtractMaterialFunction {
        program: MaterialProgramId::from_u128(0x68f1),
        function: function_id,
        name: "Soft Threshold".into(),
        expressions: vec![smoothstep],
    };

    assert_eq!(
        ron::from_str::<MaterialToolCommand>(&ron::to_string(&command).unwrap()).unwrap(),
        command
    );
    let plan = MaterialToolPlanner::plan(&document, command).unwrap();
    let function = plan.created_function().unwrap();
    assert_eq!(function.name, "Soft Threshold");
    assert_eq!(function.inputs.len(), 1);
    assert_eq!(function.outputs.len(), 1);
    assert!(function.expressions.iter().any(|expression| {
        expression.id == smoothstep
            && matches!(expression.kind, MaterialExpressionKind::Smoothstep { .. })
    }));
    let call = plan.created_expressions[0];
    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, plan.transaction).unwrap();
    assert_eq!(document.material_functions[0].id, function_id);
    assert_eq!(document.programs[0].outputs.alpha, call);
    assert!(
        !document.programs[0]
            .expressions
            .iter()
            .any(|expression| expression.id == smoothstep)
    );
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == call)
            .map(|expression| &expression.kind),
        Some(MaterialExpressionKind::FunctionCall {
            function: MaterialFunctionRef::Project(id),
            ..
        }) if *id == function_id
    ));
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
}

#[test]
fn extraction_rejects_disconnected_node_selections() {
    let mut document = authoring_document();
    let mut program = MaterialProgram::additive_sprite("Disconnected extraction");
    program.id = MaterialProgramId::from_u128(0x68f6);
    let expressions = vec![program.outputs.color, program.outputs.alpha];
    document.programs.push(program);
    let error = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ExtractMaterialFunction {
            program: MaterialProgramId::from_u128(0x68f6),
            function: MaterialFunctionId::from_u128(0x68f7),
            name: "Invalid".into(),
            expressions,
        },
    )
    .unwrap_err();
    assert!(matches!(
        error,
        MaterialToolError::DisconnectedFunctionSelection
    ));
}

#[test]
fn material_wrap_tool_handles_fanout_and_rejects_incompatible_or_stale_targets_atomically() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6900);
    let (program, pan) = reorderable_material_program(program_id);
    document.programs.push(program);
    let before = document.clone();
    let rotate = MaterialExpressionId::from_u128(0x1186);
    let missing = MaterialExpressionId::from_u128(0x69ff);

    let output_target = MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color);
    let fanout_plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::WrapMaterialExpression {
            program: program_id,
            target: output_target,
            kind: MaterialStackModifierKind::Remap,
        },
    )
    .expect("an exact output edge remains safe to wrap when its source has other consumers");
    assert_eq!(fanout_plan.created_expressions.len(), 1);

    let target = MaterialConnectionTarget::ExpressionInput {
        expression: rotate,
        input: MaterialExpressionInput::Angle,
    };
    let kind = MaterialStackModifierKind::ScaleUv;
    let error = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::WrapMaterialExpression {
            program: program_id,
            target,
            kind,
        },
    )
    .unwrap_err();
    assert!(matches!(
        error,
        MaterialToolError::IncompatibleWrap {
            kind: rejected_kind,
            target: rejected_target,
        } if rejected_kind == kind && rejected_target == target
    ));

    let stale = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::WrapMaterialExpression {
            program: program_id,
            target: MaterialConnectionTarget::ExpressionInput {
                expression: missing,
                input: MaterialExpressionInput::Uv,
            },
            kind: MaterialStackModifierKind::ScaleUv,
        },
    )
    .unwrap_err();
    assert!(matches!(
        stale,
        MaterialToolError::DestinationExpressionNotFound(expression) if expression == missing
    ));
    assert!(
        document.programs[0]
            .expressions
            .iter()
            .any(|expression| expression.id == pan)
    );
    assert_eq!(document, before);
}

#[test]
fn material_graph_duplicate_preserves_internal_selection_connections() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6a00);
    let (program, pan) = reorderable_material_program(program_id);
    let rotate = MaterialExpressionId::from_u128(0x1186);
    document.programs.push(program);

    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::DuplicateMaterialExpressions {
            program: program_id,
            expressions: vec![rotate, pan],
        },
    )
    .unwrap();
    assert_eq!(plan.created_expressions.len(), 2);
    let duplicate_pan = plan.created_expressions[0];
    let duplicate_rotate = plan.created_expressions[1];
    let mut preview = document.clone();
    MaterialCommandExecutor::execute(&mut preview, &plan.transaction).unwrap();
    assert!(matches!(
        preview.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == duplicate_rotate)
            .unwrap()
            .kind,
        MaterialExpressionKind::RotateUv { uv, .. } if uv == duplicate_pan
    ));
    assert!(MaterialCompiler.compile(&preview.programs[0]).is_ok());
}

#[test]
fn material_graph_delete_bypasses_safe_nodes_and_defaults_required_inputs() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6a10);
    let (program, _) = reorderable_material_program(program_id);
    let pan = MaterialExpressionId::from_u128(0x1183);
    let angle = MaterialExpressionId::from_u128(0x1185);
    let rotate = MaterialExpressionId::from_u128(0x1186);
    let sample = MaterialExpressionId::from_u128(0x1189);
    document.programs.push(program);

    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::DeleteMaterialExpressions {
            program: program_id,
            expressions: vec![rotate],
        },
    )
    .unwrap();
    let mut preview = document.clone();
    MaterialCommandExecutor::execute(&mut preview, &plan.transaction).unwrap();
    assert!(
        !preview.programs[0]
            .expressions
            .iter()
            .any(|expression| expression.id == rotate)
    );
    assert!(matches!(
        preview.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == sample)
            .unwrap()
            .kind,
        MaterialExpressionKind::SampleTexture { uv, .. } if uv == pan
    ));

    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::DeleteMaterialExpressions {
            program: program_id,
            expressions: vec![angle],
        },
    )
    .unwrap();
    assert_eq!(plan.created_expressions.len(), 1);
    let default = plan.created_expressions[0];
    let mut preview = document.clone();
    MaterialCommandExecutor::execute(&mut preview, &plan.transaction).unwrap();
    assert!(matches!(
        preview.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == default)
            .unwrap()
            .kind,
        MaterialExpressionKind::Constant(MaterialValue::Float(0.0))
    ));
    assert!(matches!(
        preview.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == rotate)
            .unwrap()
            .kind,
        MaterialExpressionKind::RotateUv { angle, .. } if angle == default
    ));
}

#[test]
fn material_graph_disconnect_replaces_the_edge_with_a_typed_default() {
    let mut document = authoring_document();
    let program_id = MaterialProgramId::from_u128(0x6a20);
    let (program, _) = reorderable_material_program(program_id);
    let angle = MaterialExpressionId::from_u128(0x1185);
    let rotate = MaterialExpressionId::from_u128(0x1186);
    document.programs.push(program);
    let target = MaterialConnectionTarget::ExpressionInput {
        expression: rotate,
        input: MaterialExpressionInput::Angle,
    };

    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::DisconnectMaterialConnection {
            program: program_id,
            target,
        },
    )
    .unwrap();
    assert_eq!(plan.created_expressions.len(), 1);
    let default = plan.created_expressions[0];
    let mut preview = document.clone();
    MaterialCommandExecutor::execute(&mut preview, &plan.transaction).unwrap();
    assert_ne!(default, angle);
    assert!(matches!(
        preview.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == default)
            .unwrap()
            .kind,
        MaterialExpressionKind::Constant(MaterialValue::Float(0.0))
    ));
    assert!(matches!(
        preview.programs[0]
            .expressions
            .iter()
            .find(|expression| expression.id == rotate)
            .unwrap()
            .kind,
        MaterialExpressionKind::RotateUv { angle, .. } if angle == default
    ));
}
