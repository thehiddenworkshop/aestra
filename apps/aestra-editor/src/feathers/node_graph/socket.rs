//! Original compact-header socket proxy geometry, shared by both engine graphs.
use super::node::FeathersGraphNode;
use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphSocketSide {
    Input,
    Output,
}

/// Returns the graph-local proxy anchor used by wires while a node is compact.
///
/// A compact node hides its body and therefore its real socket entities. Their cached UI
/// transforms must not be used: they describe the expanded body and can lag behind the compact
/// layout. Instead, all inputs converge on the left edge of the header and all outputs on the
/// right edge. Expanded nodes keep using their measured socket positions.
pub(crate) fn compact_graph_socket_offset(
    node: &FeathersGraphNode,
    computed: &ComputedNode,
    side: GraphSocketSide,
) -> Option<Vec2> {
    let size = computed.size() * computed.inverse_scale_factor;
    compact_graph_socket_offset_for_size(node.collapsed(), size, side)
}

pub(super) fn compact_graph_socket_offset_for_size(
    collapsed: bool,
    size: Vec2,
    side: GraphSocketSide,
) -> Option<Vec2> {
    if !collapsed || !size.is_finite() || size.min_element() <= 0.0 {
        return None;
    }

    Some(Vec2::new(
        match side {
            GraphSocketSide::Input => 0.0,
            GraphSocketSide::Output => size.x,
        },
        size.y * 0.5,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_proxies_use_logical_measured_size_and_ignore_stale_expanded_transforms() {
        let mut node =
            FeathersGraphNode::clipboard_fixture("graph".into(), "node".into(), Vec2::ZERO);
        let computed = ComputedNode {
            size: Vec2::new(448.0, 80.0),
            inverse_scale_factor: 0.5,
            ..default()
        };
        assert_eq!(
            compact_graph_socket_offset(&node, &computed, GraphSocketSide::Output),
            None
        );
        node.collapsed = true;
        assert_eq!(
            compact_graph_socket_offset(&node, &computed, GraphSocketSide::Input),
            Some(Vec2::new(0.0, 20.0))
        );
        assert_eq!(
            compact_graph_socket_offset(&node, &computed, GraphSocketSide::Output),
            Some(Vec2::new(224.0, 20.0))
        );
    }
    #[test]
    fn compact_proxy_rejects_invalid_or_unmeasured_dimensions() {
        for size in [
            Vec2::ZERO,
            Vec2::new(-1.0, 80.0),
            Vec2::splat(f32::NAN),
            Vec2::splat(f32::INFINITY),
        ] {
            assert_eq!(
                compact_graph_socket_offset_for_size(true, size, GraphSocketSide::Output),
                None
            );
        }
    }
}
