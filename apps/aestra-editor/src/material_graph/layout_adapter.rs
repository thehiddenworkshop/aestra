//! Material-specific topology adapters for the semantic-neutral M8 layout contract.
#![allow(dead_code)] // Public request wiring arrives with the explicit M9 arrange controller.

use super::*;
use crate::feathers::graph_layout::{
    GraphDirection, GraphLayoutEdge, GraphLayoutError, GraphLayoutInput, GraphLayoutNode,
    GraphLayoutNodeId, GraphLayoutNodeState, GraphLayoutRegion, GraphLayoutResult,
};
use aestra_compiler::{
    MaterialFunctionBodyProjection, MaterialFunctionGraphProjection, MaterialFunctionGraphTarget,
};

/// Retains the only material-aware mapping around an engine request. Engines see opaque IDs.
#[derive(Debug, Clone)]
pub(crate) struct LayoutAdapter {
    pub input: GraphLayoutInput,
    semantic_by_layout: BTreeMap<GraphLayoutNodeId, GraphNodeKey>,
    layout_by_semantic: BTreeMap<GraphNodeKey, GraphLayoutNodeId>,
}

impl LayoutAdapter {
    pub(crate) fn program(
        projection: &MaterialGraphProjection,
        states: &BTreeMap<GraphNodeKey, GraphLayoutNodeState>,
        region: Option<&BTreeSet<GraphNodeKey>>,
    ) -> Result<Self, GraphLayoutError> {
        let mut expected = projection
            .nodes
            .iter()
            .map(|node| GraphNodeKey::Expression(node.expression))
            .collect::<BTreeSet<_>>();
        expected.insert(GraphNodeKey::MaterialOutputs);
        let edges = projection
            .edges
            .iter()
            .map(|edge| {
                let target = match edge.target {
                    MaterialGraphEdgeTarget::Input { expression, .. }
                    | MaterialGraphEdgeTarget::FunctionInput { expression, .. } => {
                        GraphNodeKey::Expression(expression)
                    }
                    MaterialGraphEdgeTarget::Output(_) => GraphNodeKey::MaterialOutputs,
                };
                (GraphNodeKey::Expression(edge.source), target)
            })
            .collect();
        Self::build(expected, edges, states, region)
    }

    pub(crate) fn function(
        projection: &MaterialFunctionGraphProjection,
        states: &BTreeMap<GraphNodeKey, GraphLayoutNodeState>,
        region: Option<&BTreeSet<GraphNodeKey>>,
    ) -> Result<Self, GraphLayoutError> {
        let MaterialFunctionBodyProjection::Graph { nodes, edges } = &projection.body else {
            return Err(GraphLayoutError::Adapter(
                "custom WESL functions do not have arrangeable nodes".into(),
            ));
        };
        let mut expected = nodes
            .iter()
            .map(|node| GraphNodeKey::Expression(node.id))
            .collect::<BTreeSet<_>>();
        expected.insert(GraphNodeKey::FunctionOutputs);
        let edges = edges
            .iter()
            .map(|edge| {
                let target = match edge.target {
                    MaterialFunctionGraphTarget::Input { expression, .. }
                    | MaterialFunctionGraphTarget::Argument { expression, .. } => {
                        GraphNodeKey::Expression(expression)
                    }
                    MaterialFunctionGraphTarget::Output(_) => GraphNodeKey::FunctionOutputs,
                };
                (GraphNodeKey::Expression(edge.source), target)
            })
            .collect();
        Self::build(expected, edges, states, region)
    }

    fn build(
        expected: BTreeSet<GraphNodeKey>,
        edges: Vec<(GraphNodeKey, GraphNodeKey)>,
        states: &BTreeMap<GraphNodeKey, GraphLayoutNodeState>,
        region: Option<&BTreeSet<GraphNodeKey>>,
    ) -> Result<Self, GraphLayoutError> {
        let actual = states.keys().copied().collect::<BTreeSet<_>>();
        if actual != expected {
            let missing = expected.difference(&actual).count();
            let unexpected = actual.difference(&expected).count();
            return Err(GraphLayoutError::Adapter(format!(
                "geometry does not match topology ({missing} missing, {unexpected} unexpected)"
            )));
        }

        let mut semantic_by_layout = BTreeMap::new();
        let mut layout_by_semantic = BTreeMap::new();
        let mut nodes = Vec::with_capacity(expected.len());
        for (index, semantic) in expected.into_iter().enumerate() {
            let key = GraphLayoutNodeId::from_index(index)?;
            let state = states[&semantic];
            semantic_by_layout.insert(key, semantic);
            layout_by_semantic.insert(semantic, key);
            nodes.push(GraphLayoutNode {
                key,
                position: state.position,
                size: state.size,
                pinned: state.pinned,
                selected: state.selected,
            });
        }

        let edges = edges
            .into_iter()
            .map(|(source, target)| {
                Ok(GraphLayoutEdge {
                    source: *layout_by_semantic.get(&source).ok_or_else(|| {
                        GraphLayoutError::Adapter("edge source is absent from topology".into())
                    })?,
                    target: *layout_by_semantic.get(&target).ok_or_else(|| {
                        GraphLayoutError::Adapter("edge target is absent from topology".into())
                    })?,
                    // Initial layered layout needs node topology only. Stable material port
                    // adapters remain an explicit M14 concern.
                    source_port: None,
                    target_port: None,
                })
            })
            .collect::<Result<Vec<_>, GraphLayoutError>>()?;
        let region = match region {
            None => GraphLayoutRegion::Full,
            Some(region) => GraphLayoutRegion::Nodes(
                region
                    .iter()
                    .map(|semantic| {
                        layout_by_semantic.get(semantic).copied().ok_or_else(|| {
                            GraphLayoutError::Adapter(
                                "requested region contains a node outside the topology".into(),
                            )
                        })
                    })
                    .collect::<Result<_, _>>()?,
            ),
        };
        let input = GraphLayoutInput::try_new(GraphDirection::LeftToRight, nodes, edges, region)?;
        Ok(Self {
            input,
            semantic_by_layout,
            layout_by_semantic,
        })
    }

    pub(crate) fn layout_key(&self, semantic: GraphNodeKey) -> Option<GraphLayoutNodeId> {
        self.layout_by_semantic.get(&semantic).copied()
    }

    /// Converts only a fully validated candidate back to graph presentation keys.
    pub(crate) fn resolve(
        &self,
        result: &GraphLayoutResult,
    ) -> Result<BTreeMap<GraphNodeKey, Vec2>, GraphLayoutError> {
        result.validate(&self.input)?;
        Ok(result
            .positions
            .iter()
            .map(|(key, position)| (self.semantic_by_layout[key], *position))
            .collect())
    }
}

#[cfg(test)]
mod tests;
