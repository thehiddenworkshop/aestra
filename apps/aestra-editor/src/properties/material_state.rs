//! Shared actual material inspector selection, independent of property-panel composition.
use aestra_core::{MaterialExpressionId, MaterialProgramId};
use bevy::prelude::Resource;

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MaterialStackInspectorState {
    pub(crate) selected: Option<(MaterialProgramId, MaterialExpressionId)>,
}
