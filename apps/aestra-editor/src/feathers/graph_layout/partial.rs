//! Deterministic planning for targeted graph arrangement.
//!
//! The layered engine arranges a self-contained graph. This module owns the higher-level M10
//! boundary: expand a user seed into the requested branch, extract the movable induced subgraph,
//! remember every fixed node, and identify edges that attach the movable region to the frozen
//! graph. Placement and boundary-collision reconciliation consume this plan in later slices.

use super::{
    GraphLayoutEdge, GraphLayoutEngine, GraphLayoutError, GraphLayoutInput, GraphLayoutNode,
    GraphLayoutNodeId, GraphLayoutRegion, GraphLayoutResult,
};
use bevy::prelude::{Rect, Vec2};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const BOUNDARY_SPACING: f32 = 32.0;
const MAX_RECONCILIATION_CANDIDATES: usize = 512;
const MAX_BOUNDARY_DISPLACEMENT: f32 = 4_096.0;

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
    validation_input: GraphLayoutInput,
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
        let mut validation_input = input.clone();
        validation_input.region = GraphLayoutRegion::Nodes(region.clone());
        validation_input.validate()?;
        Ok(Self {
            region,
            movable_input,
            fixed_nodes,
            anchors,
            validation_input,
        })
    }

    /// Runs the selected backend only on the movable subgraph and reconciles its candidate with
    /// the frozen graph. No partial-layout policy crosses into the backend.
    pub(crate) fn layout(
        &self,
        engine: &impl GraphLayoutEngine,
    ) -> Result<GraphLayoutResult, GraphLayoutError> {
        let candidate = engine.layout(&self.movable_input)?;
        self.reconcile(&candidate)
    }

    /// Reanchors one validated movable candidate, resolves its boundary against frozen geometry,
    /// and returns a complete result whose out-of-region nodes remain byte-for-byte stationary.
    pub(crate) fn reconcile(
        &self,
        candidate: &GraphLayoutResult,
    ) -> Result<GraphLayoutResult, GraphLayoutError> {
        candidate.validate(&self.movable_input)?;
        let preferred = self.preferred_translation(candidate)?;
        let translation = self.find_boundary_translation(candidate.bounds, preferred)?;
        let mut positions = self
            .fixed_nodes
            .iter()
            .map(|(&key, node)| (key, node.position))
            .collect::<BTreeMap<_, _>>();
        positions.extend(
            candidate
                .positions
                .iter()
                .map(|(&key, &position)| (key, position + translation)),
        );
        GraphLayoutResult::from_positions(&self.validation_input, positions)
    }

    fn preferred_translation(
        &self,
        candidate: &GraphLayoutResult,
    ) -> Result<Vec2, GraphLayoutError> {
        let original = self
            .movable_input
            .nodes
            .iter()
            .map(|node| (node.key, node))
            .collect::<BTreeMap<_, _>>();
        let mut x = Vec::new();
        let mut y = Vec::new();
        for anchor in &self.anchors {
            let before = original[&anchor.movable].position;
            let after = candidate.positions[&anchor.movable];
            let delta = before - after;
            x.push(delta.x);
            y.push(delta.y);
        }
        if !x.is_empty() {
            return Ok(Vec2::new(median(&mut x), median(&mut y)));
        }

        let original_bounds = node_bounds(
            self.movable_input
                .nodes
                .iter()
                .map(|node| (node.position, node.size)),
        )
        .ok_or(GraphLayoutError::PartialLayoutConflict)?;
        Ok(original_bounds.center() - candidate.bounds.center())
    }

    fn find_boundary_translation(
        &self,
        candidate_bounds: Rect,
        preferred: Vec2,
    ) -> Result<Vec2, GraphLayoutError> {
        let preferred_min = candidate_bounds.min + preferred;
        let size = candidate_bounds.size();
        if !valid_rect(preferred_min, size) {
            return Err(GraphLayoutError::PartialLayoutConflict);
        }
        let obstacles = self
            .fixed_nodes
            .values()
            .map(|node| Rect::from_corners(node.position, node.position + node.size))
            .collect::<Vec<_>>();
        let free = |min: Vec2| {
            valid_rect(min, size)
                && obstacles
                    .iter()
                    .all(|obstacle| !rectangles_overlap(min, size, *obstacle))
        };
        if free(preferred_min) {
            return Ok(preferred);
        }

        let mut candidates = Vec::with_capacity(obstacles.len().saturating_mul(9) + 4);
        for obstacle in &obstacles {
            let xs = [
                preferred_min.x,
                obstacle.min.x - size.x - BOUNDARY_SPACING,
                obstacle.max.x + BOUNDARY_SPACING,
            ];
            let ys = [
                preferred_min.y,
                obstacle.min.y - size.y - BOUNDARY_SPACING,
                obstacle.max.y + BOUNDARY_SPACING,
            ];
            for x in xs {
                for y in ys {
                    candidates.push(Vec2::new(x, y));
                }
            }
        }
        if let Some(bounds) = fixed_bounds(&obstacles) {
            candidates.extend([
                Vec2::new(bounds.min.x - size.x - BOUNDARY_SPACING, preferred_min.y),
                Vec2::new(bounds.max.x + BOUNDARY_SPACING, preferred_min.y),
                Vec2::new(preferred_min.x, bounds.min.y - size.y - BOUNDARY_SPACING),
                Vec2::new(preferred_min.x, bounds.max.y + BOUNDARY_SPACING),
            ]);
        }
        candidates.retain(|position| {
            position.is_finite()
                && position.distance_squared(preferred_min)
                    <= MAX_BOUNDARY_DISPLACEMENT * MAX_BOUNDARY_DISPLACEMENT
        });
        candidates.sort_by(|left, right| {
            left.distance_squared(preferred_min)
                .total_cmp(&right.distance_squared(preferred_min))
                .then_with(|| left.x.total_cmp(&right.x))
                .then_with(|| left.y.total_cmp(&right.y))
        });
        candidates.dedup();
        candidates
            .into_iter()
            .take(MAX_RECONCILIATION_CANDIDATES)
            .find(|position| free(*position))
            .map(|min| preferred + (min - preferred_min))
            .ok_or(GraphLayoutError::PartialLayoutConflict)
    }
}

