//! Exact `elkrs` adapter for Aestra's semantic-neutral layout boundary.

use super::*;
use serde_json::{Value, json};

const NODE_SPACING: f32 = 32.0;
const LAYER_SPACING: f32 = 72.0;

/// Deterministic, whole-graph layered layout. Partial layout and pin constraints are deliberately
/// rejected until their coordinate semantics are defined by the later interaction milestones.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ElkLayeredLayout;

impl GraphLayoutEngine for ElkLayeredLayout {
    fn layout(&self, input: &GraphLayoutInput) -> Result<GraphLayoutResult, GraphLayoutError> {
        input.validate()?;
        if !matches!(input.region, GraphLayoutRegion::Full) {
            return Err(adapter("partial graph layout is not supported yet"));
        }
        if input.nodes.iter().any(|node| node.pinned) {
            return Err(adapter("pinned graph layout is not supported yet"));
        }

        let direction = match input.direction {
            GraphDirection::LeftToRight => "RIGHT",
            GraphDirection::TopToBottom => "DOWN",
        };
        let children = input
            .nodes
            .iter()
            .map(|node| {
                json!({
                    "id": node_id(node.key),
                    "width": node.size.x,
                    "height": node.size.y,
                    "layoutOptions": {
                        "org.eclipse.elk.nodeSize.constraints": "[]"
                    }
                })
            })
            .collect::<Vec<_>>();
        let edges = input
            .edges
            .iter()
            .enumerate()
            .map(|(index, edge)| {
                json!({
                    "id": format!("e{index}"),
                    "sources": [node_id(edge.source)],
                    "targets": [node_id(edge.target)]
                })
            })
            .collect::<Vec<_>>();
        let request = json!({
            "id": "root",
            "layoutOptions": {
                "org.eclipse.elk.algorithm": "org.eclipse.elk.layered",
                "org.eclipse.elk.direction": direction,
                "org.eclipse.elk.edgeRouting": "SPLINES",
                "org.eclipse.elk.padding": "[top=0,left=0,bottom=0,right=0]",
                "org.eclipse.elk.spacing.nodeNode": NODE_SPACING,
                "org.eclipse.elk.layered.spacing.nodeNodeBetweenLayers": LAYER_SPACING,
                "org.eclipse.elk.randomSeed": 1
            },
            "children": children,
            "edges": edges
        });
        let output = elkrs::create_elk()
            .layout_json(&request.to_string())
            .map_err(|error| adapter(format!("elkrs failed: {error}")))?;
        let children = output
            .get("children")
            .and_then(Value::as_array)
            .ok_or_else(|| adapter("elkrs result has no children"))?;
        let mut positions = BTreeMap::new();
        for child in children {
            let id = parse_node_id(child.get("id").and_then(Value::as_str))?;
            let x = coordinate(child, "x")?;
            let y = coordinate(child, "y")?;
            if positions.insert(id, Vec2::new(x, y)).is_some() {
                return Err(adapter(format!("elkrs returned duplicate node {id:?}")));
            }
        }
        GraphLayoutResult::from_positions(input, positions)
    }
}

fn adapter(message: impl Into<String>) -> GraphLayoutError {
    GraphLayoutError::Adapter(message.into())
}

fn node_id(id: GraphLayoutNodeId) -> String {
    format!("n{}", id.index())
}

fn parse_node_id(value: Option<&str>) -> Result<GraphLayoutNodeId, GraphLayoutError> {
    let value = value.ok_or_else(|| adapter("elkrs result node has no id"))?;
    let index = value
        .strip_prefix('n')
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| adapter(format!("elkrs returned invalid node id {value:?}")))?;
    GraphLayoutNodeId::from_index(index)
}

