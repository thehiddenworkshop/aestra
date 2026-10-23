//! Production drag transactions over real placement memory, snapping and wire insertion.
use super::{
    drag_assist,
    graph_cursor::{EntityCursor, OverrideCursor},
    graph_pointer::{PointerDrag, PointerDragEnd, PointerDragStart},
    insertion,
    memory::GraphViewportMemory,
    navigation::MIN_ZOOM,
    node::*,
};
use bevy::{picking::pointer::PointerButton, prelude::*, window::SystemCursorIcon};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource, Default)]
pub(super) struct GraphNodeDragGesture {
    pub(super) active: Option<Entity>,
    pub(super) members: BTreeSet<Entity>,
    before: BTreeMap<String, (Vec2, bool)>,
}

pub(super) fn begin_graph_node_drag<F: bevy::ecs::query::QueryFilter>(
    mut drag: On<PointerDragStart>,
    mut nodes: Query<(Entity, &mut FeathersGraphNode)>,
    parents: Query<&ChildOf>,
    controls: Query<(), F>,
    modified_targets: Query<(), With<GraphModifiedDragTarget>>,
    keys: Res<ButtonInput<KeyCode>>,
    memory: Res<GraphViewportMemory>,
    mut gesture: ResMut<GraphNodeDragGesture>,
    mut assistance: drag_assist::Context,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary || keys.pressed(KeyCode::Space) {
        return;
    }
    let Some(entity) = graph_node_from_target(
        // Global observers run again while bubbling. Keep the original control
        // ancestry, otherwise a rejected socket drag can become a node drag later.
        drag.original_event_target(),
        &nodes.as_readonly(),
        &parents,
        &controls,
    ) else {
        return;
    };
    let modifier = if !modified_targets.contains(entity) {
        None
    } else if keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight) {
        Some(GraphNodeDragModifier::Duplicate)
    } else if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
        Some(GraphNodeDragModifier::Upstream)
    } else if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        Some(GraphNodeDragModifier::Downstream)
    } else {
        None
    };
    let Ok((_, node)) = nodes.get(entity) else {
        return;
    };
    let graph = node.graph_key.clone();
    let selected = node.selected;
    let Some((viewport, _)) = assistance.viewport(entity, &parents) else {
        return;
    };
    let members = if modifier.is_none() && selected {
        nodes
            .iter()
            .filter_map(|(candidate, node)| {
                (node.selected
                    && node.graph_key == graph
                    && std::iter::once(candidate)
                        .chain(parents.iter_ancestors(candidate))
                        .any(|ancestor| ancestor == viewport))
                .then_some(candidate)
            })
            .collect::<BTreeSet<_>>()
    } else {
        BTreeSet::from([entity])
    };
    let before = members
        .iter()
        .filter_map(|member| {
            let (_, node) = nodes.get(*member).ok()?;
            Some((
                node.node_key.clone(),
                memory
                    .node(&node.graph_key, &node.node_key)
                    .unwrap_or((node.position, node.collapsed)),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let Ok((_, mut node)) = nodes.get_mut(entity) else {
        return;
    };
    node.drag_before = Some(
        memory
            .node(&node.graph_key, &node.node_key)
            .unwrap_or((node.position, node.collapsed)),
    );
    node.drag_modifier = modifier;
    node.begin_drag();
    *gesture = GraphNodeDragGesture {
        active: Some(entity),
        members,
        before,
    };
    assistance.begin(entity, &node, &parents, &memory);
    commands.queue(move |world: &mut World| insertion::begin(world, entity));
    drag.propagate(false);
}

pub(super) fn drag_graph_node(
    mut drag: On<PointerDrag>,
    mut nodes: Query<(&mut FeathersGraphNode, &mut Node, &ComputedNode)>,
    parents: Query<&ChildOf>,
    mut assistance: drag_assist::Context,
    gesture: Res<GraphNodeDragGesture>,
    mut memory: ResMut<GraphViewportMemory>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    // The press target can be reported through a rebuilt header child after a collapse or preview
    // action. The gesture captured the actual node at DragStart, so it is the durable owner for
    // every subsequent motion event.
    let Some(entity) = gesture.active else {
        return;
    };
    let Ok((graph_node, _, computed)) = nodes.get(entity) else {
        return;
    };
    if !graph_node.dragging {
        return;
    }
    // Scale the drag by the zoom of the node's own viewport. The viewport's memory key is per-view
    // (and differs from the node's per-document graph key), so resolve it by hierarchy, not by key.
    let zoom = assistance
        .viewport(entity, &parents)
        .map_or(1.0, |(_, zoom)| zoom.max(MIN_ZOOM));
    let delta = graph_drag_delta(drag.delta * computed.inverse_scale_factor, zoom);
    let moving = if gesture.active == Some(entity) {
        &gesture.members
    } else {
        return;
    };
    let position = assistance.motion(
        entity,
        delta,
        graph_node.position,
        zoom,
        &memory,
        moving,
        |id, rect, collapsed| {
            nodes.get(id).is_ok_and(|(node, _, computed)| {
                node.collapsed == collapsed
                    && (computed.size() * computed.inverse_scale_factor - rect.size())
                        .abs()
                        .max_element()
                        <= 0.5
                    && (moving.contains(&id) || node.position.distance(rect.min) <= 0.5)
            })
        },
    );
    if !position.is_finite() {
        return;
    }
    let movement = position - graph_node.position;
    for member in moving {
        let Ok((mut graph_node, mut style, _)) = nodes.get_mut(*member) else {
            continue;
        };
        if *member == entity {
            // Drag is emitted only after picking recognizes real motion. Keep the release guard.
            graph_node.note_drag_motion();
        }
        graph_node.position += movement;
        style.left = Val::Px(graph_node.position.x);
        style.top = Val::Px(graph_node.position.y);
        memory.set_node(
            graph_node.graph_key.clone(),
            graph_node.node_key.clone(),
            graph_node.position,
            graph_node.collapsed,
        );
    }
    override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::Grabbing));
    commands.queue(move |world: &mut World| insertion::motion(world, entity));
    drag.propagate(false);
}

pub(super) fn graph_drag_delta(screen_delta: Vec2, zoom: f32) -> Vec2 {
    screen_delta / zoom.max(MIN_ZOOM)
}

pub(super) fn end_graph_node_drag(
    mut drag: On<PointerDragEnd>,
    mut nodes: Query<(&mut FeathersGraphNode, &mut Node)>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut commands: Commands,
    mut memory: ResMut<GraphViewportMemory>,
    mut gesture: ResMut<GraphNodeDragGesture>,
    mut assistance: drag_assist::Context,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    // DragEnd may bubble from a collapse/preview control, or from a freshly rebuilt descendant.
    // Always finish the node captured at DragStart so the cursor and helper state cannot remain
    // active after the pointer is released.
    let Some(entity) = gesture.active else {
        return;
    };
    let mut edit = None;
    let mut modified = None;
    let mut batch = None;
    if let Ok((mut node, mut style)) = nodes.get_mut(entity) {
        if let Some(before) = node.drag_before.take() {
            let after = (node.position, node.collapsed);
            if before != after {
                if let Some(modifier) = node.drag_modifier.take() {
                    modified = Some(GraphModifiedNodeDrag {
                        graph: node.graph_key.clone(),
                        node: node.node_key.clone(),
                        before,
                        after,
                        modifier,
                    });
                    if modifier == GraphNodeDragModifier::Duplicate {
                        node.position = before.0;
                        style.left = Val::Px(before.0.x);
                        style.top = Val::Px(before.0.y);
                        memory.set_node(&node.graph_key, &node.node_key, before.0, before.1);
                    }
                } else {
                    // A displaced node's displayed drop location becomes its authored base,
                    // including a drag-start/end with no intervening motion event.
                    memory.set_node(&node.graph_key, &node.node_key, after.0, after.1);
                    if gesture.before.len() > 1 {
                        batch = Some(GraphPresentationBatchEdit {
                            graph: node.graph_key.clone(),
                            before: std::mem::take(&mut gesture.before),
                            origin: Some(entity),
                        });
                    } else {
                        edit = Some(GraphPresentationEdit {
                            graph: node.graph_key.clone(),
                            node: node.node_key.clone(),
                            before,
                            after,
                            origin: Some(entity),
                        });
                    }
                }
            }
        }
        node.drag_modifier = None;
        node.end_drag();
    }
    commands.queue(move |world: &mut World| {
        insertion::finish(world, entity, edit);
        if let Some(modified) = modified {
            world.trigger(modified);
        }
        if let Some(batch) = batch {
            world.trigger(batch);
        }
    });
    gesture.active = None;
    gesture.members.clear();
    gesture.before.clear();
    assistance.end(entity);
    override_cursor.0 = None;
    drag.propagate(false);
}

#[cfg(test)]
#[path = "tests/drag.rs"]
mod tests;
