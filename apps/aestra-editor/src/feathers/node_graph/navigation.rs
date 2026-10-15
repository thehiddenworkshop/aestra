//! Shared viewport navigation and coordinate transforms; no document or geometry ownership.
use super::graph_cursor::{EntityCursor, OverrideCursor};
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    ui::RelativeCursorPosition,
    window::{CursorMoved, PrimaryWindow, SystemCursorIcon},
};

// Editing remains usable close up; wide graphs can zoom out to an overview.
pub(super) const MIN_ZOOM: f32 = 0.08;
pub(super) const MAX_ZOOM: f32 = 2.0;
pub(super) const FRAME_PADDING: f32 = 42.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphFrameTarget {
    All,
    Selection,
}

/// A visible overlay that owns pointer navigation above a graph viewport.
///
/// Graph wheel navigation is message-based rather than pointer-event based, so overlays use this
/// marker to prevent a wheel gesture from both scrolling the overlay and zooming the graph below.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FeathersGraphNavigationBlocker;

#[derive(Component, Debug, Clone)]
pub(crate) struct FeathersGraphViewport {
    pub(super) key: String,
    pub(super) pan: Vec2,
    pub(super) zoom: f32,
    pub(super) content_size: Vec2,
    pub(super) selection_bounds: Option<Rect>,
    pub(super) frame_request: Option<GraphFrameTarget>,
    pub(super) measured_frame: Option<(GraphFrameTarget, GraphView)>,
}

impl FeathersGraphViewport {
    #[cfg(test)]
    pub(crate) fn clipboard_fixture(key: String, pan: Vec2, zoom: f32) -> Self {
        Self {
            key,
            pan,
            zoom,
            content_size: Vec2::splat(1000.0),
            selection_bounds: None,
            frame_request: None,
            measured_frame: None,
        }
    }

    pub(crate) fn project_graph_point(&self, point: Vec2) -> Vec2 {
        self.pan + point * self.zoom
    }

