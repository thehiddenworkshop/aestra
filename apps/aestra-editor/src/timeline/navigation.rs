//! Production wheel/pan, popup dismissal and focus restoration; panel composition stays native.
use super::interaction_state::*;
use super::motion_execution::timeline_duration;
use super::navigation_cursor::OverrideCursor;
use super::{EditorSession, TimelineState};
use crate::feathers::context_menu::should_dismiss_pointer_context_menu;
use bevy::{
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    ui::RelativeCursorPosition,
    window::{CursorIcon, PrimaryWindow, SystemCursorIcon},
};

pub(super) fn navigate_timeline(
    mut wheel: MessageReader<MouseWheel>,
    mut motion: MessageReader<MouseMotion>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    canvases: Query<(&RelativeCursorPosition, &ComputedNode), With<TimelineCanvas>>,
    track_panes: Query<(&TimelineVerticalPane, &ComputedNode), Without<TimelineCanvas>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        if state.drag.take().is_some()
            | state.effect_clip_drag.take().is_some()
            | state.marker_drag.take().is_some()
            | state.choreography_event_drag.take().is_some()
        {
            state.snap_guide = None;
            override_cursor.0 = None;
            **cursor = CursorIcon::System(SystemCursorIcon::Default);
        }
        state.restore_context_emitter_focus = state.context_emitter;
        state.restore_context_effect_clip_focus = state.context_effect_clip;
        if state.context_emitter.take().is_some()
            | state.color_picker_emitter.take().is_some()
            | state.context_effect_clip.take().is_some()
            | state.automation_menu_emitter.take().is_some()
        {
            session.ui_revision += 1;
        }
    }
    let pointer_delta = motion
        .read()
        .fold(Vec2::ZERO, |sum, event| sum + event.delta);
    let hovered = canvases.iter().find(|(cursor, _)| cursor.cursor_over());

    if buttons.just_pressed(MouseButton::Middle) && hovered.is_some() {
        state.panning = true;
    }
    if buttons.just_released(MouseButton::Middle) {
        state.panning = false;
    }
    if state.panning
        && buttons.pressed(MouseButton::Middle)
        && let Some((_, canvas)) = hovered
    {
        let width = canvas.size().x.max(1.0);
        let delta_time = -pointer_delta.x / width * state.view.span();
        state.pan_by(delta_time, timeline_duration(&session));
    }

    let (scroll, track_scroll) =
        wheel
            .read()
            .fold((Vec2::ZERO, 0.0), |(time_sum, track_sum), event| {
                let time_scale = match event.unit {
                    MouseScrollUnit::Line => 1.0,
                    MouseScrollUnit::Pixel => 0.01,
                };
                let track_scale = match event.unit {
                    MouseScrollUnit::Line => 21.0,
                    MouseScrollUnit::Pixel => 1.0,
                };
                (
                    time_sum + Vec2::new(event.x, event.y) * time_scale,
                    track_sum + event.y * track_scale,
                )
            });
    // The native ScrollArea on the header list owns vertical wheel scrolling. The synchronized
    // pane system mirrors that offset to the track canvas on the same frame.
    let Some((cursor, _)) = hovered else {
        return;
    };
    if scroll == Vec2::ZERO {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    match timeline_wheel_intent(scroll, track_scroll, control, shift) {
        TimelineWheelIntent::ZoomTime(amount) => {
            if let Some(position) = cursor.normalized {
                let anchor = state.view.time_at(timeline_cursor_fraction(position.x));
                state.zoom_at(
                    anchor,
                    0.82_f32.powf(amount),
                    timeline_duration(&session),
                    session.tick_rate(),
                );
            }
        }
        TimelineWheelIntent::PanTime(amount) => {
            let span = state.view.span();
            state.pan_by(-amount * span * 0.08, timeline_duration(&session));
        }
        TimelineWheelIntent::ScrollTracks(amount) => {
            let maximum = track_panes
                .iter()
                .find(|(kind, _)| **kind == TimelineVerticalPane::Tracks)
                .map(|(_, computed)| (computed.content_size().y - computed.size().y).max(0.0))
                .unwrap_or(0.0);
            state.vertical_scroll = (state.vertical_scroll - amount).clamp(0.0, maximum);
        }
    }
}

