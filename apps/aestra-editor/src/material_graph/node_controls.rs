//! Original node selection scopes and inline numeric/toggle identities.
use crate::material_document::MaterialEditingTarget;
use crate::material_graph::MaterialSelectionScope;
use aestra_core::{MaterialExpressionId, MaterialProgramId, material::MaterialValue};
use bevy::prelude::*;

/// Records the selection scope of an interactive graph-node button, so the node-background system
/// and node-click handler resolve selection per view without walking the entity hierarchy.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct MaterialGraphNodeScope(pub(super) MaterialSelectionScope);

#[derive(Component, Debug, Clone)]
pub(super) struct MaterialGraphDefaultNumberControl {
    pub(super) program: MaterialProgramId,
    pub(super) editing_target: MaterialEditingTarget,
    pub(super) expression: MaterialExpressionId,
    pub(super) value: MaterialValue,
    pub(super) component: u8,
}

#[derive(Component, Debug, Clone)]
pub(super) struct MaterialGraphDefaultToggleControl {
    pub(super) program: MaterialProgramId,
    pub(super) editing_target: MaterialEditingTarget,
    pub(super) expression: MaterialExpressionId,
    pub(super) value: bool,
}
