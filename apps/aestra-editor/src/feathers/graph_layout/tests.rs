use super::*;

fn id(value: usize) -> GraphLayoutNodeId {
    GraphLayoutNodeId::from_index(value).unwrap()
}

fn node(value: usize, position: Vec2) -> GraphLayoutNode {
    GraphLayoutNode {
        key: id(value),
        position,
        size: Vec2::new(80.0, 40.0),
        pinned: false,
        selected: false,
    }
}

fn input(region: GraphLayoutRegion) -> GraphLayoutInput {
    GraphLayoutInput::try_new(
        GraphDirection::LeftToRight,
        vec![
            node(2, Vec2::new(200.0, 0.0)),
            node(0, Vec2::ZERO),
            node(1, Vec2::new(100.0, 0.0)),
        ],
        vec![
            GraphLayoutEdge {
                source: id(1),
                target: id(2),
                source_port: None,
                target_port: None,
            },
            GraphLayoutEdge {
                source: id(0),
                target: id(1),
                source_port: None,
                target_port: None,
            },
        ],
        region,
    )
    .unwrap()
}

#[test]
fn input_is_canonical_and_rejects_invalid_snapshots() {
    let input = input(GraphLayoutRegion::Full);
    assert_eq!(
        input.nodes.iter().map(|node| node.key).collect::<Vec<_>>(),
        [id(0), id(1), id(2)]
    );
    assert_eq!(
        input
            .edges
            .iter()
            .map(|edge| (edge.source, edge.target))
            .collect::<Vec<_>>(),
        [(id(0), id(1)), (id(1), id(2))]
    );

    let duplicate = GraphLayoutInput::try_new(
        GraphDirection::LeftToRight,
        vec![node(0, Vec2::ZERO), node(0, Vec2::ONE)],
        vec![],
        GraphLayoutRegion::Full,
    );
    assert_eq!(
        duplicate.unwrap_err(),
        GraphLayoutError::DuplicateNode(id(0))
    );

    let missing = GraphLayoutInput::try_new(
        GraphDirection::LeftToRight,
        vec![node(0, Vec2::ZERO)],
        vec![GraphLayoutEdge {
            source: id(0),
            target: id(3),
            source_port: None,
            target_port: None,
        }],
        GraphLayoutRegion::Full,
    );
    assert_eq!(
        missing.unwrap_err(),
        GraphLayoutError::MissingEdgeNode(id(3))
    );
}

struct ReverseEngine;

impl GraphLayoutEngine for ReverseEngine {
    fn layout(&self, input: &GraphLayoutInput) -> Result<GraphLayoutResult, GraphLayoutError> {
        let positions = input
            .nodes
            .iter()
            .map(|node| {
                (
                    node.key,
                    Vec2::new(
                        (input.nodes.len() - 1 - node.key.0 as usize) as f32 * 120.0,
                        20.0,
                    ),
                )
            })
            .collect();
        GraphLayoutResult::from_positions(input, positions)
    }
}

#[test]
fn engine_contract_is_deterministic_and_semantic_neutral() {
    let first = ReverseEngine
        .layout(&input(GraphLayoutRegion::Full))
        .unwrap();
    let second = ReverseEngine
        .layout(&input(GraphLayoutRegion::Full))
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.positions[&id(0)], Vec2::new(240.0, 20.0));
    assert_eq!(
        first.bounds,
        Rect::from_corners(Vec2::new(0.0, 20.0), Vec2::new(320.0, 60.0))
    );
}

#[test]
fn result_rejects_incomplete_non_finite_and_fixed_node_moves() {
    let mut input = input(GraphLayoutRegion::Nodes(BTreeSet::from([id(1)])));
    input.nodes[1].pinned = true;
    let original = input
        .nodes
        .iter()
        .map(|node| (node.key, node.position))
        .collect::<BTreeMap<_, _>>();

    let mut missing = original.clone();
    missing.remove(&id(2));
    assert_eq!(
        GraphLayoutResult::from_positions(&input, missing).unwrap_err(),
        GraphLayoutError::IncompleteResult
    );

    let mut moved_outside = original.clone();
    moved_outside.insert(id(0), Vec2::new(10.0, 0.0));
    assert_eq!(
        GraphLayoutResult::from_positions(&input, moved_outside).unwrap_err(),
        GraphLayoutError::MovedFixedNode(id(0))
    );

    let mut moved_pinned = original.clone();
    moved_pinned.insert(id(1), Vec2::new(110.0, 0.0));
    assert_eq!(
        GraphLayoutResult::from_positions(&input, moved_pinned).unwrap_err(),
        GraphLayoutError::MovedFixedNode(id(1))
    );

    let mut non_finite = original;
    non_finite.insert(id(1), Vec2::new(f32::NAN, 0.0));
    assert_eq!(
        GraphLayoutResult::from_positions(&input, non_finite).unwrap_err(),
        GraphLayoutError::NonFiniteResult(id(1))
    );
}

#[test]
fn result_bounds_must_contain_every_node() {
    let input = input(GraphLayoutRegion::Full);
    let positions = input
        .nodes
        .iter()
        .map(|node| (node.key, node.position))
        .collect();
    let result = GraphLayoutResult {
        positions,
        bounds: Rect::from_corners(Vec2::ZERO, Vec2::new(50.0, 50.0)),
    };
    assert_eq!(
        result.validate(&input),
        Err(GraphLayoutError::InvalidBounds)
    );
}
