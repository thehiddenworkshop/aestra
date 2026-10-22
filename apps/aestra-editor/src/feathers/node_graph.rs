//! Reusable Feathers-styled building blocks for spatial node graphs.
//!
//! This module owns presentation only: canvas styling, node chrome, socket hit targets, and the
//! anti-aliased wire material. Domain panels retain their semantic graph model and commands.

#[cfg(test)]
use super::{button::FeathersActionButton, number_input::EditorNumberInput};
use crate::theme;
#[cfg(test)]
use bevy::feathers::controls::{FeathersCheckbox, FeathersTextInputContainer};
#[cfg(test)]
use bevy::picking::pointer::PointerButton;
use bevy::{asset::embedded_asset, prelude::*};
#[cfg(test)]
use bevy::{ui_widgets::Activate, window::SystemCursorIcon};
#[cfg(test)]
use std::collections::BTreeSet;

mod drag_assist;
mod drag_context;
mod memory;
pub(crate) use memory::GraphViewportMemory;
mod drag;
#[cfg(test)]
use drag::*;
#[path = "node_graph/interaction/pointer_020.rs"]
mod graph_pointer;
mod node;
pub(crate) use node::{
    FeathersGraphNode, GraphModifiedDragTarget, GraphModifiedNodeDrag, GraphNodeDragModifier,
    GraphPinEdit, GraphPresentationBatchEdit, GraphPresentationEdit,
};
mod marquee;
#[cfg(test)]
use marquee::{GraphMarqueeGesture, begin_graph_marquee};
pub(crate) use marquee::{GraphMarqueeSelection, GraphSelectionMode};
#[path = "node_graph/navigation/cursor_020.rs"]
mod graph_cursor;
#[cfg(test)]
use bevy::ui::RelativeCursorPosition;
#[cfg(test)]
use graph_cursor::EntityCursor;
#[cfg(test)]
use graph_cursor::OverrideCursor;
mod navigation;
use navigation::*;
pub(crate) use navigation::{
    FeathersGraphNavigationBlocker, FeathersGraphViewport, GraphFrameTarget,
};
mod materials;
pub(crate) use materials::GraphWireMaterial;
mod framing;
pub(crate) use drag_assist::spawn_controls as spawn_graph_drag_controls;
pub(crate) mod geometry;
pub(crate) mod insertion;
#[cfg(test)]
#[path = "node_graph/insertion/tests.rs"]
mod insertion_tests;
mod overlay;
pub(crate) mod placement;
#[cfg(test)]
#[path = "node_graph/placement/tests.rs"]
mod placement_tests;
mod resize;

mod viewport_presentation;
#[cfg(test)]
use viewport_presentation::*;
mod node_controls;
#[cfg(test)]
use node_controls::*;
mod composition;
#[path = "node_graph/composition_input_020.rs"]
mod composition_input;
#[path = "node_graph/node_control_input_020.rs"]
mod node_control_input;
#[cfg(test)]
use composition::FeathersGraphNodeBody;
#[cfg(test)]
pub(crate) use composition::FeathersGraphSocket;
#[cfg(test)]
use composition::GraphCollapseAction;
#[cfg(test)]
use composition::SELECTED_BORDER_WIDTH;
#[cfg(test)]
pub(crate) use composition::graph_canvas_bundle;
#[cfg(test)]
use composition::graph_port_row_node;
pub(crate) use composition::{
    GraphNodePreviewToggleProps, GraphNodeProps, GraphPortProps, GraphViewportProps,
    spawn_graph_node, spawn_graph_node_preview, spawn_graph_node_preview_toggle, spawn_graph_port,
    spawn_graph_port_with, spawn_graph_viewport,
};
#[cfg(test)]
pub(crate) use node::FeathersGraphNodePreview;
#[cfg(test)]
use viewport_presentation::MIN_SELECTED_BORDER_SCREEN_WIDTH;
mod metrics;
pub(crate) use metrics::{NODE_HEADER_HEIGHT, NODE_PREVIEW_SIZE, NODE_WIDTH, PORT_ROW_HEIGHT};

