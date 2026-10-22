//! Actual graph drop identity; execution stays with the native browser/drop host.
use crate::material_graph::keys::material_graph_view_key;
use crate::{EditorSession, ProjectEffectCatalog, material_document::MaterialEditingTarget};
use aestra_core::{MaterialFunctionId, MaterialProgramId};
use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Owner {
    Program(MaterialProgramId),
    Function(MaterialFunctionId),
}

/// A surviving UI entity must not edit a graph from a previous document/target.
#[derive(Component, Clone)]
pub(crate) struct GraphDropTarget {
    pub(super) owner: Owner,
    pub(super) context: MaterialEditingTarget,
    pub(super) effect: aestra_core::EffectId,
}

impl GraphDropTarget {
    #[cfg(test)]
    pub(crate) fn program(session: &EditorSession, id: MaterialProgramId) -> Self {
        Self::program_for(session, id, &session.material_target)
    }
    pub(crate) fn program_for(
        session: &EditorSession,
        id: MaterialProgramId,
        context: &MaterialEditingTarget,
    ) -> Self {
        Self {
            owner: Owner::Program(id),
            context: context.clone(),
            effect: session.effect.id,
        }
    }
    #[cfg(test)]
    pub(crate) fn function(session: &EditorSession, id: MaterialFunctionId) -> Self {
        Self::function_for(session, id, &session.material_target)
    }
    pub(crate) fn function_for(
        session: &EditorSession,
        id: MaterialFunctionId,
        context: &MaterialEditingTarget,
    ) -> Self {
        Self {
            owner: Owner::Function(id),
            context: context.clone(),
            effect: session.effect.id,
        }
    }
    pub(super) fn check(&self, session: &EditorSession) -> Result<(), String> {
        if session.pending_change.is_some() {
            return Err("Resolve the pending change before adding a function".into());
        }
        if matches!(self.context, MaterialEditingTarget::EffectInstance)
            && self.effect != session.effect.id
        {
            return Err("Graph changed; drag the function again".into());
        }
        if !matches!(
            (&self.owner, &self.context),
            (Owner::Program(_), MaterialEditingTarget::EffectInstance)
                | (Owner::Program(_), MaterialEditingTarget::Program { .. })
                | (Owner::Function(_), MaterialEditingTarget::Function { .. })
        ) {
            return Err("Graph target changed; drag the function again".into());
        }
        Ok(())
    }
    pub(crate) fn editing_target(&self) -> &MaterialEditingTarget {
        &self.context
    }
    pub(super) fn key(&self, catalog: &ProjectEffectCatalog) -> String {
        match self.owner {
            Owner::Program(id) => material_graph_view_key(id),
            Owner::Function(id) => format!("function:{}:{id}", catalog.root().display()),
        }
    }
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod tests;
