//! Production timeline geometry, scroll synchronization and drop/reorder presentation.
use super::TimelineState;
use super::drop_execution::{choreography_insertion_index, effect_clip_placement};
use super::gesture_math::nice_timeline_step;
use super::interaction_state::*;
use super::motion_execution::timeline_duration;
use super::navigation::timeline_cursor_fraction;
use super::projections::{
    automation_lanes_height, referenced_track_projections, visible_automation_lane_count,
};
use super::region_gestures::timeline_region_preview_timing;
use super::state::{TimelineView, normalized_choreography_order};
use crate::{EditorSession, ProjectEffectCatalog, theme};
use aestra_core::{ChoreographyTrackId, EffectAsset};
use bevy::{
    input_focus::{FocusCause, InputFocus, InputFocusVisible},
    prelude::*,
    ui::RelativeCursorPosition,
    ui_widgets::{ActiveDescendant, ListBox, ScrollIntoView},
};

pub(super) fn update_timeline_visuals(
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    mut clips: Query<
        (&TimelineClip, &mut Node),
        (Without<Playhead>, Without<TimelineClipInteraction>),
    >,
    mut clip_controls: Query<
        (&TimelineClipInteraction, &mut Node),
        (Without<TimelineClip>, Without<Playhead>),
    >,
    mut playheads: Query<
        &mut Node,
        (
            With<Playhead>,
            Without<TimelineClip>,
            Without<TimelineClipInteraction>,
        ),
    >,
    mut guides: Query<
        &mut Node,
        (
            With<TimelineSnapGuide>,
            Without<TimelineClip>,
            Without<Playhead>,
            Without<TimelineRulerTick>,
            Without<TimelineClipInteraction>,
        ),
    >,
    mut ticks: Query<
        (&TimelineRulerTick, &Children, &mut Node),
        (
            Without<TimelineClip>,
            Without<Playhead>,
            Without<TimelineSnapGuide>,
            Without<TimelineClipInteraction>,
        ),
    >,
    mut texts: Query<&mut Text>,
) {
    state.ensure_duration(timeline_duration(&session));
    let view = state.view;
    let width = canvases
        .iter()
        .map(|canvas| canvas.size().x)
        .fold(0.0, f32::max)
        .max(320.0);

    for (clip, mut node) in &mut clips {
        let Some(emitter) = session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == clip.emitter)
        else {
            node.display = Display::None;
            continue;
        };
        let Some(region) = emitter.timeline_region(clip.region) else {
            node.display = Display::None;
            continue;
        };
        let (start, duration) = timeline_region_preview_timing(&state, clip.emitter, region);
        apply_timeline_bar_geometry(&mut node, start, duration, view);
    }

    for (control, mut node) in &mut clip_controls {
        let Some(emitter) = session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == control.emitter)
        else {
            node.display = Display::None;
            continue;
        };
        let Some(region) = emitter.timeline_region(control.region) else {
            node.display = Display::None;
            continue;
        };
        let (start, duration) = timeline_region_preview_timing(&state, control.emitter, region);
        let boundary_visible = match control.kind {
            TimelineDragKind::Move => true,
            TimelineDragKind::TrimStart => timeline_boundary_is_visible(start, view),
            TimelineDragKind::TrimEnd => timeline_boundary_is_visible(start + duration, view),
        };
        node.display = if boundary_visible {
            Display::Flex
        } else {
            Display::None
        };
    }

    let playhead_position = view.normalized_time(session.time());
    for mut node in &mut playheads {
        node.display = if (0.0..=1.0).contains(&playhead_position) {
            Display::Flex
        } else {
            Display::None
        };
        node.left = Val::Percent(playhead_position.clamp(0.0, 1.0) * 100.0);
    }
    for mut node in &mut guides {
        if let Some(time) = state.snap_guide
            && (view.start..=view.end).contains(&time)
        {
            node.display = Display::Flex;
            node.left = Val::Percent(view.normalized_time(time) * 100.0);
        } else {
            node.display = Display::None;
        }
    }

    let step = nice_timeline_step(view.span(), width);
    let first = (view.start / step).ceil() * step;
    for (tick, children, mut node) in &mut ticks {
        let time = first + tick.0 as f32 * step;
        if time > view.end + step * 0.001 {
            node.display = Display::None;
            continue;
        }
        node.display = Display::Flex;
        node.left = Val::Percent(view.normalized_time(time) * 100.0);
        if let Some(child) = children.first()
            && let Ok(mut text) = texts.get_mut(*child)
        {
            text.0 = format_timeline_tick(time, step);
        }
    }
}

