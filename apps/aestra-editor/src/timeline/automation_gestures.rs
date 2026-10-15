//! Production automation pointer controllers and live key/curve/lane presentation.
use super::automation_data::{
    AutomationLaneKeys, add_automation_key_at_pointer_action, automation_lane_keys,
    automation_lane_value,
};
use super::automation_execution::set_automation_lane_value_command;
use super::gesture_math::{nice_timeline_step, screen_distance_to_logical};
use super::interaction_state::*;
use super::navigation_cursor::{EntityCursor, OverrideCursor};
use super::state::TimelineView;
use super::timeline_pointer::{PointerClick, PointerDrag, PointerDragEnd, PointerDragStart};
use super::{ChoreographyAction, TimelineSnapMode, TimelineState};
use crate::feathers::automation_curve::{self, AutomationCurveData};
use crate::{CurvesState, EditorSession, localization::Localizer};
use aestra_core::{EffectAsset, EmitterId, Value};
use bevy::{
    picking::pointer::PointerButton,
    prelude::*,
    ui::RelativeCursorPosition,
    window::{CursorIcon, PrimaryWindow, SystemCursorIcon},
};

pub(super) fn emitter_preview_timing(
    session: &EditorSession,
    state: &TimelineState,
    emitter: EmitterId,
) -> Option<(f32, f32)> {
    let emitter = session
        .effect
        .emitters
        .iter()
        .find(|candidate| candidate.id == emitter)?;
    let region = state
        .selected_emitter_region
        .filter(|(selected_emitter, _)| *selected_emitter == emitter.id)
        .and_then(|(_, region)| emitter.timeline_region(region))
        .or_else(|| emitter.timeline_regions().into_iter().next())?;
    Some(
        state
            .drag
            .filter(|drag| drag.emitter == emitter.id && drag.region == region.id)
            .map_or(
                (region.start_time - region.source_offset, emitter.duration),
                |drag| {
                    (
                        drag.current_start - drag.current_source_offset,
                        drag.source_duration,
                    )
                },
            ),
    )
}

pub(super) fn apply_automation_graph_geometry(
    node: &mut Node,
    start_time: f32,
    duration: f32,
    view: TimelineView,
) {
    node.display = if start_time + duration > view.start && start_time < view.end {
        Display::Flex
    } else {
        Display::None
    };
    node.left = Val::Percent(view.normalized_time(start_time) * 100.0);
    node.width = Val::Percent(duration / view.span().max(0.001) * 100.0);
}

pub(super) fn update_automation_lane_graph_visuals(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    mut graphs: Query<(&TimelineAutomationLaneGraph, &mut Node)>,
) {
    for (graph, mut node) in &mut graphs {
        let Some((start, duration)) = emitter_preview_timing(&session, &state, graph.0.emitter)
        else {
            node.display = Display::None;
            continue;
        };
        apply_automation_graph_geometry(&mut node, start, duration, state.view);
    }
}

pub(super) fn automation_key_drag_preview_data(
    effect: &EffectAsset,
    drag: &TimelineAutomationKeyDrag,
) -> Option<AutomationCurveData> {
    let mut preview = automation_lane_keys(effect, &drag.selection.lane)?.graph_data();
    let normalized_time =
        ((drag.current_time - drag.source_start) / drag.source_duration.max(0.001)).clamp(0.0, 1.0);
    match &mut preview {
        AutomationCurveData::Curve { points, .. } => {
            let point = points.get_mut(drag.selection.key)?;
            point.time = normalized_time;
            if let Some(value) = drag.current_value {
                point.value = value;
            }
        }
        AutomationCurveData::Gradient(points) => {
            points.get_mut(drag.selection.key)?.time = normalized_time;
        }
    }
    Some(preview)
}

