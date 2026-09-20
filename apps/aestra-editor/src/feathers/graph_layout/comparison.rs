//! Development-only quality reporting for the native and qualified reference backends.
//!
//! Nothing in this module selects a production backend. It runs both engines over the same
//! immutable input and reports comparable geometry, timing and determinism evidence.

use super::{
    ElkLayeredLayout, GraphLayoutEdge, GraphLayoutEngine, GraphLayoutError, GraphLayoutInput,
    GraphLayoutResult, native::AestraLayeredLayout,
};
use bevy::prelude::{Rect, Vec2};
use std::{fmt, time::Duration, time::Instant};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LayoutQualityMetrics {
    pub(crate) node_overlaps: usize,
    pub(crate) edge_crossings: usize,
    pub(crate) total_edge_span: f32,
    pub(crate) bounds: Rect,
    pub(crate) runtime: Duration,
    pub(crate) determinism_hash: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BackendComparison {
    pub(crate) native: LayoutQualityMetrics,
    pub(crate) elkrs: LayoutQualityMetrics,
}

impl BackendComparison {
    pub(crate) fn run(input: &GraphLayoutInput) -> Result<Self, GraphLayoutError> {
        let canonical = GraphLayoutInput::try_new(
            input.direction,
            input.nodes.clone(),
            input.edges.clone(),
            input.region.clone(),
        )?;
        Ok(Self {
            native: measure(&AestraLayeredLayout, &canonical)?,
            elkrs: measure(&ElkLayeredLayout, &canonical)?,
        })
    }
}

impl fmt::Display for BackendComparison {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "backend  overlaps  crossings  edge-span  bounds  runtime  hash"
        )?;
        write_metrics(formatter, "native", &self.native)?;
        write_metrics(formatter, "elkrs", &self.elkrs)
    }
}

fn write_metrics(
    formatter: &mut fmt::Formatter<'_>,
    name: &str,
    metrics: &LayoutQualityMetrics,
) -> fmt::Result {
    let size = metrics.bounds.size();
    writeln!(
        formatter,
        "{name}  {}  {}  {:.1}  {:.1}x{:.1}  {:?}  {:016x}",
        metrics.node_overlaps,
        metrics.edge_crossings,
        metrics.total_edge_span,
        size.x,
        size.y,
        metrics.runtime,
        metrics.determinism_hash
    )
}

fn measure(
    engine: &dyn GraphLayoutEngine,
    input: &GraphLayoutInput,
) -> Result<LayoutQualityMetrics, GraphLayoutError> {
    let started = Instant::now();
    let result = engine.layout(input)?;
    let runtime = started.elapsed();
    result.validate(input)?;
    Ok(metrics(input, &result, runtime))
}

fn metrics(
    input: &GraphLayoutInput,
    result: &GraphLayoutResult,
    runtime: Duration,
) -> LayoutQualityMetrics {
    let centers = input
        .nodes
        .iter()
        .map(|node| (node.key, result.positions[&node.key] + node.size * 0.5))
        .collect();
    LayoutQualityMetrics {
        node_overlaps: overlap_count(input, result),
        edge_crossings: crossing_count(&input.edges, &centers),
        total_edge_span: total_edge_span(&input.edges, &centers),
        bounds: result.bounds,
        runtime,
        determinism_hash: determinism_hash(result),
    }
}

fn overlap_count(input: &GraphLayoutInput, result: &GraphLayoutResult) -> usize {
    input
        .nodes
        .iter()
        .enumerate()
        .map(|(index, left)| {
            let left_bounds = node_bounds(left.size, result.positions[&left.key]);
            input
                .nodes
                .iter()
                .skip(index + 1)
                .filter(|right| {
                    rectangles_overlap(
                        left_bounds,
                        node_bounds(right.size, result.positions[&right.key]),
                    )
                })
                .count()
        })
        .sum()
}

fn node_bounds(size: Vec2, position: Vec2) -> Rect {
    Rect::from_corners(position, position + size)
}

fn rectangles_overlap(left: Rect, right: Rect) -> bool {
    left.min.x < right.max.x
        && right.min.x < left.max.x
        && left.min.y < right.max.y
        && right.min.y < left.max.y
}

fn crossing_count(
    edges: &[GraphLayoutEdge],
    centers: &std::collections::BTreeMap<super::GraphLayoutNodeId, Vec2>,
) -> usize {
    edges
        .iter()
        .enumerate()
        .map(|(index, left)| {
            edges
                .iter()
                .skip(index + 1)
                .filter(|right| !shares_endpoint(left, right))
                .filter(|right| {
                    let (left_start, left_end) = edge_centers(centers, left);
                    let (right_start, right_end) = edge_centers(centers, right);
                    segments_properly_cross(left_start, left_end, right_start, right_end)
                })
                .count()
        })
        .sum()
}

fn shares_endpoint(left: &GraphLayoutEdge, right: &GraphLayoutEdge) -> bool {
    left.source == right.source
        || left.source == right.target
        || left.target == right.source
        || left.target == right.target
}

fn edge_centers(
    centers: &std::collections::BTreeMap<super::GraphLayoutNodeId, Vec2>,
    edge: &GraphLayoutEdge,
) -> (Vec2, Vec2) {
    (centers[&edge.source], centers[&edge.target])
}

fn segments_properly_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let ab_c = (b - a).perp_dot(c - a);
    let ab_d = (b - a).perp_dot(d - a);
    let cd_a = (d - c).perp_dot(a - c);
    let cd_b = (d - c).perp_dot(b - c);
    ab_c * ab_d < 0.0 && cd_a * cd_b < 0.0
}

