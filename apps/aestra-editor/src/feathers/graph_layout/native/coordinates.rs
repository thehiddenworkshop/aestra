//! Rectangle-aware coordinate assignment for independently laid-out components.

use super::{
    CanonicalGraph, ExpandedComponent, ExpandedGraph, LayerNodeId, PositionedComponent,
    PositionedGraph, native_adapter,
};
use crate::feathers::graph_layout::{GraphDirection, GraphLayoutError, GraphLayoutNodeId};
use bevy::prelude::{Rect, Vec2};
use std::collections::BTreeMap;

pub(super) const INTRA_RANK_SPACING: f32 = 32.0;
pub(super) const INTER_RANK_SPACING: f32 = 72.0;
pub(super) const COMPONENT_SPACING: f32 = 96.0;

pub(super) fn assign(
    expanded: &ExpandedGraph,
    graph: &CanonicalGraph,
) -> Result<PositionedGraph, GraphLayoutError> {
    let sizes = graph
        .nodes
        .iter()
        .map(|node| (node.key, node.size))
        .collect::<BTreeMap<_, _>>();
    let components = expanded
        .components
        .iter()
        .map(|component| assign_component(component, graph.direction, &sizes))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PositionedGraph { components })
}

pub(super) fn pack(graph: &mut PositionedGraph) {
    let component_count = graph.components.len();
    let mut vertical_cursor = 0.0;
    for (index, component) in graph.components.iter_mut().enumerate() {
        let offset = Vec2::new(
            -component.bounds.min.x,
            vertical_cursor - component.bounds.min.y,
        );
        for position in component.positions.values_mut() {
            *position += offset;
        }
        component.bounds =
            Rect::from_corners(component.bounds.min + offset, component.bounds.max + offset);
        vertical_cursor = component.bounds.max.y;
        if index + 1 < component_count {
            vertical_cursor += COMPONENT_SPACING;
        }
    }
}

fn assign_component(
    component: &ExpandedComponent,
    direction: GraphDirection,
    sizes: &BTreeMap<GraphLayoutNodeId, Vec2>,
) -> Result<PositionedComponent, GraphLayoutError> {
    let mut flow_cursor = 0.0;
    let mut positions = BTreeMap::new();

    for (rank, layer) in component.layers.iter().enumerate() {
        let mut cross_cursor = 0.0;
        let flow_extent = layer.iter().try_fold(0.0_f32, |extent, node| {
            Ok::<_, GraphLayoutError>(extent.max(flow_size(node_size(*node, sizes)?, direction)))
        })?;

        for &node in layer {
            let size = node_size(node, sizes)?;
            let position = orient(flow_cursor, cross_cursor, direction);
            positions.insert(node, position);
            cross_cursor += cross_size(size, direction);
            if node != *layer.last().expect("rank must contain a node") {
                cross_cursor += INTRA_RANK_SPACING;
            }
        }

        flow_cursor += flow_extent;
        if rank + 1 < component.layers.len() {
            flow_cursor += INTER_RANK_SPACING;
        }
    }

    let bounds = real_bounds(&component.real_nodes, &positions, sizes)?;
    Ok(PositionedComponent {
        real_nodes: component.real_nodes.clone(),
        positions,
        bounds,
    })
}

fn node_size(
    node: LayerNodeId,
    sizes: &BTreeMap<GraphLayoutNodeId, Vec2>,
) -> Result<Vec2, GraphLayoutError> {
    match node {
        LayerNodeId::Real(key) => sizes
            .get(&key)
            .copied()
            .ok_or_else(|| native_adapter(format!("missing measured size for {key:?}"))),
        LayerNodeId::Virtual(_) => Ok(Vec2::ZERO),
    }
}

fn flow_size(size: Vec2, direction: GraphDirection) -> f32 {
    match direction {
        GraphDirection::LeftToRight => size.x,
        GraphDirection::TopToBottom => size.y,
    }
}

fn cross_size(size: Vec2, direction: GraphDirection) -> f32 {
    match direction {
        GraphDirection::LeftToRight => size.y,
        GraphDirection::TopToBottom => size.x,
    }
}

fn orient(flow: f32, cross: f32, direction: GraphDirection) -> Vec2 {
    match direction {
        GraphDirection::LeftToRight => Vec2::new(flow, cross),
        GraphDirection::TopToBottom => Vec2::new(cross, flow),
    }
}

fn real_bounds(
    real_nodes: &[GraphLayoutNodeId],
    positions: &BTreeMap<LayerNodeId, Vec2>,
    sizes: &BTreeMap<GraphLayoutNodeId, Vec2>,
) -> Result<Rect, GraphLayoutError> {
    let mut bounds: Option<Rect> = None;
    for &key in real_nodes {
        let position = positions
            .get(&LayerNodeId::Real(key))
            .copied()
            .ok_or_else(|| native_adapter(format!("missing assigned position for {key:?}")))?;
        let size = sizes
            .get(&key)
            .copied()
            .ok_or_else(|| native_adapter(format!("missing measured size for {key:?}")))?;
        let node_bounds = Rect::from_corners(position, position + size);
        bounds = Some(bounds.map_or(node_bounds, |current| {
            Rect::from_corners(
                current.min.min(node_bounds.min),
                current.max.max(node_bounds.max),
            )
        }));
    }
    bounds.ok_or_else(|| native_adapter("expanded component contains no real nodes"))
}
