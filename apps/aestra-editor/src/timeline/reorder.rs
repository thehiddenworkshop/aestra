//! Actual emitter/effect-clip track drag lifecycle and semantic order transactions.
use super::TimelineState;
use super::interaction_state::*;
use super::navigation_cursor::{EntityCursor, OverrideCursor};
use super::state::normalized_choreography_order;
use super::timeline_pointer::{PointerDragDrop, PointerDragEnd, PointerDragStart};
use crate::{EditorSession, localization::Localizer};
use aestra_authoring::EffectCommand;
use aestra_core::{ChoreographyTrackId, EffectClipId, EmitterId};
use bevy::{
    prelude::*,
    ui::RelativeCursorPosition,
    window::{CursorIcon, PrimaryWindow, SystemCursorIcon},
};

pub(super) fn track_reorder_index(
    source_index: usize,
    target_index: usize,
    before: bool,
    len: usize,
) -> Option<usize> {
    if len < 2 || source_index >= len || target_index >= len || source_index == target_index {
        return None;
    }
    let insertion_boundary = target_index + usize::from(!before);
    let final_index = insertion_boundary - usize::from(source_index < insertion_boundary);
    (final_index != source_index).then_some(final_index.min(len - 1))
}

pub(super) fn begin_emitter_track_reorder(
    mut drag: On<PointerDragStart>,
    handles: Query<&EmitterTrackReorderHandle>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(handle) = handles.get(drag.event_target()) else {
        return;
    };
    state.reorder_drag = Some(handle.emitter);
    override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::Grabbing));
    **cursor = CursorIcon::System(SystemCursorIcon::Grabbing);
    drag.propagate(false);
}

pub(super) fn finish_emitter_track_reorder(
    mut drag: On<PointerDragEnd>,
    handles: Query<&EmitterTrackReorderHandle>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    if !handles.contains(drag.event_target()) {
        return;
    }
    state.reorder_drag = None;
    override_cursor.0 = None;
    **cursor = CursorIcon::System(SystemCursorIcon::Grab);
    drag.propagate(false);
}

pub(super) fn dragged_emitter(
    mut entity: Entity,
    handles: &Query<&EmitterTrackReorderHandle>,
    parents: &Query<&ChildOf>,
) -> Option<EmitterId> {
    loop {
        if let Ok(handle) = handles.get(entity) {
            return Some(handle.emitter);
        }
        entity = parents.get(entity).ok()?.parent();
    }
}

pub(super) fn drop_emitter_track_reorder(
    mut drop: On<PointerDragDrop>,
    headers: Query<(&TimelineTrackHeader, &RelativeCursorPosition)>,
    handles: Query<&EmitterTrackReorderHandle>,
    parents: Query<&ChildOf>,
    mut state: ResMut<TimelineState>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let mut target_entity = drop.event_target();
    let (target, relative_cursor) = loop {
        if let Ok(header) = headers.get(target_entity) {
            break header;
        }
        let Ok(parent) = parents.get(target_entity) else {
            return;
        };
        target_entity = parent.parent();
    };
    let Some(source) = dragged_emitter(drop.dropped, &handles, &parents) else {
        return;
    };
    drop.propagate(false);
    state.reorder_drag = None;
    override_cursor.0 = None;
    **cursor = CursorIcon::System(SystemCursorIcon::Default);

    let source = ChoreographyTrackId::Emitter(source);
    let mut order = normalized_choreography_order(&session.effect);
    let Some(source_index) = order.iter().position(|track| *track == source) else {
        return;
    };
    let Some(target_index) = order.iter().position(|track| *track == target.track) else {
        return;
    };
    let before = relative_cursor
        .normalized
        .is_none_or(|position| position.y < 0.0);
    let Some(index) = track_reorder_index(source_index, target_index, before, order.len()) else {
        return;
    };
    let track = order.remove(source_index);
    order.insert(index, track);
    session.execute(
        localizer.text("timeline-reordered-emitter"),
        EffectCommand::SetChoreographyOrder { order },
        true,
    );
}

pub(super) fn begin_effect_clip_track_reorder(
    mut drag: On<PointerDragStart>,
    handles: Query<&EffectClipTrackReorderHandle>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(handle) = handles.get(drag.event_target()) else {
        return;
    };
    state.effect_clip_reorder_drag = Some(handle.clip);
    override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::Grabbing));
    **cursor = CursorIcon::System(SystemCursorIcon::Grabbing);
    drag.propagate(false);
}

pub(super) fn finish_effect_clip_track_reorder(
    mut drag: On<PointerDragEnd>,
    handles: Query<&EffectClipTrackReorderHandle>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    if !handles.contains(drag.event_target()) {
        return;
    }
    state.effect_clip_reorder_drag = None;
    override_cursor.0 = None;
    **cursor = CursorIcon::System(SystemCursorIcon::Grab);
    drag.propagate(false);
}

pub(super) fn dragged_effect_clip(
    mut entity: Entity,
    handles: &Query<&EffectClipTrackReorderHandle>,
    parents: &Query<&ChildOf>,
) -> Option<EffectClipId> {
    loop {
        if let Ok(handle) = handles.get(entity) {
            return Some(handle.clip);
        }
        entity = parents.get(entity).ok()?.parent();
    }
}

pub(super) fn drop_effect_clip_track_reorder(
    mut drop: On<PointerDragDrop>,
    headers: Query<(&TimelineTrackHeader, &RelativeCursorPosition)>,
    handles: Query<&EffectClipTrackReorderHandle>,
    parents: Query<&ChildOf>,
    mut state: ResMut<TimelineState>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let mut target_entity = drop.event_target();
    let (target, relative_cursor) = loop {
        if let Ok(header) = headers.get(target_entity) {
            break header;
        }
        let Ok(parent) = parents.get(target_entity) else {
            return;
        };
        target_entity = parent.parent();
    };
    let Some(source) = dragged_effect_clip(drop.dropped, &handles, &parents) else {
        return;
    };
    drop.propagate(false);
    state.effect_clip_reorder_drag = None;
    override_cursor.0 = None;
    **cursor = CursorIcon::System(SystemCursorIcon::Default);
    let source = ChoreographyTrackId::EffectClip(source);
    let mut order = normalized_choreography_order(&session.effect);
    let Some(source_index) = order.iter().position(|track| *track == source) else {
        return;
    };
    let Some(target_index) = order.iter().position(|track| *track == target.track) else {
        return;
    };
    let before = relative_cursor
        .normalized
        .is_none_or(|position| position.y < 0.0);
    let Some(index) = track_reorder_index(source_index, target_index, before, order.len()) else {
        return;
    };
    let track = order.remove(source_index);
    order.insert(index, track);
    session.execute(
        localizer.text("timeline-reordered-effect-clip"),
        EffectCommand::SetChoreographyOrder { order },
        true,
    );
}

#[cfg(test)]
#[path = "tests/reorder.rs"]
mod tests;
