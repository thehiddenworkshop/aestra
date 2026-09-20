//! Deterministic, Aestra-owned layered-layout preparation.
//!
//! M9A.1 deliberately stops at a canonical DAG. Later milestones consume this representation
//! for layer assignment, virtual nodes, crossing reduction and coordinate generation. The
//! production `elkrs` adapter remains active until that pipeline satisfies the acceptance gates.

use super::{
    GraphDirection, GraphLayoutEdge, GraphLayoutError, GraphLayoutInput, GraphLayoutNode,
    GraphLayoutNodeId, GraphLayoutRegion,
};
use std::collections::{BTreeMap, BTreeSet};

mod crossings;
pub(crate) use crossings::CrossingMetrics;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AestraLayeredLayout;

impl AestraLayeredLayout {
    /// Builds the stable topology consumed by the native layered-layout stages.
    pub(crate) fn canonicalize(
        &self,
        input: &GraphLayoutInput,
    ) -> Result<CanonicalGraph, GraphLayoutError> {
        CanonicalGraph::try_from_input(input)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CanonicalGraph {
    pub(crate) direction: GraphDirection,
    pub(crate) region: GraphLayoutRegion,
    pub(crate) nodes: Vec<GraphLayoutNode>,
    pub(crate) edges: Vec<GraphLayoutEdge>,
    pub(crate) predecessors: BTreeMap<GraphLayoutNodeId, Vec<GraphLayoutNodeId>>,
    pub(crate) successors: BTreeMap<GraphLayoutNodeId, Vec<GraphLayoutNodeId>>,
    pub(crate) components: Vec<Vec<GraphLayoutNodeId>>,
    pub(crate) topological_order: Vec<GraphLayoutNodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LayerAssignment {
    pub(crate) ranks: BTreeMap<GraphLayoutNodeId, usize>,
    pub(crate) components: Vec<RankedComponent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RankedComponent {
    pub(crate) nodes: Vec<GraphLayoutNodeId>,
    pub(crate) layers: Vec<Vec<GraphLayoutNodeId>>,
}

/// Internal layered-graph identity. Only `Real` IDs can ever map back to editor nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LayerNodeId {
    Real(GraphLayoutNodeId),
    Virtual(VirtualNodeId),
}

/// A deterministic layout-only identity derived from the full canonical edge and its rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct VirtualNodeId {
    pub(crate) edge: GraphLayoutEdge,
    pub(crate) rank: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LayerEdge {
    pub(crate) source: LayerNodeId,
    pub(crate) target: LayerNodeId,
    pub(crate) original: GraphLayoutEdge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpandedComponent {
    pub(crate) real_nodes: Vec<GraphLayoutNodeId>,
    pub(crate) layers: Vec<Vec<LayerNodeId>>,
    pub(crate) edges: Vec<LayerEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpandedGraph {
    /// Ranks exist only for real/editor nodes. Virtual ranks live exclusively in their IDs.
    pub(crate) real_ranks: BTreeMap<GraphLayoutNodeId, usize>,
    pub(crate) components: Vec<ExpandedComponent>,
}

impl ExpandedGraph {
    pub(crate) fn rank(&self, node: LayerNodeId) -> Option<usize> {
        match node {
            LayerNodeId::Real(key) => self.real_ranks.get(&key).copied(),
            LayerNodeId::Virtual(node) => Some(node.rank),
        }
    }

    /// Deterministically reduces crossings without changing ranks or topology.
    pub(crate) fn minimize_crossings(&mut self) -> CrossingMetrics {
        crossings::minimize(self)
    }
}

impl CanonicalGraph {
    fn try_from_input(input: &GraphLayoutInput) -> Result<Self, GraphLayoutError> {
        input.validate()?;

        let mut nodes = input.nodes.clone();
        nodes.sort_by_key(|node| node.key);

        // Exact duplicates carry no additional topology and would otherwise make later stages
        // sensitive to serialization accidents. Port-distinct edges remain distinct.
        let mut edges = input.edges.clone();
        edges.sort();
        edges.dedup();

        let keys = nodes.iter().map(|node| node.key).collect::<Vec<_>>();
        let mut predecessor_sets = empty_neighbor_sets(&keys);
        let mut successor_sets = empty_neighbor_sets(&keys);
        for edge in &edges {
            predecessor_sets
                .get_mut(&edge.target)
                .expect("validated target must exist")
                .insert(edge.source);
            successor_sets
                .get_mut(&edge.source)
                .expect("validated source must exist")
                .insert(edge.target);
        }

        let predecessors = sorted_neighbors(predecessor_sets);
        let successors = sorted_neighbors(successor_sets);
        let topological_order = stable_topological_order(&keys, &predecessors, &successors)
            .ok_or_else(|| GraphLayoutError::CycleDetected(find_cycle(&keys, &successors)))?;
        let components = weak_components(&keys, &predecessors, &successors);

        Ok(Self {
            direction: input.direction,
            region: input.region.clone(),
            nodes,
            edges,
            predecessors,
            successors,
            components,
            topological_order,
        })
    }

    /// Assigns each component independently using the longest path from any source.
    ///
    /// Rank zero is the component's source side. Every edge advances by at least one rank and
    /// stable node keys order peers inside a rank. Material/function output nodes are ordinary
    /// sinks at this semantic-neutral boundary; because they consume the graph's output branches,
    /// the longest-path rule naturally places them at the component's final rank.
    pub(crate) fn longest_path_layers(&self) -> LayerAssignment {
        let mut ranks = BTreeMap::new();
        let mut components = Vec::with_capacity(self.components.len());

        for component in &self.components {
            let component_nodes = component.iter().copied().collect::<BTreeSet<_>>();
            for &key in &self.topological_order {
                if !component_nodes.contains(&key) {
                    continue;
                }
                let rank = self
                    .predecessors
                    .get(&key)
                    .into_iter()
                    .flatten()
                    .filter_map(|predecessor| ranks.get(predecessor).copied())
                    .max()
                    .map_or(0, |rank| rank + 1);
                ranks.insert(key, rank);
            }

            let layer_count = component
                .iter()
                .filter_map(|key| ranks.get(key).copied())
                .max()
                .map_or(0, |rank| rank + 1);
            let mut layers = vec![Vec::new(); layer_count];
            for &key in component {
                if let Some(&rank) = ranks.get(&key) {
                    layers[rank].push(key);
                }
            }
            components.push(RankedComponent {
                nodes: component.clone(),
                layers,
            });
        }

        LayerAssignment { ranks, components }
    }

    /// Replaces each edge spanning multiple ranks with adjacent-rank layout segments.
    ///
    /// Virtual nodes use a type that cannot be confused with an editor `GraphLayoutNodeId` and
    /// are retained only by the native layout pipeline. Each segment remembers its canonical edge
    /// so later routing can collapse the chain back to one authored connection.
    pub(crate) fn expand_long_edges(&self) -> ExpandedGraph {
        let assignment = self.longest_path_layers();
        let mut component_by_node = BTreeMap::new();
        let mut components = assignment
            .components
            .iter()
            .enumerate()
            .map(|(component_index, component)| {
                for &key in &component.nodes {
                    component_by_node.insert(key, component_index);
                }
                ExpandedComponent {
                    real_nodes: component.nodes.clone(),
                    layers: component
                        .layers
                        .iter()
                        .map(|layer| layer.iter().copied().map(LayerNodeId::Real).collect())
                        .collect(),
                    edges: Vec::new(),
                }
            })
            .collect::<Vec<_>>();

        for &edge in &self.edges {
            let (Some(&source_rank), Some(&target_rank), Some(&component_index)) = (
                assignment.ranks.get(&edge.source),
                assignment.ranks.get(&edge.target),
                component_by_node.get(&edge.source),
            ) else {
                debug_assert!(false, "canonical edge must have ranks and a component");
                continue;
            };
            let Some(component) = components.get_mut(component_index) else {
                debug_assert!(false, "canonical edge component must exist");
                continue;
            };
            debug_assert!(target_rank > source_rank);

            let mut previous = LayerNodeId::Real(edge.source);
            for rank in (source_rank + 1)..target_rank {
                let virtual_node = LayerNodeId::Virtual(VirtualNodeId { edge, rank });
                let Some(layer) = component.layers.get_mut(rank) else {
                    debug_assert!(false, "virtual rank must exist in its component");
                    continue;
                };
                layer.push(virtual_node);
                component.edges.push(LayerEdge {
                    source: previous,
                    target: virtual_node,
                    original: edge,
                });
                previous = virtual_node;
            }
            component.edges.push(LayerEdge {
                source: previous,
                target: LayerNodeId::Real(edge.target),
                original: edge,
            });
        }

        for component in &mut components {
            for layer in &mut component.layers {
                layer.sort();
                layer.dedup();
            }
            component.edges.sort();
            component.edges.dedup();
        }

        ExpandedGraph {
            real_ranks: assignment.ranks,
            components,
        }
    }
}

fn empty_neighbor_sets(
    keys: &[GraphLayoutNodeId],
) -> BTreeMap<GraphLayoutNodeId, BTreeSet<GraphLayoutNodeId>> {
    keys.iter()
        .copied()
        .map(|key| (key, BTreeSet::new()))
        .collect()
}

fn sorted_neighbors(
    neighbors: BTreeMap<GraphLayoutNodeId, BTreeSet<GraphLayoutNodeId>>,
) -> BTreeMap<GraphLayoutNodeId, Vec<GraphLayoutNodeId>> {
    neighbors
        .into_iter()
        .map(|(key, neighbors)| (key, neighbors.into_iter().collect()))
        .collect()
}

fn stable_topological_order(
    keys: &[GraphLayoutNodeId],
    predecessors: &BTreeMap<GraphLayoutNodeId, Vec<GraphLayoutNodeId>>,
    successors: &BTreeMap<GraphLayoutNodeId, Vec<GraphLayoutNodeId>>,
) -> Option<Vec<GraphLayoutNodeId>> {
    let mut indegrees = predecessors
        .iter()
        .map(|(&key, neighbors)| (key, neighbors.len()))
        .collect::<BTreeMap<_, _>>();
    let mut ready = keys
        .iter()
        .copied()
        .filter(|key| indegrees[key] == 0)
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(keys.len());

    while let Some(key) = ready.pop_first() {
        order.push(key);
        for successor in &successors[&key] {
            let indegree = indegrees
                .get_mut(successor)
                .expect("canonical successor must exist");
            *indegree -= 1;
            if *indegree == 0 {
                ready.insert(*successor);
            }
        }
    }

    (order.len() == keys.len()).then_some(order)
}

fn weak_components(
    keys: &[GraphLayoutNodeId],
    predecessors: &BTreeMap<GraphLayoutNodeId, Vec<GraphLayoutNodeId>>,
    successors: &BTreeMap<GraphLayoutNodeId, Vec<GraphLayoutNodeId>>,
) -> Vec<Vec<GraphLayoutNodeId>> {
    let mut visited = BTreeSet::new();
    let mut components = Vec::new();

    for &start in keys {
        if visited.contains(&start) {
            continue;
        }
        let mut pending = BTreeSet::from([start]);
        let mut component = Vec::new();
        while let Some(key) = pending.pop_first() {
            if !visited.insert(key) {
                continue;
            }
            component.push(key);
            pending.extend(
                predecessors[&key]
                    .iter()
                    .chain(&successors[&key])
                    .copied()
                    .filter(|neighbor| !visited.contains(neighbor)),
            );
        }
        component.sort();
        components.push(component);
    }

    components
}

/// Returns one deterministic simple cycle, rotated so its smallest node is first.
fn find_cycle(
    keys: &[GraphLayoutNodeId],
    successors: &BTreeMap<GraphLayoutNodeId, Vec<GraphLayoutNodeId>>,
) -> Vec<GraphLayoutNodeId> {
    let mut colors = keys
        .iter()
        .copied()
        .map(|key| (key, 0_u8))
        .collect::<BTreeMap<_, _>>();

    for &start in keys {
        if colors[&start] != 0 {
            continue;
        }
        colors.insert(start, 1);
        let mut path = vec![start];
        let mut frames = vec![(start, 0_usize)];

        while let Some((key, next_index)) = frames.last_mut() {
            if let Some(&next) = successors[key].get(*next_index) {
                *next_index += 1;
                match colors[&next] {
                    0 => {
                        colors.insert(next, 1);
                        path.push(next);
                        frames.push((next, 0));
                    }
                    1 => {
                        let cycle_start = path
                            .iter()
                            .position(|candidate| *candidate == next)
                            .expect("active node must be on the DFS path");
                        let mut cycle = path[cycle_start..].to_vec();
                        let smallest = cycle
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, key)| **key)
                            .map(|(index, _)| index)
                            .unwrap_or(0);
                        cycle.rotate_left(smallest);
                        return cycle;
                    }
                    _ => {}
                }
            } else {
                let (finished, _) = frames.pop().expect("current frame must exist");
                let path_node = path.pop().expect("current path node must exist");
                debug_assert_eq!(finished, path_node);
                colors.insert(finished, 2);
            }
        }
    }

    debug_assert!(
        false,
        "topological validation reported a cycle without a cycle path"
    );
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feathers::graph_layout::{ElkLayeredLayout, GraphLayoutEngine};
    use bevy::prelude::Vec2;

    fn id(value: usize) -> GraphLayoutNodeId {
        GraphLayoutNodeId::from_index(value).unwrap()
    }

    fn node(value: usize) -> GraphLayoutNode {
        GraphLayoutNode {
            key: id(value),
            position: Vec2::new(value as f32 * 10.0, 0.0),
            size: Vec2::new(80.0, 40.0),
            pinned: false,
            selected: false,
        }
    }

    fn edge(source: usize, target: usize) -> GraphLayoutEdge {
        GraphLayoutEdge {
            source: id(source),
            target: id(target),
            source_port: None,
            target_port: None,
        }
    }

    fn input(nodes: &[usize], edges: &[(usize, usize)]) -> GraphLayoutInput {
        GraphLayoutInput::try_new(
            GraphDirection::LeftToRight,
            nodes.iter().copied().map(node).collect(),
            edges
                .iter()
                .copied()
                .map(|(source, target)| edge(source, target))
                .collect(),
            GraphLayoutRegion::Full,
        )
        .unwrap()
    }

    #[test]
    fn canonical_output_is_independent_of_input_storage_order() {
        let ordered = input(&[0, 1, 2, 3], &[(0, 2), (1, 2), (2, 3)]);
        let mut shuffled = ordered.clone();
        shuffled.nodes.reverse();
        shuffled.edges.reverse();
        shuffled.edges.push(edge(1, 2));

        let expected = AestraLayeredLayout.canonicalize(&ordered).unwrap();
        let actual = AestraLayeredLayout.canonicalize(&shuffled).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn components_neighbors_and_topological_order_are_stable() {
        let graph = AestraLayeredLayout
            .canonicalize(&input(&[5, 4, 3, 2, 1, 0], &[(1, 2), (0, 2), (3, 4)]))
            .unwrap();

        assert_eq!(graph.predecessors[&id(2)], [id(0), id(1)]);
        assert_eq!(graph.successors[&id(0)], [id(2)]);
        assert!(graph.successors[&id(5)].is_empty());
        assert_eq!(
            graph.components,
            [vec![id(0), id(1), id(2)], vec![id(3), id(4)], vec![id(5)]]
        );
        assert_eq!(
            graph.topological_order,
            [id(0), id(1), id(2), id(3), id(4), id(5)]
        );
    }

    #[test]
    fn parallel_port_edges_have_one_topological_relationship() {
        let mut input = input(&[0, 1], &[(0, 1)]);
        input.edges.push(GraphLayoutEdge {
            source: id(0),
            target: id(1),
            source_port: Some(super::super::GraphLayoutPortId(2)),
            target_port: Some(super::super::GraphLayoutPortId(3)),
        });
        let graph = AestraLayeredLayout.canonicalize(&input).unwrap();

        assert_eq!(graph.edges.len(), 2);
        assert_eq!(graph.successors[&id(0)], [id(1)]);
        assert_eq!(graph.predecessors[&id(1)], [id(0)]);
        assert_eq!(graph.topological_order, [id(0), id(1)]);
    }

    #[test]
    fn cycle_error_contains_a_stable_simple_cycle_not_its_dependents() {
        let cyclic = input(&[0, 1, 2, 3], &[(0, 1), (1, 2), (2, 0), (2, 3)]);
        assert_eq!(
            AestraLayeredLayout.canonicalize(&cyclic),
            Err(GraphLayoutError::CycleDetected(vec![id(0), id(1), id(2)]))
        );

        let self_loop = input(&[0], &[(0, 0)]);
        assert_eq!(
            AestraLayeredLayout.canonicalize(&self_loop),
            Err(GraphLayoutError::CycleDetected(vec![id(0)]))
        );
    }

    #[test]
    fn empty_graph_has_empty_canonical_collections() {
        let graph = AestraLayeredLayout.canonicalize(&input(&[], &[])).unwrap();
        assert!(graph.nodes.is_empty());
        assert!(graph.edges.is_empty());
        assert!(graph.components.is_empty());
        assert!(graph.topological_order.is_empty());
    }

    #[test]
    fn longest_path_ranks_advance_dependencies_and_put_output_last() {
        // Node 5 represents the semantic output and consumes both a short and a long branch.
        let graph = AestraLayeredLayout
            .canonicalize(&input(
                &[0, 1, 2, 3, 4, 5],
                &[(0, 2), (1, 3), (3, 4), (2, 5), (4, 5)],
            ))
            .unwrap();
        let assignment = graph.longest_path_layers();

        assert_eq!(assignment.ranks[&id(0)], 0);
        assert_eq!(assignment.ranks[&id(1)], 0);
        assert_eq!(assignment.ranks[&id(2)], 1);
        assert_eq!(assignment.ranks[&id(3)], 1);
        assert_eq!(assignment.ranks[&id(4)], 2);
        assert_eq!(assignment.ranks[&id(5)], 3);
        assert_eq!(
            assignment.components[0].layers,
            [
                vec![id(0), id(1)],
                vec![id(2), id(3)],
                vec![id(4)],
                vec![id(5)]
            ]
        );
        for edge in &graph.edges {
            assert!(assignment.ranks[&edge.source] < assignment.ranks[&edge.target]);
        }
    }

    #[test]
    fn disconnected_components_start_at_zero_and_keep_independent_depths() {
        let graph = AestraLayeredLayout
            .canonicalize(&input(&[0, 1, 2, 3, 4, 5], &[(0, 1), (1, 2), (3, 4)]))
            .unwrap();
        let assignment = graph.longest_path_layers();

        assert_eq!(
            assignment.components,
            [
                RankedComponent {
                    nodes: vec![id(0), id(1), id(2)],
                    layers: vec![vec![id(0)], vec![id(1)], vec![id(2)]],
                },
                RankedComponent {
                    nodes: vec![id(3), id(4)],
                    layers: vec![vec![id(3)], vec![id(4)]],
                },
                RankedComponent {
                    nodes: vec![id(5)],
                    layers: vec![vec![id(5)]],
                },
            ]
        );
        assert_eq!(assignment.ranks[&id(3)], 0);
        assert_eq!(assignment.ranks[&id(5)], 0);
    }

    #[test]
    fn longest_path_assignment_is_stable_across_input_order_and_direction() {
        let ordered = input(&[0, 1, 2, 3], &[(0, 2), (1, 2), (2, 3)]);
        let mut shuffled = ordered.clone();
        shuffled.nodes.reverse();
        shuffled.edges.reverse();
        shuffled.direction = GraphDirection::TopToBottom;

        let expected = AestraLayeredLayout
            .canonicalize(&ordered)
            .unwrap()
            .longest_path_layers();
        let actual = AestraLayeredLayout
            .canonicalize(&shuffled)
            .unwrap()
            .longest_path_layers();
        assert_eq!(actual, expected);
    }

    #[test]
    fn empty_graph_has_empty_layer_assignment() {
        let assignment = AestraLayeredLayout
            .canonicalize(&input(&[], &[]))
            .unwrap()
            .longest_path_layers();
        assert!(assignment.ranks.is_empty());
        assert!(assignment.components.is_empty());
    }

    #[test]
    fn long_edges_expand_into_adjacent_rank_segments() {
        let graph = AestraLayeredLayout
            .canonicalize(&input(&[0, 1, 2, 3], &[(0, 1), (1, 2), (2, 3), (0, 3)]))
            .unwrap();
        let long_edge = edge(0, 3);
        let first = LayerNodeId::Virtual(VirtualNodeId {
            edge: long_edge,
            rank: 1,
        });
        let second = LayerNodeId::Virtual(VirtualNodeId {
            edge: long_edge,
            rank: 2,
        });
        let expanded = graph.expand_long_edges();
        let component = &expanded.components[0];

        assert!(component.layers[1].contains(&first));
        assert!(component.layers[2].contains(&second));
        assert!(component.edges.contains(&LayerEdge {
            source: LayerNodeId::Real(id(0)),
            target: first,
            original: long_edge,
        }));
        assert!(component.edges.contains(&LayerEdge {
            source: first,
            target: second,
            original: long_edge,
        }));
        assert!(component.edges.contains(&LayerEdge {
            source: second,
            target: LayerNodeId::Real(id(3)),
            original: long_edge,
        }));
        for edge in &component.edges {
            assert_eq!(
                expanded.rank(edge.target),
                expanded.rank(edge.source).map(|rank| rank + 1)
            );
        }
    }

    #[test]
    fn adjacent_edges_do_not_create_virtual_nodes() {
        let graph = AestraLayeredLayout
            .canonicalize(&input(&[0, 1, 2], &[(0, 1), (1, 2)]))
            .unwrap();
        let expanded = graph.expand_long_edges();
        let component = &expanded.components[0];

        assert!(
            component
                .layers
                .iter()
                .flatten()
                .all(|node| matches!(node, LayerNodeId::Real(_)))
        );
        assert_eq!(component.edges.len(), 2);
        assert_eq!(
            component.real_nodes,
            graph.nodes.iter().map(|node| node.key).collect::<Vec<_>>()
        );
    }

    #[test]
    fn parallel_long_edges_receive_distinct_deterministic_virtual_ids() {
        let mut input = input(&[0, 1, 2, 3], &[(0, 1), (1, 2), (2, 3), (0, 3)]);
        input.edges.push(GraphLayoutEdge {
            source: id(0),
            target: id(3),
            source_port: Some(super::super::GraphLayoutPortId(7)),
            target_port: Some(super::super::GraphLayoutPortId(9)),
        });
        let mut shuffled = input.clone();
        shuffled.nodes.reverse();
        shuffled.edges.reverse();

        let expected = AestraLayeredLayout
            .canonicalize(&input)
            .unwrap()
            .expand_long_edges();
        let actual = AestraLayeredLayout
            .canonicalize(&shuffled)
            .unwrap()
            .expand_long_edges();
        assert_eq!(actual, expected);

        let virtual_nodes = actual.components[0]
            .layers
            .iter()
            .flatten()
            .filter(|node| matches!(node, LayerNodeId::Virtual(_)))
            .copied()
            .collect::<BTreeSet<_>>();
        assert_eq!(virtual_nodes.len(), 4);
    }

    #[test]
    fn virtual_nodes_stay_out_of_real_identity_and_empty_components() {
        let graph = AestraLayeredLayout
            .canonicalize(&input(&[0, 1, 2, 3, 4], &[(0, 1), (1, 2), (2, 3), (0, 3)]))
            .unwrap();
        let expanded = graph.expand_long_edges();

        assert_eq!(expanded.real_ranks.len(), graph.nodes.len());
        assert_eq!(expanded.components[1].real_nodes, [id(4)]);
        assert_eq!(
            expanded.components[1].layers,
            [vec![LayerNodeId::Real(id(4))]]
        );
        assert!(expanded.components[1].edges.is_empty());

        let empty = AestraLayeredLayout
            .canonicalize(&input(&[], &[]))
            .unwrap()
            .expand_long_edges();
        assert!(empty.real_ranks.is_empty());
        assert!(empty.components.is_empty());
    }

    #[test]
    fn crossing_minimization_orders_a_crossed_two_layer_graph() {
        let graph = AestraLayeredLayout
            .canonicalize(&input(&[0, 1, 2, 3], &[(0, 2), (0, 3), (1, 2)]))
            .unwrap();
        let mut expanded = graph.expand_long_edges();
        let metrics = expanded.minimize_crossings();

        assert_eq!(metrics.crossings_before, 1);
        assert_eq!(metrics.crossings_after, 0);
        assert!(metrics.sweep_pairs > 0);
        assert_eq!(
            expanded.components[0].layers,
            [
                vec![LayerNodeId::Real(id(0)), LayerNodeId::Real(id(1))],
                vec![LayerNodeId::Real(id(3)), LayerNodeId::Real(id(2))]
            ]
        );
    }

    #[test]
    fn crossing_minimization_is_deterministic_with_virtual_nodes() {
        let ordered = input(
            &[0, 1, 2, 3, 4, 5],
            &[(0, 2), (1, 3), (2, 4), (3, 5), (0, 5), (1, 4)],
        );
        let mut shuffled = ordered.clone();
        shuffled.nodes.reverse();
        shuffled.edges.reverse();

        let mut expected = AestraLayeredLayout
            .canonicalize(&ordered)
            .unwrap()
            .expand_long_edges();
        let expected_metrics = expected.minimize_crossings();
        let mut actual = AestraLayeredLayout
            .canonicalize(&shuffled)
            .unwrap()
            .expand_long_edges();
        let actual_metrics = actual.minimize_crossings();

        assert_eq!(actual, expected);
        assert_eq!(actual_metrics, expected_metrics);
        assert!(actual_metrics.crossings_after <= actual_metrics.crossings_before);
        assert!(
            actual.components[0]
                .layers
                .iter()
                .flatten()
                .any(|node| matches!(node, LayerNodeId::Virtual(_)))
        );
    }

    #[test]
    fn crossing_minimization_matches_elkrs_on_a_representative_branch_graph() {
        let input = input(
            &[0, 1, 2, 3, 4, 5, 6],
            &[(6, 0), (6, 1), (6, 2), (0, 5), (1, 4), (2, 3)],
        );
        let elk = ElkLayeredLayout.layout(&input).unwrap();
        let terminal_edges = input
            .edges
            .iter()
            .filter(|edge| edge.source != id(6))
            .copied()
            .collect::<Vec<_>>();
        let elk_crossings = terminal_edges
            .iter()
            .enumerate()
            .map(|(index, first)| {
                terminal_edges
                    .iter()
                    .skip(index + 1)
                    .filter(|second| {
                        (elk.positions[&first.source].y - elk.positions[&second.source].y)
                            * (elk.positions[&first.target].y - elk.positions[&second.target].y)
                            < 0.0
                    })
                    .count()
            })
            .sum::<usize>();

        let mut expanded = AestraLayeredLayout
            .canonicalize(&input)
            .unwrap()
            .expand_long_edges();
        let metrics = expanded.minimize_crossings();

        assert_eq!(metrics.crossings_before, 3);
        assert_eq!(metrics.crossings_after, elk_crossings);
        assert_eq!(metrics.crossings_after, 0);
    }

    #[test]
    fn crossing_minimization_keeps_zero_crossing_and_empty_graphs_stable() {
        let graph = AestraLayeredLayout
            .canonicalize(&input(&[0, 1, 2], &[(0, 1), (1, 2)]))
            .unwrap();
        let mut expanded = graph.expand_long_edges();
        let before = expanded.clone();
        assert_eq!(expanded.minimize_crossings(), CrossingMetrics::default());
        assert_eq!(expanded, before);

        let mut empty = AestraLayeredLayout
            .canonicalize(&input(&[], &[]))
            .unwrap()
            .expand_long_edges();
        assert_eq!(empty.minimize_crossings(), CrossingMetrics::default());
    }
}
