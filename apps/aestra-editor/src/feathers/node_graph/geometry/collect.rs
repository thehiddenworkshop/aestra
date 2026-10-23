use super::super::{
    navigation::FeathersGraphViewport,
    node::{FeathersGraphNode, FeathersGraphNodePreview},
};
use super::model::{
    GraphGeometryNode, GraphGeometryPort, GraphGeometryRegistry, GraphGeometryView,
    GraphNodeGeometry, GraphPortGeometry, GraphViewKey, NodeObservation, ViewObservation,
};
use crate::{
    document_registry::DocumentManager,
    editor_view_registry::{ActiveEditorContext, EditorViewManager},
};
use bevy::prelude::*;
use std::{collections::BTreeMap, path::PathBuf};

/// Runs after UI layout/transform/clipping. Only the registry is mutable: no collector can move a
/// node, trigger a semantic compile, persist a layout, or manufacture an Undo entry.
#[allow(clippy::type_complexity)]
pub(super) fn collect_graph_geometry(
    In(project): In<Option<PathBuf>>,
    mut registry: ResMut<GraphGeometryRegistry>,
    views: Query<(Entity, &GraphGeometryView, &ComputedNode), With<FeathersGraphViewport>>,
    nodes: Query<(
        Entity,
        &GraphGeometryNode,
        &FeathersGraphNode,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    ports: Query<(
        Entity,
        &GraphGeometryPort,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    previews: Query<(Entity, &ComputedNode, &Node), With<FeathersGraphNodePreview>>,
    parents: Query<&ChildOf>,
    styles: Query<(&Node, Option<&Visibility>)>,
    documents: Option<Res<DocumentManager>>,
    editor_views: Option<Res<EditorViewManager>>,
    active: Option<Res<ActiveEditorContext>>,
) {
    let visible = |entity| {
        std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .all(|ancestor| {
                styles.get(ancestor).map_or(true, |(node, visibility)| {
                    node.display != Display::None && visibility != Some(&Visibility::Hidden)
                })
            })
    };
    let mut observations = BTreeMap::<GraphViewKey, ViewObservation>::new();
    let mut view_entities = BTreeMap::new();
    for (entity, view, computed) in &views {
        if !visible(entity)
            || logical_size(computed).is_none()
            || project
                .as_ref()
                .is_some_and(|root| *root != view.key.document.project)
        {
            continue;
        }
        let generation = if let Some(id) = view
            .key
            .view
            .filter(|_| documents.is_some() || editor_views.is_some())
        {
            // A docked view must resolve through both authorities. A partially torn-down
            // registry must not silently reinterpret a stale dock tab as a tool panel.
            // Resource-free widget fixtures retain the existing unbound measurement mode.
            let (Some(editor_views), Some(documents)) =
                (editor_views.as_deref(), documents.as_deref())
            else {
                continue;
            };
            let Some(open) = editor_views
                .view(id)
                .and_then(|view| documents.document(view.document))
            else {
                continue;
            };
            if open.key != view.key.document.asset {
                continue;
            }
            Some(open.id)
        } else {
            documents
                .as_ref()
                .and_then(|documents| documents.find(&view.key.document.asset))
        };
        view_entities.insert(entity, view.key.clone());
        match observations.entry(view.key.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(ViewObservation {
                    entity,
                    generation,
                    expected: view.nodes.clone(),
                    nodes: BTreeMap::new(),
                    complete: true,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                // Never resolve duplicate live view identities by ECS iteration order.
                entry.get_mut().complete = false;
            }
        }
    }
    let mut node_entities = BTreeMap::new();
    for (entity, marker, node, computed, transform) in &nodes {
        let Some(view) = parents
            .iter_ancestors(entity)
            .find_map(|ancestor| view_entities.get(&ancestor))
        else {
            continue;
        };
        let observation = observations.get_mut(view).unwrap();
        let Some(size) = logical_size(computed).filter(|_| {
            visible(entity)
                && node.position.is_finite()
                && transform.affine().is_finite()
                && transform.try_inverse().is_some()
        }) else {
            observation.complete = false;
            continue;
        };
        node_entities.insert(entity, (view.clone(), marker.key));
        if observation
            .nodes
            .insert(
                marker.key,
                NodeObservation {
                    entity,
                    geometry: GraphNodeGeometry {
                        effective_position: node.position,
                        size,
                        compact_size: size,
                        preview: marker.preview,
                        collapsed: node.collapsed,
                        content: marker.content,
                        ports: Vec::new(),
                        geometry_revision: 0,
                    },
                    content: marker.content,
                    preview: marker.preview,
                    collapsed: node.collapsed,
                    inverse_scale: computed.inverse_scale_factor,
                },
            )
            .is_some()
        {
            observation.complete = false;
        }
    }
    // Preview blocks are the final children of the body. Use their actual computed
    // height, not the material adapter's bootstrap estimate. Margins are logical Px.
    for (entity, computed, style) in &previews {
        if !visible(entity) {
            continue;
        }
        let Some((view, key)) = parents
            .iter_ancestors(entity)
            .find_map(|id| node_entities.get(&id))
        else {
            continue;
        };
        let observation = observations.get_mut(view).unwrap();
        let (Val::Px(top), Val::Px(bottom)) = (style.margin.top, style.margin.bottom) else {
            observation.complete = false;
            continue;
        };
        let Some(size) = logical_size(computed) else {
            observation.complete = false;
            continue;
        };
        let node = observation.nodes.get_mut(key).unwrap();
        node.geometry.compact_size.y -= size.y + top + bottom;
        if !node.geometry.compact_size.is_finite()
            || node.geometry.compact_size.min_element() <= 0.0
        {
            observation.complete = false;
        }
    }
    for (entity, key, computed, transform) in &ports {
        if !visible(entity) {
            continue;
        } // Collapsed bodies deliberately have no visible ports.
        let Some((node_entity, (view, node_key))) = parents
            .iter_ancestors(entity)
            .find_map(|ancestor| node_entities.get(&ancestor).map(|key| (ancestor, key)))
        else {
            continue;
        };
        let observation = observations.get_mut(view).unwrap();
        let (_, _, _, node_computed, node_transform) = nodes.get(node_entity).unwrap();
        let offset = logical_size(computed)
            .and_then(|_| port_offset(node_computed, node_transform, transform));
        let node = observation.nodes.get_mut(node_key).unwrap();
        if let Some(offset) =
            offset.filter(|_| !node.geometry.ports.iter().any(|port| port.key == *key))
        {
            node.geometry
                .ports
                .push(GraphPortGeometry { key: *key, offset });
        } else {
            observation.complete = false;
        }
    }
    let focused = active.and_then(|active| {
        let id = active.active_view?;
        let view = editor_views.as_ref()?.view(id)?;
        (active.active_document == Some(view.document)
            && documents.as_ref()?.is_open(view.document))
        .then_some(id)
    });
    registry.observe(observations, focused);
}

fn logical_size(node: &ComputedNode) -> Option<Vec2> {
    let size = node.size() * node.inverse_scale_factor;
    (node.inverse_scale_factor.is_finite()
        && node.inverse_scale_factor > 0.0
        && size.is_finite()
        && size.min_element() > 0.0)
        .then_some(size)
}

fn port_offset(
    node: &ComputedNode,
    transform: &UiGlobalTransform,
    port: &UiGlobalTransform,
) -> Option<Vec2> {
    // Global centers contain canvas pan/zoom; inverse node transform removes those. Its local
    // coordinates are still physical layout pixels, centered on the node, not its top-left.
    let center = port.to_scale_angle_translation().2;
    let local = transform.try_inverse()?.transform_point2(center);
    let offset = (local + node.size() * 0.5) * node.inverse_scale_factor;
    offset.is_finite().then_some(offset)
}

#[cfg(test)]
#[path = "../tests/geometry_collect.rs"]
mod tests;
