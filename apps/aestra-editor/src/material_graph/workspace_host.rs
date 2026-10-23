//! Production complete material/function workspace host, shared with qualification.
#[cfg(test)]
#[path = "tests/workspace_host.rs"]
mod tests;
use crate::feathers::{node_graph::GraphViewportMemory, scroll::spawn_vertical_scroll_area};
use crate::material_graph::{
    MaterialGraphPaletteState, MaterialGraphPreviewState, MaterialGraphSelectionState,
    header::spawn_header, workspace,
};
use crate::{EditorSession, Localizer, ProjectEffectCatalog, ScrollMemoryKey, theme};
use aestra_compiler::MaterialCompiler;
use bevy::prelude::*;

#[allow(
    clippy::too_many_arguments,
    reason = "Preserve the explicit native workspace host contract"
)]
pub(crate) fn spawn_material_graph_workspace(
    parent: &mut ChildSpawnerCommands,
    override_target: Option<&crate::material_document::MaterialEditingTarget>,
    view: Option<crate::docking::EditorViewId>,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    palette: &MaterialGraphPaletteState,
    function_menus: &crate::material_function_editor::graph::FunctionGraphMenuState,
    selection: &MaterialGraphSelectionState,
    previews: &MaterialGraphPreviewState,
    graph_memory: &GraphViewportMemory,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    // `override_target` renders a specific editor view's document (M4c); the docked tool panel
    // passes `None` and follows the session's material target.
    let target = override_target.unwrap_or(&session.material_target);
    if target.function().is_some() {
        parent
            .spawn(Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|panel| {
                if session
                    .graph_function_for(target, catalog)
                    .is_ok_and(|function| function.custom_wesl.is_none())
                {
                    crate::material_function_editor::spawn_graph(
                        panel,
                        session,
                        catalog,
                        asset_server,
                        graph_memory,
                        selection,
                        function_menus,
                        target,
                        view,
                    );
                    return;
                }
                spawn_header(
                    panel,
                    None,
                    "",
                    view,
                    previews,
                    true,
                    localizer,
                    asset_server,
                );
                let text = function_inspection_text(target, session, catalog);
                spawn_vertical_scroll_area(
                    panel,
                    ScrollMemoryKey::MaterialFunctionInspector,
                    Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        min_width: Val::Px(0.0),
                        padding: UiRect::all(Val::Px(12.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        ..default()
                    },
                    |body| {
                        body.spawn(Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            flex_shrink: 0.0,
                            row_gap: Val::Px(8.0),
                            ..default()
                        })
                        .with_children(|body| {
                            if let Ok(function) = session.graph_function_for(target, catalog) {
                                crate::material_function_editor::spawn(body, &function);
                            }
                            body.spawn((
                                Text::new(&session.status),
                                TextFont {
                                    font_size: 13.0.into(),
                                    ..default()
                                },
                                TextColor(theme::TEXT_MUTED),
                            ));
                            body.spawn((
                                Text::new(text),
                                TextFont {
                                    font_size: 13.0.into(),
                                    ..default()
                                },
                                TextColor(Color::WHITE),
                                Node {
                                    width: Val::Percent(100.0),
                                    min_width: Val::Px(0.0),
                                    ..default()
                                },
                            ));
                        });
                    },
                );
            });
        return;
    }
    workspace::spawn(
        parent,
        target,
        view,
        session,
        catalog,
        palette,
        selection,
        previews,
        graph_memory,
        localizer,
        asset_server,
    );
}

fn function_inspection_text(
    target: &crate::material_document::MaterialEditingTarget,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
) -> String {
    let function = match session.graph_function_for(target, catalog) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let library = match catalog.material_functions() {
        Ok(values) => aestra_compiler::MaterialFunctionLibrary::new(values),
        Err(error) => return error,
    };
    let projection = MaterialCompiler.project_function_graph(&function, &library);
    let mut text = format!(
        "{} — {}\n\nInputs\n",
        projection.name,
        if function.custom_wesl.is_some() {
            "Code function · Custom WESL (read-only)\nThis function is implemented in source code, not an editable node graph."
        } else {
            "Graph function"
        }
    );
    for input in &projection.inputs {
        text.push_str(&format!(
            "{} [{:?}] — {:?}\n",
            input.name, input.value_type, input.default
        ));
    }
    text.push_str("\nOutputs\n");
    for output in &projection.outputs {
        text.push_str(&format!("{} [{:?}]\n", output.name, output.value_type));
    }
    match projection.body {
        aestra_compiler::MaterialFunctionBodyProjection::Graph { nodes, edges } => {
            text.push_str(&format!("\nGraph body — {} nodes, {} connections\nEditing controls are not yet available.\n", nodes.len(), edges.len()));
            for node in nodes {
                text.push_str(&format!("\n{}: {:?}\n", node.id, node.kind));
            }
        }
        aestra_compiler::MaterialFunctionBodyProjection::CustomWesl(source) => {
            text.push_str("\nCustom WESL source (read-only)\n\n");
            text.push_str(&source.source);
        }
    }
    if !projection.diagnostics.is_valid() {
        text.push_str(&format!("\n\nDiagnostics\n{}", projection.diagnostics));
    }
    text
}
