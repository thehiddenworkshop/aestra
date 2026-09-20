//! Deterministic crossing minimization for the expanded adjacent-rank graph.

use super::{ExpandedComponent, ExpandedGraph, LayerNodeId};
use std::{cmp::Ordering, collections::BTreeMap};

const MAX_SWEEP_PAIRS: usize = 8;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CrossingMetrics {
    pub(crate) crossings_before: usize,
    pub(crate) crossings_after: usize,
    pub(crate) sweep_pairs: usize,
}

pub(super) fn minimize(graph: &mut ExpandedGraph) -> CrossingMetrics {
    let mut metrics = CrossingMetrics::default();
    for component in &mut graph.components {
        let result = minimize_component(component);
        metrics.crossings_before += result.crossings_before;
        metrics.crossings_after += result.crossings_after;
        metrics.sweep_pairs = metrics.sweep_pairs.max(result.sweep_pairs);
    }
    metrics
}

fn minimize_component(component: &mut ExpandedComponent) -> CrossingMetrics {
    let crossings_before = crossing_count(component);
    if crossings_before == 0 || component.layers.len() < 2 {
        return CrossingMetrics {
            crossings_before,
            crossings_after: crossings_before,
            sweep_pairs: 0,
        };
    }

    let neighbors = Neighbors::new(component);
    let mut best_layers = component.layers.clone();
    let mut best_crossings = crossings_before;
    let mut sweep_pairs = 0;

    for _ in 0..MAX_SWEEP_PAIRS {
        sweep_pairs += 1;
        let mut changed = sweep_down(component, &neighbors);
        changed |= transpose_component(component, &neighbors);
        changed |= sweep_up(component, &neighbors);
        changed |= transpose_component(component, &neighbors);

        let crossings = crossing_count(component);
        if crossings < best_crossings {
            best_crossings = crossings;
            best_layers.clone_from(&component.layers);
        }
        if best_crossings == 0 || !changed {
            break;
        }
    }

    component.layers = best_layers;
    CrossingMetrics {
        crossings_before,
        crossings_after: best_crossings,
        sweep_pairs,
    }
}

#[derive(Default)]
struct Neighbors {
    incoming: BTreeMap<LayerNodeId, Vec<LayerNodeId>>,
    outgoing: BTreeMap<LayerNodeId, Vec<LayerNodeId>>,
}

impl Neighbors {
    fn new(component: &ExpandedComponent) -> Self {
        let mut result = Self::default();
        for edge in &component.edges {
            result
                .incoming
                .entry(edge.target)
                .or_default()
                .push(edge.source);
            result
                .outgoing
                .entry(edge.source)
                .or_default()
                .push(edge.target);
        }
        for neighbors in result.incoming.values_mut() {
            neighbors.sort();
        }
        for neighbors in result.outgoing.values_mut() {
            neighbors.sort();
        }
        result
    }
}

fn sweep_down(component: &mut ExpandedComponent, neighbors: &Neighbors) -> bool {
    let mut changed = false;
    for rank in 1..component.layers.len() {
        let adjacent = component.layers[rank - 1].clone();
        changed |=
            reorder_by_barycenter(&mut component.layers[rank], &adjacent, &neighbors.incoming);
    }
    changed
}

fn sweep_up(component: &mut ExpandedComponent, neighbors: &Neighbors) -> bool {
    let mut changed = false;
    for rank in (0..component.layers.len() - 1).rev() {
        let adjacent = component.layers[rank + 1].clone();
        changed |=
            reorder_by_barycenter(&mut component.layers[rank], &adjacent, &neighbors.outgoing);
    }
    changed
}

#[derive(Clone, Copy)]
struct ScoredNode {
    node: LayerNodeId,
    original: usize,
    sum: usize,
    count: usize,
}

