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
        "../../../assets/materials/pulse_wave.aestra.material-function.ron"
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
