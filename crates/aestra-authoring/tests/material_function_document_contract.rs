use aestra_authoring::{
    MaterialAuthoringDocument, MaterialCommand, MaterialCommandError, MaterialCommandHistory,
    MaterialTransaction,
};
use aestra_core::{
    MaterialExpressionId, MaterialFunctionId, MaterialFunctionInputId, MaterialFunctionOutputId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialFunctionInput,
        MaterialFunctionOutput, MaterialFunctionRef, MaterialProgram, MaterialSchemaVersion,
        MaterialValue, MaterialValueType,
    },
};
use std::collections::BTreeMap;

fn function() -> MaterialFunction {
    let input = MaterialFunctionInputId::new();
    let expression = MaterialExpressionId::new();
    MaterialFunction {
        id: MaterialFunctionId::new(),
        schema_version: MaterialSchemaVersion::CURRENT,
        name: "Identity".into(),
        inputs: vec![MaterialFunctionInput {
            id: input,
            default: None,
            name: "Value".into(),
            value_type: MaterialValueType::Float,
        }],
        outputs: vec![MaterialFunctionOutput {
            id: MaterialFunctionOutputId::new(),
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            expression,
        }],
        expressions: vec![MaterialExpression {
            id: expression,
            kind: MaterialExpressionKind::FunctionInput(input),
        }],
        custom_wesl: None,
    }
}

fn replacement(function: MaterialFunction) -> MaterialTransaction {
    MaterialTransaction::single(
        "Edit function",
        MaterialCommand::ReplaceMaterialFunction {
            id: function.id,
            function,
        },
    )
}

#[test]
fn body_commands_are_atomic_and_round_trip_through_history() {
    use aestra_authoring::{MaterialExpressionInput, MaterialFunctionBodyCommand as B};
    let function = function();
    let id = function.id;
    let input = function.expressions[0].id;
    let output = function.outputs[0].id;
    let constant = MaterialExpressionId::new();
    let sum = MaterialExpressionId::new();
    let command = |edit| MaterialCommand::EditMaterialFunctionBody { function: id, edit };
    let mut document =
        MaterialAuthoringDocument::standalone(vec![]).with_material_functions([function]);
    let before = document.clone();
    let mut history = MaterialCommandHistory::default();
    history
        .execute(
            &mut document,
            MaterialTransaction::new(
                "Build body",
                vec![
                    command(B::Add {
                        expression: MaterialExpression {
                            id: constant,
                            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
                        },
                        index: 1,
                    }),
                    command(B::Add {
                        expression: MaterialExpression {
                            id: sum,
                            kind: MaterialExpressionKind::Add(input, constant),
                        },
                        index: 2,
                    }),
                    command(B::SetOutput {
                        output,
                        source: sum,
                    }),
                ],
            ),
        )
        .unwrap();
    let built = document.clone();
    // Connected deletion must not erase incoming edges implicitly.
    assert!(
        history
            .execute(
                &mut document,
                MaterialTransaction::single(
                    "Unsafe delete",
                    command(B::Remove {
                        expression: constant
                    })
                )
            )
            .is_err()
    );
    assert_eq!(document, built);
    history
        .execute(
            &mut document,
            MaterialTransaction::new(
                "Rewire then remove",
                vec![
                    command(B::Rewire {
                        expression: sum,
                        input: MaterialExpressionInput::Right,
                        source: input,
                    }),
                    command(B::Remove {
                        expression: constant,
                    }),
                    command(B::Replace {
                        expression: sum,
                        replacement: MaterialExpression {
                            id: sum,
                            kind: MaterialExpressionKind::Multiply(input, input),
                        },
                    }),
                ],
            ),
        )
        .unwrap();
    let edited = document.clone();
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, built);
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
    history.redo(&mut document).unwrap().unwrap();
    history.redo(&mut document).unwrap().unwrap();
    assert_eq!(document, edited);
}

