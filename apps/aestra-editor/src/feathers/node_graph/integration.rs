//! Production graph resources, observers and schedule order shared by both engine gates.
use super::{
    composition::{FeathersGraphSocket, GraphCollapseAction},
    drag::*,
    drag_assist, geometry,
    marquee::*,
    materials::{GraphGridMaterial, GraphWireMaterial},
    memory::GraphViewportMemory,
    navigation::*,
    node::begin_graph_node_press,
    node_controls::*,
    overlay,
    toolbar::{handle_graph_frame_buttons, queue_graph_frame_activation},
    viewport_presentation::*,
};
use crate::feathers::{action_activation::FeathersActionButton, number_input::EditorNumberInput};
use bevy::feathers::controls::{FeathersCheckbox, FeathersTextInputContainer};
use bevy::{prelude::*, ui_render::prelude::UiMaterialPlugin};

pub(super) type GraphNodeControlFilter = Or<(
    With<FeathersGraphSocket>,
    With<GraphCollapseAction>,
    With<EditorNumberInput>,
    With<FeathersCheckbox>,
    With<FeathersTextInputContainer>,
    With<FeathersActionButton>,
)>;

/// Hosts order graph restoration/navigation after their own document lifecycle and UI rebuild.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GraphWidgetSync;

pub(super) fn register(app: &mut App) {
    geometry::register(app);
    app.add_plugins(UiMaterialPlugin::<GraphWireMaterial>::default())
        .add_plugins(UiMaterialPlugin::<GraphGridMaterial>::default())
        .init_resource::<GraphViewportMemory>()
        .init_resource::<overlay::GraphOverlays>()
        .init_resource::<drag_assist::State>()
        .init_resource::<GraphPanGesture>()
        .init_resource::<GraphMarqueeGesture>()
        .init_resource::<GraphNodeDragGesture>()
        .add_observer(queue_graph_frame_activation)
        .add_observer(queue_graph_collapse_activation)
        .add_observer(queue_graph_pin_activation)
        .add_observer(begin_graph_node_press::<GraphNodeControlFilter>)
        .add_observer(begin_graph_node_drag::<GraphNodeControlFilter>)
        .add_observer(drag_graph_node)
        .add_observer(end_graph_node_drag)
        .add_observer(begin_graph_marquee::<GraphNodeControlFilter>)
        .add_observer(update_graph_marquee)
        .add_observer(end_graph_marquee)
        .add_observer(cancel_graph_marquee)
        .add_observer(drag_assist::toggle)
        .add_systems(
            Update,
            (
                attach_graph_grid_materials,
                remember_graph_viewports,
                restore_graph_viewports,
                restore_graph_nodes,
                overlay::reconcile,
                overlay::sync_notice,
                sync_graph_nodes_from_memory,
                handle_graph_frame_buttons,
                handle_graph_collapse_buttons,
                handle_graph_pin_buttons,
                navigate_graph_viewports,
                cleanup_graph_marquee,
                sync_graph_viewport_transforms,
                sync_graph_selection_outlines,
                update_socket_visuals,
                drag_assist::sync_controls,
                drag_assist::sync_guides,
            )
                .chain()
                .in_set(GraphWidgetSync),
        )
        .add_systems(
            PostUpdate,
            sync_graph_grid_materials.after(bevy::ui::UiSystems::Layout),
        );
}

#[cfg(test)]
#[path = "integration/control_tests.rs"]
mod control_tests;
#[cfg(test)]
#[path = "integration/tests.rs"]
mod tests;