pub(super) fn update_automation_key_visuals(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    mut keys: Query<(&TimelineAutomationKeySelection, &mut Node), With<TimelineAutomationKey>>,
) {
    let drag_preview = state
        .automation_key_drag
        .as_ref()
        .and_then(|drag| automation_key_drag_preview_data(&session.effect, drag));
    for (selection, mut node) in &mut keys {
        let Some((emitter_start, emitter_duration)) =
            emitter_preview_timing(&session, &state, selection.lane.emitter)
        else {
            node.display = Display::None;
            continue;
        };
        let time = state
            .automation_key_drag
            .as_ref()
            .filter(|drag| drag.selection == *selection)
            .map(|drag| drag.current_time)
            .or_else(|| {
                automation_lane_keys(&session.effect, &selection.lane)
                    .and_then(|keys| keys.times().nth(selection.key))
                    .map(|normalized| emitter_start + normalized * emitter_duration)
            });
        let Some(time) = time else {
            node.display = Display::None;
            continue;
        };
        let position = state.view.normalized_time(time);
        node.display = if (0.0..=1.0).contains(&position) {
            Display::Flex
        } else {
            Display::None
        };
        node.left = Val::Percent(position.clamp(0.0, 1.0) * 100.0);
        if state
            .automation_key_drag
            .as_ref()
            .is_some_and(|drag| drag.selection.lane == selection.lane)
            && let Some(AutomationCurveData::Curve { .. }) = drag_preview.as_ref()
        {
            node.top = Val::Percent(
                drag_preview
                    .as_ref()
                    .map_or(50.0, |preview| preview.key_top_percent(selection.key)),
            );
        }
    }
}

pub(super) fn update_automation_curve_drag_preview(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    graphs: Query<(&TimelineAutomationLaneGraph, &Children)>,
    mut rasters: Query<&mut automation_curve::AutomationCurveRaster>,
) {
    let Some(drag) = state.automation_key_drag.as_ref() else {
        return;
    };
    let Some(preview) = automation_key_drag_preview_data(&session.effect, drag) else {
        return;
    };
    for (graph, children) in &graphs {
        if graph.0 != drag.selection.lane {
            continue;
        }
        for child in children.iter() {
            let Ok(mut raster) = rasters.get_mut(child) else {
                continue;
            };
            if raster.data() != &preview {
                raster.set_data(preview.clone());
            }
        }
    }
}

pub(super) fn snap_automation_key_time(
    candidate: f32,
    session: &EditorSession,
    mode: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
) -> (f32, Option<f32>) {
    match mode {
        TimelineSnapMode::None => (candidate, None),
        TimelineSnapMode::Frames => {
            let frame = 1.0 / session.tick_rate().max(1) as f32;
            let snapped = (candidate / frame).round() * frame;
            (snapped, Some(snapped))
        }
        TimelineSnapMode::Seconds => {
            let interval = nice_timeline_step(view.span(), canvas_width) / 5.0;
            let snapped = (candidate / interval).round() * interval;
            (snapped, Some(snapped))
        }
        TimelineSnapMode::Smart => {
            let threshold = view.span() / canvas_width.max(1.0) * 9.0;
            let frame = 1.0 / session.tick_rate().max(1) as f32;
            let mut targets = vec![
                0.0,
                session.playback_duration(),
                session.time(),
                (candidate / frame).round() * frame,
            ];
            targets.extend(session.effect.markers.iter().map(|marker| marker.time));
            targets.extend(
                session
                    .effect
                    .choreography_events
                    .iter()
                    .map(|event| event.time),
            );
            for emitter in &session.effect.emitters {
                for region in emitter.timeline_regions() {
                    targets.push(region.start_time);
                    targets.push(region.end_time());
                }
            }
            for clip in &session.effect.effect_clips {
                targets.push(clip.start_time);
                targets.push(clip.start_time + clip.duration);
            }
            let nearest = targets.into_iter().min_by(|left, right| {
                (candidate - *left)
                    .abs()
                    .total_cmp(&(candidate - *right).abs())
            });
            nearest
                .filter(|target| (candidate - *target).abs() <= threshold)
                .map_or((candidate, None), |target| (target, Some(target)))
        }
    }
}

pub(super) fn add_automation_key_from_graph(
    mut click: On<PointerClick>,
    graphs: Query<(&TimelineAutomationLaneGraph, &RelativeCursorPosition)>,
    session: Res<EditorSession>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary || click.count < 2 {
        return;
    }
    let Ok((graph, cursor)) = graphs.get(click.event_target()) else {
        return;
    };
    let Some(position) = cursor.normalized else {
        return;
    };
    let Some(action) = add_automation_key_at_pointer_action(&session.effect, &graph.0, position)
    else {
        return;
    };
    commands.trigger(action);
    click.propagate(false);
}

pub(super) fn select_automation_key(
    mut click: On<PointerClick>,
    controls: Query<&TimelineAutomationKeySelection, With<TimelineAutomationKey>>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(selection) = controls.get(click.event_target()) else {
        return;
    };
    commands.trigger(ChoreographyAction::SelectAutomationKey(selection.clone()));
    click.propagate(false);
}

