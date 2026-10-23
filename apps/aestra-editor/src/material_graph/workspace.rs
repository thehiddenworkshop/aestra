//! Production material/effect workspace; the function-editor branch stays in its native host.
use crate::feathers::{
    context_menu::{
        spawn_pointer_context_menu_item, spawn_pointer_context_menu_shortcut_item,
        spawn_pointer_context_menu_sized, spawn_pointer_context_submenu,
    },
    node_graph::{FeathersGraphNavigationBlocker, GraphViewportMemory},
    panel::spawn_panel_empty_state,
};
use crate::material_document::MaterialEditingTarget;
use crate::material_graph::{
    MaterialGraphPaletteState, MaterialGraphPreviewState, MaterialGraphSelectionState, arrange,
    asset_drop::GraphDropTarget,
    clipboard,
    composition::spawn_material_graph_canvas,
    header::{material_graph_viewport_key, spawn_header},
    menu_actions::MaterialGraphContextAction,
    palette::{
        MaterialGraphConnectionMenu, MaterialGraphNodeMenu, MaterialGraphPaletteAnchor,
        material_graph_palette_options, spawn_material_graph_palette,
    },
    palette_state::{MaterialGraphConnectionMenuOpen, MaterialGraphNodeMenuOpen},
    projection::selected_projection_for,
};
use crate::{EditorSession, Localizer, ProjectEffectCatalog, theme};
use bevy::prelude::*;

#[allow(
    clippy::too_many_arguments,
    reason = "Preserve the explicit workspace composition contract"
)]
pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    target: &MaterialEditingTarget,
    view: Option<crate::docking::EditorViewId>,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    palette: &MaterialGraphPaletteState,
    selection: &MaterialGraphSelectionState,
    previews: &MaterialGraphPreviewState,
    graph_memory: &GraphViewportMemory,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            let projection = selected_projection_for(target, session, catalog);
            // Pan/zoom is per-view (independent split cameras); node positions stay per-document.
            let viewport_key = projection
                .as_ref()
                .ok()
                .map(|(_, graph, _, _)| material_graph_viewport_key(graph.program, view))
                .unwrap_or_default();
            spawn_header(
                panel,
                projection
                    .as_ref()
                    .ok()
                    .map(|(name, graph, _, _)| (name.as_str(), graph)),
                &viewport_key,
                view,
                previews,
                target.program().is_some(),
                localizer,
                asset_server,
            );
            let Ok((_program_name, projection, instance, program_definition)) = projection else {
                spawn_panel_empty_state(
                    panel,
                    &localizer.text("material-graph-empty"),
                    &projection
                        .err()
                        .unwrap_or_else(|| localizer.text("material-graph-empty-description")),
                    theme::ACCENT,
                );
                return;
            };
            let viewport = spawn_material_graph_canvas(
                panel,
                catalog.root(),
                &projection,
                &program_definition,
                target,
                view,
                &viewport_key,
                instance,
                selection,
                previews,
                graph_memory,
                localizer,
                asset_server,
            );
            panel
                .commands()
                .entity(viewport)
                .insert(GraphDropTarget::program_for(
                    session,
                    projection.program,
                    target,
                ));
            if let Some(open) = palette.open.as_ref().filter(|open| {
                open.program == projection.program
                    && open.scope == view
                    && open.editing_target == *target
            }) {
                let options = material_graph_palette_options(
                    session,
                    catalog,
                    &open.editing_target,
                    projection.program,
                    &projection,
                    open.connection,
                );
                panel.commands().entity(viewport).with_children(|viewport| {
                    spawn_material_graph_palette(
                        viewport,
                        open,
                        &options,
                        &palette.query,
                        localizer,
                    );
                });
            }
            if let Some(open) = palette
                .node_menu
                .as_ref()
                .filter(|open| open.program == projection.program && open.scope == view)
            {
                panel.commands().entity(viewport).with_children(|viewport| {
                    spawn_material_graph_node_menu(viewport, open, localizer);
                });
            }
            if let Some(open) = palette
                .connection_menu
                .as_ref()
                .filter(|open| open.program == projection.program && open.scope == view)
            {
                panel.commands().entity(viewport).with_children(|viewport| {
                    spawn_material_graph_connection_menu(viewport, open);
                });
            }
        });
}

fn spawn_material_graph_node_menu(
    parent: &mut ChildSpawnerCommands,
    open: &MaterialGraphNodeMenuOpen,
    localizer: &Localizer,
) {
    spawn_pointer_context_menu_sized(
        parent,
        open.menu_position,
        190.0,
        MaterialGraphPaletteAnchor,
        (MaterialGraphNodeMenu, FeathersGraphNavigationBlocker),
        |menu| {
            for (label, shortcut, action) in [
                ("Copy", "Ctrl+C", clipboard::Shortcut::Copy),
                ("Cut", "Ctrl+X", clipboard::Shortcut::Cut),
                ("Paste", "Ctrl+V", clipboard::Shortcut::Paste),
            ] {
                spawn_pointer_context_menu_shortcut_item(
                    menu,
                    label,
                    shortcut,
                    MaterialGraphContextAction::Clipboard(open.program, action),
                );
            }
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("material-graph-extract-function"),
                MaterialGraphContextAction::ExtractFunction(open.program),
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("material-graph-duplicate-nodes"),
                MaterialGraphContextAction::Duplicate(open.program),
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("material-graph-delete-nodes"),
                MaterialGraphContextAction::Delete(open.program),
            );
            spawn_pointer_context_submenu(
                menu,
                &localizer.text("material-graph-arrange-section"),
                |menu| {
                    for (label, scope) in [
                        (
                            "material-graph-arrange-selection",
                            arrange::ArrangeScope::Selection,
                        ),
                        (
                            "material-graph-arrange-upstream",
                            arrange::ArrangeScope::Upstream,
                        ),
                        (
                            "material-graph-arrange-downstream",
                            arrange::ArrangeScope::Downstream,
                        ),
                    ] {
                        spawn_pointer_context_menu_item(
                            menu,
                            &localizer.text(label),
                            MaterialGraphContextAction::Arrange(open.program, scope),
                        );
                    }
                },
            );
        },
    );
}

fn spawn_material_graph_connection_menu(
    parent: &mut ChildSpawnerCommands,
    open: &MaterialGraphConnectionMenuOpen,
) {
    spawn_pointer_context_menu_sized(
        parent,
        open.menu_position,
        190.0,
        MaterialGraphPaletteAnchor,
        (MaterialGraphConnectionMenu, FeathersGraphNavigationBlocker),
        |menu| {
            for (index, connection) in open.connections.iter().copied().enumerate() {
                let label = if open.connections.len() == 1 {
                    "Reset connection".to_owned()
                } else {
                    format!("Reset connection {}", index + 1)
                };
                spawn_pointer_context_menu_item(
                    menu,
                    &label,
                    MaterialGraphContextAction::Disconnect(connection),
                );
            }
        },
    );
}

#[cfg(test)]
#[path = "tests/workspace.rs"]
mod tests;
