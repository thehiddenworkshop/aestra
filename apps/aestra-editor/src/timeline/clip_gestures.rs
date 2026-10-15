//! Production effect-clip drag observers, source-aware trimming, snapping and commit policy.
use super::gesture_math::{
    nice_timeline_step, screen_distance_to_logical, timeline_drag_cursor, timeline_system_cursor,
};
use super::interaction_state::*;
use super::navigation_cursor::{EntityCursor, OverrideCursor};
use super::state::TimelineView;
use super::timeline_pointer::{PointerDrag, PointerDragEnd, PointerDragStart};
use super::{ChoreographyAction, TimelineSnapMode, TimelineState};
use crate::{EditorSession, ProjectEffectCatalog, localization::Localizer};
use aestra_authoring::EffectCommand;
use aestra_core::EffectClipId;
use bevy::{
    prelude::*,
    window::{CursorIcon, PrimaryWindow},
};

pub(super) fn snap_effect_clip_boundary(
    candidate: f32,
    clip: EffectClipId,
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
            for emitter in &session.effect.emitters {
                for region in emitter.timeline_regions() {
                    targets.push(region.start_time);
                    targets.push(region.end_time());
                }
            }
            for other in &session.effect.effect_clips {
                if other.id != clip {
                    targets.push(other.start_time);
                    targets.push(other.start_time + other.duration);
                }
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

pub(super) fn snap_effect_clip_moved_timing(
    start: f32,
    duration: f32,
    clip: EffectClipId,
    session: &EditorSession,
    mode: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
) -> (f32, Option<f32>) {
    let start_snap = snap_effect_clip_boundary(start, clip, session, mode, view, canvas_width);
    if mode != TimelineSnapMode::Smart {
        return start_snap;
    }
    let end = start + duration;
    let end_snap = snap_effect_clip_boundary(end, clip, session, mode, view, canvas_width);
    let start_delta = (start_snap.0 - start).abs();
    let end_delta = (end_snap.0 - end).abs();
    match (start_snap.1, end_snap.1) {
        (None, Some(guide)) => (start + end_snap.0 - end, Some(guide)),
        (Some(_), Some(guide)) if end_delta < start_delta => {
            (start + end_snap.0 - end, Some(guide))
        }
        _ => start_snap,
    }
}

pub(super) fn begin_effect_clip_timeline_drag(
    drag: On<PointerDragStart>,
    targets: Query<&TimelineEffectClipInteraction>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(target) = targets.get(drag.event_target()) else {
        return;
    };
    let Some(clip) = session
        .effect
        .effect_clips
        .iter()
        .find(|clip| clip.id == target.clip)
    else {
        return;
    };
    let source = catalog.cached_effect(clip.source).ok();
    state.drag = None;
    state.effect_clip_drag = Some(EffectClipTimelineDrag {
        clip: clip.id,
        kind: target.kind,
        pointer_start: 0.0,
        original_start: clip.start_time,
        original_source_offset: clip.source_offset,
        original_duration: clip.duration,
        current_start: clip.start_time,
        current_source_offset: clip.source_offset,
        current_duration: clip.duration,
        source_duration: source
            .as_ref()
            .map_or(clip.source_offset + clip.duration, |effect| effect.duration),
        source_looping: source
            .as_ref()
            .is_some_and(|effect| effect.playback_mode.is_looping()),
    });
    override_cursor.0 = Some(EntityCursor::System(timeline_system_cursor(
        target.kind,
        true,
    )));
    **cursor = timeline_drag_cursor(target.kind, true);
}

pub(super) fn move_effect_clip_timeline_drag(
    mut drag_event: On<PointerDrag>,
    targets: Query<&TimelineEffectClipInteraction>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    window: Single<&Window, With<PrimaryWindow>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    let Ok(target) = targets.get(drag_event.event_target()) else {
        return;
    };
    drag_event.propagate(false);
    let Some(mut drag) = state.effect_clip_drag else {
        return;
    };
    if drag.clip != target.clip || drag.kind != target.kind {
        return;
    }
    let width = canvases
        .iter()
        .map(|canvas| canvas.size().x)
        .fold(0.0, f32::max)
        .max(1.0);
    let logical_distance = screen_distance_to_logical(drag_event.distance.x, window.scale_factor());
    let pointer_time = logical_distance / width * state.view.span();
    let mut snap_guide = state.snap_guide;
    update_effect_clip_timeline_drag(
        &mut drag,
        pointer_time,
        &session,
        state.snap,
        state.view,
        width,
        &mut snap_guide,
    );
    state.effect_clip_drag = Some(drag);
    state.snap_guide = snap_guide;
}

pub(super) fn finish_effect_clip_timeline_drag(
    drag_event: On<PointerDragEnd>,
    targets: Query<&TimelineEffectClipInteraction>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    localizer: Res<Localizer>,
    mut commands: Commands,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(target) = targets.get(drag_event.event_target()) else {
        return;
    };
    let Some(drag) = state.effect_clip_drag.take() else {
        return;
    };
    if drag.clip != target.clip || drag.kind != target.kind {
        state.effect_clip_drag = Some(drag);
        return;
    }
    state.snap_guide = None;
    override_cursor.0 = None;
    **cursor = timeline_drag_cursor(target.kind, false);
    commit_effect_clip_timeline_drag(&mut session, drag, &localizer);
    commands.trigger(ChoreographyAction::SelectEffectClip(target.clip));
}

pub(super) fn update_effect_clip_timeline_drag(
    drag: &mut EffectClipTimelineDrag,
    pointer_time: f32,
    session: &EditorSession,
    snap: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
    snap_guide: &mut Option<f32>,
) {
    let effect_duration = session.playback_duration();
    let minimum_duration = (1.0 / session.tick_rate().max(1) as f32).max(0.001);
    let pointer_delta = pointer_time - drag.pointer_start;
    *snap_guide = None;
    match drag.kind {
        TimelineDragKind::Move => {
            let maximum_start = (effect_duration - drag.original_duration).max(0.0);
            let unsnapped = (drag.original_start + pointer_delta).clamp(0.0, maximum_start);
            let (start, guide) = snap_effect_clip_moved_timing(
                unsnapped,
                drag.original_duration,
                drag.clip,
                session,
                snap,
                view,
                canvas_width,
            );
            drag.current_start = start.clamp(0.0, maximum_start);
            drag.current_source_offset = drag.original_source_offset;
            drag.current_duration = drag.original_duration;
            *snap_guide = guide;
        }
        TimelineDragKind::TrimStart => {
            let end = drag.original_start + drag.original_duration;
            let minimum_start = if drag.source_looping {
                0.0
            } else {
                (drag.original_start - drag.original_source_offset).max(0.0)
            };
            let maximum_start = (end - minimum_duration).max(minimum_start);
            let unsnapped =
                (drag.original_start + pointer_delta).clamp(minimum_start, maximum_start);
            let (start, guide) =
                snap_effect_clip_boundary(unsnapped, drag.clip, session, snap, view, canvas_width);
            drag.current_start = start.clamp(minimum_start, maximum_start);
            let delta = drag.current_start - drag.original_start;
            let source_offset = drag.original_source_offset + delta;
            drag.current_source_offset = if drag.source_looping && drag.source_duration > 0.0 {
                source_offset.rem_euclid(drag.source_duration)
            } else {
                source_offset.max(0.0)
            };
            drag.current_duration = (drag.original_duration - delta).max(minimum_duration);
            *snap_guide = guide;
        }
        TimelineDragKind::TrimEnd => {
            let source_maximum = if drag.source_looping {
                effect_duration
            } else {
                drag.original_start
                    + (drag.source_duration - drag.original_source_offset).max(minimum_duration)
            };
            let maximum_end = effect_duration.min(source_maximum);
            let minimum_end = drag.original_start + minimum_duration;
            let unsnapped = (drag.original_start + drag.original_duration + pointer_delta)
                .clamp(minimum_end, maximum_end.max(minimum_end));
            let (end, guide) =
                snap_effect_clip_boundary(unsnapped, drag.clip, session, snap, view, canvas_width);
            let end = end.clamp(minimum_end, maximum_end.max(minimum_end));
            drag.current_start = drag.original_start;
            drag.current_source_offset = drag.original_source_offset;
            drag.current_duration = (end - drag.original_start).max(minimum_duration);
            *snap_guide = guide;
        }
    }
}

pub(super) fn commit_effect_clip_timeline_drag(
    session: &mut EditorSession,
    drag: EffectClipTimelineDrag,
    localizer: &Localizer,
) {
    let changed = (drag.current_start - drag.original_start).abs() > 0.000_1
        || (drag.current_source_offset - drag.original_source_offset).abs() > 0.000_1
        || (drag.current_duration - drag.original_duration).abs() > 0.000_1;
    if !changed {
        return;
    }
    let label = localizer.text(match drag.kind {
        TimelineDragKind::Move => "timeline-move-effect-clip-command",
        TimelineDragKind::TrimStart | TimelineDragKind::TrimEnd => {
            "timeline-trim-effect-clip-command"
        }
    });
    session.execute(
        label,
        EffectCommand::SetEffectClipTiming {
            id: drag.clip,
            start_time: drag.current_start,
            source_offset: drag.current_source_offset,
            duration: drag.current_duration,
        },
        false,
    );
}

#[cfg(test)]
#[path = "clip_gestures/tests.rs"]
mod tests;