#[test]
fn body_argument_disconnect_uses_default_and_undo_restores_connection() {
    use aestra_authoring::MaterialFunctionBodyCommand as B;
    let mut dependency = function();
    dependency.inputs[0].default = Some(MaterialValue::Float(0.25));
    let mut caller = function();
    let call = MaterialExpressionId::new();
    let source = caller.expressions[0].id;
    caller.expressions.push(MaterialExpression {
        id: call,
        kind: MaterialExpressionKind::FunctionCall {
            function: MaterialFunctionRef::Project(dependency.id),
            arguments: BTreeMap::from([(dependency.inputs[0].id, source)]),
            output: dependency.outputs[0].id,
        },
    });
    caller.outputs[0].expression = call;
    let command = |source| {
        MaterialTransaction::single(
            "Argument",
            MaterialCommand::EditMaterialFunctionBody {
                function: caller.id,
                edit: B::SetArgument {
                    expression: call,
                    input: dependency.inputs[0].id,
                    source,
                },
            },
        )
    };
    let mut document = MaterialAuthoringDocument::standalone(vec![])
        .with_material_functions([dependency.clone(), caller.clone()]);
    let before = document.clone();
    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, command(None)).unwrap();
    let disconnected = document.clone();
    history
        .execute(&mut document, command(Some(source)))
        .unwrap();
    assert_eq!(document, before);
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, disconnected);
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
    document.material_functions[0].inputs[0].default = None;
    let required = document.clone();
    assert!(history.execute(&mut document, command(None)).is_err());
    assert_eq!(document, required);
}