pub(super) fn begin_automation_lane_resize(
    mut drag: On<PointerDragStart>,
    handles: Query<&AutomationLaneId, With<TimelineAutomationLaneResizeHandle>>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(lane) = handles.get(drag.event_target()) else {
        return;
    };
    state.automation_lane_resize = Some((lane.clone(), state.automation_lane_height(lane)));
    override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::NsResize));
    **cursor = CursorIcon::System(SystemCursorIcon::NsResize);
    drag.propagate(false);
}

pub(super) fn move_automation_lane_resize(
    mut drag: On<PointerDrag>,
    handles: Query<&AutomationLaneId, With<TimelineAutomationLaneResizeHandle>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut state: ResMut<TimelineState>,
    mut lanes: Query<(&AutomationLaneId, &mut Node), With<TimelineAutomationLane>>,
) {
    let Ok(lane) = handles.get(drag.event_target()) else {
        return;
    };
    let Some((active, start)) = state.automation_lane_resize.clone() else {
        return;
    };
    if active != *lane {
        return;
    }
    let distance = screen_distance_to_logical(drag.distance.y, window.scale_factor());
    let height =
        (start + distance).clamp(automation_curve::MIN_HEIGHT, automation_curve::MAX_HEIGHT);
    state.automation_lane_heights.insert(active.clone(), height);
    for (candidate, mut node) in &mut lanes {
        if *candidate == active {
            node.height = Val::Px(height);
        }
    }
    drag.propagate(false);
}

pub(super) fn finish_automation_lane_resize(
    mut drag: On<PointerDragEnd>,
    handles: Query<&AutomationLaneId, With<TimelineAutomationLaneResizeHandle>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(lane) = handles.get(drag.event_target()) else {
        return;
    };
    if state
        .automation_lane_resize
        .take()
        .is_some_and(|(active, _)| active == *lane)
    {
        session.ui_revision += 1;
    }
    override_cursor.0 = None;
    **cursor = CursorIcon::System(SystemCursorIcon::NsResize);
    drag.propagate(false);
}

pub(super) fn begin_automation_key_drag(
    mut drag: On<PointerDragStart>,
    controls: Query<&TimelineAutomationKeySelection, With<TimelineAutomationKey>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(selection) = controls.get(drag.event_target()) else {
        return;
    };
    let Some(emitter) = session
        .effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == selection.lane.emitter)
    else {
        return;
    };
    let Some(keys) = automation_lane_keys(&session.effect, &selection.lane) else {
        return;
    };
    let Some(normalized) = keys.times().nth(selection.key) else {
        return;
    };
    let region = state
        .selected_emitter_region
        .filter(|(selected_emitter, _)| *selected_emitter == emitter.id)
        .and_then(|(_, region)| emitter.timeline_region(region))
        .or_else(|| emitter.timeline_regions().into_iter().next())
        .expect("an emitter always has a timeline region");
    let source_start = region.start_time - region.source_offset;
    let source_duration = emitter.duration;
    let time = source_start + normalized * source_duration;
    let value = keys.curve_value(selection.key);
    let cursor_icon = if matches!(keys, AutomationLaneKeys::Gradient(_)) {
        SystemCursorIcon::EwResize
    } else {
        SystemCursorIcon::Grabbing
    };
    state.automation_key_drag = Some(TimelineAutomationKeyDrag {
        selection: selection.clone(),
        source_start,
        source_duration,
        original_time: time,
        current_time: time,
        original_value: value,
        current_value: value,
    });
    override_cursor.0 = Some(EntityCursor::System(cursor_icon));
    **cursor = CursorIcon::System(cursor_icon);
    drag.propagate(false);
}

