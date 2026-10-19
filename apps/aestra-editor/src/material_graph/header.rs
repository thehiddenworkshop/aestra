//! Actual material workspace header composition and per-view camera identity.
use crate::{
    Localizer,
    feathers::node_graph::{GraphFrameAction, GraphFrameTarget, spawn_graph_frame_button},
    material_graph::{
        MaterialGraphPreviewState, MaterialGraphPreviewTarget, MaterialSelectionScope, arrange,
        menu_actions::MaterialGraphToolbarAction,
    },
    theme,
};
use aestra_compiler::MaterialGraphProjection;
use aestra_core::MaterialProgramId;
use bevy::prelude::*;

pub(super) fn spawn_header(
    parent: &mut ChildSpawnerCommands,
    projection: Option<(&str, &MaterialGraphProjection)>,
    frame_key: &str,
    scope: MaterialSelectionScope,
    previews: &MaterialGraphPreviewState,
    standalone: bool,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    parent
        .spawn(crate::feathers::node_graph::graph_toolbar_bundle())
        .with_children(|header| {
            if standalone {
                spawn_material_graph_toolbar_button(
                    header,
                    asset_server,
                    "icons/chevron-left.svg",
                    localizer.text("material-document-back"),
                    MaterialGraphToolbarAction::EffectContext,
                );
            }
            if let Some((name, graph)) = projection {
                spawn_material_graph_toolbar_button(
                    header,
                    asset_server,
                    "icons/plus.svg",
                    localizer.text("material-graph-add-node"),
                    MaterialGraphToolbarAction::AddNode(graph.program, scope),
                );
                spawn_graph_frame_button(
                    header,
                    asset_server,
                    "icons/frame-all.svg",
                    localizer.text("material-graph-frame-all"),
                    GraphFrameAction::new(frame_key.to_owned(), GraphFrameTarget::All),
                );
                spawn_graph_frame_button(
                    header,
                    asset_server,
                    "icons/frame-selection.svg",
                    localizer.text("material-graph-frame-selection"),
                    GraphFrameAction::new(frame_key.to_owned(), GraphFrameTarget::Selection),
                );
                spawn_material_graph_toolbar_button(
                    header,
                    asset_server,
                    "icons/graph-align.svg",
                    localizer.text("material-graph-arrange"),
                    MaterialGraphToolbarAction::Arrange(
                        graph.program,
                        scope,
                        arrange::ArrangeScope::Graph,
                    ),
                );
                let all_previews_visible = graph_preview_targets(graph)
                    .all(|target| previews.is_visible(graph.program, target));
                spawn_material_graph_toolbar_button(
                    header,
                    asset_server,
                    if all_previews_visible {
                        "icons/hide.svg"
                    } else {
                        "icons/show.svg"
                    },
                    localizer.text(if all_previews_visible {
                        "material-graph-hide-all-previews"
                    } else {
                        "material-graph-show-all-previews"
                    }),
                    MaterialGraphToolbarAction::ToggleAllPreviews(graph.program, scope),
                );
                spawn_material_graph_toolbar_button(
                    header,
                    asset_server,
                    "icons/folder.svg",
                    localizer.text("browser-locate"),
                    MaterialGraphToolbarAction::LocateSource(graph.program),
                );
                crate::feathers::node_graph::spawn_graph_drag_controls(header, asset_server);
                header.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                header.spawn((
                    Text::new(format!(
                        "{}{name}  ·  {} NODES  ·  {} LINKS",
                        if standalone {
                            format!("{} · ", localizer.text("material-document-shared"))
                        } else {
                            String::new()
                        },
                        graph.nodes.len(),
                        graph.edges.len()
                    )),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                ));
            }
        });
}

pub(super) fn spawn_material_graph_toolbar_button(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    icon_path: &'static str,
    label: String,
    action: MaterialGraphToolbarAction,
) -> Entity {
    crate::feathers::node_graph::spawn_graph_tool_button(
        parent,
        asset_server,
        icon_path,
        label,
        action,
    )
}

pub(super) fn graph_preview_targets(
    graph: &MaterialGraphProjection,
) -> impl Iterator<Item = MaterialGraphPreviewTarget> + '_ {
    graph
        .nodes
        .iter()
        .map(|node| MaterialGraphPreviewTarget::Expression(node.expression))
        .chain(std::iter::once(MaterialGraphPreviewTarget::Output))
}

/// Per-view pan/zoom key; node placement remains per document.
pub(super) fn material_graph_viewport_key(
    program: MaterialProgramId,
    view: Option<crate::docking::EditorViewId>,
) -> String {
    match view {
        Some(view) => format!("material:{program}#view:{}", view.0),
        None => format!("material:{program}#tool"),
    }
}
#[cfg(test)]
#[path = "header/tests.rs"]
mod tests;