pub(super) fn update_timeline_marker_visuals(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    mut markers: Query<(&TimelineMarker, &mut Node)>,
) {
    for (control, mut node) in &mut markers {
        let Some(marker) = session
            .effect
            .markers
            .iter()
            .find(|marker| marker.id == control.marker)
        else {
            node.display = Display::None;
            continue;
        };
        let time = state
            .marker_drag
            .filter(|drag| drag.marker == marker.id)
            .map_or(marker.time, |drag| drag.current_time);
        let position = state.view.normalized_time(time);
        node.display = if (0.0..=1.0).contains(&position) {
            Display::Flex
        } else {
            Display::None
        };
        node.left = Val::Percent(position.clamp(0.0, 1.0) * 100.0);
    }
}

pub(super) fn update_choreography_event_visuals(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    mut events: Query<(&TimelineChoreographyEvent, &mut Node)>,
) {
    for (control, mut node) in &mut events {
        let Some(event) = session
            .effect
            .choreography_events
            .iter()
            .find(|event| event.id == control.event)
        else {
            node.display = Display::None;
            continue;
        };
        let time = state
            .choreography_event_drag
            .filter(|drag| drag.event == event.id)
            .map_or(event.time, |drag| drag.current_time);
        let position = state.view.normalized_time(time);
        node.display = if (0.0..=1.0).contains(&position) {
            Display::Flex
        } else {
            Display::None
        };
        node.left = Val::Percent(position.clamp(0.0, 1.0) * 100.0);
    }
}

