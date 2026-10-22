//! Complete production function graph registration, in native observer order.
use crate::material_function_editor::graph::{
    body_input, clipboard,
    connection::{self, ConnectionPreview},
    context_input,
    menu_state::FunctionGraphMenuState,
    modified_input, selection_input, socket_palette, wires,
};
use bevy::prelude::*;

pub(crate) fn register(app: &mut App) {
    socket_palette::register(app);
    app.init_resource::<ConnectionPreview>()
        .init_resource::<FunctionGraphMenuState>()
        .init_resource::<crate::material_graph::MaterialGraphSelectionState>()
        .init_resource::<crate::material_graph::clipboard::GraphClipboard>()
        // Graph observers registered here (e.g. handle_function_graph_context_action) read the
        // viewport pan/zoom memory; own its presence so every context registering these observers —
        // including focused tests — has it, not just apps that also add the feathers node-graph plugin.
        .init_resource::<crate::feathers::node_graph::GraphViewportMemory>();
    connection::register_drag(app);
    selection_input::register(app);
    modified_input::register(app);
    context_input::register(app);
    body_input::register_actions(app);
    connection::register_drop(app);
    body_input::register_constants(app);
    wires::register(app);
    context_input::register_dismissal(app);
    clipboard::register(app);
}
