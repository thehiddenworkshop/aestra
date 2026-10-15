//! Shared node presentation state and pointer ownership; semantic edits stay with the host.
use super::graph_pointer::PointerPress;
use bevy::{
    ecs::query::{QueryData, QueryFilter},
    picking::pointer::PointerButton,
    prelude::*,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphNodeDragModifier {
    Duplicate,
    Upstream,
    Downstream,
}

/// Marks semantic expression nodes that support modifier-drag duplication/branch movement.
/// Synthetic output nodes intentionally omit this marker and retain ordinary movement.
#[derive(Component)]
pub(crate) struct GraphModifiedDragTarget;

/// Marks the measured optional preview block inside a graph node body.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FeathersGraphNodePreview;

#[derive(Event, Debug, Clone)]
pub(crate) struct GraphModifiedNodeDrag {
    pub graph: String,
    pub node: String,
    pub before: (Vec2, bool),
    pub after: (Vec2, bool),
    pub modifier: GraphNodeDragModifier,
}

#[derive(Event, Debug, Clone)]
pub(crate) struct GraphPresentationBatchEdit {
    pub graph: String,
    pub before: BTreeMap<String, (Vec2, bool)>,
    pub origin: Option<Entity>,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct FeathersGraphNode {
    pub(super) graph_key: String,
    pub(super) node_key: String,
    pub(super) position: Vec2,
    pub(super) selected: bool,
    pub(super) pinned: bool,
    pub(super) collapsed: bool,
    pub(super) dragging: bool,
    pub(super) drag_before: Option<(Vec2, bool)>,
    pub(super) drag_modifier: Option<GraphNodeDragModifier>,
    pub(super) suppress_release_click: bool,
}

/// One completed user gesture, never camera motion or passive memory synchronization.
#[derive(Event, Clone)]
pub(crate) struct GraphPresentationEdit {
    pub(crate) graph: String,
    pub(crate) node: String,
    pub(crate) before: (Vec2, bool),
    pub(crate) after: (Vec2, bool),
    pub(crate) origin: Option<Entity>,
}

#[derive(Event, Clone)]
pub(crate) struct GraphPinEdit {
    pub(crate) graph: String,
    pub(crate) node: String,
    pub(crate) before: bool,
    pub(crate) after: bool,
    pub(crate) origin: Option<Entity>,
}

impl FeathersGraphNode {
    #[cfg(test)]
    pub(crate) fn clipboard_fixture(graph_key: String, node_key: String, position: Vec2) -> Self {
        Self {
            graph_key,
            node_key,
            position,
            selected: false,
            pinned: false,
            collapsed: false,
            dragging: false,
            drag_before: None,
            drag_modifier: None,
            suppress_release_click: false,
        }
    }

    pub(crate) fn position(&self) -> Vec2 {
        self.position
    }

    pub(crate) fn collapsed(&self) -> bool {
        self.collapsed
    }

    pub(crate) fn graph_key(&self) -> &str {
        &self.graph_key
    }

    pub(crate) fn node_key(&self) -> &str {
        &self.node_key
    }

    pub(super) fn begin_drag(&mut self) {
        self.dragging = true;
        self.suppress_release_click = false;
    }

    pub(super) fn note_drag_motion(&mut self) {
        self.suppress_release_click = true;
    }

    pub(super) fn end_drag(&mut self) {
        self.dragging = false;
    }

    pub(crate) fn consume_suppressed_release_click(&mut self) -> bool {
        std::mem::take(&mut self.suppress_release_click)
    }
}

pub(super) fn begin_graph_node_press<F: QueryFilter>(
    press: On<PointerPress>,
    mut nodes: Query<&mut FeathersGraphNode>,
    parents: Query<&ChildOf>,
    controls: Query<(), F>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Some(entity) = graph_node_from_target(
        press.event_target(),
        &nodes.as_readonly(),
        &parents,
        &controls,
    ) else {
        return;
    };
    if let Ok(mut node) = nodes.get_mut(entity) {
        // A new press starts a new click-or-drag gesture. This clears a stale release guard when a
        // platform did not synthesize a Click after the previous drag.
        node.suppress_release_click = false;
    }
}

pub(super) fn graph_node_from_target<D: QueryData, F: QueryFilter, C: QueryFilter>(
    mut entity: Entity,
    nodes: &Query<D, F>,
    parents: &Query<&ChildOf>,
    controls: &Query<(), C>,
) -> Option<Entity> {
    loop {
        if controls.contains(entity) {
            return None;
        }
        if nodes.contains(entity) {
            return Some(entity);
        }
        entity = parents.get(entity).ok()?.parent();
    }
}