pub(super) fn update_effect_clip_visuals(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    mut clips: Query<
        (&TimelineEffectClip, &mut Node),
        (
            Without<TimelineReferencedEmitter>,
            Without<TimelineEffectClipInteraction>,
        ),
    >,
    mut controls: Query<
        (&TimelineEffectClipInteraction, &mut Node),
        (
            Without<TimelineEffectClip>,
            Without<TimelineReferencedEmitter>,
        ),
    >,
    mut children: Query<(&TimelineReferencedEmitter, &mut Node), Without<TimelineEffectClip>>,
) {
    let view = state.view;
    for (marker, mut node) in &mut clips {
        let Some(clip) = session
            .effect
            .effect_clips
            .iter()
            .find(|clip| clip.id == marker.clip)
        else {
            node.display = Display::None;
            continue;
        };
        let (start_time, duration) = state
            .effect_clip_drag
            .filter(|drag| drag.clip == marker.clip)
            .map_or((clip.start_time, clip.duration), |drag| {
                (drag.current_start, drag.current_duration)
            });
        apply_timeline_bar_geometry(&mut node, start_time, duration, view);
    }
    for (control, mut node) in &mut controls {
        let Some(clip) = session
            .effect
            .effect_clips
            .iter()
            .find(|clip| clip.id == control.clip)
        else {
            node.display = Display::None;
            continue;
        };
        let (start_time, duration) = state
            .effect_clip_drag
            .filter(|drag| drag.clip == control.clip)
            .map_or((clip.start_time, clip.duration), |drag| {
                (drag.current_start, drag.current_duration)
            });
        let boundary_visible = match control.kind {
            TimelineDragKind::Move => true,
            TimelineDragKind::TrimStart => timeline_boundary_is_visible(start_time, view),
            TimelineDragKind::TrimEnd => timeline_boundary_is_visible(start_time + duration, view),
        };
        node.display = if boundary_visible {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (marker, mut node) in &mut children {
        let Some(clip) = session
            .effect
            .effect_clips
            .iter()
            .find(|clip| clip.id == marker.clip)
        else {
            node.display = Display::None;
            continue;
        };
        let (clip_start, source_offset, clip_duration) = state
            .effect_clip_preview_timing(marker.clip)
            .unwrap_or((clip.start_time, clip.source_offset, clip.duration));
        let source_start = marker.source_start.max(source_offset);
        let source_end =
            (marker.source_start + marker.source_duration).min(source_offset + clip_duration);
        if source_end <= source_start {
            node.display = Display::None;
            continue;
        }
        let start_time = clip_start + source_start - source_offset;
        let duration = source_end - source_start;
        apply_timeline_bar_geometry(&mut node, start_time, duration, view);
    }
}

pub(super) fn update_effect_drop_preview(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    catalog: Res<ProjectEffectCatalog>,
    canvases: Query<&RelativeCursorPosition, With<TimelineCanvas>>,
    mut previews: Query<&mut Node, With<TimelineEffectDropPreview>>,
    mut labels: Query<&mut Text, With<TimelineEffectDropPreviewLabel>>,
) {
    let cursor = canvases.single().ok().and_then(|cursor| cursor.normalized);
    for mut node in &mut previews {
        let (Some(preview), Some(cursor)) = (state.effect_drop_preview.as_ref(), cursor) else {
            node.display = Display::None;
            continue;
        };
        let pointer_time = state.view.time_at(timeline_cursor_fraction(cursor.x));
        let Some((start, duration)) = effect_clip_placement(
            pointer_time,
            session.playback_duration(),
            preview.source_duration,
        ) else {
            node.display = Display::None;
            continue;
        };
        node.display = Display::Flex;
        node.left = Val::Percent(state.view.normalized_time(start) * 100.0);
        node.width = Val::Percent((duration / state.view.span() * 100.0).clamp(0.05, 100.0));
        let (_, insertion_offset) = choreography_insertion_layout(
            &session.effect,
            &state,
            &catalog,
            state.effect_drop_insertion,
        );
        node.top = Val::Px((29.0 + insertion_offset - state.vertical_scroll).max(29.0));
        for mut label in &mut labels {
            label.0.clone_from(&preview.display_name);
        }
    }
}

pub(super) fn sync_timeline_horizontal_scroll(
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut viewports: Query<
        (&ComputedNode, &mut ScrollPosition),
        With<TimelineHorizontalScrollViewport>,
    >,
    mut contents: Query<
        &mut Node,
        (
            With<TimelineHorizontalScrollContent>,
            Without<TimelineHorizontalGutter>,
            Without<TimelineHorizontalScrollbarGutter>,
        ),
    >,
    mut gutters: Query<
        &mut Node,
        (
            Or<(
                With<TimelineHorizontalGutter>,
                With<TimelineHorizontalScrollbarGutter>,
            )>,
            Without<TimelineHorizontalScrollContent>,
        ),
    >,
) {
    let duration = session.playback_duration().max(0.05);
    let span = state.view.span().clamp(0.001, duration);
    let overflow = span < duration * 0.999;
    for mut content in &mut contents {
        content.width = Val::Percent((duration / span * 100.0).clamp(100.0, 100_000.0));
    }
    for mut gutter in &mut gutters {
        gutter.display = if overflow {
            Display::Flex
        } else {
            Display::None
        };
    }

    let Ok((computed, mut position)) = viewports.single_mut() else {
        return;
    };
    let maximum = (computed.content_size().x - computed.size().x).max(0.0);
    let scrollable_time = (duration - span).max(0.0);
    if !position.is_added() && position.is_changed() && maximum > 0.5 {
        let start = (position.x / maximum).clamp(0.0, 1.0) * scrollable_time;
        state.view.start = start;
        state.view.end = start + span;
    }
    let desired = if scrollable_time > 0.0 && maximum > 0.5 {
        (state.view.start / scrollable_time).clamp(0.0, 1.0) * maximum
    } else {
        0.0
    };
    if (position.x - desired).abs() > 0.01 || position.y.abs() > 0.01 {
        position.0 = Vec2::new(desired, 0.0);
    }
}

pub(super) fn reveal_timeline_emitter(
    mut commands: Commands,
    mut state: ResMut<TimelineState>,
    headers: Query<(Entity, &EmitterTrackHeader)>,
    panes: Query<(Entity, &TimelineVerticalPane), With<ListBox>>,
    mut focus: ResMut<InputFocus>,
    mut focus_visible: ResMut<InputFocusVisible>,
) {
    let Some(target) = state.reveal_emitter else {
        return;
    };
    if state.reveal_wait_frames > 0 {
        state.reveal_wait_frames -= 1;
        return;
    }
    let Some((row, _)) = headers.iter().find(|(_, header)| header.emitter == target) else {
        return;
    };
    let Some((list, _)) = panes
        .iter()
        .find(|(_, pane)| **pane == TimelineVerticalPane::Headers)
    else {
        return;
    };

    commands.entity(list).insert(ActiveDescendant(Some(row)));
    commands.trigger(ScrollIntoView { entity: row });
    focus.set(list, FocusCause::Navigated);
    focus_visible.0 = false;
    state.reveal_emitter = None;
}

pub(super) fn sync_timeline_vertical_scroll(
    mut state: ResMut<TimelineState>,
    mut panes: Query<(&TimelineVerticalPane, &ComputedNode, &mut ScrollPosition)>,
    mut gutters: Query<&mut Node, With<TimelineVerticalScrollbarGutter>>,
) {
    let mut changed_header = None;
    let mut changed_tracks = None;
    for (kind, computed, position) in &mut panes {
        if position.is_added() || !position.is_changed() || computed.size().y <= 0.5 {
            continue;
        }
        let maximum = (computed.content_size().y - computed.size().y).max(0.0);
        let value = position.y.clamp(0.0, maximum);
        match kind {
            TimelineVerticalPane::Headers => changed_header = Some(value),
            TimelineVerticalPane::Tracks => changed_tracks = Some(value),
        }
    }

    let maximum = panes
        .iter()
        .filter(|(kind, computed, _)| {
            **kind == TimelineVerticalPane::Tracks && computed.size().y > 0.5
        })
        .map(|(_, computed, _)| (computed.content_size().y - computed.size().y).max(0.0))
        .next()
        .or_else(|| {
            panes
                .iter()
                .filter(|(kind, computed, _)| {
                    **kind == TimelineVerticalPane::Headers && computed.size().y > 0.5
                })
                .map(|(_, computed, _)| (computed.content_size().y - computed.size().y).max(0.0))
                .next()
        });
    let Some(maximum) = maximum else {
        return;
    };
    for mut gutter in &mut gutters {
        gutter.display = if maximum > 0.5 {
            Display::Flex
        } else {
            Display::None
        };
    }
    state.vertical_scroll = resolved_vertical_scroll(
        state.vertical_scroll,
        changed_header,
        changed_tracks,
        maximum,
    );
    for (_, _, mut position) in &mut panes {
        if (position.y - state.vertical_scroll).abs() > 0.01 || position.x.abs() > 0.01 {
            position.0 = Vec2::new(0.0, state.vertical_scroll);
        }
    }
}

pub(super) fn resolved_vertical_scroll(
    previous: f32,
    changed_header: Option<f32>,
    changed_tracks: Option<f32>,
    maximum: f32,
) -> f32 {
    changed_tracks
        .or(changed_header)
        .unwrap_or(previous)
        .clamp(0.0, maximum.max(0.0))
}

pub(super) fn update_timeline_time_label(
    session: Res<EditorSession>,
    mut labels: Query<&mut Text, With<TimeLabel>>,
) {
    if !session.is_changed() {
        return;
    }
    for mut text in &mut labels {
        text.0 = format!(
            "F{:05}  ·  {:02}:{:06.3}  /  00:{:06.3}  ·  {}",
            session.frame(),
            0,
            session.time(),
            session.playback_duration(),
            session.seek_status()
        );
    }
}

pub(super) fn format_timeline_tick(time: f32, step: f32) -> String {
    if step >= 1.0 {
        format!("{time:.1}")
    } else if step >= 0.1 {
        format!("{time:.2}")
    } else {
        format!("{time:.3}")
    }
}

pub(super) fn timeline_vertical_content_height(
    effect: &EffectAsset,
    state: &TimelineState,
    catalog: &ProjectEffectCatalog,
) -> f32 {
    let mut height = 0.0;
    for track in normalized_choreography_order(effect) {
        height += 31.0;
        if let ChoreographyTrackId::EffectClip(id) = track
            && let Some(clip) = effect.effect_clips.iter().find(|clip| clip.id == id)
        {
            height += referenced_track_projections(catalog, state, clip).len() as f32 * 27.0;
        }
        if let ChoreographyTrackId::Emitter(id) = track
            && state.expanded_automation_emitters.contains(&id)
            && let Some(emitter) = effect.emitters.iter().find(|emitter| emitter.id == id)
        {
            height += automation_lanes_height(state, emitter);
        }
    }
    if state.effect_drop_preview.is_some() {
        height += 31.0;
    }
    height
}

pub(super) fn hovered_timeline_header_insertion(
    headers: &Query<(&TimelineTrackHeader, &RelativeCursorPosition)>,
) -> Option<(ChoreographyTrackId, bool)> {
    headers
        .iter()
        .find_map(|(header, cursor)| timeline_insertion_for_cursor(header.track, cursor))
}

pub(super) fn hovered_timeline_track_row_insertion(
    rows: &Query<(&TimelineTrackDropRow, &RelativeCursorPosition)>,
) -> Option<(ChoreographyTrackId, bool)> {
    rows.iter()
        .find_map(|(row, cursor)| timeline_insertion_for_cursor(row.track, cursor))
}

pub(super) fn update_effect_drop_insertion(
    mut state: ResMut<TimelineState>,
    headers: Query<(&TimelineTrackHeader, &RelativeCursorPosition)>,
    rows: Query<(&TimelineTrackDropRow, &RelativeCursorPosition)>,
    spacers: Query<&RelativeCursorPosition, With<TimelineEffectDropSpacer>>,
) {
    if state.effect_drop_preview.is_none() {
        state.effect_drop_insertion = None;
        return;
    }
    let hovered = hovered_timeline_header_insertion(&headers)
        .or_else(|| hovered_timeline_track_row_insertion(&rows));
    if hovered.is_some() {
        state.effect_drop_insertion = hovered;
    } else if !spacers.iter().any(RelativeCursorPosition::cursor_over) {
        state.effect_drop_insertion = None;
    }
}

pub(super) fn sync_effect_drop_track_gap(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    catalog: Res<ProjectEffectCatalog>,
    mut rows: Query<(&TimelineChoreographyGridRow, &mut Node), Without<TimelineEffectDropSpacer>>,
    mut spacers: Query<&mut Node, With<TimelineEffectDropSpacer>>,
) {
    let insertion_row = state.effect_drop_preview.as_ref().map(|_| {
        choreography_insertion_grid_row(
            &session.effect,
            &state,
            &catalog,
            state.effect_drop_insertion,
        )
    });
    for (base, mut node) in &mut rows {
        let row = base.0 + i16::from(insertion_row.is_some_and(|insertion| base.0 >= insertion));
        node.grid_row = GridPlacement::start(row);
    }
    for mut node in &mut spacers {
        let Some(insertion_row) = insertion_row else {
            node.display = Display::None;
            continue;
        };
        node.display = Display::Flex;
        node.grid_row = GridPlacement::start(insertion_row);
    }
}

pub(super) fn timeline_insertion_for_cursor(
    track: ChoreographyTrackId,
    cursor: &RelativeCursorPosition,
) -> Option<(ChoreographyTrackId, bool)> {
    cursor.cursor_over().then(|| {
        let before = cursor.normalized.is_none_or(|position| position.y < 0.0);
        (track, before)
    })
}

pub(super) fn timeline_boundary_is_visible(time: f32, view: TimelineView) -> bool {
    (view.start..=view.end).contains(&time)
}

pub(super) fn apply_timeline_bar_geometry(
    node: &mut Node,
    start_time: f32,
    duration: f32,
    view: TimelineView,
) {
    let visible_start = start_time.max(view.start);
    let visible_end = (start_time + duration).min(view.end);
    if visible_end <= visible_start {
        node.display = Display::None;
        return;
    }
    node.display = Display::Flex;
    node.left = Val::Percent(view.normalized_time(visible_start) * 100.0);
    node.width =
        Val::Percent(((visible_end - visible_start) / view.span() * 100.0).clamp(0.05, 100.0));
}

pub(super) fn choreography_grid_row(
    effect: &EffectAsset,
    state: &TimelineState,
    catalog: &ProjectEffectCatalog,
    target: ChoreographyTrackId,
) -> i16 {
    let mut row = 1_i16;
    for track in normalized_choreography_order(effect) {
        if track == target {
            return row;
        }
        row = row.saturating_add(1);
        if let ChoreographyTrackId::EffectClip(id) = track
            && let Some(clip) = effect.effect_clips.iter().find(|clip| clip.id == id)
        {
            let child_count = referenced_track_projections(catalog, state, clip).len();
            row = row.saturating_add(child_count.min(i16::MAX as usize) as i16);
        }
        if let ChoreographyTrackId::Emitter(id) = track
            && state.expanded_automation_emitters.contains(&id)
            && let Some(emitter) = effect.emitters.iter().find(|emitter| emitter.id == id)
        {
            row = row.saturating_add(
                visible_automation_lane_count(state, emitter).min(i16::MAX as usize) as i16,
            );
        }
    }
    row
}

pub(super) fn choreography_insertion_grid_row(
    effect: &EffectAsset,
    state: &TimelineState,
    catalog: &ProjectEffectCatalog,
    insertion: Option<(ChoreographyTrackId, bool)>,
) -> i16 {
    choreography_insertion_layout(effect, state, catalog, insertion).0
}

pub(super) fn choreography_insertion_layout(
    effect: &EffectAsset,
    state: &TimelineState,
    catalog: &ProjectEffectCatalog,
    insertion: Option<(ChoreographyTrackId, bool)>,
) -> (i16, f32) {
    let order = normalized_choreography_order(effect);
    let insertion_index = insertion.map_or(order.len(), |(target, before)| {
        choreography_insertion_index(&order, target, before)
    });
    let mut row = 1_i16;
    let mut offset = 0.0;
    for track in order.into_iter().take(insertion_index) {
        row = row.saturating_add(1);
        offset += 31.0;
        if let ChoreographyTrackId::EffectClip(id) = track
            && let Some(clip) = effect.effect_clips.iter().find(|clip| clip.id == id)
        {
            let child_count = referenced_track_projections(catalog, state, clip).len();
            row = row.saturating_add(child_count.min(i16::MAX as usize) as i16);
            offset += child_count as f32 * 27.0;
        }
        if let ChoreographyTrackId::Emitter(id) = track
            && state.expanded_automation_emitters.contains(&id)
            && let Some(emitter) = effect.emitters.iter().find(|emitter| emitter.id == id)
        {
            let lane_count = visible_automation_lane_count(state, emitter);
            row = row.saturating_add(lane_count.min(i16::MAX as usize) as i16);
            offset += automation_lanes_height(state, emitter);
        }
    }
    (row, offset)
}

pub(super) fn sync_effect_clip_reorder_hints(
    state: Res<TimelineState>,
    mut headers: Query<(
        &EffectClipTrackHeader,
        &RelativeCursorPosition,
        &mut Node,
        &mut BorderColor,
    )>,
) {
    let base = theme::BORDER.with_alpha(0.55);
    for (header, cursor, mut node, mut border) in &mut headers {
        node.border.top = Val::Px(0.0);
        node.border.bottom = Val::Px(1.0);
        border.top = base;
        border.bottom = base;
        let target = ChoreographyTrackId::EffectClip(header.clip);
        if state.effect_drop_preview.is_some() {
            continue;
        }
        let dragging = state
            .effect_clip_reorder_drag
            .map(ChoreographyTrackId::EffectClip)
            .or_else(|| state.reorder_drag.map(ChoreographyTrackId::Emitter));
        if dragging.is_none_or(|source| source == target) || !cursor.cursor_over() {
            continue;
        }
        apply_timeline_insertion_border(
            cursor.normalized.is_none_or(|position| position.y < 0.0),
            &mut node,
            &mut border,
        );
    }
}

pub(super) fn sync_emitter_reorder_hints(
    state: Res<TimelineState>,
    mut headers: Query<(
        &EmitterTrackHeader,
        &RelativeCursorPosition,
        &mut Node,
        &mut BorderColor,
    )>,
) {
    let base = theme::BORDER.with_alpha(0.55);
    for (header, cursor, mut node, mut border) in &mut headers {
        node.border.top = Val::Px(0.0);
        node.border.bottom = Val::Px(1.0);
        border.top = base;
        border.bottom = base;

        let target = ChoreographyTrackId::Emitter(header.emitter);
        if state.effect_drop_preview.is_some() {
            continue;
        }
        let dragging = state
            .reorder_drag
            .map(ChoreographyTrackId::Emitter)
            .or_else(|| {
                state
                    .effect_clip_reorder_drag
                    .map(ChoreographyTrackId::EffectClip)
            });
        if dragging.is_none_or(|source| source == target) || !cursor.cursor_over() {
            continue;
        }
        apply_timeline_insertion_border(
            cursor.normalized.is_none_or(|position| position.y < 0.0),
            &mut node,
            &mut border,
        );
    }
}

pub(super) fn sync_timeline_track_drop_hints(
    mut rows: Query<(&mut Node, &mut BorderColor), With<TimelineTrackDropRow>>,
) {
    let base = theme::BORDER.with_alpha(0.45);
    for (mut node, mut border) in &mut rows {
        node.border.top = Val::Px(0.0);
        node.border.bottom = Val::Px(1.0);
        border.top = base;
        border.bottom = base;
    }
}

pub(super) fn apply_timeline_insertion_border(
    before: bool,
    node: &mut Node,
    border: &mut BorderColor,
) {
    if before {
        node.border.top = Val::Px(3.0);
        border.top = theme::DOCK_TARGET;
    } else {
        node.border.bottom = Val::Px(3.0);
        border.bottom = theme::DOCK_TARGET;
    }
}

#[cfg(test)]
#[path = "presentation/tests.rs"]
mod tests;