pub(crate) struct FeathersNodeGraphPlugin;
mod toolbar;
pub(crate) use toolbar::{
    GraphFrameAction, graph_toolbar_bundle, spawn_graph_frame_button, spawn_graph_tool_button,
    spawn_graph_toolbar_summary,
};
#[path = "node_graph/toolbar_input_020.rs"]
mod toolbar_input;

mod integration;
#[cfg(test)]
use integration::GraphNodeControlFilter;
pub(crate) use integration::GraphWidgetSync;

impl Plugin for FeathersNodeGraphPlugin {
    fn build(&self, app: &mut App) {
        // Keep asset paths in this native wrapper: moving embedded_asset! changes its namespace.
        embedded_asset!(app, "shaders/node_graph_wire.wesl");
        embedded_asset!(app, "shaders/node_graph_grid.wesl");
        integration::register(app);
    }
}

/// Graph-space layer rendered below the node canvas with the exact same pan/zoom transform.
mod wire_layer;
pub(crate) use wire_layer::FeathersGraphWireLayer;

mod socket;
#[cfg(test)]
use socket::compact_graph_socket_offset_for_size;
pub(crate) use socket::{GraphSocketSide, compact_graph_socket_offset};

mod preview_toggle;
pub(crate) use preview_toggle::FeathersGraphNodePreviewToggle;

