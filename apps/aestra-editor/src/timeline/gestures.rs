//! Production seeking and marker/event pointer gestures; native UI composition is separate.
use super::gesture_math::*;
use super::interaction_state::*;
use super::navigation::timeline_cursor_fraction;
use super::navigation_cursor::{EntityCursor, OverrideCursor};
use super::timeline_pointer::{PointerDrag, PointerDragEnd, PointerDragStart, PointerPress};
use super::{EditorSession, TimelineAction, TimelineState};
use crate::localization::Localizer;
use aestra_authoring::EffectCommand;
use bevy::{
    picking::pointer::PointerButton,
    prelude::*,
    ui::RelativeCursorPosition,
    window::{CursorIcon, PrimaryWindow, SystemCursorIcon},
};

pub(super) fn seek_timeline_on_press(
    press: On<PointerPress>,
    timelines: Query<&RelativeCursorPosition, With<TimelineCanvas>>,
    mut state: ResMut<TimelineState>,
    mut session: ResMut<EditorSession>,
) {
    if press.button == PointerButton::Primary {
        if state.context_emitter.take().is_some()
            | state.color_picker_emitter.take().is_some()
            | state.context_effect_clip.take().is_some()
        {
            session.ui_revision += 1;
        }
        seek_timeline_to_pointer(press.event_target(), &timelines, &state, &mut session);
    }
}

pub(super) fn seek_timeline_on_drag(
    drag: On<PointerDrag>,
    timelines: Query<&RelativeCursorPosition, With<TimelineCanvas>>,
    state: Res<TimelineState>,
    mut session: ResMut<EditorSession>,
) {
    if drag.button == PointerButton::Primary {
        seek_timeline_to_pointer(drag.event_target(), &timelines, &state, &mut session);
    }
}

pub(super) fn seek_timeline_to_pointer(
    entity: Entity,
    timelines: &Query<&RelativeCursorPosition, With<TimelineCanvas>>,
    state: &TimelineState,
    session: &mut EditorSession,
) {
    let Ok(cursor) = timelines.get(entity) else {
        return;
    };
    let Some(position) = cursor.normalized else {
        return;
    };
    session.seek_time(state.view.time_at(timeline_cursor_fraction(position.x)));
}

pub(super) fn stop_timeline_control_press(mut press: On<PointerPress>) {
    press.propagate(false);
}

pub(super) fn stop_timeline_control_drag(mut drag: On<PointerDrag>) {
    drag.propagate(false);
}

pub(super) fn begin_timeline_marker_drag(
    drag: On<PointerDragStart>,
    markers: Query<&TimelineMarker>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(control) = markers.get(drag.event_target()) else {
        return;
    };
    let Some(marker) = session
        .effect
        .markers
        .iter()
        .find(|marker| marker.id == control.marker)
    else {
        return;
    };
    state.marker_drag = Some(TimelineMarkerDrag {
        marker: marker.id,
        original_time: marker.time,
        current_time: marker.time,
    });
    override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::Grabbing));
    **cursor = CursorIcon::System(SystemCursorIcon::Grabbing);
}

pub(super) fn move_timeline_marker_drag(
    mut drag_event: On<PointerDrag>,
    markers: Query<&TimelineMarker>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    window: Single<&Window, With<PrimaryWindow>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    let Ok(control) = markers.get(drag_event.event_target()) else {
        return;
    };
    drag_event.propagate(false);
    let Some(mut drag) = state.marker_drag else {
        return;
    };
    if drag.marker != control.marker {
        return;
    }
    let width = canvases
        .iter()
        .map(|canvas| canvas.size().x)
        .fold(0.0, f32::max)
        .max(1.0);
    let logical_distance = screen_distance_to_logical(drag_event.distance.x, window.scale_factor());
    let candidate = (drag.original_time + logical_distance / width * state.view.span())
        .clamp(0.0, session.playback_duration());
    let (time, guide) = snap_marker_time(
        candidate,
        drag.marker,
        &session,
        state.snap,
        state.view,
        width,
    );
    drag.current_time = time.clamp(0.0, session.playback_duration());
    state.marker_drag = Some(drag);
    state.snap_guide = guide;
}

