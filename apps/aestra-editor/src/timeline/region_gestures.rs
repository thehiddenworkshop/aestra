//! Production emitter-region preview, snapping and pointer drag transactions.
use super::gesture_math::*;
use super::interaction_state::*;
use super::navigation_cursor::{EntityCursor, OverrideCursor};
use super::state::TimelineView;
use super::timeline_pointer::{PointerDrag, PointerDragEnd, PointerDragStart};
use super::{ChoreographyAction, EditorSession, TimelineSnapMode, TimelineState};
use aestra_authoring::EffectCommand;
use aestra_core::{EmitterId, EmitterRegion, EmitterRegionId};
use bevy::{
    prelude::*,
    window::{CursorIcon, PrimaryWindow},
};
use std::collections::BTreeSet;

pub(super) fn timeline_region_preview_timing(
    state: &TimelineState,
    emitter: EmitterId,
    region: EmitterRegion,
) -> (f32, f32) {
    let Some(drag) = state.drag.filter(|drag| drag.emitter == emitter) else {
        return (region.start_time, region.duration);
    };
    if drag.region == region.id {
        return (drag.current_start, drag.current_duration);
    }
    if drag.kind == TimelineDragKind::Move
        && let Some(member) = state
            .drag_regions
            .iter()
            .find(|member| member.region == region.id)
    {
        return (
            member.original_start + drag.current_start - drag.original_start,
            member.duration,
        );
    }
    (region.start_time, region.duration)
}

pub(super) fn snap_timeline_boundary(
    candidate: f32,
    emitter: EmitterId,
    region: EmitterRegionId,
    ignored_regions: &[TimelineRegionMove],
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
            for other in &session.effect.emitters {
                for other_region in other.timeline_regions() {
                    if other.id != emitter
                        || (other_region.id != region
                            && !ignored_regions
                                .iter()
                                .any(|ignored| ignored.region == other_region.id))
                    {
                        targets.push(other_region.start_time);
                        targets.push(other_region.end_time());
                    }
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

pub(super) fn snap_moved_timing(
    start: f32,
    duration: f32,
    emitter: EmitterId,
    region: EmitterRegionId,
    ignored_regions: &[TimelineRegionMove],
    session: &EditorSession,
    mode: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
) -> (f32, Option<f32>) {
    let start_snap = snap_timeline_boundary(
        start,
        emitter,
        region,
        ignored_regions,
        session,
        mode,
        view,
        canvas_width,
    );
    if mode != TimelineSnapMode::Smart {
        return start_snap;
    }
    let end = start + duration;
    let end_snap = snap_timeline_boundary(
        end,
        emitter,
        region,
        ignored_regions,
        session,
        mode,
        view,
        canvas_width,
    );
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

pub(super) fn update_timeline_drag(
    drag: &mut TimelineDrag,
    drag_regions: &[TimelineRegionMove],
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
            let group_start = drag_regions
                .iter()
                .map(|region| region.original_start)
                .fold(drag.original_start, f32::min);
            let group_end = drag_regions
                .iter()
                .map(|region| region.original_start + region.duration)
                .fold(drag.original_start + drag.original_duration, f32::max);
            let minimum_start = drag.original_start - group_start;
            let maximum_start = drag.original_start + effect_duration - group_end;
            let unsnapped = (drag.original_start + pointer_delta)
                .clamp(minimum_start, maximum_start.max(minimum_start));
            let (start, guide) = snap_moved_timing(
                unsnapped,
                drag.original_duration,
                drag.emitter,
                drag.region,
                drag_regions,
                session,
                snap,
                view,
                canvas_width,
            );
            drag.current_start = start.clamp(minimum_start, maximum_start.max(minimum_start));
            drag.current_duration = drag.original_duration;
            drag.current_source_offset = drag.original_source_offset;
            *snap_guide = guide;
        }
        TimelineDragKind::TrimStart => {
            let end = drag.original_start + drag.original_duration;
            let minimum_start = 0.0;
            let unsnapped = (drag.original_start + pointer_delta)
                .clamp(minimum_start, (end - minimum_duration).max(minimum_start));
            let (start, guide) = snap_timeline_boundary(
                unsnapped,
                drag.emitter,
                drag.region,
                &[],
                session,
                snap,
                view,
                canvas_width,
            );
            drag.current_start = start.clamp(minimum_start, end - minimum_duration);
            drag.current_source_offset =
                (drag.original_source_offset + drag.current_start - drag.original_start).max(0.0);
            drag.current_duration = end - drag.current_start;
            *snap_guide = guide;
        }
        TimelineDragKind::TrimEnd => {
            let maximum_end = effect_duration;
            let unsnapped = (drag.original_start + drag.original_duration + pointer_delta)
                .clamp(drag.original_start + minimum_duration, maximum_end);
            let (end, guide) = snap_timeline_boundary(
                unsnapped,
                drag.emitter,
                drag.region,
                &[],
                session,
                snap,
                view,
                canvas_width,
            );
            let end = end.clamp(drag.original_start + minimum_duration, maximum_end);
            drag.current_start = drag.original_start;
            drag.current_source_offset = drag.original_source_offset;
            drag.current_duration = end - drag.original_start;
            *snap_guide = guide;
        }
    }
}

pub(super) fn commit_timeline_drag(
    session: &mut EditorSession,
    drag: TimelineDrag,
    drag_regions: &[TimelineRegionMove],
) {
    let changed = (drag.current_start - drag.original_start).abs() > 0.000_1
        || (drag.current_source_offset - drag.original_source_offset).abs() > 0.000_1
        || (drag.current_duration - drag.original_duration).abs() > 0.000_1;
    if changed {
        let label = match drag.kind {
            TimelineDragKind::Move => "Moved emitter on timeline",
            TimelineDragKind::TrimStart | TimelineDragKind::TrimEnd => {
                "Trimmed emitter on timeline"
            }
        };
        let Some(emitter) = session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == drag.emitter)
            .cloned()
        else {
            return;
        };
        if drag.kind == TimelineDragKind::Move && drag_regions.len() > 1 {
            let delta = drag.current_start - drag.original_start;
            let moved = drag_regions
                .iter()
                .map(|member| member.region)
                .collect::<BTreeSet<_>>();
            let mut regions = emitter.timeline_regions();
            for region in &mut regions {
                if moved.contains(&region.id) {
                    region.start_time += delta;
                }
            }
            session.execute(
                label,
                EffectCommand::SetEmitterRegions {
                    id: drag.emitter,
                    regions: emitter.normalize_timeline_regions(regions),
                },
                false,
            );
            return;
        }
        let Some(transaction) = session.emitter_region_timing_transaction(
            drag.emitter,
            drag.region,
            drag.current_start,
            drag.current_source_offset,
            drag.current_duration,
            label,
        ) else {
            return;
        };
        session.execute_transaction(transaction, false);
    }
}

pub(super) fn begin_timeline_clip_drag(
    drag: On<PointerDragStart>,
    targets: Query<&TimelineClipInteraction>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(target) = targets.get(drag.event_target()) else {
        return;
    };
    let Some(emitter) = session
        .effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == target.emitter)
    else {
        return;
    };
    let Some(region) = emitter.timeline_region(target.region) else {
        return;
    };
    state.drag_regions = if target.kind == TimelineDragKind::Move
        && state
            .selected_emitter_regions
            .contains(&(target.emitter, target.region))
    {
        emitter
            .timeline_regions()
            .into_iter()
            .filter(|region| {
                state
                    .selected_emitter_regions
                    .contains(&(target.emitter, region.id))
            })
            .map(|region| TimelineRegionMove {
                region: region.id,
                original_start: region.start_time,
                duration: region.duration,
            })
            .collect()
    } else if target.kind == TimelineDragKind::Move {
        vec![TimelineRegionMove {
            region: region.id,
            original_start: region.start_time,
            duration: region.duration,
        }]
    } else {
        Vec::new()
    };
    state.drag = Some(TimelineDrag {
        emitter: target.emitter,
        region: target.region,
        kind: target.kind,
        pointer_start: 0.0,
        original_start: region.start_time,
        original_duration: region.duration,
        original_source_offset: region.source_offset,
        current_start: region.start_time,
        current_duration: region.duration,
        current_source_offset: region.source_offset,
        source_duration: emitter.duration,
    });
    override_cursor.0 = Some(EntityCursor::System(timeline_system_cursor(
        target.kind,
        true,
    )));
    **cursor = timeline_drag_cursor(target.kind, true);
}

