//! Mount completed preview images without coupling canvas assembly to raster scheduling.
use crate::feathers::node_graph::spawn_graph_node_preview;
use crate::material_graph::{MaterialGraphPreviewState, MaterialGraphPreviewTarget};
use aestra_core::{MaterialId, MaterialProgramId, material::MaterialValueType};
use bevy::prelude::*;

#[derive(Component, Debug, Clone)]
pub(super) struct MaterialGraphPreviewRaster {
    pub(super) program: MaterialProgramId,
    pub(super) editing_target: crate::material_document::MaterialEditingTarget,
    pub(super) instance: Option<MaterialId>,
    pub(super) target: MaterialGraphPreviewTarget,
    pub(super) value_type: Option<MaterialValueType>,
}

/// Mount the last completed image in the same command batch as the node.
pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    previews: &MaterialGraphPreviewState,
    request: MaterialGraphPreviewRaster,
) {
    let cached = previews
        .cache
        .iter()
        .find(|(key, _)| {
            key.program == request.program
                && key.target == request.target
                && key.instance == request.instance
                && match &request.editing_target {
                    crate::material_document::MaterialEditingTarget::Program { root, .. } => {
                        &key.root == root
                    }
                    _ => true,
                }
        })
        .map(|(_, cached)| cached.image.clone());
    let entity = spawn_graph_node_preview(parent, request);
    if let Some(image) = cached {
        parent
            .commands()
            .entity(entity)
            .insert(ImageNode::new(image).with_mode(NodeImageMode::Stretch));
    }
}
