//! Frame requests consume stable post-layout geometry from the exact visible view. Camera
//! changes are applied by the following Update's canvas sync; Bevy layout is never re-entered.
use super::{geometry::*, *};
use std::collections::BTreeSet;

pub(super) fn frame_graph_viewports(
    registry: Res<GraphGeometryRegistry>,
    mut viewports: Query<(
        Entity,
        &ComputedNode,
        Option<&GraphGeometryView>,
        &mut FeathersGraphViewport,
    )>,
    nodes: Query<(Entity, &GraphGeometryNode, &FeathersGraphNode)>,
    parents: Query<&ChildOf>,
) {
    for (entity, computed, geometry, mut viewport) in &mut viewports {
        let Some(target) = viewport.frame_request else {
            continue;
        };
        let viewport_size = computed.size() * computed.inverse_scale_factor;
        if !viewport_size.is_finite() || viewport_size.min_element() <= 0.0 {
            continue;
        }
        let bounds = if let Some(geometry) = geometry {
            let Some(snapshot) = registry.mounted_view_snapshot(&geometry.key, entity) else {
                // New/rebuilt/hidden/resizing views defer until complete and stable, rather than
                // consuming another view's size or clearing the request on a guessed rectangle.
                continue;
            };
            let selected = if target == GraphFrameTarget::Selection {
                nodes
                    .iter()
                    .filter(|(node_entity, _, node)| {
                        node.selected && parents.iter_ancestors(*node_entity).any(|p| p == entity)
                    })
                    .map(|(_, marker, _)| marker.key)
                    .collect::<BTreeSet<_>>()
            } else {
                BTreeSet::new()
            };
            snapshot
                .bounds(|key| selected.is_empty() || selected.contains(key))
                .unwrap_or_else(|| Rect::from_corners(Vec2::ZERO, viewport.content_size))
        } else {
            // Generic widget clients without geometry adapters retain their bootstrap fallback.
            match target {
                GraphFrameTarget::All => Rect::from_corners(Vec2::ZERO, viewport.content_size),
                GraphFrameTarget::Selection => viewport
                    .selection_bounds
                    .unwrap_or_else(|| Rect::from_corners(Vec2::ZERO, viewport.content_size)),
            }
        };
        let maximum_zoom = if target == GraphFrameTarget::All {
            1.0
        } else {
            MAX_ZOOM
        };
        let view = framed_graph_view(bounds, viewport_size, maximum_zoom);
        // Stage the camera, not placement. Apply next Update before UI layout so the node
        // canvas, grid and wires all render the same camera. Navigation can cancel the request.
        viewport.measured_frame = Some((target, view));
    }
}

#[cfg(test)]
mod tests;
