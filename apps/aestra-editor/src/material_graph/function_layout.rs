//! Function graphs share the persisted node/camera schema, while retaining distinct document IDs.
use super::*;
use crate::{document::DocumentKey, feathers::node_graph::geometry::GraphGeometryRegistry};
use aestra_core::material::MaterialFunction;
use std::path::Path;

const OUTPUT_NODE: &str = "outputs";

pub(crate) fn function_graph_memory_key(root: &Path, function: MaterialFunctionId) -> String {
    format!("function:{}:{function}", root.display())
}

pub(super) fn restore(
    root: &Path,
    document: &ProjectEditorLayout,
    memory: &mut GraphViewportMemory,
) {
    for (function, layout) in &document.function_graphs {
        let key = function_graph_memory_key(root, *function);
        if let Some(viewport) = layout.viewport {
            memory.set_view(&key, Vec2::from_array(viewport.pan), viewport.zoom);
        }
        for (expression, node) in &layout.nodes {
            memory.set_node(
                &key,
                expression.to_string(),
                Vec2::from_array(node.position),
                node.collapsed,
            );
        }
        if let Some(node) = layout.output {
            memory.set_node(
                &key,
                OUTPUT_NODE,
                Vec2::from_array(node.position),
                node.collapsed,
            );
        }
    }
}

pub(super) fn update(
    root: &Path,
    document: &mut ProjectEditorLayout,
    functions: &[MaterialFunction],
    memory: &GraphViewportMemory,
) {
    for function in functions
        .iter()
        .filter(|function| function.custom_wesl.is_none())
    {
        let key = function_graph_memory_key(root, function.id);
        let expressions = function
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<BTreeSet<_>>();
        let layout = document.function_graphs.entry(function.id).or_default();
        layout.retain_expressions(&expressions);
        layout.nodes = expressions
            .into_iter()
            .filter_map(|expression| {
                memory
                    .node(&key, &expression.to_string())
                    .map(|(position, collapsed)| {
                        (
                            expression,
                            MaterialGraphNodeLayout {
                                position: position.to_array(),
                                collapsed,
                            },
                        )
                    })
            })
            .collect();
        layout.output =
            memory
                .node(&key, OUTPUT_NODE)
                .map(|(position, collapsed)| MaterialGraphNodeLayout {
                    position: position.to_array(),
                    collapsed,
                });
        layout.viewport = memory
            .view(&key)
            .map(|(pan, zoom)| MaterialGraphViewportLayout {
                pan: pan.to_array(),
                zoom,
            });
    }
}

/// The registry elects one visible measurement owner (focused, retained, stable fallback). Only
/// that view supplies the next-open camera; sibling views keep independent live cameras.
pub(super) fn mirror_graph_camera(
    catalog: Res<ProjectEffectCatalog>,
    registry: Res<GraphGeometryRegistry>,
    views: Query<&GraphGeometryView>,
    mut memory: ResMut<GraphViewportMemory>,
) {
    let documents = views
        .iter()
        .map(|view| &view.key.document)
        .collect::<BTreeSet<_>>();
    for document in documents {
        if document.project != catalog.root() {
            continue;
        }
        let Some(snapshot) = registry.snapshot(document) else {
            continue;
        };
        let key = match document.asset {
            DocumentKey::MaterialFunction(id) => function_graph_memory_key(catalog.root(), id),
            DocumentKey::MaterialProgram(id) => material_graph_view_key(id),
            DocumentKey::WeslSource(_) => continue,
        };
        let viewport_key = snapshot.measured_in.view.map_or_else(
            || format!("{key}#tool"),
            |view| format!("{key}#view:{}", view.0),
        );
        let Some(camera) = memory.view(&viewport_key) else {
            continue;
        };
        if memory.view(&key) != Some(camera) {
            memory.set_view(key, camera.0, camera.1);
        }
    }
}

#[cfg(test)]
mod tests;
