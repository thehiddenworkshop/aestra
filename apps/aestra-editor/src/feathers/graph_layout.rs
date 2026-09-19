//! Semantic-neutral contract for explicit graph arrangement engines.
//!
//! This boundary owns only immutable input snapshots and candidate validation. It does not read
//! ECS state, mutate documents, persist positions, or create history entries. M9 can therefore
//! qualify an external engine without coupling it to material graph types.
#![allow(dead_code)] // M8 contract is intentionally consumed by the M9 engine/controller.

use bevy::prelude::{Rect, Vec2};
use std::collections::{BTreeMap, BTreeSet};

mod elk;
pub(crate) use elk::ElkLayeredLayout;

pub(crate) const MAX_LAYOUT_NODES: usize = 4_096;
pub(crate) const MAX_LAYOUT_EDGES: usize = 16_384;
const POSITION_EPSILON: f32 = 0.001;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct GraphLayoutNodeId(u32);

impl GraphLayoutNodeId {
    pub(crate) fn from_index(index: usize) -> Result<Self, GraphLayoutError> {
        u32::try_from(index)
            .map(Self)
            .map_err(|_| GraphLayoutError::TooManyNodes(index))
    }

    pub(crate) const fn index(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct GraphLayoutPortId(pub u32);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum GraphDirection {
    #[default]
    LeftToRight,
    TopToBottom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GraphLayoutRegion {
    Full,
    Nodes(BTreeSet<GraphLayoutNodeId>),
}

impl GraphLayoutRegion {
    fn contains(&self, key: GraphLayoutNodeId) -> bool {
        match self {
            Self::Full => true,
            Self::Nodes(nodes) => nodes.contains(&key),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GraphLayoutNodeState {
    pub position: Vec2,
    pub size: Vec2,
    pub pinned: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GraphLayoutNode {
    pub key: GraphLayoutNodeId,
    pub position: Vec2,
    pub size: Vec2,
    pub pinned: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GraphLayoutEdge {
    pub source: GraphLayoutNodeId,
    pub target: GraphLayoutNodeId,
    pub source_port: Option<GraphLayoutPortId>,
    pub target_port: Option<GraphLayoutPortId>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GraphLayoutInput {
    pub direction: GraphDirection,
    pub nodes: Vec<GraphLayoutNode>,
    pub edges: Vec<GraphLayoutEdge>,
    pub region: GraphLayoutRegion,
}

impl GraphLayoutInput {
    /// Establishes canonical ordering before an input crosses the engine boundary.
    pub(crate) fn try_new(
        direction: GraphDirection,
        mut nodes: Vec<GraphLayoutNode>,
        mut edges: Vec<GraphLayoutEdge>,
        region: GraphLayoutRegion,
    ) -> Result<Self, GraphLayoutError> {
        nodes.sort_by_key(|node| node.key);
        edges.sort();
        let input = Self {
            direction,
            nodes,
            edges,
            region,
        };
        input.validate()?;
        Ok(input)
    }

    pub(crate) fn validate(&self) -> Result<(), GraphLayoutError> {
        if self.nodes.len() > MAX_LAYOUT_NODES {
            return Err(GraphLayoutError::TooManyNodes(self.nodes.len()));
        }
        if self.edges.len() > MAX_LAYOUT_EDGES {
            return Err(GraphLayoutError::TooManyEdges(self.edges.len()));
        }
        let mut keys = BTreeSet::new();
        for node in &self.nodes {
            if !keys.insert(node.key) {
                return Err(GraphLayoutError::DuplicateNode(node.key));
            }
            if !node.position.is_finite() {
                return Err(GraphLayoutError::NonFinitePosition(node.key));
            }
            if !node.size.is_finite() || node.size.cmple(Vec2::ZERO).any() {
                return Err(GraphLayoutError::InvalidSize(node.key));
            }
        }
        for edge in &self.edges {
            if !keys.contains(&edge.source) {
                return Err(GraphLayoutError::MissingEdgeNode(edge.source));
            }
            if !keys.contains(&edge.target) {
                return Err(GraphLayoutError::MissingEdgeNode(edge.target));
            }
        }
        if let GraphLayoutRegion::Nodes(region) = &self.region {
            if region.is_empty() {
                return Err(GraphLayoutError::EmptyRegion);
            }
            if let Some(key) = region.iter().find(|key| !keys.contains(key)) {
                return Err(GraphLayoutError::MissingRegionNode(*key));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GraphLayoutResult {
    pub positions: BTreeMap<GraphLayoutNodeId, Vec2>,
    pub bounds: Rect,
}

impl GraphLayoutResult {
    pub(crate) fn from_positions(
        input: &GraphLayoutInput,
        positions: BTreeMap<GraphLayoutNodeId, Vec2>,
    ) -> Result<Self, GraphLayoutError> {
        let bounds = bounds(input, &positions)?;
        let result = Self { positions, bounds };
        result.validate(input)?;
        Ok(result)
    }

    /// Rejects incomplete, stale-looking, non-finite, or constraint-breaking candidates.
    pub(crate) fn validate(&self, input: &GraphLayoutInput) -> Result<(), GraphLayoutError> {
        input.validate()?;
        if self.positions.len() != input.nodes.len() {
            return Err(GraphLayoutError::IncompleteResult);
        }
        for node in &input.nodes {
            let Some(position) = self.positions.get(&node.key).copied() else {
                return Err(GraphLayoutError::IncompleteResult);
            };
            if !position.is_finite() {
                return Err(GraphLayoutError::NonFiniteResult(node.key));
            }
            if (node.pinned || !input.region.contains(node.key)) && !close(position, node.position)
            {
                return Err(GraphLayoutError::MovedFixedNode(node.key));
            }
        }
        if self
            .positions
            .keys()
            .any(|key| !input.nodes.iter().any(|node| node.key == *key))
        {
            return Err(GraphLayoutError::UnknownResultNode);
        }
        if !self.bounds.min.is_finite() || !self.bounds.max.is_finite() {
            return Err(GraphLayoutError::InvalidBounds);
        }
        let required = bounds(input, &self.positions)?;
        if self.bounds.min.x > required.min.x + POSITION_EPSILON
            || self.bounds.min.y > required.min.y + POSITION_EPSILON
            || self.bounds.max.x + POSITION_EPSILON < required.max.x
            || self.bounds.max.y + POSITION_EPSILON < required.max.y
        {
            return Err(GraphLayoutError::InvalidBounds);
        }
        Ok(())
    }
}

pub(crate) trait GraphLayoutEngine: Send + Sync {
    fn layout(&self, input: &GraphLayoutInput) -> Result<GraphLayoutResult, GraphLayoutError>;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum GraphLayoutError {
    #[error("graph has {0} nodes; layout is limited to {MAX_LAYOUT_NODES}")]
    TooManyNodes(usize),
    #[error("graph has {0} edges; layout is limited to {MAX_LAYOUT_EDGES}")]
    TooManyEdges(usize),
    #[error("duplicate layout node {0:?}")]
    DuplicateNode(GraphLayoutNodeId),
    #[error("layout node {0:?} has a non-finite position")]
    NonFinitePosition(GraphLayoutNodeId),
    #[error("layout node {0:?} has a non-finite or non-positive size")]
    InvalidSize(GraphLayoutNodeId),
    #[error("layout edge references missing node {0:?}")]
    MissingEdgeNode(GraphLayoutNodeId),
    #[error("layout region cannot be empty")]
    EmptyRegion,
    #[error("layout region references missing node {0:?}")]
    MissingRegionNode(GraphLayoutNodeId),
    #[error("layout result does not contain exactly the input nodes")]
    IncompleteResult,
    #[error("layout result contains an unknown node")]
    UnknownResultNode,
    #[error("layout result for {0:?} is non-finite")]
    NonFiniteResult(GraphLayoutNodeId),
    #[error("layout result moved pinned or out-of-region node {0:?}")]
    MovedFixedNode(GraphLayoutNodeId),
    #[error("layout result bounds are invalid or do not contain all nodes")]
    InvalidBounds,
    #[error("layout adapter: {0}")]
    Adapter(String),
}

fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).abs().max_element() <= POSITION_EPSILON
}

fn bounds(
    input: &GraphLayoutInput,
    positions: &BTreeMap<GraphLayoutNodeId, Vec2>,
) -> Result<Rect, GraphLayoutError> {
    let mut result: Option<Rect> = None;
    for node in &input.nodes {
        let position = positions
            .get(&node.key)
            .copied()
            .ok_or(GraphLayoutError::IncompleteResult)?;
        if !position.is_finite() {
            return Err(GraphLayoutError::NonFiniteResult(node.key));
        }
        let rect = Rect::from_corners(position, position + node.size);
        result = Some(result.map_or(rect, |bounds| {
            Rect::from_corners(bounds.min.min(rect.min), bounds.max.max(rect.max))
        }));
    }
    Ok(result.unwrap_or_default())
}

#[cfg(test)]
mod tests;
