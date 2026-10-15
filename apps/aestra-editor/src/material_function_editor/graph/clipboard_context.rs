//! Actual function graph identity and target activation shared with clipboard consumers.
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_core::{MaterialExpressionId, MaterialFunctionId};
use bevy::prelude::*;

#[derive(Component, Clone, Copy)]
pub(crate) struct View(pub(crate) MaterialFunctionId);
#[derive(Component, Clone, Copy)]
pub(crate) struct ViewScope(pub(crate) crate::material_graph::MaterialSelectionScope);
#[derive(Component, Clone, Copy)]
pub(crate) struct FunctionGraphNodeAction {
    pub(crate) owner: MaterialFunctionId,
    pub(crate) expression: MaterialExpressionId,
    pub(crate) scope: crate::material_graph::MaterialSelectionScope,
}

pub(crate) fn activate_function_graph_target(
    session: &mut EditorSession,
    catalog: &ProjectEffectCatalog,
    owner: MaterialFunctionId,
) -> bool {
    match session.open_material_function(catalog, owner) {
        Ok(()) => true,
        Err(error) => {
            session.status = format!("Function unavailable: {error}");
            false
        }
    }
}