fn reorder_by_barycenter(
    layer: &mut Vec<LayerNodeId>,
    adjacent: &[LayerNodeId],
    neighbors: &BTreeMap<LayerNodeId, Vec<LayerNodeId>>,
) -> bool {
    let positions = adjacent
        .iter()
        .enumerate()
        .map(|(index, &node)| (node, index))
        .collect::<BTreeMap<_, _>>();
    let mut scored = layer
        .iter()
        .copied()
        .enumerate()
        .map(|(original, node)| {
            let (sum, count) = neighbors
                .get(&node)
                .into_iter()
                .flatten()
                .filter_map(|neighbor| positions.get(neighbor).copied())
                .fold((0_usize, 0_usize), |(sum, count), position| {
                    (sum + position, count + 1)
                });
            if count == 0 {
                ScoredNode {
                    node,
                    original,
                    sum: original,
                    count: 1,
                }
            } else {
                ScoredNode {
                    node,
                    original,
                    sum,
                    count,
                }
            }
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        compare_fraction(left.sum, left.count, right.sum, right.count)
            .then_with(|| left.node.cmp(&right.node))
            .then_with(|| left.original.cmp(&right.original))
    });
    let reordered = scored
        .into_iter()
        .map(|entry| entry.node)
        .collect::<Vec<_>>();
    if reordered == *layer {
        false
    } else {
        *layer = reordered;
        true
    }
}

fn compare_fraction(
    left_sum: usize,
    left_count: usize,
    right_sum: usize,
    right_count: usize,
) -> Ordering {
    ((left_sum as u128) * (right_count as u128)).cmp(&((right_sum as u128) * (left_count as u128)))
}

/// Repeatedly swaps adjacent peers only when the swap strictly lowers crossings on the two
/// neighboring rank boundaries. Every accepted swap therefore decreases total crossings.
fn transpose_component(component: &mut ExpandedComponent, neighbors: &Neighbors) -> bool {
    let mut changed = false;
    for rank in 0..component.layers.len() {
        let layer_len = component.layers[rank].len();
        if layer_len < 2 {
            continue;
        }
        let previous = rank
            .checked_sub(1)
            .map(|previous| positions(&component.layers[previous]));
        let next = component.layers.get(rank + 1).map(|next| positions(next));

        for _ in 0..layer_len {
            let mut pass_changed = false;
            for index in 0..(layer_len - 1) {
                let left = component.layers[rank][index];
                let right = component.layers[rank][index + 1];
                let mut before = 0;
                let mut after = 0;
                if let Some(previous) = &previous {
                    let cost = pair_crossings(
                        neighbors.incoming.get(&left),
                        neighbors.incoming.get(&right),
                        previous,
                    );
                    before += cost.0;
                    after += cost.1;
                }
                if let Some(next) = &next {
                    let cost = pair_crossings(
                        neighbors.outgoing.get(&left),
                        neighbors.outgoing.get(&right),
                        next,
                    );
                    before += cost.0;
                    after += cost.1;
                }
                if after < before {
                    component.layers[rank].swap(index, index + 1);
                    changed = true;
                    pass_changed = true;
                }
            }
            if !pass_changed {
                break;
            }
        }
    }
    changed
}

fn positions(layer: &[LayerNodeId]) -> BTreeMap<LayerNodeId, usize> {
    layer
        .iter()
        .enumerate()
        .map(|(index, &node)| (node, index))
        .collect()
}

fn pair_crossings(
    left_neighbors: Option<&Vec<LayerNodeId>>,
    right_neighbors: Option<&Vec<LayerNodeId>>,
    positions: &BTreeMap<LayerNodeId, usize>,
) -> (usize, usize) {
    let mut before = 0;
    let mut after = 0;
    for left in left_neighbors.into_iter().flatten() {
        let Some(&left_position) = positions.get(left) else {
            continue;
        };
        for right in right_neighbors.into_iter().flatten() {
            let Some(&right_position) = positions.get(right) else {
                continue;
            };
            match left_position.cmp(&right_position) {
                Ordering::Greater => before += 1,
                Ordering::Less => after += 1,
                Ordering::Equal => {}
            }
        }
    }
    (before, after)
}

