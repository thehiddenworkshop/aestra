//! Shared presentation memory: authored bases are distinct from temporary display offsets.
use super::navigation::GraphView;
use bevy::prelude::*;
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GraphNodeView {
    /// Authored base position. Temporary display offsets must never be stored here.
    pub(super) position: Vec2,
    pub(super) collapsed: bool,
}

#[derive(Resource, Default)]
pub(crate) struct GraphViewportMemory {
    pub(super) views: HashMap<String, GraphView>,
    pub(super) nodes: HashMap<(String, String), GraphNodeView>,
    pub(super) pinned: HashSet<(String, String)>,
    pub(super) offsets: HashMap<(String, String), Vec2>,
    pub(super) placement_revisions: HashMap<(String, String), u64>,
    pub(super) offset_epochs: HashMap<String, u64>,
    pub(super) graph_revisions: HashMap<String, u64>,
    pub(super) serial: u64,
}

impl GraphViewportMemory {
    pub(crate) fn base_nodes(
        &self,
        graph: &str,
    ) -> std::collections::BTreeMap<String, (Vec2, bool)> {
        self.nodes
            .iter()
            .filter(|((key, _), _)| key == graph)
            .map(|((_, key), node)| (key.clone(), (node.position, node.collapsed)))
            .collect()
    }
    pub(crate) fn pinned_nodes(&self, graph: &str) -> BTreeSet<String> {
        self.pinned
            .iter()
            .filter(|(key, _)| key == graph)
            .map(|(_, node)| node.clone())
            .collect()
    }
    /// Remove a document and all of its independent view cameras without touching other widgets.
    pub(crate) fn retain_graphs(&mut self, keep: impl Fn(&str) -> bool) {
        self.views.retain(|key, _| keep(key));
        self.nodes.retain(|(key, _), _| keep(key));
        self.pinned.retain(|(key, _)| keep(key));
        self.offsets.retain(|(key, _), _| keep(key));
        self.placement_revisions.retain(|(key, _), _| keep(key));
        self.offset_epochs.retain(|key, _| keep(key));
        self.graph_revisions.retain(|key, _| keep(key));
    }

    pub(crate) fn retain_nodes(&mut self, graph: &str, keep: impl Fn(&str) -> bool) {
        self.nodes
            .retain(|(key, node), _| key != graph || keep(node));
        self.pinned.retain(|(key, node)| key != graph || keep(node));
        self.offsets
            .retain(|(key, node), _| key != graph || keep(node));
        self.placement_revisions
            .retain(|(key, node), _| key != graph || keep(node));
    }

    /// Semantic reload/edit invalidates session displacement, not authored base placement.
    pub(crate) fn clear_offsets(&mut self, graph: &str) {
        self.offsets.retain(|(key, _), _| key != graph);
        self.touch_graph(graph);
        self.offset_epochs.insert(graph.to_owned(), self.serial);
    }

    /// Monotonic document-local token used to reject stale asynchronous layout results.
    pub(crate) fn placement_revision(&self, graph: &str) -> u64 {
        self.graph_revisions.get(graph).copied().unwrap_or_default()
    }

    fn touch_graph(&mut self, graph: &str) {
        self.serial += 1;
        self.graph_revisions.insert(graph.to_owned(), self.serial);
    }

    pub(crate) fn view(&self, graph_key: &str) -> Option<(Vec2, f32)> {
        self.views.get(graph_key).map(|view| (view.pan, view.zoom))
    }

    pub(crate) fn set_view(&mut self, graph_key: impl Into<String>, pan: Vec2, zoom: f32) {
        self.views.insert(graph_key.into(), GraphView { pan, zoom });
    }

    pub(crate) fn node_position(&self, graph_key: &str, node_key: &str) -> Option<Vec2> {
        let key = (graph_key.to_owned(), node_key.to_owned());
        self.nodes
            .get(&key)
            .map(|node| node.position + self.offsets.get(&key).copied().unwrap_or(Vec2::ZERO))
    }

    /// Persistent base position and collapse state, never the displayed/effective position.
    pub(crate) fn node(&self, graph_key: &str, node_key: &str) -> Option<(Vec2, bool)> {
        self.nodes
            .get(&(graph_key.to_owned(), node_key.to_owned()))
            .map(|node| (node.position, node.collapsed))
    }

    pub(crate) fn is_pinned(&self, graph_key: &str, node_key: &str) -> bool {
        self.pinned
            .contains(&(graph_key.to_owned(), node_key.to_owned()))
    }

    pub(crate) fn set_pinned(&mut self, graph_key: &str, node_key: &str, pinned: bool) {
        let key = (graph_key.to_owned(), node_key.to_owned());
        let changed = if pinned {
            self.pinned.insert(key)
        } else {
            self.pinned.remove(&key)
        };
        if changed {
            self.touch_graph(graph_key);
        }
    }

    pub(crate) fn set_node(
        &mut self,
        graph_key: impl Into<String>,
        node_key: impl Into<String>,
        position: Vec2,
        collapsed: bool,
    ) {
        let graph_key = graph_key.into();
        let key = (graph_key.clone(), node_key.into());
        if self
            .nodes
            .get(&key)
            .is_none_or(|node| node.position != position || node.collapsed != collapsed)
        {
            self.touch_graph(&graph_key);
            self.placement_revisions.insert(key.clone(), self.serial);
        }
        self.offsets.remove(&key);
        self.nodes.insert(
            key,
            GraphNodeView {
                position,
                collapsed,
            },
        );
    }

    pub(crate) fn place_node(
        &mut self,
        graph_key: impl Into<String>,
        node_key: impl Into<String>,
        position: Vec2,
    ) {
        self.set_node(graph_key, node_key, position, false);
    }

    pub(crate) fn remove_node(&mut self, graph_key: &str, node_key: &str) {
        let key = (graph_key.to_owned(), node_key.to_owned());
        let changed = self.nodes.remove(&key).is_some()
            | self.pinned.remove(&key)
            | self.offsets.remove(&key).is_some()
            | self.placement_revisions.remove(&key).is_some();
        if changed {
            self.touch_graph(graph_key);
        }
    }

    /// Replacement rather than accumulation keeps rebuild/remeasurement idempotent.
    #[allow(dead_code)]
    pub(crate) fn set_temporary_offset(
        &mut self,
        graph_key: &str,
        node_key: &str,
        offset: Vec2,
    ) -> bool {
        let key = (graph_key.to_owned(), node_key.to_owned());
        let Some(node) = self.nodes.get(&key) else {
            return false;
        };
        if !offset.is_finite() || !(node.position + offset).is_finite() {
            return false;
        }
        if offset == Vec2::ZERO {
            self.offsets.remove(&key);
        } else {
            self.offsets.insert(key, offset);
        }
        true
    }

    pub(super) fn set_collapsed(
        &mut self,
        graph_key: &str,
        node_key: &str,
        fallback: Vec2,
        collapsed: bool,
    ) {
        // Collapse changes presentation, not placement: preserve the base even while displaced.
        let key = (graph_key.to_owned(), node_key.to_owned());
        self.nodes
            .entry(key)
            .or_insert(GraphNodeView {
                position: fallback,
                collapsed,
            })
            .collapsed = collapsed;
    }
}
