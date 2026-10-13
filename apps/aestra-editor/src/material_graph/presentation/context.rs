//! Actual project/session resolution, compound snapshot capture and layout validation.
use super::{
    semantic,
    transaction::{self, Snapshot, Transaction},
};
use crate::history::asset_order::Context;
use crate::material_graph::{
    MaterialGraphPreviewState, MaterialGraphPreviewTarget, keys::material_graph_expression_node_key,
};
use crate::{
    ProjectEffectCatalog, feathers::node_graph::GraphViewportMemory, session::EditorSession,
};
use aestra_core::MaterialProgramId;
use std::collections::BTreeSet;

impl Snapshot {
    pub(crate) fn capture(
        graph: &str,
        catalog: &ProjectEffectCatalog,
        session: &EditorSession,
        memory: &GraphViewportMemory,
    ) -> Option<Self> {
        let (stamp, keys) = semantic(graph, catalog, session)?;
        let mut nodes = memory.base_nodes(graph);
        nodes.retain(|key, _| keys.contains(key));
        let mut pinned = memory.pinned_nodes(graph);
        pinned.retain(|key| keys.contains(key));
        Some(Self {
            root: catalog.root().to_owned(),
            generation: catalog.content_revision().generation,
            graph: graph.into(),
            stamp,
            keys,
            nodes,
            pinned,
            order_serial: session.operation_order.edit_serial(),
        })
    }

    /// Attach to the command that just succeeded. No second Undo item for insertion placement.
    pub(crate) fn attach(
        self,
        catalog: &ProjectEffectCatalog,
        session: &mut EditorSession,
        memory: &mut GraphViewportMemory,
    ) {
        if let Some(transaction) = self.transaction(catalog, session, memory) {
            session
                .operation_order
                .attach_layout(Context::current(session), transaction);
        }
    }

    pub(crate) fn attach_with_previews(
        self,
        catalog: &ProjectEffectCatalog,
        session: &mut EditorSession,
        memory: &mut GraphViewportMemory,
        previews: &mut MaterialGraphPreviewState,
        program: MaterialProgramId,
    ) {
        if let Some(mut transaction) = self.transaction(catalog, session, memory) {
            let before = visible(previews, program);
            let after = before
                .iter()
                .filter(|target| match target {
                    MaterialGraphPreviewTarget::Output => true,
                    MaterialGraphPreviewTarget::Expression(id) => transaction
                        .after
                        .keys
                        .contains(&material_graph_expression_node_key(*id)),
                })
                .copied()
                .collect::<BTreeSet<_>>();
            if before != after {
                previews
                    .visible
                    .retain(|(id, target)| *id != program || after.contains(target));
                transaction.previews = Some((program, before, after));
            }
            session
                .operation_order
                .attach_layout(Context::current(session), transaction);
        }
    }

    fn transaction(
        self,
        catalog: &ProjectEffectCatalog,
        session: &EditorSession,
        memory: &mut GraphViewportMemory,
    ) -> Option<Transaction> {
        // A no-op semantic command must not overwrite an earlier history entry's layout.
        if session.operation_order.edit_serial() != self.order_serial + 1 {
            return None;
        }
        let mut after = Self::capture(&self.graph, catalog, session, memory)?;
        after.nodes.retain(|node, _| after.keys.contains(node));
        memory.retain_nodes(&self.graph, |node| after.keys.contains(node));
        Some(Transaction {
            before: self,
            after,
            previews: None,
            invalidated: false,
        })
    }

    pub(super) fn is_current(
        &self,
        catalog: &ProjectEffectCatalog,
        session: &EditorSession,
    ) -> bool {
        self.root == catalog.root()
            && self.generation == catalog.content_revision().generation
            && semantic(&self.graph, catalog, session)
                .is_some_and(|(stamp, keys)| stamp == self.stamp && keys == self.keys)
    }
}

pub(super) fn semantic(
    graph: &str,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Option<(u64, BTreeSet<String>)> {
    if graph.starts_with("material:") {
        semantic::program(
            graph,
            crate::material_graph::graph_inventory::programs(catalog, session),
            |id| catalog.material_program(id).is_ok(),
        )
    } else {
        semantic::function(
            graph,
            catalog.root(),
            catalog.material_functions().ok()?,
            |id| {
                catalog
                    .content()
                    .cached_material_function(aestra_core::material::MaterialFunctionRef::Project(
                        id,
                    ))
                    .is_ok()
            },
        )
    }
}

impl Transaction {
    /// Host semantic resolution remains authoritative; the shared core validates before mutation.
    pub(crate) fn validate(
        &self,
        undo: bool,
        catalog: &ProjectEffectCatalog,
        session: &EditorSession,
        memory: &GraphViewportMemory,
    ) -> Result<(), String> {
        self.validate_layout(
            undo,
            catalog.root(),
            catalog.content_revision().generation,
            |graph| semantic(graph, catalog, session).map(|(stamp, _)| stamp),
            memory,
        )
    }

    pub(crate) fn apply(
        &self,
        undo: bool,
        memory: &mut GraphViewportMemory,
        previews: &mut MaterialGraphPreviewState,
    ) {
        self.apply_layout(undo, memory, &mut previews.visible);
    }
}

pub(crate) fn visible(
    previews: &MaterialGraphPreviewState,
    program: MaterialProgramId,
) -> BTreeSet<MaterialGraphPreviewTarget> {
    transaction::visible_targets(&previews.visible, program)
}