fn crossing_count(component: &ExpandedComponent) -> usize {
    if component.layers.len() < 2 {
        return 0;
    }
    let locations = component
        .layers
        .iter()
        .enumerate()
        .flat_map(|(rank, layer)| {
            layer
                .iter()
                .enumerate()
                .map(move |(position, &node)| (node, (rank, position)))
        })
        .collect::<BTreeMap<_, _>>();
    let mut boundaries = vec![Vec::new(); component.layers.len() - 1];
    for edge in &component.edges {
        let (Some(&(source_rank, source_position)), Some(&(target_rank, target_position))) =
            (locations.get(&edge.source), locations.get(&edge.target))
        else {
            debug_assert!(false, "expanded edge endpoints must exist in layers");
            continue;
        };
        debug_assert_eq!(target_rank, source_rank + 1);
        if let Some(boundary) = boundaries.get_mut(source_rank) {
            boundary.push((source_position, target_position));
        }
    }
    boundaries
        .into_iter()
        .enumerate()
        .map(|(rank, edges)| boundary_crossings(edges, component.layers[rank + 1].len()))
        .sum()
}

fn boundary_crossings(mut edges: Vec<(usize, usize)>, target_count: usize) -> usize {
    edges.sort_unstable();
    let mut crossings = 0;
    let mut seen = 0;
    let mut targets = Fenwick::new(target_count);
    let mut start = 0;
    while start < edges.len() {
        let source = edges[start].0;
        let end = edges[start..]
            .iter()
            .position(|edge| edge.0 != source)
            .map_or(edges.len(), |offset| start + offset);
        for &(_, target) in &edges[start..end] {
            crossings += seen - targets.prefix_inclusive(target);
        }
        for &(_, target) in &edges[start..end] {
            targets.add(target);
            seen += 1;
        }
        start = end;
    }
    crossings
}

struct Fenwick {
    tree: Vec<usize>,
}

impl Fenwick {
    fn new(len: usize) -> Self {
        Self {
            tree: vec![0; len + 1],
        }
    }

    fn add(&mut self, index: usize) {
        let mut cursor = index + 1;
        while cursor < self.tree.len() {
            self.tree[cursor] += 1;
            cursor += cursor.isolate_lowest_one();
        }
    }

    fn prefix_inclusive(&self, index: usize) -> usize {
        let mut cursor = (index + 1).min(self.tree.len().saturating_sub(1));
        let mut total = 0;
        while cursor > 0 {
            total += self.tree[cursor];
            cursor &= cursor - 1;
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feathers::graph_layout::{GraphLayoutEdge, GraphLayoutNodeId};

    fn id(value: usize) -> LayerNodeId {
        LayerNodeId::Real(GraphLayoutNodeId::from_index(value).unwrap())
    }

    fn edge(source: usize, target: usize) -> super::super::LayerEdge {
        let original = GraphLayoutEdge {
            source: GraphLayoutNodeId::from_index(source).unwrap(),
            target: GraphLayoutNodeId::from_index(target).unwrap(),
            source_port: None,
            target_port: None,
        };
        super::super::LayerEdge {
            source: id(source),
            target: id(target),
            original,
        }
    }

    #[test]
    fn adjacent_transpose_strictly_reduces_crossings() {
        let mut component = ExpandedComponent {
            real_nodes: Vec::new(),
            layers: vec![vec![id(0), id(1)], vec![id(2), id(3)]],
            edges: vec![edge(0, 3), edge(1, 2)],
        };
        let neighbors = Neighbors::new(&component);
        assert_eq!(crossing_count(&component), 1);
        assert!(transpose_component(&mut component, &neighbors));
        assert_eq!(crossing_count(&component), 0);
        assert_ne!(
            component.layers,
            vec![vec![id(0), id(1)], vec![id(2), id(3)]]
        );
    }

    #[test]
    fn equal_source_or_target_does_not_count_as_a_crossing() {
        assert_eq!(boundary_crossings(vec![(0, 1), (0, 0)], 2), 0);
        assert_eq!(boundary_crossings(vec![(0, 0), (1, 0)], 2), 0);
        assert_eq!(boundary_crossings(vec![(0, 1), (1, 0)], 2), 1);
    }

    #[test]
    fn equal_barycenters_use_stable_layout_identity() {
        let adjacent = vec![id(0), id(1)];
        let mut layer = vec![id(3), id(2)];
        let neighbors = BTreeMap::from([(id(2), adjacent.clone()), (id(3), adjacent.clone())]);

        assert!(reorder_by_barycenter(&mut layer, &adjacent, &neighbors));
        assert_eq!(layer, [id(2), id(3)]);
    }
}
