use aestra_compiler::{
    MaterialCompiler, MaterialGraphEdgeTarget, MaterialGraphFunction, MaterialGraphNodeKind,
    MaterialGraphOutputKind,
};
use aestra_core::{
    MaterialExpressionId,
    material::{MaterialExpression, MaterialExpressionKind, MaterialProgram, MaterialValue},
};

#[test]
fn graph_projection_is_deterministic_typed_and_source_mapped() {
    let mut program = MaterialProgram::additive_sprite("Graph contract");
    let value = MaterialExpressionId::from_u128(0x6101);
    let edge_min = MaterialExpressionId::from_u128(0x6102);
    let edge_max = MaterialExpressionId::from_u128(0x6103);
    let smoothstep = MaterialExpressionId::from_u128(0x6104);
    program.expressions.extend([
        MaterialExpression {
            id: value,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: edge_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: edge_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: smoothstep,
            kind: MaterialExpressionKind::Smoothstep {
                edge_min,
                edge_max,
                value,
            },
        },
    ]);
    program.outputs.alpha = smoothstep;

    let compiler = MaterialCompiler;
    let ir = compiler.compile(&program).unwrap();
    let projection = compiler.project_graph(&program, Some(&ir));
    assert!(projection.diagnostics.is_valid());
    assert_eq!(projection.nodes.len(), program.expressions.len());
    let node = projection
        .nodes
        .iter()
        .find(|node| node.expression == smoothstep)
        .unwrap();
    assert_eq!(
        node.kind,
        MaterialGraphNodeKind::Function(MaterialGraphFunction::Smoothstep)
    );
    assert_eq!(
        node.inputs
            .iter()
            .map(|port| port.name.as_str())
            .collect::<Vec<_>>(),
        ["edge_min", "edge_max", "value"]
    );
    assert!(node.value_type.is_some());
    assert!(node.evaluation_domain.is_some());
    assert!(node.reachable);
    assert!(node.ir_value.is_some());
    assert!(projection.edges.iter().any(|edge| {
        edge.source == smoothstep
            && edge.target == MaterialGraphEdgeTarget::Output(MaterialGraphOutputKind::Alpha)
    }));

    let mut reordered = program.clone();
    reordered.expressions.reverse();
    assert_eq!(
        projection,
        compiler.project_graph(&reordered, Some(&compiler.compile(&reordered).unwrap()))
    );
}

#[test]
fn graph_projection_keeps_disabled_unreachable_and_invalid_nodes_visible() {
    let mut program = MaterialProgram::additive_sprite("Debug graph");
    let orphan = MaterialExpressionId::from_u128(0x6201);
    let missing = MaterialExpressionId::from_u128(0x62ff);
    program.expressions.push(MaterialExpression {
        id: orphan,
        kind: MaterialExpressionKind::Smoothstep {
            edge_min: missing,
            edge_max: missing,
            value: missing,
        },
    });
    program.disabled_expressions.push(orphan);

    let projection = MaterialCompiler.project_graph(&program, None);
    assert!(!projection.diagnostics.is_valid());
    let orphan = projection
        .nodes
        .iter()
        .find(|node| node.expression == orphan)
        .unwrap();
    assert!(orphan.disabled);
    assert!(!orphan.reachable);
    assert_eq!(orphan.value_type, None);
    assert_eq!(orphan.inputs.len(), 3);
}
