use super::*;
use aestra_authoring::{MaterialConnectionTarget, MaterialOutputSocket};
use aestra_compiler::MaterialGraphCreateKind;
use aestra_core::{
    MaterialExpressionId, MaterialFunctionId,
    material::{MaterialExpressionKind, MaterialValue, MaterialValueType},
};

fn document() -> MaterialAuthoringDocument {
    MaterialAuthoringDocument::standalone(vec![
        MaterialProgram::additive_sprite("Commands").normalized(),
    ])
}

fn create(document: MaterialAuthoringDocument) -> PreparedEdit {
    let program = document.programs[0].id;
    prepare(
        document,
        program,
        MaterialToolCommand::CreateMaterialGraphNode {
            program,
            kind: MaterialGraphCreateKind::Constant(MaterialValueType::Float),
            source: None,
            target: None,
        },
    )
    .unwrap()
}

#[test]
fn creation_is_detached_and_returns_the_same_validated_plan_the_host_publishes() {
    let original = document();
    let bytes = serde_json::to_string(&original).unwrap();
    let edit = create(original.clone());
    assert_eq!(edit.before, original.programs[0]);
    assert_eq!(
        edit.after.expressions.len(),
        edit.before.expressions.len() + 1
    );
    assert_eq!(edit.plan.created_expressions.len(), 1);
    let mut applied = original.clone();
    let outcome = MaterialCommandExecutor::execute(&mut applied, &edit.plan.transaction).unwrap();
    assert_eq!(applied.programs[0], edit.after);
    assert_eq!(outcome.diff, edit.plan.diff);
    assert_eq!(serde_json::to_string(&original).unwrap(), bytes);
    MaterialCommandExecutor::execute(&mut applied, &outcome.inverse).unwrap();
    assert_eq!(applied, original);
}

#[test]
fn connection_and_disconnect_use_real_typed_outputs() {
    let created = create(document());
    let program = created.after.id;
    let source = created.plan.created_expressions[0];
    let input = MaterialAuthoringDocument::standalone(vec![created.after]);
    let connected = prepare(
        input,
        program,
        MaterialToolCommand::ConnectMaterialExpression {
            program,
            source,
            target: MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha),
        },
    )
    .unwrap();
    assert_eq!(connected.after.outputs.alpha, source);
    let disconnected = prepare(
        MaterialAuthoringDocument::standalone(vec![connected.after]),
        program,
        MaterialToolCommand::DisconnectMaterialConnection {
            program,
            target: MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha),
        },
    )
    .unwrap();
    assert_ne!(disconnected.after.outputs.alpha, source);
    assert!(disconnected.after.expressions.iter().any(|expression| {
        expression.id == disconnected.after.outputs.alpha
            && matches!(
                expression.kind,
                MaterialExpressionKind::Constant(MaterialValue::Float(_))
            )
    }));
}

#[test]
fn duplicate_then_delete_preserves_the_original_expression() {
    let created = create(document());
    let program = created.after.id;
    let source = created.plan.created_expressions[0];
    let original = created.after;
    let duplicated = prepare(
        MaterialAuthoringDocument::standalone(vec![original.clone()]),
        program,
        MaterialToolCommand::DuplicateMaterialExpressions {
            program,
            expressions: vec![source],
        },
    )
    .unwrap();
    let duplicate = duplicated.plan.created_expressions[0];
    assert_ne!(source, duplicate);
    let deleted = prepare(
        MaterialAuthoringDocument::standalone(vec![duplicated.after]),
        program,
        MaterialToolCommand::DeleteMaterialExpressions {
            program,
            expressions: vec![duplicate],
        },
    )
    .unwrap();
    assert_eq!(deleted.after, original);
}

#[test]
fn invalid_source_type_and_cycle_fail_without_mutating_the_input() {
    let input = document();
    let program = input.programs[0].id;
    let color = input.programs[0].outputs.color;
    let bytes = serde_json::to_string(&input).unwrap();
    for command in [
        MaterialToolCommand::ConnectMaterialExpression {
            program,
            source: MaterialExpressionId::new(),
            target: MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color),
        },
        MaterialToolCommand::ConnectMaterialExpression {
            program,
            source: color,
            target: MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha),
        },
        MaterialToolCommand::ReplaceMaterialExpression {
            program,
            expression: color,
            replacement: MaterialExpressionKind::Add(color, color),
        },
    ] {
        assert!(prepare(input.clone(), program, command).is_err());
        assert_eq!(serde_json::to_string(&input).unwrap(), bytes);
    }
}

#[test]
fn missing_program_or_deleted_node_is_rejected_before_publication() {
    let input = document();
    let program = input.programs[0].id;
    let command = MaterialToolCommand::DeleteMaterialExpressions {
        program,
        expressions: vec![MaterialExpressionId::new()],
    };
    assert!(prepare(input.clone(), MaterialProgramId::new(), command.clone()).is_err());
    assert!(prepare(input, program, command).is_err());
}

#[test]
fn extraction_returns_function_payload_and_a_reversible_combined_transaction() {
    let input = document();
    let program = input.programs[0].id;
    let selected = input.programs[0].outputs.color;
    let function = MaterialFunctionId::new();
    let edit = prepare(
        input.clone(),
        program,
        MaterialToolCommand::ExtractMaterialFunction {
            program,
            function,
            name: "Extracted color".into(),
            expressions: vec![selected],
        },
    )
    .unwrap();
    assert_eq!(edit.plan.created_function().unwrap().id, function);
    let mut applied = input.clone();
    let outcome = MaterialCommandExecutor::execute(&mut applied, &edit.plan.transaction).unwrap();
    assert_eq!(applied.material_functions.len(), 1);
    assert_eq!(applied.programs[0], edit.after);
    MaterialCommandExecutor::execute(&mut applied, &outcome.inverse).unwrap();
    assert_eq!(applied, input);
}