fn total_edge_span(
    edges: &[GraphLayoutEdge],
    centers: &std::collections::BTreeMap<super::GraphLayoutNodeId, Vec2>,
) -> f32 {
    edges
        .iter()
        .map(|edge| {
            let (source, target) = edge_centers(centers, edge);
            let delta = (target - source).abs();
            delta.x + delta.y
        })
        .sum()
}

fn determinism_hash(result: &GraphLayoutResult) -> u64 {
    let mut hash = FNV_OFFSET;
    for (&key, &position) in &result.positions {
        hash_u32(&mut hash, key.index());
        hash_f32(&mut hash, position.x);
        hash_f32(&mut hash, position.y);
    }
    hash_f32(&mut hash, result.bounds.min.x);
    hash_f32(&mut hash, result.bounds.min.y);
    hash_f32(&mut hash, result.bounds.max.x);
    hash_f32(&mut hash, result.bounds.max.y);
    hash
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn hash_f32(hash: &mut u64, value: f32) {
    hash_u32(hash, if value == 0.0 { 0 } else { value.to_bits() });
}

fn hash_u32(hash: &mut u64, value: u32) {
    for byte in value.to_le_bytes() {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feathers::graph_layout::{
        GraphDirection, GraphLayoutNode, GraphLayoutNodeId, GraphLayoutRegion,
    };
    use std::collections::BTreeMap;

    fn id(index: usize) -> GraphLayoutNodeId {
        GraphLayoutNodeId::from_index(index).unwrap()
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
                .copied()
                .enumerate()
                .map(|(index, size)| GraphLayoutNode {
                    key: id(index),
                    position: Vec2::ZERO,
                    size,
                    pinned: false,
                    selected: false,
                })
                .collect(),
            edges
                .iter()
                .map(|&(source, target)| GraphLayoutEdge {
                    source: id(source),
                    target: id(target),
                    source_port: None,
                    target_port: None,
                })
                .collect(),
            GraphLayoutRegion::Full,
        )
        .unwrap()
    }

    #[test]
    fn quality_metrics_count_overlap_crossing_span_bounds_and_stable_hash() {
        let input = input(
            GraphDirection::LeftToRight,
            &[Vec2::splat(10.0); 4],
            &[(0, 3), (1, 2)],
        );
        let result = GraphLayoutResult::from_positions(
            &input,
            BTreeMap::from([
                (id(0), Vec2::new(0.0, 0.0)),
                (id(1), Vec2::new(0.0, 20.0)),
                (id(2), Vec2::new(30.0, 0.0)),
                (id(3), Vec2::new(30.0, 20.0)),
            ]),
        )
        .unwrap();
        let first = metrics(&input, &result, Duration::from_micros(7));
        let second = metrics(&input, &result, Duration::from_micros(9));

        assert_eq!(first.node_overlaps, 0);
        assert_eq!(first.edge_crossings, 1);
        assert_eq!(first.total_edge_span, 100.0);
        assert_eq!(
            first.bounds,
            Rect::from_corners(Vec2::ZERO, Vec2::new(40.0, 30.0))
        );
        assert_eq!(first.determinism_hash, second.determinism_hash);
        assert_ne!(first.determinism_hash, FNV_OFFSET);
    }

    #[test]
    fn representative_backends_report_valid_zero_overlap_results() {
        let input = input(
            GraphDirection::LeftToRight,
            &[
                Vec2::new(90.0, 45.0),
                Vec2::new(70.0, 80.0),
                Vec2::new(110.0, 35.0),
                Vec2::new(60.0, 100.0),
                Vec2::new(120.0, 50.0),
                Vec2::new(80.0, 60.0),
                Vec2::new(75.0, 40.0),
            ],
            &[(6, 0), (6, 1), (6, 2), (0, 5), (1, 4), (2, 3)],
        );
        let comparison = BackendComparison::run(&input).unwrap();

        assert_eq!(comparison.native.node_overlaps, 0);
        assert_eq!(comparison.elkrs.node_overlaps, 0);
        assert_eq!(comparison.native.edge_crossings, 0);
        assert_eq!(comparison.elkrs.edge_crossings, 0);
        assert!(comparison.native.total_edge_span > 0.0);
        assert!(comparison.elkrs.total_edge_span > 0.0);
        let report = comparison.to_string();
        assert!(report.contains("native"));
        assert!(report.contains("elkrs"));
        assert!(report.contains("edge-span"));
    }

    #[test]
    fn hashes_are_repeatable_across_runs_and_canonical_input_order() {
        let input = input(
            GraphDirection::TopToBottom,
            &[Vec2::new(80.0, 40.0); 5],
            &[(0, 2), (1, 2), (2, 3)],
        );
        let mut shuffled = input.clone();
        shuffled.nodes.reverse();
        shuffled.edges.reverse();

        let first = BackendComparison::run(&input).unwrap();
        let repeated = BackendComparison::run(&input).unwrap();
        let reordered = BackendComparison::run(&shuffled).unwrap();

        assert_eq!(
            first.native.determinism_hash,
            repeated.native.determinism_hash
        );
        assert_eq!(
            first.elkrs.determinism_hash,
            repeated.elkrs.determinism_hash
        );
        assert_eq!(
            first.native.determinism_hash,
            reordered.native.determinism_hash
        );
        assert_eq!(
            first.elkrs.determinism_hash,
            reordered.elkrs.determinism_hash
        );
    }
}