pub(super) fn dismiss_timeline_popovers(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    color_surfaces: Query<
        &RelativeCursorPosition,
        Or<(
            With<EmitterTrackColorChip>,
            With<EmitterTrackColorPickerPopover>,
        )>,
    >,
    menu_surfaces: Query<
        &RelativeCursorPosition,
        Or<(
            With<EmitterTrackContextMenu>,
            With<EffectClipTrackContextMenu>,
        )>,
    >,
    automation_surfaces: Query<
        &RelativeCursorPosition,
        Or<(
            With<EmitterAutomationMenuButton>,
            With<EmitterAutomationVisibilityMenu>,
        )>,
    >,
    mut state: ResMut<TimelineState>,
    mut session: ResMut<EditorSession>,
) {
    let primary_pressed = buttons.just_pressed(MouseButton::Left);
    let dismiss_color = should_dismiss_timeline_popover(
        state.color_picker_emitter.is_some(),
        color_surfaces
            .iter()
            .any(RelativeCursorPosition::cursor_over),
        primary_pressed,
    );
    let dismiss_menu = should_dismiss_pointer_context_menu(
        state.context_emitter.is_some() || state.context_effect_clip.is_some(),
        primary_pressed,
        keys.just_pressed(KeyCode::Escape),
        menu_surfaces
            .iter()
            .any(RelativeCursorPosition::cursor_over),
    );
    let dismiss_automation = should_dismiss_pointer_context_menu(
        state.automation_menu_emitter.is_some(),
        primary_pressed,
        keys.just_pressed(KeyCode::Escape),
        automation_surfaces
            .iter()
            .any(RelativeCursorPosition::cursor_over),
    );
    if !dismiss_color && !dismiss_menu && !dismiss_automation {
        return;
    }
    if dismiss_color {
        state.color_picker_emitter = None;
    }
    if dismiss_menu {
        state.restore_context_emitter_focus = state.context_emitter;
        state.restore_context_effect_clip_focus = state.context_effect_clip;
        state.context_emitter = None;
        state.context_effect_clip = None;
    }
    if dismiss_automation {
        state.automation_menu_emitter = None;
    }
    session.ui_revision += 1;
}

pub(super) fn restore_timeline_context_menu_focus(
    mut focus: Option<ResMut<InputFocus>>,
    emitter_headers: Query<(Entity, &EmitterTrackHeader)>,
    clip_headers: Query<(Entity, &EffectClipTrackHeader)>,
    mut state: ResMut<TimelineState>,
) {
    let Some(focus) = focus.as_deref_mut() else {
        return;
    };
    if let Some(emitter) = state.restore_context_emitter_focus
        && let Some((entity, _)) = emitter_headers
            .iter()
            .find(|(_, header)| header.emitter == emitter)
    {
        focus.set(entity, FocusCause::Navigated);
        state.restore_context_emitter_focus = None;
        state.restore_context_effect_clip_focus = None;
        return;
    }
    if let Some(clip) = state.restore_context_effect_clip_focus
        && let Some((entity, _)) = clip_headers.iter().find(|(_, header)| header.clip == clip)
    {
        focus.set(entity, FocusCause::Navigated);
        state.restore_context_emitter_focus = None;
        state.restore_context_effect_clip_focus = None;
    }
}

pub(super) fn should_dismiss_timeline_popover(
    open: bool,
    pointer_over_surface: bool,
    primary_pressed: bool,
) -> bool {
    open && primary_pressed && !pointer_over_surface
}

pub(super) fn timeline_wheel_intent(
    time_delta: Vec2,
    track_delta: f32,
    control: bool,
    shift: bool,
) -> TimelineWheelIntent {
    let dominant = if time_delta.x.abs() > time_delta.y.abs() {
        time_delta.x
    } else {
        time_delta.y
    };
    if control {
        TimelineWheelIntent::ZoomTime(dominant)
    } else if shift || time_delta.x.abs() > time_delta.y.abs() {
        TimelineWheelIntent::PanTime(dominant)
    } else {
        TimelineWheelIntent::ScrollTracks(track_delta)
    }
}

pub(super) fn timeline_cursor_fraction(relative_x: f32) -> f32 {
    (relative_x + 0.5).clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum TimelineWheelIntent {
    ScrollTracks(f32),
    PanTime(f32),
    ZoomTime(f32),
}

#[cfg(test)]
#[path = "tests/navigation.rs"]
mod tests;
