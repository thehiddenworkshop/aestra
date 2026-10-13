//! Real ECS presentation state; validates before semantic/history mutation.
use super::transaction::Transaction;
use crate::{
    ProjectEffectCatalog, feathers::node_graph::GraphViewportMemory,
    material_graph::MaterialGraphPreviewState, session::EditorSession,
};
use bevy::prelude::*;

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct State<'w> {
    memory: Option<ResMut<'w, GraphViewportMemory>>,
    previews: Option<ResMut<'w, MaterialGraphPreviewState>>,
}
impl State<'_> {
    pub(crate) fn validate(
        &self,
        transaction: &Transaction,
        undo: bool,
        catalog: &ProjectEffectCatalog,
        session: &EditorSession,
    ) -> Result<(), String> {
        let memory = self
            .memory
            .as_deref()
            .ok_or("Graph layout is unavailable")?;
        transaction
            .validate_previews(undo, self.previews.as_deref().map(|state| &state.visible))?;
        transaction.validate(undo, catalog, session, memory)
    }
    pub(crate) fn apply(&mut self, transaction: &Transaction, undo: bool) {
        // Canvas-less semantic hosts have no preview state. Validation only permits this for
        // transactions without a preview delta; their placement still applies normally.
        let mut no_previews = MaterialGraphPreviewState::default();
        transaction.apply(
            undo,
            self.memory.as_deref_mut().unwrap(),
            self.previews.as_deref_mut().unwrap_or(&mut no_previews),
        );
    }
}