pub(super) fn move_automation_key_drag(
    mut drag_event: On<PointerDrag>,
    controls: Query<&TimelineAutomationKeySelection, With<TimelineAutomationKey>>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    window: Single<&Window, With<PrimaryWindow>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    let Ok(selection) = controls.get(drag_event.event_target()) else {
        return;
    };
    let Some(mut drag) = state.automation_key_drag.clone() else {
        return;
    };
    if drag.selection != *selection {
        return;
    }
    drag_event.propagate(false);
    let width = canvases
        .iter()
        .map(|canvas| canvas.size().x)
        .fold(0.0, f32::max)
        .max(1.0);
    let logical_distance_x =
        screen_distance_to_logical(drag_event.distance.x, window.scale_factor());
    let candidate = drag.original_time + logical_distance_x / width * state.view.span();
    let (candidate, guide) =
        snap_automation_key_time(candidate, &session, state.snap, state.view, width);
    let Some(keys) = automation_lane_keys(&session.effect, &selection.lane) else {
        return;
    };
    let epsilon = drag.source_duration.max(0.001) * 0.0005;
    let lower = selection
        .key
        .checked_sub(1)
        .and_then(|index| keys.times().nth(index))
        .map_or(drag.source_start, |time| {
            drag.source_start + time * drag.source_duration + epsilon
        });
    let upper = keys
        .times()
        .nth(selection.key + 1)
        .map_or(drag.source_start + drag.source_duration, |time| {
            drag.source_start + time * drag.source_duration - epsilon
        });
    drag.current_time = candidate.clamp(lower.min(upper), upper.max(lower));
    if let Some(original_value) = drag.original_value {
        let graph = keys.graph_data();
        let original_top = graph.top_percent_for_value(original_value).unwrap_or(50.0);
        let logical_distance_y =
            screen_distance_to_logical(drag_event.distance.y, window.scale_factor());
        let candidate_top = original_top
            + logical_distance_y / state.automation_lane_height(&selection.lane).max(1.0) * 100.0;
        drag.current_value = graph.value_for_top_percent(candidate_top);
    }
    state.automation_key_drag = Some(drag);
    state.snap_guide = guide.filter(|time| (*time - candidate).abs() <= 0.0001);
}

pub(super) fn finish_automation_key_drag(
    mut drag_event: On<PointerDragEnd>,
    controls: Query<&TimelineAutomationKeySelection, With<TimelineAutomationKey>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut curves: ResMut<CurvesState>,
    localizer: Res<Localizer>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(selection) = controls.get(drag_event.event_target()) else {
        return;
    };
    let Some(drag) = state.automation_key_drag.take() else {
        return;
    };
    if drag.selection != *selection {
        state.automation_key_drag = Some(drag);
        return;
    }
    drag_event.propagate(false);
    state.snap_guide = None;
    override_cursor.0 = None;
    **cursor = CursorIcon::System(if drag.original_value.is_some() {
        SystemCursorIcon::Grab
    } else {
        SystemCursorIcon::EwResize
    });
    let time_changed = (drag.current_time - drag.original_time).abs() > 0.0001;
    let value_changed = drag
        .original_value
        .zip(drag.current_value)
        .is_some_and(|(original, current)| (current - original).abs() > 0.0001);
    if !time_changed && !value_changed {
        return;
    }
    let normalized =
        ((drag.current_time - drag.source_start) / drag.source_duration.max(0.001)).clamp(0.0, 1.0);
    let command = match automation_lane_value(&session.effect, &selection.lane) {
        Some(Value::Curve(mut curve)) => {
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            key.time = normalized;
            if let Some(value) = drag.current_value {
                key.value = value;
            }
            curve.keys[selection.key] = key;
            let Some(command) = set_automation_lane_value_command(
                &session.effect,
                &selection.lane,
                Value::Curve(curve),
            ) else {
                return;
            };
            command
        }
        Some(Value::Vec3Curve(mut curves_value)) => {
            let Some(curve) = selection
                .lane
                .channel
                .and_then(|channel| curves_value.curves.get_mut(channel as usize))
            else {
                return;
            };
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            key.time = normalized;
            if let Some(value) = drag.current_value {
                key.value = value;
            }
            curve.keys[selection.key] = key;
            let Some(command) = set_automation_lane_value_command(
                &session.effect,
                &selection.lane,
                Value::Vec3Curve(curves_value),
            ) else {
                return;
            };
            command
        }
        Some(Value::Gradient(mut gradient)) => {
            let Some(mut key) = gradient.keys.get(selection.key).copied() else {
                return;
            };
            key.time = normalized;
            gradient.keys[selection.key] = key;
            let Some(command) = set_automation_lane_value_command(
                &session.effect,
                &selection.lane,
                Value::Gradient(gradient),
            ) else {
                return;
            };
            command
        }
        _ => return,
    };
    if session.execute(
        localizer.text("timeline-move-automation-key-command"),
        command,
        true,
    ) {
        curves.select_key_channel(
            selection.lane.module,
            selection.lane.input,
            selection.key,
            selection.lane.channel,
        );
        state.selected_automation_key = Some(selection.clone());
    }
}

impl TimelineState {
    pub(super) fn automation_lane_height(&self, lane: &AutomationLaneId) -> f32 {
        self.automation_lane_heights
            .get(lane)
            .copied()
            .unwrap_or(automation_curve::DEFAULT_HEIGHT)
    }
}

#[cfg(test)]
#[path = "automation_gestures/tests.rs"]
mod tests;
