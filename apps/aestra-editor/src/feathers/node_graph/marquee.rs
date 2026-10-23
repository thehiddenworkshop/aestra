//! Real pointer-driven marquee selection, independent of host document routing.
use super::{graph_pointer::*, navigation::FeathersGraphViewport, node::FeathersGraphNode};
use crate::theme;
use bevy::{
    ecs::query::QueryFilter,
    picking::pointer::{Location, PointerButton, PointerId},
    prelude::*,
};
use std::collections::BTreeSet;

#[derive(Resource, Default)]
pub(super) struct GraphMarqueeGesture {
    pointer: Option<(PointerId, Location)>,
    viewport: Option<Entity>,
    overlay: Option<Entity>,
    start: Vec2,
    current: Vec2,
    mode: GraphSelectionMode,
}

#[derive(Component)]
pub(super) struct GraphMarqueeOverlay;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum GraphSelectionMode {
    #[default]
    Replace,
    Add,
    Toggle,
}

#[derive(Event, Debug, Clone)]
pub(crate) struct GraphMarqueeSelection {
    pub viewport: Entity,
    pub nodes: BTreeSet<String>,
    pub mode: GraphSelectionMode,
}

fn graph_viewport_from_target<F: QueryFilter>(
    mut entity: Entity,
    viewports: &Query<(&ComputedNode, &UiGlobalTransform, &FeathersGraphViewport)>,
    nodes: &Query<(), With<FeathersGraphNode>>,
    controls: &Query<(), F>,
    parents: &Query<&ChildOf>,
) -> Option<Entity> {
    loop {
        if nodes.contains(entity) || controls.contains(entity) {
            return None;
        }
        if viewports.contains(entity) {
            return Some(entity);
        }
        entity = parents.get(entity).ok()?.parent();
    }
}

pub(super) fn begin_graph_marquee<F: QueryFilter>(
    mut drag: On<PointerDragStart>,
    viewports: Query<(&ComputedNode, &UiGlobalTransform, &FeathersGraphViewport)>,
    nodes: Query<(), With<FeathersGraphNode>>,
    controls: Query<(), F>,
    parents: Query<&ChildOf>,
    keys: Res<ButtonInput<KeyCode>>,
    ui_scale: Res<UiScale>,
    mut gesture: ResMut<GraphMarqueeGesture>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary
        || keys.pressed(KeyCode::Space)
        || gesture.viewport.is_some()
    {
        return;
    }
    let Some(viewport) = graph_viewport_from_target(
        drag.original_event_target(),
        &viewports,
        &nodes,
        &controls,
        &parents,
    ) else {
        return;
    };
    let Ok((computed, transform, _)) = viewports.get(viewport) else {
        return;
    };
    let pointer = drag.graph_pointer();
    let start = marquee_position(pointer.1.position, computed, transform, ui_scale.0);
    if !start.is_finite() {
        return;
    }
    let mode = if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
        GraphSelectionMode::Toggle
    } else if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        GraphSelectionMode::Add
    } else {
        GraphSelectionMode::Replace
    };
    let overlay = commands
        .spawn((
            GraphMarqueeOverlay,
            ChildOf(viewport),
            Pickable::IGNORE,
            GlobalZIndex(200),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(start.x),
                top: Val::Px(start.y),
                width: Val::Px(0.0),
                height: Val::Px(0.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.38, 0.22, 0.72, 0.16)),
            BorderColor::all(theme::ACCENT),
        ))
        .id();
    *gesture = GraphMarqueeGesture {
        pointer: Some(pointer),
        viewport: Some(viewport),
        overlay: Some(overlay),
        start,
        current: start,
        mode,
    };
    drag.propagate(false);
}

pub(super) fn update_graph_marquee(
    mut drag: On<PointerDrag>,
    viewports: Query<(&ComputedNode, &UiGlobalTransform, &FeathersGraphViewport)>,
    mut overlays: Query<&mut Node, With<GraphMarqueeOverlay>>,
    mut gesture: ResMut<GraphMarqueeGesture>,
    ui_scale: Res<UiScale>,
) {
    let pointer = drag.graph_pointer();
    if drag.button != PointerButton::Primary || !owns_pointer(&gesture, &pointer) {
        return;
    }
    let (Some(viewport), Some(overlay)) = (gesture.viewport, gesture.overlay) else {
        return;
    };
    let Ok((computed, transform, _)) = viewports.get(viewport) else {
        return;
    };
    let current = marquee_position(pointer.1.position, computed, transform, ui_scale.0);
    if !current.is_finite() {
        return;
    }
    gesture.current = current;
    if let Ok(mut node) = overlays.get_mut(overlay) {
        let min = gesture.start.min(current);
        let size = (current - gesture.start).abs();
        node.left = Val::Px(min.x);
        node.top = Val::Px(min.y);
        node.width = Val::Px(size.x);
        node.height = Val::Px(size.y);
    }
    drag.propagate(false);
}

