//! Complete production material graph plugin registration, in native order.
use crate::material_graph::{
    appearance::handle_material_graph_actions,
    arrange, asset_drop, clipboard,
    controls::{
        focus_material_graph_number_input, handle_material_graph_default_number_change,
        handle_material_graph_default_toggle_change, handle_material_graph_preview_actions,
        handle_material_graph_toolbar_actions, stop_material_graph_preview_toggle_click,
        sync_material_graph_default_number_inputs,
    },
    host_integration, insertion, keyboard, layout_persistence,
    menu_actions::{
        handle_material_graph_context_actions, handle_material_graph_palette_actions,
        queue_material_graph_menu_action_activation,
    },
    menu_opening::{
        open_material_graph_node_menu, open_material_graph_palette, open_material_graph_pin_menu,
        select_material_graph_canvas,
    },
    node_actions::{
        handle_modified_material_node_drag, open_material_graph_function_call,
        select_material_graph_marquee, select_material_graph_node,
    },
    node_previews,
    palette::{
        dismiss_material_graph_palette, focus_material_graph_palette_search,
        open_material_graph_palette_from_keyboard, update_material_graph_palette_search,
    },
    presentation, preset_previews,
};
use bevy::prelude::*;
pub(crate) struct EditorMaterialGraphPlugin;
impl Plugin for EditorMaterialGraphPlugin {
    fn build(&self, app: &mut App) {
        asset_drop::register(app);
        arrange::register(app);
        insertion::register(app);
        layout_persistence::register(app);
        host_integration::register(app);
        node_previews::register(app);
        preset_previews::register(app);
        app.init_resource::<clipboard::GraphClipboard>();
        presentation::register(app);
        app.add_observer(select_material_graph_node)
            .add_observer(open_material_graph_function_call)
            .add_observer(select_material_graph_marquee)
            .add_observer(handle_modified_material_node_drag)
            .add_observer(open_material_graph_palette)
            .add_observer(open_material_graph_pin_menu)
            .add_observer(open_material_graph_node_menu)
            .add_observer(select_material_graph_canvas)
            .add_observer(stop_material_graph_preview_toggle_click)
            .add_observer(focus_material_graph_number_input)
            .add_observer(handle_material_graph_default_number_change)
            .add_observer(handle_material_graph_default_toggle_change)
            .add_observer(update_material_graph_palette_search)
            .add_observer(queue_material_graph_menu_action_activation)
            .add_systems(
                Update,
                (
                    open_material_graph_palette_from_keyboard,
                    dismiss_material_graph_palette,
                    focus_material_graph_palette_search,
                    handle_material_graph_palette_actions,
                    handle_material_graph_context_actions,
                    handle_material_graph_actions,
                    handle_material_graph_toolbar_actions,
                    handle_material_graph_preview_actions,
                    sync_material_graph_default_number_inputs,
                ),
            );
        keyboard::register(app);
    }
}

#[cfg(test)]
#[path = "registration/tests.rs"]
mod tests;
