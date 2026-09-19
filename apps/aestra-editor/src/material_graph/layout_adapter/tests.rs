use super::*;
use crate::feathers::graph_layout::{GraphLayoutNodeState, GraphLayoutResult};
use aestra_compiler::MaterialFunctionLibrary;
use aestra_core::material::{MaterialFunction, MaterialProgram};

fn state(index: usize) -> GraphLayoutNodeState {
    GraphLayoutNodeState {
        position: Vec2::new(index as f32 * 120.0, index as f32 * 30.0),
        size: Vec2::new(100.0 + index as f32, 60.0),
        pinned: false,
        selected: index.is_multiple_of(2),
    }
}

#[test]
fn program_projection_maps_to_opaque_deterministic_topology() {
    let mut program = MaterialProgram::additive_sprite("M8 adapter");
    program.node_constants = program.expressions.iter().map(|node| node.id).collect();
    let projection = MaterialCompiler.project_graph(&program, None);
    let mut keys = projection
        .nodes
        .iter()
        .map(|node| GraphNodeKey::Expression(node.expression))
        .collect::<BTreeSet<_>>();
    keys.insert(GraphNodeKey::MaterialOutputs);
    let states = keys
        .iter()
        .enumerate()
        .map(|(index, key)| (*key, state(index)))
        .collect::<BTreeMap<_, _>>();

    let adapter = LayoutAdapter::program(&projection, &states, None).unwrap();
    assert_eq!(adapter.input.nodes.len(), keys.len());
    assert_eq!(adapter.input.edges.len(), projection.edges.len());
    assert_eq!(adapter.input.region, GraphLayoutRegion::Full);
    assert!(
        adapter
            .input
            .edges
            .iter()
            .all(|edge| adapter.semantic_by_layout[&edge.target] == GraphNodeKey::MaterialOutputs)
    );

    let positions = adapter
        .input
        .nodes
        .iter()
        .map(|node| (node.key, node.position + Vec2::splat(8.0)))
        .collect();
    let result = GraphLayoutResult::from_positions(&adapter.input, positions).unwrap();
    let resolved = adapter.resolve(&result).unwrap();
    assert_eq!(resolved.len(), states.len());
    for (key, original) in states {
        assert_eq!(resolved[&key], original.position + Vec2::splat(8.0));
    }
}

#[test]
fn function_adapter_is_independent_of_projection_vector_order() {
    let function = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap();
    let mut projection =
        MaterialCompiler.project_function_graph(&function, &MaterialFunctionLibrary::default());
    let MaterialFunctionBodyProjection::Graph { nodes, edges } = &projection.body else {
        panic!("expected graph function");
    };
    let mut keys = nodes
        .iter()
        .map(|node| GraphNodeKey::Expression(node.id))
        .collect::<BTreeSet<_>>();
    keys.insert(GraphNodeKey::FunctionOutputs);
    let states = keys
        .iter()
        .enumerate()
        .map(|(index, key)| (*key, state(index)))
        .collect::<BTreeMap<_, _>>();

    let first = LayoutAdapter::function(&projection, &states, None).unwrap();
    let expected_edges = edges.len();
    let MaterialFunctionBodyProjection::Graph { nodes, edges } = &mut projection.body else {
        unreachable!();
    };
    nodes.reverse();
    edges.reverse();
    let second = LayoutAdapter::function(&projection, &states, None).unwrap();
    assert_eq!(first.input, second.input);
    assert_eq!(first.input.edges.len(), expected_edges);
}

#[test]
fn adapter_rejects_incomplete_geometry_and_unknown_regions() {
    let mut program = MaterialProgram::additive_sprite("M8 validation");
    program.node_constants = program.expressions.iter().map(|node| node.id).collect();
    let projection = MaterialCompiler.project_graph(&program, None);
    let states = BTreeMap::from([(
        GraphNodeKey::MaterialOutputs,
        GraphLayoutNodeState {
            position: Vec2::ZERO,
            size: Vec2::splat(100.0),
            pinned: false,
            selected: false,
        },
    )]);
    assert!(matches!(
        LayoutAdapter::program(&projection, &states, None),
        Err(GraphLayoutError::Adapter(message)) if message.contains("missing")
    ));

    let mut complete = projection
        .nodes
        .iter()
        .map(|node| {
            (
                GraphNodeKey::Expression(node.expression),
                GraphLayoutNodeState {
                    position: Vec2::ZERO,
                    size: Vec2::splat(100.0),
                    pinned: false,
                    selected: false,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    complete.insert(
        GraphNodeKey::MaterialOutputs,
        states[&GraphNodeKey::MaterialOutputs],
    );
    let unknown = GraphNodeKey::FunctionOutputs;
    assert!(matches!(
        LayoutAdapter::program(&projection, &complete, Some(&BTreeSet::from([unknown]))),
        Err(GraphLayoutError::Adapter(message)) if message.contains("region")
    ));
}