fn coordinate(node: &Value, name: &str) -> Result<f32, GraphLayoutError> {
    let value = node
        .get(name)
        .and_then(Value::as_f64)
        .ok_or_else(|| adapter(format!("elkrs result node has no numeric {name}")))?;
    let coordinate = value as f32;
    if !coordinate.is_finite() {
        return Err(adapter(format!("elkrs returned a non-finite {name}")));
    }
    Ok(coordinate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(index: usize) -> GraphLayoutNodeId {
        GraphLayoutNodeId::from_index(index).unwrap()
    }

    fn node(index: usize, size: Vec2) -> GraphLayoutNode {
        GraphLayoutNode {
            key: id(index),
            position: Vec2::ZERO,
            size,
            pinned: false,
            selected: false,
        }
    }

    fn input(
        direction: GraphDirection,
        sizes: &[Vec2],
        edges: &[(usize, usize)],
    ) -> GraphLayoutInput {
        GraphLayoutInput::try_new(
            direction,
            sizes
                .iter()
                .enumerate()
                .map(|(index, size)| node(index, *size))
                .collect(),
            edges
                .iter()
                .map(|(source, target)| GraphLayoutEdge {
                    source: id(*source),
                    target: id(*target),
                    source_port: None,
                    target_port: None,
                })
                .collect(),
            GraphLayoutRegion::Full,
        )
        .unwrap()
    }

    fn overlaps(input: &GraphLayoutInput, result: &GraphLayoutResult) -> bool {
        input.nodes.iter().enumerate().any(|(index, a)| {
            input.nodes.iter().skip(index + 1).any(|b| {
                let a_min = result.positions[&a.key];
                let a_max = a_min + a.size;
                let b_min = result.positions[&b.key];
                let b_max = b_min + b.size;
                a_min.x < b_max.x && a_max.x > b_min.x && a_min.y < b_max.y && a_max.y > b_min.y
            })
        })
    }

    #[test]
    fn layered_layout_is_deterministic_left_to_right_and_non_overlapping() {
        let input = input(
            GraphDirection::LeftToRight,
            &[
                Vec2::new(120.0, 50.0),
                Vec2::new(180.0, 90.0),
                Vec2::new(80.0, 140.0),
                Vec2::new(200.0, 60.0),
            ],
            &[(0, 1), (0, 2), (1, 3), (2, 3)],
        );
        let first = ElkLayeredLayout.layout(&input).unwrap();
        let second = ElkLayeredLayout.layout(&input).unwrap();
        assert_eq!(first, second);
        assert!(!overlaps(&input, &first));
        for edge in &input.edges {
            let source = first.positions[&edge.source];
            let target = first.positions[&edge.target];
            let width = input
                .nodes
                .iter()
                .find(|node| node.key == edge.source)
                .unwrap()
                .size
                .x;
            assert!(target.x >= source.x + width);
        }
    }

    #[test]
    fn top_to_bottom_and_disconnected_nodes_use_measured_sizes() {
        let input = input(
            GraphDirection::TopToBottom,
            &[
                Vec2::new(320.0, 40.0),
                Vec2::new(60.0, 170.0),
                Vec2::new(110.0, 90.0),
            ],
            &[(0, 1)],
        );
        let result = ElkLayeredLayout.layout(&input).unwrap();
        assert!(!overlaps(&input, &result));
        assert!(result.positions[&id(1)].y >= result.positions[&id(0)].y + 40.0);
    }

    #[test]
    fn cycles_are_bounded_and_constraints_fail_closed() {
        let cycle = input(
            GraphDirection::LeftToRight,
            &[Vec2::splat(50.0); 3],
            &[(0, 1), (1, 2), (2, 0)],
        );
        assert!(ElkLayeredLayout.layout(&cycle).is_ok());

        let mut partial = cycle.clone();
        partial.region = GraphLayoutRegion::Nodes(BTreeSet::from([id(0)]));
        assert!(matches!(
            ElkLayeredLayout.layout(&partial),
            Err(GraphLayoutError::Adapter(_))
        ));

        let mut pinned = cycle;
        pinned.nodes[0].pinned = true;
        assert!(matches!(
            ElkLayeredLayout.layout(&pinned),
            Err(GraphLayoutError::Adapter(_))
        ));
    }

    #[test]
    fn layered_order_removes_avoidable_depth_column_crossings() {
        let input = input(
            GraphDirection::LeftToRight,
            &[Vec2::splat(50.0); 6],
            &[(0, 5), (1, 4), (2, 3)],
        );
        let result = ElkLayeredLayout.layout(&input).unwrap();
        let naive = input
            .nodes
            .iter()
            .map(|node| (node.key, Vec2::new(0.0, node.key.index() as f32)))
            .collect::<BTreeMap<_, _>>();
        assert!(crossings(&input.edges, &result.positions) < crossings(&input.edges, &naive));
    }

    fn crossings(
        edges: &[GraphLayoutEdge],
        positions: &BTreeMap<GraphLayoutNodeId, Vec2>,
    ) -> usize {
        edges
            .iter()
            .enumerate()
            .map(|(index, first)| {
                edges
                    .iter()
                    .skip(index + 1)
                    .filter(|second| {
                        (positions[&first.source].y - positions[&second.source].y)
                            * (positions[&first.target].y - positions[&second.target].y)
                            < 0.0
                    })
                    .count()
            })
            .sum()
    }
}
