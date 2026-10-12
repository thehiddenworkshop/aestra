//! Presentation transaction data and validation/apply policy; host resolves semantic identity.
use crate::feathers::node_graph::GraphViewportMemory;
use crate::material_graph::MaterialGraphPreviewTarget;
use aestra_core::MaterialProgramId;
use bevy::prelude::Vec2;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

type Nodes = BTreeMap<String, (Vec2, bool)>;
type Visibility = BTreeSet<(MaterialProgramId, MaterialGraphPreviewTarget)>;

#[derive(Clone)]
pub(crate) struct Snapshot {
    pub(super) root: PathBuf,
    pub(super) generation: u64,
    pub(super) graph: String,
    pub(super) stamp: u64,
    pub(super) keys: BTreeSet<String>,
    pub(super) nodes: Nodes,
    pub(super) pinned: BTreeSet<String>,
    pub(super) order_serial: u64,
}

#[derive(Clone)]
pub(crate) struct Transaction {
    pub(super) before: Snapshot,
    pub(super) after: Snapshot,
    pub(super) previews: Option<(
        MaterialProgramId,
        BTreeSet<MaterialGraphPreviewTarget>,
        BTreeSet<MaterialGraphPreviewTarget>,
    )>,
    pub(super) invalidated: bool,
}

impl Transaction {
    pub(crate) fn invalidate(&mut self, graph: Option<&str>) {
        if graph.is_none_or(|graph| self.before.graph == graph) {
            self.invalidated = true;
        }
    }

    /// Validate before touching semantics or presentation. Removed identities are only restored
    /// by a compound command whose semantic history restores them first.
    pub(crate) fn validate_layout(
        &self,
        undo: bool,
        root: &std::path::Path,
        generation: u64,
        semantic_stamp: impl FnOnce(&str) -> Option<u64>,
        memory: &GraphViewportMemory,
    ) -> Result<(), String> {
        let expected = if undo { &self.after } else { &self.before };
        if self.invalidated
            || expected.root != root
            || expected.generation != generation
            || semantic_stamp(&expected.graph).is_none_or(|stamp| stamp != expected.stamp)
        {
            return Err("Layout history is stale: the graph was removed or reloaded".into());
        }
        for key in self.changed_nodes() {
            if self
                .before
                .nodes
                .get(&key)
                .into_iter()
                .chain(self.after.nodes.get(&key))
                .any(|(position, _)| !position.is_finite())
            {
                return Err("Invalid layout position".into());
            }
            if memory.node(&expected.graph, &key) != expected.nodes.get(&key).copied() {
                return Err("Layout changed since this action; history was not applied".into());
            }
            if memory.is_pinned(&expected.graph, &key) != expected.pinned.contains(&key) {
                return Err(
                    "Layout pinning changed since this action; history was not applied".into(),
                );
            }
        }
        Ok(())
    }

    fn changed_nodes(&self) -> BTreeSet<String> {
        self.before
            .nodes
            .keys()
            .chain(self.after.nodes.keys())
            .filter(|key| self.before.nodes.get(*key) != self.after.nodes.get(*key))
            .chain(self.before.pinned.symmetric_difference(&self.after.pinned))
            .cloned()
            .collect()
    }

    pub(crate) fn apply_layout(
        &self,
        undo: bool,
        memory: &mut GraphViewportMemory,
        previews: &mut Visibility,
    ) {
        let target = if undo { &self.before } else { &self.after };
        for key in self.changed_nodes() {
            if let Some((position, collapsed)) = target.nodes.get(&key) {
                memory.set_node(&target.graph, &key, *position, *collapsed);
                memory.set_pinned(&target.graph, &key, target.pinned.contains(&key));
            } else {
                memory.remove_node(&target.graph, &key);
            }
        }
        memory.clear_offsets(&target.graph);
        if let Some((program, before, after)) = &self.previews {
            previews.retain(|(id, _)| id != program);
            previews.extend(
                (if undo { before } else { after })
                    .iter()
                    .map(|target| (*program, *target)),
            );
        }
    }
}

impl Transaction {
    pub(crate) fn validate_previews(
        &self,
        undo: bool,
        previews: Option<&Visibility>,
    ) -> Result<(), String> {
        if let Some((program, before, after)) = &self.previews {
            let previews = previews.ok_or("Graph previews are unavailable")?;
            if visible_targets(previews, *program) != *(if undo { after } else { before }) {
                return Err("Preview visibility changed since this action".into());
            }
        }
        Ok(())
    }
}

pub(super) fn visible_targets(
    previews: &Visibility,
    program: MaterialProgramId,
) -> BTreeSet<MaterialGraphPreviewTarget> {
    previews
        .iter()
        .filter(|(id, _)| *id == program)
        .map(|(_, target)| *target)
        .collect()
}

#[cfg(test)]
#[path = "transaction/tests.rs"]
mod tests;
