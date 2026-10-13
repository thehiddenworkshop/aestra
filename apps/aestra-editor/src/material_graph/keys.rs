//! Persisted graph/node identities, shared by presentation and semantic resolution.
use aestra_core::{MaterialExpressionId, MaterialFunctionId, MaterialProgramId};
use std::path::Path;

pub(super) const MATERIAL_GRAPH_OUTPUT_NODE_KEY: &str = "output";

/// Document layout identity; multiple views share node placement, not their camera.
pub(crate) fn material_graph_view_key(program: MaterialProgramId) -> String {
    format!("material:{program}")
}

pub(crate) fn material_graph_expression_node_key(expression: MaterialExpressionId) -> String {
    format!("expression:{expression}")
}

pub(crate) fn function_graph_memory_key(root: &Path, function: MaterialFunctionId) -> String {
    format!("function:{}:{function}", root.display())
}