pub(super) fn finish_timeline_marker_drag(
    drag_event: On<PointerDragEnd>,
    markers: Query<&TimelineMarker>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    localizer: Res<Localizer>,
    mut commands: Commands,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(control) = markers.get(drag_event.event_target()) else {
        return;
    };
    let Some(drag) = state.marker_drag.take() else {
        return;
    };
    if drag.marker != control.marker {
        state.marker_drag = Some(drag);
        return;
    }
    state.snap_guide = None;
    override_cursor.0 = None;
    **cursor = CursorIcon::System(SystemCursorIcon::Grab);
    if (drag.current_time - drag.original_time).abs() > 0.000_1 {
        session.execute(
            localizer.text("timeline-move-marker-command"),
            EffectCommand::SetMarkerTime {
                id: drag.marker,
                time: drag.current_time,
            },
            true,
        );
    }
    commands.trigger(TimelineAction::SelectMarker(drag.marker));
}

pub(super) fn begin_choreography_event_drag(
    drag: On<PointerDragStart>,
    events: Query<&TimelineChoreographyEvent>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(control) = events.get(drag.event_target()) else {
        return;
    };
    let Some(event) = session
        .effect
        .choreography_events
        .iter()
        .find(|event| event.id == control.event)
    else {
        return;
    };
    state.choreography_event_drag = Some(TimelineChoreographyEventDrag {
        event: event.id,
        original_time: event.time,
        current_time: event.time,
    });
    override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::Grabbing));
    **cursor = CursorIcon::System(SystemCursorIcon::Grabbing);
}

pub(super) fn move_choreography_event_drag(
    mut drag_event: On<PointerDrag>,
    events: Query<&TimelineChoreographyEvent>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    window: Single<&Window, With<PrimaryWindow>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    let Ok(control) = events.get(drag_event.event_target()) else {
        return;
    };
    drag_event.propagate(false);
    let Some(mut drag) = state.choreography_event_drag else {
        return;
    };
    if drag.event != control.event {
        return;
    }
    let width = canvases
        .iter()
        .map(|canvas| canvas.size().x)
        .fold(0.0, f32::max)
        .max(1.0);
    let logical_distance = screen_distance_to_logical(drag_event.distance.x, window.scale_factor());
    let candidate = (drag.original_time + logical_distance / width * state.view.span())
        .clamp(0.0, session.playback_duration());
    let (time, guide) = snap_choreography_event_time(
        candidate, drag.event, &session, state.snap, state.view, width,
    );
    drag.current_time = time.clamp(0.0, session.playback_duration());
    state.choreography_event_drag = Some(drag);
    state.snap_guide = guide;
}

pub(super) fn finish_choreography_event_drag(
    drag_event: On<PointerDragEnd>,
    events: Query<&TimelineChoreographyEvent>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut commands: Commands,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(control) = events.get(drag_event.event_target()) else {
        return;
    };
    let Some(drag) = state.choreography_event_drag.take() else {
        return;
    };
    if drag.event != control.event {
        state.choreography_event_drag = Some(drag);
        return;
    }
    state.snap_guide = None;
    override_cursor.0 = None;
    **cursor = CursorIcon::System(SystemCursorIcon::Grab);
    if (drag.current_time - drag.original_time).abs() > 0.000_1 {
        session.execute(
            "Moved choreography event",
            EffectCommand::SetChoreographyEventTime {
                id: drag.event,
                time: drag.current_time,
            },
            true,
        );
    }
    commands.trigger(TimelineAction::SelectChoreographyEvent(drag.event));
}

#[cfg(test)]
#[path = "gestures/tests.rs"]
mod tests;