#[cfg(test)]
mod layout_contract_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_graph_toolbar_has_fixed_compact_chrome() {
        let mut world = World::new();
        let entity = world.spawn(graph_toolbar_bundle()).id();
        let node = world.get::<Node>(entity).unwrap();
        assert_eq!(node.height, Val::Px(38.0));
        assert_eq!(node.flex_shrink, 0.0);
        assert_eq!(node.column_gap, Val::Px(4.0));
        assert_eq!(
            world.get::<BackgroundColor>(entity).unwrap().0,
            theme::PANEL_LIGHT
        );
    }

    fn assert_vec2_close(actual: Vec2, expected: Vec2) {
        assert!(
            actual.distance(expected) < 0.001,
            "expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn compact_nodes_route_wires_to_header_edges() {
        let size = Vec2::new(180.0, 42.0);
        assert_eq!(
            compact_graph_socket_offset_for_size(true, size, GraphSocketSide::Input),
            Some(Vec2::new(0.0, 21.0))
        );
        assert_eq!(
            compact_graph_socket_offset_for_size(true, size, GraphSocketSide::Output),
            Some(Vec2::new(180.0, 21.0))
        );
    }

    #[test]
    fn expanded_or_unmeasured_nodes_do_not_use_compact_proxy_anchors() {
        assert_eq!(
            compact_graph_socket_offset_for_size(
                false,
                Vec2::new(180.0, 42.0),
                GraphSocketSide::Input,
            ),
            None
        );
        assert_eq!(
            compact_graph_socket_offset_for_size(true, Vec2::ZERO, GraphSocketSide::Output),
            None
        );
    }

    #[test]
    fn nodes_sync_from_shared_memory_across_views() {
        use bevy::ecs::system::RunSystemOnce;
        // Node layout is per document, so two views of one graph share a memory entry. Moving a
        // node (writing memory) must update every view's node entity, not just the dragged one, so
        // wires stay attached in the other view.
        let mut app = App::new();
        let mut memory = GraphViewportMemory::default();
        memory.set_node("material:p", "expression:n", Vec2::new(120.0, 40.0), false);
        app.insert_resource(memory);
        let node = || {
            (
                FeathersGraphNode {
                    graph_key: "material:p".into(),
                    node_key: "expression:n".into(),
                    position: Vec2::ZERO,
                    selected: false,
                    pinned: false,
                    collapsed: false,
                    dragging: false,
                    drag_before: None,
                    drag_modifier: None,
                    suppress_release_click: false,
                },
                Node::default(),
            )
        };
        let left = app.world_mut().spawn(node()).id();
        let right = app.world_mut().spawn(node()).id();

        app.world_mut()
            .run_system_once(sync_graph_nodes_from_memory)
            .unwrap();

        for view in [left, right] {
            assert_eq!(
                app.world().get::<FeathersGraphNode>(view).unwrap().position,
                Vec2::new(120.0, 40.0)
            );
        }
    }

    #[test]
    fn cursor_centered_zoom_keeps_the_graph_point_under_the_pointer() {
        let before = GraphView {
            pan: Vec2::new(-180.0, 75.0),
            zoom: 0.8,
        };
        let cursor = Vec2::new(430.0, 260.0);
        let anchor = (cursor - before.pan) / before.zoom;

        let after = zoomed_graph_view_at(before, cursor, 2.0);

        assert!(after.zoom > before.zoom);
        assert_vec2_close(after.pan + anchor * after.zoom, cursor);
    }

    #[test]
    fn graph_can_zoom_out_to_overview_scale_without_losing_cursor_anchor() {
        let cursor = Vec2::new(430.0, 260.0);
        let before = GraphView {
            pan: Vec2::new(-180.0, 75.0),
            zoom: 0.25,
        };
        let anchor = (cursor - before.pan) / before.zoom;

        let after = zoomed_graph_view_at(before, cursor, -20.0);
        assert_eq!(after.zoom, MIN_ZOOM);
        assert_vec2_close(after.pan + anchor * after.zoom, cursor);
        assert_vec2_close(
            graph_drag_delta(Vec2::new(8.0, 4.0), after.zoom),
            Vec2::new(100.0, 50.0),
        );
    }

    #[test]
    fn selected_outline_keeps_a_two_pixel_edge_at_overview_zoom() {
        assert_eq!(selection_outline_width(1.0), 0.0);
        for zoom in [0.5, 0.25, MIN_ZOOM] {
            let screen_width = (SELECTED_BORDER_WIDTH + selection_outline_width(zoom)) * zoom;
            assert!((screen_width - MIN_SELECTED_BORDER_SCREEN_WIDTH).abs() < 0.001);
        }
    }

    #[test]
    fn selected_outline_tracks_its_own_viewport_zoom() {
        use bevy::ecs::system::RunSystemOnce;

        let mut app = App::new();
        let viewport = app
            .world_mut()
            .spawn(FeathersGraphViewport {
                key: "view".into(),
                pan: Vec2::ZERO,
                zoom: MIN_ZOOM,
                content_size: Vec2::new(1000.0, 800.0),
                selection_bounds: None,
                frame_request: None,
                measured_frame: None,
            })
            .id();
        let node = app
            .world_mut()
            .spawn((
                ChildOf(viewport),
                FeathersGraphNode {
                    graph_key: "graph".into(),
                    node_key: "node".into(),
                    position: Vec2::ZERO,
                    selected: true,
                    pinned: false,
                    collapsed: false,
                    dragging: false,
                    drag_before: None,
                    drag_modifier: None,
                    suppress_release_click: false,
                },
                Outline::new(Val::Px(0.0), Val::ZERO, theme::ACCENT),
            ))
            .id();

        app.world_mut()
            .run_system_once(sync_graph_selection_outlines)
            .unwrap();
        assert_eq!(
            app.world().get::<Outline>(node).unwrap().width,
            Val::Px(selection_outline_width(MIN_ZOOM))
        );

        app.world_mut()
            .get_mut::<FeathersGraphViewport>(viewport)
            .unwrap()
            .zoom = 1.0;
        app.world_mut()
            .run_system_once(sync_graph_selection_outlines)
            .unwrap();
        assert_eq!(
            app.world().get::<Outline>(node).unwrap().width,
            Val::Px(0.0)
        );
    }

    #[test]
    fn frame_view_uses_overview_scale_for_wide_graphs() {
        let bounds = Rect::from_corners(Vec2::ZERO, Vec2::new(8_000.0, 2_000.0));
        let viewport = Vec2::new(960.0, 540.0);
        let view = framed_graph_view(bounds, viewport, 1.0);
        assert!(view.zoom < 0.25);
        assert!(view.zoom >= MIN_ZOOM);
        let visible_min = bounds.min * view.zoom + view.pan;
        let visible_max = bounds.max * view.zoom + view.pan;
        assert!(visible_min.x >= 0.0 && visible_min.y >= 0.0);
        assert!(visible_max.x <= viewport.x && visible_max.y <= viewport.y);
    }

    #[test]
    fn frame_view_centers_bounds_and_keeps_them_inside_the_viewport() {
        let bounds = Rect::from_corners(Vec2::new(200.0, 120.0), Vec2::new(1_400.0, 720.0));
        let viewport = Vec2::new(960.0, 540.0);

        let view = framed_graph_view(bounds, viewport, 1.0);
        let visible_min = bounds.min * view.zoom + view.pan;
        let visible_max = bounds.max * view.zoom + view.pan;

        assert_vec2_close((visible_min + visible_max) * 0.5, viewport * 0.5);
        assert!(visible_min.x >= 0.0 && visible_min.y >= 0.0);
        assert!(visible_max.x <= viewport.x && visible_max.y <= viewport.y);
    }

    #[test]
    fn canvas_transform_preserves_top_left_pan_while_scaling_about_its_center() {
        let pan = Vec2::new(84.0, -36.0);
        let size = Vec2::new(1_200.0, 800.0);
        let zoom = 1.5;

        let transform = graph_canvas_transform(pan, zoom, size);
        let translation = transform
            .translation
            .resolve(1.0, size, size, default(), default());
        let visual_top_left = translation + size * (1.0 - zoom) * 0.5;

        assert_vec2_close(visual_top_left, pan);
    }

    #[test]
    fn fixed_wire_layer_projects_graph_points_like_the_zoomed_node_canvas() {
        let pan = Vec2::new(-137.0, 82.0);
        let content_size = Vec2::new(1_800.0, 960.0);
        let graph_point = Vec2::new(725.0, 318.0);

        for zoom in [MIN_ZOOM, 0.25, 0.7, 1.0, 1.8] {
            let canvas = graph_canvas_transform(pan, zoom, content_size);
            let canvas_translation =
                canvas
                    .translation
                    .resolve(1.0, content_size, content_size, default(), default());
            let canvas_top_left = canvas_translation + content_size * (1.0 - canvas.scale.x) * 0.5;
            let node_socket_screen = canvas_top_left + graph_point * canvas.scale;

            let viewport = FeathersGraphViewport {
                key: "graph".into(),
                pan,
                zoom,
                content_size,
                selection_bounds: None,
                frame_request: None,
                measured_frame: None,
            };
            let wire_endpoint_screen = viewport.project_graph_point(graph_point);

            assert_vec2_close(wire_endpoint_screen, node_socket_screen);
            assert_vec2_close(
                viewport.unproject_viewport_point(wire_endpoint_screen),
                graph_point,
            );
        }
    }

    #[test]
    fn node_drag_converts_screen_motion_to_graph_motion() {
        assert_vec2_close(
            graph_drag_delta(Vec2::new(30.0, -18.0), 1.5),
            Vec2::new(20.0, -12.0),
        );
        assert_vec2_close(
            graph_drag_delta(Vec2::new(30.0, -18.0), 0.5),
            Vec2::new(60.0, -36.0),
        );
    }

    #[test]
    fn node_drag_release_click_guard_survives_until_the_release_click() {
        let mut node = FeathersGraphNode {
            graph_key: "graph".into(),
            node_key: "node".into(),
            position: Vec2::ZERO,
            selected: false,
            pinned: false,
            collapsed: false,
            dragging: false,
            drag_before: None,
            drag_modifier: None,
            suppress_release_click: false,
        };

        node.begin_drag();
        node.note_drag_motion();
        node.end_drag();

        assert!(node.consume_suppressed_release_click());
        assert!(!node.consume_suppressed_release_click());
    }

    #[test]
    fn graph_ports_anchor_inputs_left_and_outputs_right_on_the_first_body_row() {
        let input = graph_port_row_node(GraphSocketSide::Input);
        assert_eq!(input.position_type, PositionType::Relative);
        assert_eq!(input.flex_direction, FlexDirection::Row);
        assert_eq!(input.justify_content, JustifyContent::FlexStart);

        let output = graph_port_row_node(GraphSocketSide::Output);
        assert_eq!(output.position_type, PositionType::Absolute);
        assert_eq!(output.top, Val::Px(5.0));
        assert_eq!(output.right, Val::Px(0.0));
        assert_eq!(output.flex_direction, FlexDirection::RowReverse);
        assert_eq!(output.justify_content, JustifyContent::FlexStart);
    }
}
