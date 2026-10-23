//! Actual viewport camera lifecycle, grid material preparation and selection outlines.
use super::{
    composition::{FeathersGraphGrid, SELECTED_BORDER_WIDTH},
    materials::GraphGridMaterial,
    memory::GraphViewportMemory,
    navigation::{FeathersGraphViewport, GraphView, MIN_ZOOM},
    node::FeathersGraphNode,
};
use crate::theme;
use bevy::prelude::*;

pub(super) const MIN_SELECTED_BORDER_SCREEN_WIDTH: f32 = 2.0;

pub(super) fn remember_graph_viewports(
    mut memory: ResMut<GraphViewportMemory>,
    viewports: Query<Ref<FeathersGraphViewport>>,
) {
    for viewport in &viewports {
        if viewport.is_added() {
            continue;
        }
        let next = GraphView {
            pan: viewport.pan,
            zoom: viewport.zoom,
        };
        if memory.views.get(&viewport.key) != Some(&next) {
            memory.views.insert(viewport.key.clone(), next);
        }
    }
}

pub(super) fn restore_graph_viewports(
    memory: Res<GraphViewportMemory>,
    mut viewports: Query<&mut FeathersGraphViewport, Added<FeathersGraphViewport>>,
) {
    for mut viewport in &mut viewports {
        let Some(view) = memory.views.get(&viewport.key) else {
            continue;
        };
        viewport.pan = view.pan;
        viewport.zoom = view.zoom;
        viewport.frame_request = None;
    }
}

pub(super) fn attach_graph_grid_materials(
    grids: Query<
        Entity,
        (
            With<FeathersGraphGrid>,
            Without<MaterialNode<GraphGridMaterial>>,
        ),
    >,
    mut materials: ResMut<Assets<GraphGridMaterial>>,
    mut commands: Commands,
) {
    for entity in &grids {
        commands
            .entity(entity)
            .insert(MaterialNode(materials.add(GraphGridMaterial::default())));
    }
}

pub(super) fn sync_graph_grid_materials(
    viewports: Query<&FeathersGraphViewport>,
    grids: Query<(&FeathersGraphGrid, &MaterialNode<GraphGridMaterial>)>,
    mut materials: ResMut<Assets<GraphGridMaterial>>,
) {
    for (grid, material) in &grids {
        let Ok(viewport) = viewports.get(grid.viewport) else {
            continue;
        };
        let Some(mut material) = materials.get_mut(&material.0) else {
            continue;
        };
        material.pan = viewport.pan;
        material.zoom = viewport.zoom;
    }
}

/// Keep the selected edge visible when the graph canvas shrinks. The regular border contributes
/// `SELECTED_BORDER_WIDTH * zoom` screen pixels; an outline supplies only the missing amount and
/// does not change node layout or socket positions.
pub(super) fn selection_outline_width(zoom: f32) -> f32 {
    (MIN_SELECTED_BORDER_SCREEN_WIDTH / zoom.max(MIN_ZOOM) - SELECTED_BORDER_WIDTH).max(0.0)
}

pub(super) fn sync_graph_selection_outlines(
    viewports: Query<&FeathersGraphViewport>,
    parents: Query<&ChildOf>,
    mut nodes: Query<(Entity, &FeathersGraphNode, &mut Outline)>,
) {
    for (entity, graph_node, mut outline) in &mut nodes {
        let color = if graph_node.selected {
            theme::ACCENT
        } else {
            Color::NONE
        };
        if outline.color != color {
            outline.color = color;
        }
        if !graph_node.selected {
            continue;
        }
        let Some(viewport) = parents
            .iter_ancestors(entity)
            .find_map(|ancestor| viewports.get(ancestor).ok())
        else {
            continue;
        };
        let width = Val::Px(selection_outline_width(viewport.zoom));
        if outline.width != width {
            outline.width = width;
        }
    }
}

#[cfg(test)]
#[path = "tests/viewport_presentation.rs"]
mod tests;
