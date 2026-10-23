//! Production material host lifecycle, pointer registration and post-layout wire schedule.
use crate::{
    EditorSession, EditorSet,
    feathers::node_graph::GraphWidgetSync,
    material_graph::{
        MaterialGraphPaletteState, MaterialGraphSelectionState,
        connection_drag::{
            attach_material_graph_wire_materials, begin_material_connection_drag,
            finish_material_connection_drag, stop_material_socket_click,
            update_material_connection_drag, update_material_graph_wires,
        },
        connection_state::MaterialGraphGesture,
    },
};
use bevy::prelude::*;

pub(super) fn register(app: &mut App) {
    app.configure_sets(Update, GraphWidgetSync.after(EditorSet::UiRebuild));
    app.init_resource::<MaterialGraphGesture>()
        .init_resource::<MaterialGraphPaletteState>()
        .init_resource::<crate::material_function_editor::graph::FunctionGraphMenuState>()
        .init_resource::<MaterialGraphSelectionState>()
        .add_systems(
            Update,
            reset_graph_document_transients
                .after(EditorSet::MainUpdate)
                .before(EditorSet::UiRebuild),
        )
        .add_observer(begin_material_connection_drag)
        .add_observer(update_material_connection_drag)
        .add_observer(finish_material_connection_drag)
        .add_observer(stop_material_socket_click)
        .add_systems(
            PostUpdate,
            (
                attach_material_graph_wire_materials,
                ApplyDeferred,
                update_material_graph_wires,
            )
                .chain()
                .after(bevy::transform::TransformSystems::Propagate)
                .after(bevy::ui::UiSystems::PostLayout),
        );
}

pub(crate) fn clear_document_transients(world: &mut World) {
    if let Some(mut palette) = world.get_resource_mut::<MaterialGraphPaletteState>() {
        *palette = default();
    }
    if let Some(mut selection) = world.get_resource_mut::<MaterialGraphSelectionState>() {
        *selection = default();
    }
    if let Some(mut gesture) = world.get_resource_mut::<MaterialGraphGesture>() {
        *gesture = default();
    }
    if let Some(mut menus) =
        world.get_resource_mut::<crate::material_function_editor::graph::FunctionGraphMenuState>()
    {
        *menus = default();
    }
}

fn reset_graph_document_transients(
    session: Res<EditorSession>,
    mut observed: Local<Option<(crate::material_document::MaterialEditingTarget, u64)>>,
    mut palette: ResMut<MaterialGraphPaletteState>,
    mut selection: ResMut<MaterialGraphSelectionState>,
    mut gesture: ResMut<MaterialGraphGesture>,
    mut function_menus: ResMut<crate::material_function_editor::graph::FunctionGraphMenuState>,
) {
    let current = (
        session.material_target.clone(),
        session.history_generation(),
    );
    if observed.as_ref() == Some(&current) {
        return;
    }
    *observed = Some(current);
    *palette = default();
    *selection = default();
    *gesture = default();
    *function_menus = default();
}

#[cfg(test)]
#[path = "tests/host_integration.rs"]
mod tests;