pub(super) fn move_timeline_clip_drag(
    mut drag_event: On<PointerDrag>,
    targets: Query<&TimelineClipInteraction>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    window: Single<&Window, With<PrimaryWindow>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    let Ok(target) = targets.get(drag_event.event_target()) else {
        return;
    };
    drag_event.propagate(false);
    let Some(mut drag) = state.drag else {
        return;
    };
    if drag.emitter != target.emitter || drag.region != target.region || drag.kind != target.kind {
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
    let drag_regions = state.drag_regions.clone();
    update_timeline_drag(
        &mut drag,
        &drag_regions,
        pointer_time,
        &session,
        state.snap,
        state.view,
        width,
        &mut snap_guide,
    );
    state.drag = Some(drag);
    state.snap_guide = snap_guide;
}

pub(super) fn finish_timeline_clip_drag(
    drag_event: On<PointerDragEnd>,
    targets: Query<&TimelineClipInteraction>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut commands: Commands,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(target) = targets.get(drag_event.event_target()) else {
        return;
    };
    let Some(drag) = state.drag.take() else {
        return;
    };
    if drag.emitter != target.emitter || drag.region != target.region || drag.kind != target.kind {
        return;
    }
    state.snap_guide = None;
    let drag_regions = std::mem::take(&mut state.drag_regions);
    override_cursor.0 = None;
    **cursor = timeline_drag_cursor(target.kind, false);
    let preserve_multi_selection = drag.kind == TimelineDragKind::Move && drag_regions.len() > 1;
    commit_timeline_drag(&mut session, drag, &drag_regions);
    if preserve_multi_selection {
        session.select_emitter_region(target.emitter, target.region);
    } else {
        commands.trigger(ChoreographyAction::SelectEmitterRegion {
            emitter: target.emitter,
            region: target.region,
        });
    }
}
