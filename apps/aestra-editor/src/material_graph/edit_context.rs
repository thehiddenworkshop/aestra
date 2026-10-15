//! Actual material graph view/node identities shared with semantic editing consumers.
use crate::material_graph::MaterialSelectionScope;
use aestra_core::{MaterialExpressionId, MaterialProgramId};
use bevy::prelude::*;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MaterialGraphAction {
    pub(crate) program: MaterialProgramId,
    pub(crate) expression: MaterialExpressionId,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct MaterialGraphViewport {
    pub(crate) program: MaterialProgramId,
    /// The document rendered by this viewport. A restored editor tab can differ from the session's
    /// current material target until the user interacts with that tab.
    pub(crate) editing_target: crate::material_document::MaterialEditingTarget,
    /// The selection scope this viewport belongs to (the editor view, or `None` for the effect
    /// tool panel), so selection is independent per view.
    pub(crate) scope: MaterialSelectionScope,
}
