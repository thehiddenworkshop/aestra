use aestra_authoring::{
    MaterialAuthoringDocument, MaterialCommand, MaterialCommandHistory, MaterialConnectionTarget,
    MaterialExpressionInput, MaterialOutputSocket, MaterialToolCommand, MaterialToolPlanner,
    MaterialTransaction,
};
use aestra_compiler::{MaterialCompiler, MaterialGraphCreateKind, MaterialGraphFunction};
use aestra_core::{
    EffectAsset, MaterialExpressionId,
    material::{
        MaterialDomain, MaterialExpression, MaterialExpressionKind as Kind, MaterialProgram,
        MaterialValue,
    },
};

#[test]
fn normal_map_creation_rewiring_and_duplicate_are_atomic_and_undoable() {
    let mut program = MaterialProgram::additive_sprite("Normal Map authoring");
    program.domain = MaterialDomain::Mesh;
    let id = program.id;
    let mut document =
        MaterialAuthoringDocument::new(EffectAsset::new("Normal map", 2.0), vec![program]);
    let before = document.clone();
    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::CreateMaterialGraphNode {
            program: id,
            kind: MaterialGraphCreateKind::Function(MaterialGraphFunction::NormalMap),
            source: None,
            target: Some(MaterialConnectionTarget::ProgramOutput(
                MaterialOutputSocket::Color,
            )),
        },
    )
    .unwrap();
    let mut history = MaterialCommandHistory::default();
    history.execute(&mut document, plan.transaction).unwrap();
    let expression = document.programs[0].outputs.color;
    assert!(matches!(
        document.programs[0]
            .expressions
            .iter()
            .find(|e| e.id == expression)
            .unwrap()
            .kind,
        Kind::NormalMap { .. }
    ));
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
    history.redo(&mut document).unwrap().unwrap();

    // Install alternate typed values, then exercise every new semantic rewire target.
    let mut commands = Vec::new();
    for (input, value) in [
        (
            MaterialExpressionInput::Sample,
            MaterialValue::Vec4([0.5, 0.8, 0.9, 0.0]),
        ),
        (MaterialExpressionInput::Strength, MaterialValue::Float(2.0)),
        (MaterialExpressionInput::FlipY, MaterialValue::Bool(true)),
        (
            MaterialExpressionInput::Normal,
            MaterialValue::Vec3([0.0, 0.0, 1.0]),
        ),
        (
            MaterialExpressionInput::Tangent,
            MaterialValue::Vec3([1.0, 0.0, 0.0]),
        ),
        (
            MaterialExpressionInput::Bitangent,
            MaterialValue::Vec3([0.0, -1.0, 0.0]),
        ),
    ] {
        let source = MaterialExpressionId::new();
        document.programs[0].expressions.push(MaterialExpression {
            id: source,
            kind: Kind::Constant(value),
        });
        commands.push(MaterialCommand::RewireMaterialExpressionInput {
            program: id,
            expression,
            input,
            source,
        });
    }
    let before_rewire = document.clone();
    history
        .execute(
            &mut document,
            MaterialTransaction::new("Rewire Normal Map", commands),
        )
        .unwrap();
    assert!(MaterialCompiler.compile(&document.programs[0]).is_ok());
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before_rewire);
    history.redo(&mut document).unwrap().unwrap();

    let selected = document.programs[0]
        .expressions
        .iter()
        .map(|e| e.id)
        .collect();
    let before_duplicate = document.clone();
    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::DuplicateMaterialExpressions {
            program: id,
            expressions: selected,
        },
    )
    .unwrap();
    history.execute(&mut document, plan.transaction).unwrap();
    assert!(MaterialCompiler.compile(&document.programs[0]).is_ok());
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before_duplicate);
}