fn median(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        values[middle - 1] * 0.5 + values[middle] * 0.5
    } else {
        values[middle]
    }
}

fn node_bounds(nodes: impl Iterator<Item = (Vec2, Vec2)>) -> Option<Rect> {
    nodes
        .map(|(position, size)| Rect::from_corners(position, position + size))
        .reduce(|left, right| Rect::from_corners(left.min.min(right.min), left.max.max(right.max)))
}

fn fixed_bounds(obstacles: &[Rect]) -> Option<Rect> {
    obstacles
        .iter()
        .copied()
        .reduce(|left, right| Rect::from_corners(left.min.min(right.min), left.max.max(right.max)))
}

fn valid_rect(position: Vec2, size: Vec2) -> bool {
    position.is_finite()
        && size.is_finite()
        && size.min_element() > 0.0
        && (position + size).is_finite()
}

fn rectangles_overlap(min: Vec2, size: Vec2, obstacle: Rect) -> bool {
    min.x < obstacle.max.x + BOUNDARY_SPACING
        && obstacle.min.x < min.x + size.x + BOUNDARY_SPACING
        && min.y < obstacle.max.y + BOUNDARY_SPACING
        && obstacle.min.y < min.y + size.y + BOUNDARY_SPACING
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
    use crate::feathers::graph_layout::{
        GraphDirection, GraphLayoutNode, native::AestraLayeredLayout,
    };

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

    #[test]
    fn reconciled_candidate_keeps_every_frozen_node_exact_and_moves_the_region_rigidly() {
        let graph = input();
        let plan = PartialLayoutPlan::extract(
            &graph,
            &BTreeSet::from([id(1), id(2)]),
            PartialLayoutScope::Selection,
        )
        .unwrap();
        let candidate = AestraLayeredLayout.layout(&plan.movable_input).unwrap();
        let result = plan.reconcile(&candidate).unwrap();

        for (&key, node) in &plan.fixed_nodes {
            assert_eq!(result.positions[&key], node.position);
        }
        let first_delta = result.positions[&id(1)] - candidate.positions[&id(1)];
        let second_delta = result.positions[&id(2)] - candidate.positions[&id(2)];
        assert_eq!(first_delta, second_delta);
        result.validate(&plan.validation_input).unwrap();
        for movable in &plan.movable_input.nodes {
            let position = result.positions[&movable.key];
            for fixed in plan.fixed_nodes.values() {
                assert!(!rectangles_overlap(
                    position,
                    movable.size,
                    Rect::from_corners(fixed.position, fixed.position + fixed.size),
                ));
            }
        }
    }

    #[test]
    fn reconciliation_is_deterministic_for_identical_input() {
        let graph = input();
        let plan = PartialLayoutPlan::extract(
            &graph,
            &BTreeSet::from([id(2), id(3)]),
            PartialLayoutScope::Selection,
        )
        .unwrap();
        assert_eq!(
            plan.layout(&AestraLayeredLayout),
            plan.layout(&AestraLayeredLayout)
        );
    }

    #[test]
    fn isolated_region_stays_centered_in_its_existing_area() {
        let graph = GraphLayoutInput::try_new(
            GraphDirection::LeftToRight,
            vec![
                GraphLayoutNode {
                    key: id(0),
                    position: Vec2::new(1_000.0, 500.0),
                    size: Vec2::new(80.0, 50.0),
                    pinned: false,
                    selected: false,
                },
                GraphLayoutNode {
                    key: id(1),
                    position: Vec2::new(-500.0, -500.0),
                    size: Vec2::new(80.0, 50.0),
                    pinned: false,
                    selected: false,
                },
            ],
            vec![],
            GraphLayoutRegion::Full,
        )
        .unwrap();
        let plan = PartialLayoutPlan::extract(
            &graph,
            &BTreeSet::from([id(0)]),
            PartialLayoutScope::Selection,
        )
        .unwrap();
        let result = plan.layout(&AestraLayeredLayout).unwrap();

        assert_eq!(result.positions[&id(0)], Vec2::new(1_000.0, 500.0));
        assert_eq!(result.positions[&id(1)], Vec2::new(-500.0, -500.0));
    }

    #[test]
    fn reconciliation_reports_a_conflict_instead_of_leaving_the_local_area() {
        let graph = GraphLayoutInput::try_new(
            GraphDirection::LeftToRight,
            vec![
                GraphLayoutNode {
                    key: id(0),
                    position: Vec2::new(-10_000.0, -10_000.0),
                    size: Vec2::splat(20_000.0),
                    pinned: false,
                    selected: false,
                },
                GraphLayoutNode {
                    key: id(1),
                    position: Vec2::ZERO,
                    size: Vec2::splat(100.0),
                    pinned: false,
                    selected: false,
                },
            ],
            vec![],
            GraphLayoutRegion::Full,
        )
        .unwrap();
        let plan = PartialLayoutPlan::extract(
            &graph,
            &BTreeSet::from([id(1)]),
            PartialLayoutScope::Selection,
        )
        .unwrap();
        assert_eq!(
            plan.layout(&AestraLayeredLayout),
            Err(GraphLayoutError::PartialLayoutConflict)
        );
    }
}