#[test]
fn graph_body_command_rejects_custom_wesl_without_touching_source() {
    use aestra_authoring::MaterialFunctionBodyCommand as B;
    let function = MaterialFunction::from_ron(include_str!(
        "../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    let mut document =
        MaterialAuthoringDocument::standalone(vec![]).with_material_functions([function.clone()]);
    let before = document.clone();
    let result = MaterialCommandHistory::default().execute(
        &mut document,
        MaterialTransaction::single(
            "Invalid graph edit",
            MaterialCommand::EditMaterialFunctionBody {
                function: function.id,
                edit: B::SetOutput {
                    output: function.outputs[0].id,
                    source: MaterialExpressionId::new(),
                },
            },
        ),
    );
    assert!(matches!(
        result,
        Err(MaterialCommandError::CustomWeslBodyReadOnly)
    ));
    assert_eq!(document, before);
}

#[test]
fn invalid_body_edits_preserve_document_and_redo() {
    use aestra_authoring::{MaterialExpressionInput, MaterialFunctionBodyCommand as B};
    let original = function();
    let expression = original.expressions[0].id;
    let mut document =
        MaterialAuthoringDocument::standalone(vec![]).with_material_functions([original.clone()]);
    let mut history = MaterialCommandHistory::default();
    let mut renamed = original.clone();
    renamed.name = "Redo survives".into();
    history
        .execute(&mut document, replacement(renamed))
        .unwrap();
    history.undo(&mut document).unwrap().unwrap();
    let before = document.clone();
    let missing = MaterialExpressionId::new();
    for edit in [
        B::Add {
            expression: original.expressions[0].clone(),
            index: 99,
        },
        B::Add {
            expression: original.expressions[0].clone(),
            index: 1,
        },
        B::Remove {
            expression: missing,
        },
        B::Replace {
            expression,
            replacement: MaterialExpression {
                id: missing,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
            },
        },
        B::SetOutput {
            output: original.outputs[0].id,
            source: missing,
        },
        B::Rewire {
            expression,
            input: MaterialExpressionInput::Left,
            source: expression,
        },
        B::Replace {
            expression,
            replacement: MaterialExpression {
                id: expression,
                kind: MaterialExpressionKind::Add(expression, expression),
            },
        },
    ] {
        assert!(
            history
                .execute(
                    &mut document,
                    MaterialTransaction::single(
                        "Invalid",
                        MaterialCommand::EditMaterialFunctionBody {
                            function: original.id,
                            edit
                        }
                    )
                )
                .is_err()
        );
        assert_eq!(document, before);
        assert!(history.can_redo());
        assert!(!history.can_undo());
    }
}

#[test]
fn unused_function_signature_and_body_edits_preserve_identity_and_undo_without_a_program() {
    let original = function();
    let mut document =
        MaterialAuthoringDocument::standalone(vec![]).with_material_functions([original.clone()]);
    let before = document.clone();
    let mut changed = original.clone();
    changed.name = "Renamed".into();
    changed.inputs[0].name = "Amount".into();
    changed.outputs[0].name = "Intensity".into();
    changed.expressions[0].kind = MaterialExpressionKind::Constant(MaterialValue::Float(0.5));
    let mut history = MaterialCommandHistory::default();
    assert!(
        !history
            .execute(&mut document, replacement(changed.clone()))
            .unwrap()
            .is_empty()
    );
    assert!(document.effect.is_none());
    assert!(document.programs.is_empty());
    assert_eq!(document.material_functions, [changed.clone()]);
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
    history.redo(&mut document).unwrap().unwrap();
    assert_eq!(document.material_functions, [changed]);
}

#[test]
fn incompatible_signature_preserves_call_arguments_and_history() {
    let original = function();
    let mut program = MaterialProgram::additive_sprite("Caller");
    let argument = program.outputs.alpha;
    let call = MaterialExpressionId::new();
    program.expressions.push(MaterialExpression {
        id: call,
        kind: MaterialExpressionKind::FunctionCall {
            function: MaterialFunctionRef::Project(original.id),
            arguments: BTreeMap::from([(original.inputs[0].id, argument)]),
            output: original.outputs[0].id,
        },
    });
    program.outputs.alpha = call;
    let mut document = MaterialAuthoringDocument::standalone(vec![program])
        .with_material_functions([original.clone()]);
    document.validate().unwrap();
    let before = document.clone();
    let mut changed = original;
    changed.inputs[0].value_type = MaterialValueType::Vec3;
    changed.outputs[0].value_type = MaterialValueType::Vec3;
    let plan = document
        .plan_function_edit(changed.id, changed.clone())
        .unwrap();
    assert_eq!(plan.call_sites.len(), 1);
    assert_eq!(plan.call_sites[0].expression, call);
    assert!(plan.diagnostics.clone().into_result().is_err());
    assert_eq!(
        document, before,
        "preflight must preserve all authored links"
    );
    let mut history = MaterialCommandHistory::default();
    assert!(matches!(
        history.execute(&mut document, replacement(changed)),
        Err(MaterialCommandError::Validation(_))
    ));
    assert_eq!(document, before);
    assert!(!history.can_undo());
}

#[test]
fn replacement_cannot_change_function_identity() {
    let original = function();
    let mut document =
        MaterialAuthoringDocument::standalone(vec![]).with_material_functions([original.clone()]);
    let before = document.clone();
    let mut changed = original.clone();
    changed.id = MaterialFunctionId::new();
    let transaction = MaterialTransaction::single(
        "Wrong identity",
        MaterialCommand::ReplaceMaterialFunction {
            id: original.id,
            function: changed,
        },
    );
    assert!(matches!(
        MaterialCommandHistory::default().execute(&mut document, transaction),
        Err(MaterialCommandError::IdentityChanged { .. })
    ));
    assert_eq!(document, before);
}

#[test]
fn recursive_body_preflight_reports_cycle_without_changing_source() {
    let original = function();
    let document =
        MaterialAuthoringDocument::standalone(vec![]).with_material_functions([original.clone()]);
    let mut changed = original.clone();
    let call = MaterialExpressionId::new();
    changed.expressions.push(MaterialExpression {
        id: call,
        kind: MaterialExpressionKind::FunctionCall {
            function: MaterialFunctionRef::Project(original.id),
            arguments: BTreeMap::from([(original.inputs[0].id, original.expressions[0].id)]),
            output: original.outputs[0].id,
        },
    });
    changed.outputs[0].expression = call;
    let plan = document.plan_function_edit(original.id, changed).unwrap();
    assert!(plan.diagnostics.into_result().is_err());
    assert_eq!(document.material_functions, [original]);
}

#[test]
fn custom_wesl_metadata_edit_and_history_preserve_exact_source() {
    let original = MaterialFunction::from_ron(include_str!(
        "../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    let mut document =
        MaterialAuthoringDocument::standalone(vec![]).with_material_functions([original.clone()]);
    let mut changed = original.clone();
    changed.name = "Inspected pulse".into();
    let mut history = MaterialCommandHistory::default();
    history
        .execute(&mut document, replacement(changed))
        .unwrap();
    assert_eq!(
        document.material_functions[0].custom_wesl,
        original.custom_wesl
    );
    assert!(document.material_functions[0].expressions.is_empty());
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document.material_functions, [original]);
}
