//! Production host-pose pointer selection/retiming, release policy and in-place key visuals.
use super::TimelineState;
use super::gesture_math::snap_marker_time;
use super::interaction_state::TimelineCanvas;
use super::motion_execution::{commit, move_key};
use super::motion_state::{Action, KeyDrag, active};
use super::timeline_pointer::{
    PointerClick, PointerDrag, PointerDragEnd, PointerDragStart, TimelinePointerLocation,
};
use crate::{EditorSession, theme};
use aestra_authoring::{EffectCommand, EffectTransaction, SemanticTarget};
use aestra_core::{EffectId, MarkerId};
use bevy::{
    camera::NormalizedRenderTarget, ecs::entity::ContainsEntity, picking::pointer::PointerButton,
    prelude::*,
};

#[derive(Component)]
pub(super) struct KeyControl {
    pub(super) effect: EffectId,
    pub(super) index: usize,
}

pub(super) fn click_key(
    mut event: On<PointerClick>,
    controls: Query<&KeyControl>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    if state.host_motion.released_key == Some(event.entity) {
        state.host_motion.released_key = None;
        event.propagate(false);
        return;
    }
    if let Ok(key) = controls.get(event.entity)
        && key.effect == session.effect.id
    {
        event.propagate(false);
        commands.trigger(Action::Select(key.index));
    }
}

pub(super) fn begin_drag(
    mut event: On<PointerDragStart>,
    controls: Query<&KeyControl>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Ok(key) = controls.get(event.entity) else {
        return;
    };
    event.propagate(false);
    if key.effect != session.effect.id || key.index == 0 {
        return;
    }
    let Some(original) = session.effect.host_transform_track.clone() else {
        return;
    };
    let Some(pose) = original.keys().get(key.index).cloned() else {
        return;
    };
    // Selection visuals update in place: rebuilding here would despawn the drag target.
    state.host_motion.effect = Some(key.effect);
    state.host_motion.selected = Some(key.index);
    state.clear_emitter_selection();
    state.selected_automation_key = None;
    state.inspected_child = None;
    session.selected_emitter_region = None;
    session.selection.primary = SemanticTarget::Effect(session.effect.id);
    state.host_motion.drag = Some(KeyDrag {
        effect: key.effect,
        index: key.index,
        start: pose.time,
        candidate: original.clone(),
        original,
        selected: key.index,
    });
}

pub(super) fn drag_key(
    mut event: On<PointerDrag>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    parents: Query<&ChildOf>,
    windows: Query<&Window>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Some(drag) = &state.host_motion.drag else {
        return;
    };
    if drag.effect != session.effect.id {
        return;
    }
    event.propagate(false);
    let mut entity = event.entity;
    let canvas = loop {
        if let Ok(canvas) = canvases.get(entity) {
            break canvas;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let window_scale = match &event.timeline_pointer_location().target {
        NormalizedRenderTarget::Window(window) => windows
            .get(window.entity())
            .map_or(1.0, Window::scale_factor),
        _ => 1.0,
    };
    // Picking uses logical window pixels; ComputedNode uses physical pixels.
    // Do not divide by UiScale again, especially in a floating timeline window.
    let width = canvas.size().x / window_scale;
    let candidate = drag_time(drag.start, event.distance.x, width, state.view.span());
    let (time, _) = snap_marker_time(
        candidate,
        MarkerId::from_u128(0),
        &session,
        state.snap,
        state.view,
        width,
    );
    let Ok((track, index)) = move_key(&drag.original, drag.index, time) else {
        return;
    };
    if track == drag.candidate {
        return;
    }
    if session.preview_interaction(EffectTransaction::single(
        "Move host pose",
        EffectCommand::SetHostTransformTrack {
            track: Some(track.clone()),
        },
    )) && let Some(drag) = &mut state.host_motion.drag
    {
        drag.candidate = track;
        drag.selected = index;
    }
}

pub(super) fn drag_time(start: f32, distance: f32, logical_width: f32, span: f32) -> f32 {
    (start + distance / logical_width.max(1.0) * span).max(0.0)
}

pub(super) fn end_drag(
    mut event: On<PointerDragEnd>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Some(drag) = state.host_motion.drag.take() else {
        return;
    };
    event.propagate(false);
    // A Click may follow DragEnd on the old entity. Its positional index is no
    // longer authoritative after retiming across another key.
    state.host_motion.released_key = Some(event.entity);
    let revision = session.ui_revision;
    if drag.effect == session.effect.id
        && session.effect.host_transform_track.as_ref() == Some(&drag.original)
    {
        commit(
            &mut session,
            &mut state,
            Some(drag.candidate),
            Some(drag.selected),
        );
    } else {
        session.restore_interaction_preview();
    }
    if session.ui_revision == revision {
        // Even a no-op drag can have selected a different key without rebuilding
        // during the gesture. Refresh its inspector once the target is released.
        session.ui_revision += 1;
    }
}

pub(super) fn update_keys(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    mut controls: Query<(&KeyControl, &mut Node, &mut BackgroundColor)>,
) {
    let times = state
        .host_motion
        .drag
        .as_ref()
        .map(|drag| drag.original.curves.key_times())
        .or_else(|| {
            session
                .effect
                .host_transform_track
                .as_ref()
                .map(|track| track.curves.key_times())
        })
        .unwrap_or_default();
    let dragged_time = state.host_motion.drag.as_ref().and_then(|drag| {
        drag.candidate
            .curves
            .key_times()
            .get(drag.selected)
            .copied()
    });
    for (key, mut node, mut color) in &mut controls {
        let time = if let Some(drag) = &state.host_motion.drag {
            if key.index == drag.index {
                dragged_time
            } else {
                times.get(key.index).copied()
            }
        } else {
            times.get(key.index).copied()
        };
        let position = time.map(|t| state.view.normalized_time(t));
        let display = if key.effect == session.effect.id
            && position.is_some_and(|p| (0.0..=1.0).contains(&p))
        {
            Display::Flex
        } else {
            Display::None
        };
        let left = Val::Percent(position.unwrap_or(0.0).clamp(0.0, 1.0) * 100.0);
        if node.display != display {
            node.display = display;
        }
        if node.left != left {
            node.left = left;
        }
        let background =
            if active(&session, &state) && state.host_motion.selected == Some(key.index) {
                theme::ACCENT
            } else {
                theme::ACCENT_DIM
            };
        color.set_if_neq(BackgroundColor(background));
    }
}

#[cfg(test)]
#[path = "motion_gestures/tests.rs"]
mod tests;
