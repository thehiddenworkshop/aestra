//! Pure, bounded local resize planning in unzoomed logical graph units.
//! No ECS, material semantics, persistence, history, or UI registration. M5 will adapt
//! owner-elected geometry into this input and consume candidates as temporary offsets.
//! This is a directional heuristic, not a global minimum-displacement solver.

use bevy::math::Vec2;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Revision {
    pub generation: u64,
    pub topology: u64,
    pub geometry: u64,
    pub placement: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Node {
    /// Effective snapshot position, never the persistent base-position store.
    pub position: Vec2,
    pub size: Vec2,
    /// Includes explicit pins, ongoing drags and protected manual edits.
    pub frozen: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Snapshot<K: Ord, D> {
    /// Adapter must include project, document and measurement-owner identity.
    pub identity: D,
    pub revision: Revision,
    pub nodes: BTreeMap<K, Node>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Limits {
    pub spacing: Vec2,
    pub epsilon: f32,
    pub max_nodes: usize,
    pub max_iterations: usize,
    pub max_affected: usize,
    pub max_pair_checks: usize,
    pub max_node_displacement: f32,
    pub max_total_displacement: f32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            spacing: Vec2::new(22.0, 22.0),
            epsilon: 0.01,
            max_nodes: 4096,
            max_iterations: 256,
            max_affected: 64,
            max_pair_checks: 200_000,
            max_node_displacement: 2048.0,
            max_total_displacement: 8192.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Budget {
    Nodes,
    Iterations,
    AffectedNodes,
    PairChecks,
    NodeDisplacement,
    TotalDisplacement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Conflict<K> {
    InvalidInput,
    InvalidNode(K),
    MissingRoot(K),
    Anchors(K, K),
    Budget(Budget),
    Stale,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Stats {
    pub iterations: usize,
    pub pair_checks: usize,
    pub affected_nodes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Failure<K> {
    pub conflict: Conflict<K>,
    pub stats: Stats,
}

#[derive(Debug, Clone, Copy)]
enum Axis {
    Right,
    Down,
    Either,
}

#[derive(Debug)]
pub(super) struct Candidate<K: Ord, D> {
    before: Snapshot<K, D>,
    positions: BTreeMap<K, Vec2>,
    pub stats: Stats,
}

impl<K: Ord + Clone, D: PartialEq> Candidate<K, D> {
    pub(super) fn positions(&self) -> &BTreeMap<K, Vec2> {
        &self.positions
    }

    /// Atomic application to a detached effective snapshot, not GraphViewportMemory.
    /// Compare the full input as well as revisions: a caller forgetting to bump a
    /// revision must not overwrite a drag, new pin, owner switch or changed geometry.
    pub(super) fn apply(self, current: &mut Snapshot<K, D>) -> Result<Stats, Conflict<K>> {
        if *current != self.before {
            return Err(Conflict::Stale);
        }
        if self.positions.is_empty() {
            return Ok(self.stats);
        }
        let Some(revision) = current.revision.placement.checked_add(1) else {
            return Err(Conflict::Stale);
        };
        let mut nodes = current.nodes.clone();
        for (key, position) in self.positions {
            nodes
                .get_mut(&key)
                .expect("validated candidate node")
                .position = position;
        }
        current.nodes = nodes;
        current.revision.placement = revision;
        Ok(self.stats)
    }
}

fn valid_size(size: Vec2) -> bool {
    size.is_finite() && size.min_element() > 0.0
}

fn valid_node(node: &Node, spacing: Vec2) -> bool {
    node.position.is_finite()
        && valid_size(node.size)
        && (node.position + node.size + spacing).is_finite()
}

fn overlaps(a: &Node, b: &Node, limits: Limits) -> bool {
    let end_a = a.position + a.size + limits.spacing;
    let end_b = b.position + b.size + limits.spacing;
    a.position.x < end_b.x - limits.epsilon
        && b.position.x < end_a.x - limits.epsilon
        && a.position.y < end_b.y - limits.epsilon
        && b.position.y < end_a.y - limits.epsilon
}

fn separation(obstacle: Node, mover: Node, axis: Axis, limits: Limits) -> (Vec2, Axis) {
    let delta = obstacle.position + obstacle.size + limits.spacing - mover.position;
    let axis = match axis {
        Axis::Either if delta.x <= delta.y => Axis::Right,
        Axis::Either => Axis::Down,
        axis => axis,
    };
    let mut position = mover.position;
    match axis {
        Axis::Right => position.x = obstacle.position.x + obstacle.size.x + limits.spacing.x,
        Axis::Down => position.y = obstacle.position.y + obstacle.size.y + limits.spacing.y,
        Axis::Either => unreachable!(),
    }
    (position, axis)
}

/// `old_sizes` contains only explicit stable resize causes, keyed identically to
/// `snapshot.nodes` (which already contains the new sizes). Every source is anchored.
/// Shrinks/noise do not seed a cascade. Baseline/manual-overlap cleanup is not our job.
/// Roots and conflicts are visited in key order, irrespective of ECS insertion order.
pub(super) fn solve<K: Ord + Clone, D: Clone>(
    snapshot: &Snapshot<K, D>,
    old_sizes: &BTreeMap<K, Vec2>,
    limits: Limits,
) -> Result<Candidate<K, D>, Failure<K>> {
    let mut stats = Stats::default();
    let result = plan(snapshot, old_sizes, limits, &mut stats);
    match result {
        Ok(positions) => Ok(Candidate {
            before: snapshot.clone(),
            positions,
            stats,
        }),
        Err(conflict) => Err(Failure { conflict, stats }),
    }
}

fn plan<K: Ord + Clone, D>(
    snapshot: &Snapshot<K, D>,
    old_sizes: &BTreeMap<K, Vec2>,
    limits: Limits,
    stats: &mut Stats,
) -> Result<BTreeMap<K, Vec2>, Conflict<K>> {
    if !limits.spacing.is_finite()
        || limits.spacing.min_element() < 0.0
        || !limits.epsilon.is_finite()
        || !(0.0..=0.5).contains(&limits.epsilon)
        || !limits.max_node_displacement.is_finite()
        || limits.max_node_displacement < 0.0
        || !limits.max_total_displacement.is_finite()
        || limits.max_total_displacement < 0.0
    {
        return Err(Conflict::InvalidInput);
    }
    if snapshot.nodes.len() > limits.max_nodes || old_sizes.len() > limits.max_nodes {
        return Err(Conflict::Budget(Budget::Nodes));
    }
    for (key, node) in &snapshot.nodes {
        if !valid_node(node, limits.spacing) {
            return Err(Conflict::InvalidNode(key.clone()));
        }
    }
    let mut active = BTreeMap::new();
    for (key, old_size) in old_sizes {
        let node = snapshot
            .nodes
            .get(key)
            .ok_or_else(|| Conflict::MissingRoot(key.clone()))?;
        if !valid_size(*old_size) {
            return Err(Conflict::InvalidNode(key.clone()));
        }
        let growth = node.size - *old_size;
        let width = growth.x > limits.epsilon;
        let height = growth.y > limits.epsilon;
        let axis = match (width, height) {
            (true, true) => Axis::Either,
            (true, false) => Axis::Right,
            (false, true) => Axis::Down,
            (false, false) => continue,
        };
        active.insert(key.clone(), axis);
    }
    let mut nodes = snapshot.nodes.clone();
    let mut positions = BTreeMap::new();
    let mut total_displacement = 0.0;
    loop {
        // This scan is also the final constraint validation: every changed node/root
        // must be clear of all obstacles. Untouched pairs are deliberately ignored.
        let mut collision = None;
        'scan: for (source, axis) in &active {
            for (other, node) in &nodes {
                if source == other {
                    continue;
                }
                if stats.pair_checks == limits.max_pair_checks {
                    return Err(Conflict::Budget(Budget::PairChecks));
                }
                stats.pair_checks += 1;
                if overlaps(&nodes[source], node, limits) {
                    collision = Some((source.clone(), other.clone(), *axis));
                    break 'scan;
                }
            }
        }
        let Some((source, other, axis)) = collision else {
            return Ok(positions);
        };
        let fixed = |key: &K| old_sizes.contains_key(key) || nodes[key].frozen;
        let (obstacle, mover) = if fixed(&other) {
            if fixed(&source) {
                return Err(Conflict::Anchors(source, other));
            }
            (other, source)
        } else {
            (source, other)
        };
        if stats.iterations == limits.max_iterations {
            return Err(Conflict::Budget(Budget::Iterations));
        }
        if !positions.contains_key(&mover) && positions.len() == limits.max_affected {
            return Err(Conflict::Budget(Budget::AffectedNodes));
        }
        let (position, axis) = separation(nodes[&obstacle], nodes[&mover], axis, limits);
        let moved = Node {
            position,
            ..nodes[&mover]
        };
        if !valid_node(&moved, limits.spacing) || overlaps(&nodes[&obstacle], &moved, limits) {
            // Huge finite coordinates can still lose spacing through f32 rounding.
            return Err(Conflict::InvalidNode(mover));
        }
        let displacement = (position - snapshot.nodes[&mover].position)
            .abs()
            .element_sum();
        if displacement > limits.max_node_displacement {
            return Err(Conflict::Budget(Budget::NodeDisplacement));
        }
        // Motion is monotonic right/down: path length equals total final L1 offset.
        total_displacement += (position - nodes[&mover].position).abs().element_sum();
        if total_displacement > limits.max_total_displacement {
            return Err(Conflict::Budget(Budget::TotalDisplacement));
        }
        nodes.insert(mover.clone(), moved);
        positions.insert(mover.clone(), position);
        active.insert(mover, axis);
        stats.iterations += 1;
        stats.affected_nodes = positions.len();
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "../../../../../benchmarks/graph-layout/resize_profile.rs"]
mod profile;
