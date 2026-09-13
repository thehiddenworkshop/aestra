use super::super::{FeathersGraphNode, FeathersGraphViewport};
use super::*;
use crate::{
    ProjectEffectCatalog,
    document::DocumentManager,
    editor_view::{ActiveEditorContext, EditorViewManager},
};

/// Runs after UI layout/transform/clipping. Only the registry is mutable: no collector can move a
/// node, trigger a semantic compile, persist a layout, or manufacture an Undo entry.
#[allow(clippy::type_complexity)]
pub(super) fn collect_graph_geometry(
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
    parents: Query<&ChildOf>,
    styles: Query<(&Node, Option<&Visibility>)>,
    catalog: Option<Res<ProjectEffectCatalog>>,
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
            || catalog
                .as_ref()
                .is_some_and(|catalog| catalog.root() != view.key.document.project)
        {
            continue;
        }
        let generation = if let (Some(id), Some(editor_views), Some(documents)) =
            (view.key.view, editor_views.as_deref(), documents.as_deref())
        {
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
    registry.observe(observations, active.and_then(|active| active.active_view));
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
mod tests {
    use super::*;

    #[test]
    fn invalid_measurements_are_unavailable_not_zero_sized_geometry() {
        for (size, inverse_scale) in [
            (Vec2::ZERO, 1.0),
            (Vec2::new(-1.0, 10.0), 1.0),
            (Vec2::new(f32::NAN, 20.0), 1.0),
            (Vec2::splat(f32::INFINITY), 1.0),
            (Vec2::splat(40.0), 0.0),
            (Vec2::splat(40.0), f32::NAN),
        ] {
            assert!(
                logical_size(&ComputedNode {
                    size,
                    inverse_scale_factor: inverse_scale,
                    ..default()
                })
                .is_none()
            );
        }
        let node = ComputedNode {
            size: Vec2::new(448.0, 160.0),
            inverse_scale_factor: 0.5,
            ..default()
        };
        assert_eq!(logical_size(&node), Some(Vec2::new(224.0, 80.0)));
        assert!(
            port_offset(
                &node,
                &UiGlobalTransform::from_scale(Vec2::ZERO),
                &UiGlobalTransform::default()
            )
            .is_none()
        );
    }
}
