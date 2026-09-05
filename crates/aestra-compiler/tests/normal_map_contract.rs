use aestra_compiler::{
    MaterialCompiler, MaterialGraphCreateKind, MaterialGraphFunction, MaterialIrInstruction,
    evaluate_normal_map,
};
use aestra_core::{
    MaterialExpressionId,
    material::{
        MaterialDomain, MaterialExpression, MaterialExpressionKind as Kind, MaterialInput,
        MaterialProgram, MaterialValue, MaterialValueType,
    },
};

fn graph() -> (MaterialProgram, MaterialExpressionId) {
    let mut program = MaterialProgram::additive_sprite("Normal map contract");
    program.domain = MaterialDomain::Mesh;
    let plan = MaterialCompiler
        .plan_graph_node_creation(
            &program,
            MaterialGraphCreateKind::Function(MaterialGraphFunction::NormalMap),
            None,
        )
        .unwrap();
    let mut program = plan.replacement;
    program.outputs.color = plan.expression;
    (program, plan.expression)
}

#[test]
fn normal_map_graph_is_typed_serializable_and_uses_the_mesh_basis() {
    let (program, expression) = graph();
    let encoded = program.to_pretty_ron().unwrap();
    let roundtrip = MaterialProgram::from_ron(&encoded).unwrap();
    assert_eq!(roundtrip.to_pretty_ron().unwrap(), encoded);
    let ir = MaterialCompiler.compile(&roundtrip).unwrap();
    let graph = MaterialCompiler.project_graph(&roundtrip, Some(&ir));
    let node = graph
        .nodes
        .iter()
        .find(|node| node.expression == expression)
        .unwrap();
    assert_eq!(node.label, "Normal Map");
    assert_eq!(node.value_type, Some(MaterialValueType::Vec3));
    assert_eq!(
        node.inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        [
            "sample",
            "strength",
            "flip_y",
            "normal",
            "tangent",
            "bitangent"
        ]
    );
    for input in [
        MaterialInput::Normal,
        MaterialInput::Tangent,
        MaterialInput::Bitangent,
    ] {
        assert!(
            ir.values
                .iter()
                .any(|value| value.instruction == MaterialIrInstruction::Input(input))
        );
    }
}

#[test]
fn normal_map_rejects_incompatible_sockets_and_deduplicates_equal_operations() {
    let (program, expression) = graph();
    let node = program
        .expressions
        .iter()
        .find(|node| node.id == expression)
        .unwrap();
    for dependency in node.kind.dependencies() {
        let mut invalid = program.clone();
        invalid
            .expressions
            .iter_mut()
            .find(|node| node.id == dependency)
            .unwrap()
            .kind = Kind::Constant(MaterialValue::Vec2([0.0; 2]));
        assert!(MaterialCompiler.compile(&invalid).is_err());
    }
    let mut program = program.clone();
    let mut duplicate = node.clone();
    duplicate.id = MaterialExpressionId::new();
    let sum = MaterialExpression {
        id: MaterialExpressionId::new(),
        kind: Kind::Add(expression, duplicate.id),
    };
    program.outputs.color = sum.id;
    program.expressions.extend([duplicate, sum]);
    let ir = MaterialCompiler.compile(&program).unwrap();
    assert_eq!(
        ir.values
            .iter()
            .filter(|value| matches!(value.instruction, MaterialIrInstruction::NormalMap { .. }))
            .count(),
        1
    );
}

#[test]
fn normal_map_reference_handles_strength_y_flip_handedness_and_degenerate_samples() {
    let n = [0.0, 0.0, 1.0];
    let t = [1.0, 0.0, 0.0];
    let b = [0.0, 1.0, 0.0];
    let check = |actual: [f32; 3], expected: [f32; 3]| {
        for (a, e) in actual.into_iter().zip(expected) {
            assert!((a - e).abs() < 1e-5, "{actual:?} != {expected:?}");
        }
    };
    check(evaluate_normal_map([0.5, 0.5, 1.0], 1.0, false, n, t, b), n);
    check(evaluate_normal_map([1.0, 0.0, 0.0], 0.0, true, n, t, b), n);
    check(evaluate_normal_map([1.0, 0.0, 0.0], -2.0, true, n, t, b), n);
    check(evaluate_normal_map([0.5; 3], 1.0, false, n, t, b), n);
    let tilted = [0.5, 0.8, 0.9];
    check(
        evaluate_normal_map(tilted, 1.0, false, n, t, b),
        [0.0, 0.6, 0.8],
    );
    check(
        evaluate_normal_map(tilted, 1.0, true, n, t, b),
        [0.0, -0.6, 0.8],
    );
    check(
        evaluate_normal_map(tilted, 1.0, false, n, t, [0.0, -1.0, 0.0]),
        [0.0, -0.6, 0.8],
    );
    check(
        evaluate_normal_map(tilted, 1.0, true, n, t, [0.0, -1.0, 0.0]),
        [0.0, 0.6, 0.8],
    );
    // Nonunit and slightly nonorthogonal interpolated basis is repaired.
    check(
        evaluate_normal_map(
            tilted,
            1.0,
            false,
            [0.0, 0.0, 2.0],
            [3.0, 0.0, 0.2],
            [0.0, 4.0, 0.3],
        ),
        [0.0, 0.6, 0.8],
    );
    let length = 2.08_f32.sqrt();
    check(
        evaluate_normal_map(tilted, 2.0, false, n, t, b),
        [0.0, 1.2 / length, 0.8 / length],
    );
}