    pub(crate) fn unproject_viewport_point(&self, point: Vec2) -> Vec2 {
        (point - self.pan) / self.zoom
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct FeathersGraphCanvas {
    pub(super) viewport: Entity,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GraphView {
    pub(super) pan: Vec2,
    pub(super) zoom: f32,
}

#[derive(Resource, Default)]
pub(super) struct GraphPanGesture {
    pub(super) viewport: Option<Entity>,
    pub(super) button: Option<MouseButton>,
    pub(super) cursor_position: Option<Vec2>,
}

pub(super) fn navigate_graph_viewports(
    mut cursor_moved: MessageReader<CursorMoved>,
    mut wheel: MessageReader<MouseWheel>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut gesture: ResMut<GraphPanGesture>,
    mut override_cursor: ResMut<OverrideCursor>,
    ui_scale: Res<UiScale>,
    primary_window: Single<(Entity, &Window), With<PrimaryWindow>>,
    mut viewports: Query<(
        Entity,
        &RelativeCursorPosition,
        &ComputedNode,
        &mut FeathersGraphViewport,
    )>,
    blockers: Query<
        &RelativeCursorPosition,
        (
            With<FeathersGraphNavigationBlocker>,
            Without<FeathersGraphViewport>,
        ),
    >,
) {
    let cursor_position = cursor_moved
        .read()
        .filter(|event| event.window == primary_window.0)
        .last()
        .map(|event| event.position)
        .or_else(|| primary_window.1.cursor_position());
    let scroll_delta = wheel
        .read()
        .filter(|event| event.window == primary_window.0)
        .fold(0.0, |sum, event| {
            let scale = match event.unit {
                MouseScrollUnit::Line => 1.0,
                MouseScrollUnit::Pixel => 0.02,
            };
            sum + event.y * scale
        });
    let navigation_blocked = blockers.iter().any(RelativeCursorPosition::cursor_over);
    let hovered = (!navigation_blocked).then(|| {
        viewports
            .iter()
            .find_map(|(entity, cursor, _, _)| cursor.cursor_over().then_some(entity))
    });
    let hovered = hovered.flatten();
    let space = keys.pressed(KeyCode::Space);
    if gesture
        .viewport
        .is_some_and(|entity| !viewports.contains(entity))
    {
        *gesture = GraphPanGesture::default();
        override_cursor.0 = None;
    }
    let was_panning = gesture.viewport.is_some();

    if !was_panning && let Some(cursor_position) = cursor_position {
        gesture.cursor_position = Some(cursor_position);
    }

    if !was_panning {
        let button = if buttons.just_pressed(MouseButton::Middle) {
            Some(MouseButton::Middle)
        } else if space && buttons.just_pressed(MouseButton::Left) {
            Some(MouseButton::Left)
        } else {
            None
        };
        if let (Some(viewport), Some(button)) = (hovered, button) {
            gesture.viewport = Some(viewport);
            gesture.button = Some(button);
        }
    }

    if let (Some(entity), Some(button)) = (gesture.viewport, gesture.button) {
        // Apply cursor movement before testing release. Otherwise the final movement and button-up
        // arriving in the same frame leaves the canvas visibly trailing a fast pointer.
        if was_panning && let Some(cursor_position) = cursor_position {
            let pointer_delta = gesture
                .cursor_position
                .replace(cursor_position)
                .map_or(Vec2::ZERO, |previous| cursor_position - previous);
            if pointer_delta != Vec2::ZERO
                && let Ok((_, _, _, mut viewport)) = viewports.get_mut(entity)
            {
                // CursorMoved already excludes display scale. Convert logical window pixels
                // to UI units once; graph zoom must not affect screen-space panning.
                viewport.pan += pointer_delta / ui_scale.0.max(0.01);
                viewport.frame_request = None;
            }
        }
        let still_active = buttons.pressed(button) && (button != MouseButton::Left || space);
        if still_active {
            override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::Grabbing));
        } else {
            gesture.viewport = None;
            gesture.button = None;
            gesture.cursor_position = None;
            override_cursor.0 = None;
        }
    }

    if gesture.viewport.is_none()
        && let Some(entity) = hovered
        && let Ok((_, _, _, mut viewport)) = viewports.get_mut(entity)
    {
        if keys.just_pressed(KeyCode::Home) {
            viewport.frame_request = Some(GraphFrameTarget::All);
        } else if keys.just_pressed(KeyCode::KeyF) {
            viewport.frame_request = Some(GraphFrameTarget::Selection);
        }
    }

    if gesture.viewport.is_some() || scroll_delta == 0.0 {
        return;
    }
    let Some(entity) = hovered else {
        return;
    };
    let Ok((_, cursor, computed, mut viewport)) = viewports.get_mut(entity) else {
        return;
    };
    let Some(normalized) = cursor.normalized else {
        return;
    };
    let cursor = (normalized + Vec2::splat(0.5)) * computed.size() * computed.inverse_scale_factor;
    let view = zoomed_graph_view_at(
        GraphView {
            pan: viewport.pan,
            zoom: viewport.zoom,
        },
        cursor,
        scroll_delta,
    );
    viewport.pan = view.pan;
    viewport.zoom = view.zoom;
    viewport.frame_request = None;
}

pub(super) fn sync_graph_viewport_transforms(
    mut viewports: Query<(Entity, &mut FeathersGraphViewport)>,
    mut canvases: Query<(&FeathersGraphCanvas, &mut UiTransform)>,
) {
    for (entity, mut viewport) in &mut viewports {
        if let Some((target, view)) = viewport.measured_frame.take()
            && viewport.frame_request == Some(target)
        {
            viewport.pan = view.pan;
            viewport.zoom = view.zoom;
            viewport.frame_request = None;
        }
        for (canvas, mut transform) in &mut canvases {
            if canvas.viewport == entity {
                *transform =
                    graph_canvas_transform(viewport.pan, viewport.zoom, viewport.content_size);
            }
        }
    }
}

pub(super) fn zoomed_graph_view_at(
    mut view: GraphView,
    cursor: Vec2,
    scroll_delta: f32,
) -> GraphView {
    let anchor = (cursor - view.pan) / view.zoom;
    view.zoom = (view.zoom * (scroll_delta * 0.12).exp()).clamp(MIN_ZOOM, MAX_ZOOM);
    view.pan = cursor - anchor * view.zoom;
    view
}

pub(super) fn framed_graph_view(bounds: Rect, viewport_size: Vec2, maximum_zoom: f32) -> GraphView {
    let extent = bounds.size().max(Vec2::splat(1.0));
    let available = (viewport_size - Vec2::splat(FRAME_PADDING * 2.0)).max(Vec2::splat(1.0));
    let zoom = (available.x / extent.x)
        .min(available.y / extent.y)
        .clamp(MIN_ZOOM, maximum_zoom);
    GraphView {
        pan: viewport_size * 0.5 - bounds.center() * zoom,
        zoom,
    }
}

pub(super) fn graph_canvas_transform(pan: Vec2, zoom: f32, content_size: Vec2) -> UiTransform {
    let centered_scale_offset = content_size * (zoom - 1.0) * 0.5;
    UiTransform {
        translation: Val2::px(
            pan.x + centered_scale_offset.x,
            pan.y + centered_scale_offset.y,
        ),
        scale: Vec2::splat(zoom),
        ..default()
    }
}

#[cfg(test)]
#[path = "navigation/tests.rs"]
mod tests;
