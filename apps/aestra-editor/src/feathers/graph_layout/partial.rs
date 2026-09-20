//! Deterministic planning for targeted graph arrangement.
//!
//! The layered engine arranges a self-contained graph. This module owns the higher-level M10
//! boundary: expand a user seed into the requested branch, extract the movable induced subgraph,
//! remember every fixed node, and identify edges that attach the movable region to the frozen
//! graph. Placement and boundary-collision reconciliation consume this plan in later slices.

use super::{
    GraphLayoutEdge, GraphLayoutError, GraphLayoutInput, GraphLayoutNode, GraphLayoutNodeId,
    GraphLayoutRegion,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartialLayoutScope {
    Selection,
    Upstream,
    Downstream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoundaryDirection {
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BoundaryAnchor {
    pub(crate) edge: GraphLayoutEdge,
    pub(crate) movable: GraphLayoutNodeId,
    pub(crate) fixed: GraphLayoutNodeId,
    pub(crate) direction: BoundaryDirection,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PartialLayoutPlan {
    pub(crate) region: BTreeSet<GraphLayoutNodeId>,
    pub(crate) movable_input: GraphLayoutInput,
    pub(crate) fixed_nodes: BTreeMap<GraphLayoutNodeId, GraphLayoutNode>,
    pub(crate) anchors: Vec<BoundaryAnchor>,
}

impl PartialLayoutPlan {
    pub(crate) fn extract(
        input: &GraphLayoutInput,
        seeds: &BTreeSet<GraphLayoutNodeId>,
        scope: PartialLayoutScope,
    ) -> Result<Self, GraphLayoutError> {
        input.validate()?;
        let region = expand_region(input, seeds, scope)?;
        let movable_nodes = input
            .nodes
            .iter()
            .filter(|node| region.contains(&node.key))
            .cloned()
            .collect();
        let movable_edges = input
            .edges
            .iter()
            .filter(|edge| region.contains(&edge.source) && region.contains(&edge.target))
            .copied()
            .collect();
        let movable_input = GraphLayoutInput::try_new(
            input.direction,
            movable_nodes,
            movable_edges,
            GraphLayoutRegion::Full,
        )?;
        let fixed_nodes = input
            .nodes
            .iter()
            .filter(|node| !region.contains(&node.key))
            .map(|node| (node.key, *node))
            .collect();
        let anchors = input
            .edges
            .iter()
            .filter_map(|edge| boundary_anchor(*edge, &region))
            .collect();
        Ok(Self {
            region,
            movable_input,
            fixed_nodes,
            anchors,
        })
    }
}

fn expand_region(
    input: &GraphLayoutInput,
    seeds: &BTreeSet<GraphLayoutNodeId>,
    scope: PartialLayoutScope,
) -> Result<BTreeSet<GraphLayoutNodeId>, GraphLayoutError> {
    if seeds.is_empty() {
        return Err(GraphLayoutError::EmptyRegion);
    }
    let known = input
        .nodes
        .iter()
        .map(|node| node.key)
        .collect::<BTreeSet<_>>();
    if let Some(seed) = seeds.iter().find(|seed| !known.contains(seed)) {
        return Err(GraphLayoutError::MissingRegionNode(*seed));
    }
    if scope == PartialLayoutScope::Selection {
        return Ok(seeds.clone());
    }

    let mut neighbors = known
        .iter()
        .map(|key| (*key, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in &input.edges {
        let (from, to) = match scope {
            PartialLayoutScope::Upstream => (edge.target, edge.source),
            PartialLayoutScope::Downstream => (edge.source, edge.target),
            PartialLayoutScope::Selection => unreachable!(),
        };
        neighbors
            .get_mut(&from)
            .expect("validated edge endpoint must exist")
            .insert(to);
    }

    let mut region = seeds.clone();
    let mut queue = VecDeque::from_iter(seeds.iter().copied());
    while let Some(node) = queue.pop_front() {
        for &neighbor in &neighbors[&node] {
            if region.insert(neighbor) {
                queue.push_back(neighbor);
            }
        }
    }
    Ok(region)
}

fn boundary_anchor(
    edge: GraphLayoutEdge,
    region: &BTreeSet<GraphLayoutNodeId>,
) -> Option<BoundaryAnchor> {
    match (region.contains(&edge.source), region.contains(&edge.target)) {
        (false, true) => Some(BoundaryAnchor {
            edge,
            movable: edge.target,
            fixed: edge.source,
            direction: BoundaryDirection::Incoming,
        }),
        (true, false) => Some(BoundaryAnchor {
            edge,
            movable: edge.source,
            fixed: edge.target,
            direction: BoundaryDirection::Outgoing,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feathers::graph_layout::{GraphDirection, GraphLayoutNode};
    use bevy::prelude::Vec2;

    fn id(index: usize) -> GraphLayoutNodeId {
        GraphLayoutNodeId::from_index(index).unwrap()
    }

    fn input() -> GraphLayoutInput {
        let nodes = (0..6)
            .map(|index| GraphLayoutNode {
                key: id(index),
                position: Vec2::new(index as f32 * 100.0, index as f32 * 10.0),
                size: Vec2::new(80.0, 50.0),
                pinned: false,
                selected: false,
            })
            .collect();
        let edges = [(0, 1), (1, 2), (2, 3), (4, 2), (3, 5)]
            .into_iter()
            .map(|(source, target)| GraphLayoutEdge {
                source: id(source),
                target: id(target),
                source_port: None,
                target_port: None,
            })
            .collect();
        GraphLayoutInput::try_new(
            GraphDirection::LeftToRight,
            nodes,
            edges,
            GraphLayoutRegion::Full,
        )
        .unwrap()
    }

    #[test]
    fn selection_extracts_only_the_induced_subgraph_and_freezes_everything_else() {
        let graph = input();
        let plan = PartialLayoutPlan::extract(
            &graph,
            &BTreeSet::from([id(1), id(2)]),
            PartialLayoutScope::Selection,
        )
        .unwrap();

        assert_eq!(plan.region, BTreeSet::from([id(1), id(2)]));
        assert_eq!(
            plan.movable_input
                .nodes
                .iter()
                .map(|node| node.key)
                .collect::<Vec<_>>(),
            vec![id(1), id(2)]
        );
        assert_eq!(plan.movable_input.edges.len(), 1);
        assert_eq!(plan.fixed_nodes.len(), 4);
        assert_eq!(plan.fixed_nodes[&id(5)], graph.nodes[5]);
        assert_eq!(
            plan.anchors,
            vec![
                BoundaryAnchor {
                    edge: graph.edges[0],
                    movable: id(1),
                    fixed: id(0),
                    direction: BoundaryDirection::Incoming,
                },
                BoundaryAnchor {
                    edge: graph.edges[2],
                    movable: id(2),
                    fixed: id(3),
                    direction: BoundaryDirection::Outgoing,
                },
                BoundaryAnchor {
                    edge: graph.edges[4],
                    movable: id(2),
                    fixed: id(4),
                    direction: BoundaryDirection::Incoming,
                },
            ]
        );
    }

    #[test]
    fn upstream_and_downstream_expand_deterministically_from_all_seeds() {
        let graph = input();
        let seeds = BTreeSet::from([id(2)]);
        let upstream =
            PartialLayoutPlan::extract(&graph, &seeds, PartialLayoutScope::Upstream).unwrap();
        let downstream =
            PartialLayoutPlan::extract(&graph, &seeds, PartialLayoutScope::Downstream).unwrap();

        assert_eq!(
            upstream.region,
            BTreeSet::from([id(0), id(1), id(2), id(4)])
        );
        assert_eq!(downstream.region, BTreeSet::from([id(2), id(3), id(5)]));
        assert_eq!(upstream.anchors.len(), 1);
        assert_eq!(upstream.anchors[0].direction, BoundaryDirection::Outgoing);
        assert_eq!(downstream.anchors.len(), 2);
        assert!(
            downstream
                .anchors
                .iter()
                .all(|anchor| anchor.direction == BoundaryDirection::Incoming)
        );
    }

    #[test]
    fn invalid_or_empty_seeds_are_rejected_before_planning() {
        let graph = input();
        assert_eq!(
            PartialLayoutPlan::extract(&graph, &BTreeSet::new(), PartialLayoutScope::Selection,),
            Err(GraphLayoutError::EmptyRegion)
        );
        assert!(matches!(
            PartialLayoutPlan::extract(
                &graph,
                &BTreeSet::from([id(99)]),
                PartialLayoutScope::Downstream,
            ),
            Err(GraphLayoutError::MissingRegionNode(key)) if key == id(99)
        ));
    }

    #[test]
    fn extracted_input_remains_valid_for_the_full_graph_engine() {
        let graph = input();
        let plan = PartialLayoutPlan::extract(
            &graph,
            &BTreeSet::from([id(1), id(2), id(3)]),
            PartialLayoutScope::Selection,
        )
        .unwrap();

        assert_eq!(plan.movable_input.region, GraphLayoutRegion::Full);
        plan.movable_input.validate().unwrap();
    }
}
