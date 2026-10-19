//! Original bootstrap layout and depth traversal used by material workspace composition.
use crate::{
    feathers::node_graph::NODE_WIDTH,
    material_graph::{
        MaterialGraphPreviewState, MaterialGraphPreviewTarget,
        node_metrics::{
            MATERIAL_PREVIEW_LAYOUT_HEIGHT, material_graph_node_row_count, node_height,
        },
    },
};
use aestra_compiler::{MaterialGraphNode, MaterialGraphProjection};
use aestra_core::MaterialExpressionId;
use bevy::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const CANVAS_PADDING: f32 = 34.0;
const NODE_GAP: f32 = 22.0;
pub(super) const COLUMN_WIDTH: f32 = 282.0;
#[derive(Debug)]
pub(super) struct MaterialGraphLayout {
    pub(super) nodes: BTreeMap<MaterialExpressionId, Vec2>,
    pub(super) output: Vec2,
    pub(super) size: Vec2,
}
pub(super) fn layout_graph(
    graph: &MaterialGraphProjection,
    previews: &MaterialGraphPreviewState,
) -> MaterialGraphLayout {
    let inputs = graph
        .nodes
        .iter()
        .map(|node| {
            (
                node.expression,
                node.inputs
                    .iter()
                    .map(|port| port.source)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut depths = BTreeMap::new();
    for node in &graph.nodes {
        expression_depth(node.expression, &inputs, &mut depths, &mut BTreeSet::new());
    }
    let mut columns = BTreeMap::<usize, Vec<&MaterialGraphNode>>::new();
    for node in &graph.nodes {
        columns
            .entry(depths.get(&node.expression).copied().unwrap_or_default())
            .or_default()
            .push(node);
    }
    let mut positions = BTreeMap::new();
    let mut maximum_y = CANVAS_PADDING;
    for (depth, nodes) in columns {
        let mut y = CANVAS_PADDING + 34.0;
        for node in nodes {
            positions.insert(
                node.expression,
                Vec2::new(CANVAS_PADDING + depth as f32 * COLUMN_WIDTH, y),
            );
            y += node_height(
                material_graph_node_row_count(node),
                node.disabled || !node.reachable,
                previews.is_visible(
                    graph.program,
                    MaterialGraphPreviewTarget::Expression(node.expression),
                ),
            ) + NODE_GAP;
        }
        maximum_y = maximum_y.max(y);
    }
    let output_depth = depths.values().copied().max().unwrap_or_default() + 1;
    let output = Vec2::new(
        CANVAS_PADDING + output_depth as f32 * COLUMN_WIDTH,
        CANVAS_PADDING + 88.0,
    );
    let width = output.x + NODE_WIDTH + CANVAS_PADDING;
    let output_height = 130.0
        + if previews.is_visible(graph.program, MaterialGraphPreviewTarget::Output) {
            MATERIAL_PREVIEW_LAYOUT_HEIGHT
        } else {
            0.0
        };
    let height = maximum_y.max(output.y + output_height) + CANVAS_PADDING;
    MaterialGraphLayout {
        nodes: positions,
        output,
        size: Vec2::new(width.max(720.0), height.max(420.0)),
    }
}

pub(crate) fn expression_depth(
    expression: MaterialExpressionId,
    inputs: &BTreeMap<MaterialExpressionId, Vec<MaterialExpressionId>>,
    memo: &mut BTreeMap<MaterialExpressionId, usize>,
    visiting: &mut BTreeSet<MaterialExpressionId>,
) -> usize {
    if let Some(depth) = memo.get(&expression) {
        return *depth;
    }
    if !visiting.insert(expression) {
        return 0;
    }
    let depth = inputs
        .get(&expression)
        .into_iter()
        .flatten()
        .filter(|source| inputs.contains_key(source))
        .map(|source| expression_depth(*source, inputs, memo, visiting) + 1)
        .max()
        .unwrap_or_default();
    visiting.remove(&expression);
    memo.insert(expression, depth);
    depth
}
#[cfg(test)]
#[path = "workspace_layout/tests.rs"]
mod tests;