pub(super) fn end_graph_marquee(
    mut drag: On<PointerDragEnd>,
    mut gesture: ResMut<GraphMarqueeGesture>,
    viewports: Query<(&ComputedNode, &UiGlobalTransform, &FeathersGraphViewport)>,
    nodes: Query<(Entity, &FeathersGraphNode, &ComputedNode)>,
    parents: Query<&ChildOf>,
    ui_scale: Res<UiScale>,
    mut commands: Commands,
) {
    let pointer = drag.graph_pointer();
    if drag.button != PointerButton::Primary || !owns_pointer(&gesture, &pointer) {
        return;
    }
    let Some(viewport_entity) = gesture.viewport.take() else {
        return;
    };
    gesture.pointer = None;
    if let Some(overlay) = gesture.overlay.take()
        && let Ok(mut overlay) = commands.get_entity(overlay)
    {
        overlay.despawn();
    }
    let Ok((computed, transform, viewport)) = viewports.get(viewport_entity) else {
        return;
    };
    // Release can carry the last movement even when there was no intervening Drag event.
    let current = marquee_position(pointer.1.position, computed, transform, ui_scale.0);
    if !current.is_finite() || !viewport.zoom.is_finite() || viewport.zoom <= 0.0 {
        return;
    }
    gesture.current = current;
    let start = viewport.unproject_viewport_point(gesture.start);
    let end = viewport.unproject_viewport_point(gesture.current);
    let marquee = Rect::from_corners(start.min(end), start.max(end));
    let selected = nodes
        .iter()
        .filter(|(entity, _, _)| {
            parents
                .iter_ancestors(*entity)
                .any(|ancestor| ancestor == viewport_entity)
        })
        .filter(|(_, node, computed)| {
            let size = computed.size() * computed.inverse_scale_factor;
            if !node.position.is_finite() || !size.is_finite() || size.min_element() <= 0.0 {
                return false;
            }
            let bounds = Rect::from_corners(node.position, node.position + size);
            marquee.min.x <= bounds.max.x
                && marquee.max.x >= bounds.min.x
                && marquee.min.y <= bounds.max.y
                && marquee.max.y >= bounds.min.y
        })
        .map(|(_, node, _)| node.node_key.clone())
        .collect();
    commands.trigger(GraphMarqueeSelection {
        viewport: viewport_entity,
        nodes: selected,
        mode: gesture.mode,
    });
    drag.propagate(false);
}

fn owns_pointer(gesture: &GraphMarqueeGesture, pointer: &(PointerId, Location)) -> bool {
    gesture
        .pointer
        .as_ref()
        .is_some_and(|owner| owner.0 == pointer.0 && owner.1.target == pointer.1.target)
}

/// UI transforms use physical pixels; pointer positions use logical window pixels.
/// Convert display/UI scale exactly once, then undo the viewport's transform.
fn marquee_position(
    pointer: Vec2,
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
    ui_scale: f32,
) -> Vec2 {
    let inverse = computed.inverse_scale_factor;
    if !inverse.is_finite() || inverse <= 0.0 || !ui_scale.is_finite() || ui_scale <= 0.0 {
        return Vec2::splat(f32::NAN);
    }
    let physical = pointer / (ui_scale * inverse);
    (transform.inverse().transform_point2(physical) + computed.size() * 0.5) * inverse
}

fn clear_marquee(gesture: &mut GraphMarqueeGesture, commands: &mut Commands) {
    if let Some(overlay) = gesture.overlay.take()
        && let Ok(mut entity) = commands.get_entity(overlay)
    {
        entity.despawn();
    }
    *gesture = GraphMarqueeGesture::default();
}

pub(super) fn cancel_graph_marquee(
    cancel: On<PointerCancel>,
    mut gesture: ResMut<GraphMarqueeGesture>,
    mut commands: Commands,
) {
    if owns_pointer(&gesture, &cancel.graph_pointer()) {
        clear_marquee(&mut gesture, &mut commands);
    }
}

pub(super) fn cleanup_graph_marquee(
    mut gesture: ResMut<GraphMarqueeGesture>,
    keys: Res<ButtonInput<KeyCode>>,
    viewports: Query<(), With<FeathersGraphViewport>>,
    overlays: Query<(), With<GraphMarqueeOverlay>>,
    mut commands: Commands,
) {
    if gesture
        .viewport
        .is_some_and(|entity| !viewports.contains(entity))
        || gesture
            .overlay
            .is_some_and(|entity| !overlays.contains(entity))
        || keys.just_pressed(KeyCode::Escape)
    {
        clear_marquee(&mut gesture, &mut commands);
    }
}

#[cfg(test)]
#[path = "tests/marquee.rs"]
mod tests;
