//! Actual semantic arrangement request; asynchronous execution remains in arrange.
use crate::{
    feathers::node_graph::geometry::{GraphNodeKey, GraphViewKey},
    material_document::MaterialEditingTarget,
};
use bevy::prelude::*;
use std::collections::BTreeSet;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArrangeScope {
    Graph,
    Selection,
    Upstream,
    Downstream,
}
#[derive(Event, Debug, Clone)]
pub(crate) struct ArrangeGraph {
    pub view: GraphViewKey,
    pub editing_target: MaterialEditingTarget,
    pub scope: ArrangeScope,
    pub seeds: BTreeSet<GraphNodeKey>,
}
