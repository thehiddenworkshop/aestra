//! Production semantic snapshot resolution. Inventory and source readiness come from the host.
use crate::material_graph::keys::{
    MATERIAL_GRAPH_OUTPUT_NODE_KEY, function_graph_memory_key, material_graph_expression_node_key,
    material_graph_view_key,
};
use aestra_core::{
    MaterialFunctionId, MaterialProgramId,
    material::{MaterialFunction, MaterialProgram, MaterialProgramRef},
};
use std::{
    collections::BTreeSet,
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};

fn stamp(value: &impl std::fmt::Debug) -> u64 {
    let mut hash = DefaultHasher::new();
    format!("{value:?}").hash(&mut hash);
    hash.finish()
}

pub(super) fn program(
    graph: &str,
    programs: impl IntoIterator<Item = MaterialProgram>,
    available: impl FnOnce(MaterialProgramId) -> bool,
) -> Option<(u64, BTreeSet<String>)> {
    let program = programs
        .into_iter()
        .find(|p| material_graph_view_key(p.id) == graph)?;
    if !available(program.id)
        && MaterialProgram::built_in(MaterialProgramRef::BuiltIn(program.id)).is_none()
    {
        return None;
    }
    // Inline single-use constants have no independent canvas geometry or base placement.
    let inline = program.inline_constants();
    let keys = program
        .expressions
        .iter()
        .filter(|expression| !inline.contains(&expression.id))
        .map(|e| material_graph_expression_node_key(e.id))
        .chain(std::iter::once(MATERIAL_GRAPH_OUTPUT_NODE_KEY.into()))
        .collect();
    Some((stamp(&program), keys))
}

pub(super) fn function(
    graph: &str,
    root: &Path,
    functions: impl IntoIterator<Item = MaterialFunction>,
    available: impl FnOnce(MaterialFunctionId) -> bool,
) -> Option<(u64, BTreeSet<String>)> {
    let function = functions
        .into_iter()
        .find(|f| function_graph_memory_key(root, f.id) == graph && f.custom_wesl.is_none())?;
    if !available(function.id) {
        return None;
    }
    let keys = function
        .expressions
        .iter()
        .map(|e| e.id.to_string())
        .chain(std::iter::once("outputs".into()))
        .collect();
    Some((stamp(&function), keys))
}

#[cfg(test)]
#[path = "semantic/tests.rs"]
mod tests;
