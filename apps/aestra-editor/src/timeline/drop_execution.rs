//! Actual timeline asset-drop preview, placement transaction and rejection feedback.
use super::asset_drop::{EffectDragRows, EffectDragSource};
use super::interaction_state::*;
use super::navigation::timeline_cursor_fraction;
use super::state::normalized_choreography_order;
use super::timeline_pointer::{
    PointerDragDrop, PointerDragEnd, PointerDragEnter, PointerDragLeave, PointerDragStart,
};
use super::{TimelineState, asset_drop};
use crate::{EditorSession, ProjectEffectCatalog, localization::Localizer};
use aestra_authoring::{EffectCommand, EffectTransaction};
use aestra_core::{ChoreographyTrackId, EffectClip};
use bevy::{picking::pointer::PointerButton, prelude::*, ui::RelativeCursorPosition};
use fluent_bundle::FluentArgs;

pub(super) fn dragged_project_effect(
    entity: Entity,
    rows: &EffectDragRows,
    parents: &Query<&ChildOf>,
) -> Option<EffectDragSource> {
    crate::asset_drop::nearest(entity, parents, |entity| {
        rows.get(entity).ok().cloned().map(EffectDragSource)
    })
    .map(|(_, source)| source)
}

pub(super) fn show_invalid_timeline_drop_feedback(
    mut enter: On<PointerDragEnter>,
    rows: EffectDragRows,
    parents: Query<&ChildOf>,
    mut feedback: Query<(&mut TimelineInvalidDropFeedback, &mut Node)>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut commands: Commands,
    guard: asset_drop::DropGuard,
) {
    if enter.button != PointerButton::Primary {
        return;
    }
    let Some(row) = dragged_project_effect(enter.dragged, &rows, &parents) else {
        return;
    };
    state.browser_drop = Some(row.0.clone());
    let result = guard.check().and_then(|()| row.preview(&catalog, &session));
    state.effect_drop_preview = result.as_ref().ok().cloned();
    for (mut feedback, mut node) in &mut feedback {
        feedback.rejected = false;
        feedback.timer.reset();
        feedback.timer.pause();
        node.display = Display::None;
    }
    if let Err(reason) = result {
        commands.trigger(RejectProjectEffectDrop { reason });
    }
    enter.propagate(false);
}

pub(super) fn hide_invalid_timeline_drop_feedback(
    mut leave: On<PointerDragLeave>,
    rows: EffectDragRows,
    parents: Query<&ChildOf>,
    mut feedback: Query<(&TimelineInvalidDropFeedback, &mut Node)>,
    mut state: ResMut<TimelineState>,
) {
    if dragged_project_effect(leave.dragged, &rows, &parents).is_none() {
        return;
    }
    for (feedback, mut node) in &mut feedback {
        if !feedback.rejected {
            node.display = Display::None;
        }
    }
    if state.browser_drop.take().is_some() {
        state.effect_drop_preview = None;
        state.effect_drop_insertion = None;
    }
    leave.propagate(false);
}

pub(super) fn begin_project_effect_drag_preview(
    drag: On<PointerDragStart>,
    rows: EffectDragRows,
    parents: Query<&ChildOf>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some(row) = dragged_project_effect(drag.entity, &rows, &parents) else {
        return;
    };
    state.effect_drop_preview = row.preview(&catalog, &session).ok();
    state.effect_drop_insertion = None;
}

pub(super) fn finish_project_effect_drag_preview(
    drag: On<PointerDragEnd>,
    rows: EffectDragRows,
    parents: Query<&ChildOf>,
    mut state: ResMut<TimelineState>,
) {
    if dragged_project_effect(drag.entity, &rows, &parents).is_none() {
        return;
    }
    state.effect_drop_preview = None;
    state.effect_drop_insertion = None;
    state.browser_drop = None;
}

pub(super) fn drop_project_effect_on_timeline(
    mut drop: On<PointerDragDrop>,
    rows: EffectDragRows,
    parents: Query<&ChildOf>,
    canvases: Query<&RelativeCursorPosition, With<TimelineCanvas>>,
    catalog: Res<ProjectEffectCatalog>,
    mut state: ResMut<TimelineState>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
    mut feedback: Query<(&mut TimelineInvalidDropFeedback, &mut Node)>,
    mut commands: Commands,
    guard: asset_drop::DropGuard,
) {
    if drop.button != PointerButton::Primary {
        return;
    }
    let Some(source_row) = dragged_project_effect(drop.dropped, &rows, &parents) else {
        return;
    };
    drop.propagate(false);
    let insertion = state.effect_drop_insertion.take();
    state.effect_drop_preview = None;

    let result = guard.check_release().and_then(|()| {
        canvases
            .single()
            .ok()
            .and_then(|cursor| cursor.normalized)
            .ok_or_else(|| "the drop position is outside the timeline".to_string())
            .and_then(|cursor| {
                let pointer_time = state.view.time_at(timeline_cursor_fraction(cursor.x));
                insert_project_effect_clip(
                    source_row,
                    pointer_time,
                    insertion,
                    &catalog,
                    &mut session,
                    &localizer,
                )
            })
    });

    finish_project_effect_drop(result, &mut feedback, &mut commands);
}

pub(super) fn drop_project_effect_on_track_headers(
    mut drop: On<PointerDragDrop>,
    rows: EffectDragRows,
    parents: Query<&ChildOf>,
    catalog: Res<ProjectEffectCatalog>,
    mut state: ResMut<TimelineState>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
    mut feedback: Query<(&mut TimelineInvalidDropFeedback, &mut Node)>,
    mut commands: Commands,
    guard: asset_drop::DropGuard,
) {
    if drop.button != PointerButton::Primary {
        return;
    }
    let Some(source_row) = dragged_project_effect(drop.dropped, &rows, &parents) else {
        return;
    };
    drop.propagate(false);
    let insertion = state.effect_drop_insertion.take();
    state.effect_drop_preview = None;

    let playhead_time = session.time();
    let result = guard.check_release().and_then(|()| {
        insert_project_effect_clip(
            source_row,
            playhead_time,
            insertion,
            &catalog,
            &mut session,
            &localizer,
        )
    });
    finish_project_effect_drop(result, &mut feedback, &mut commands);
}

pub(super) fn insert_project_effect_clip(
    source_row: EffectDragSource,
    pointer_time: f32,
    insertion: Option<(ChoreographyTrackId, bool)>,
    catalog: &ProjectEffectCatalog,
    session: &mut EditorSession,
    localizer: &Localizer,
) -> Result<(), String> {
    if session.pending_change.is_some() {
        return Err("Resolve the pending change before adding an effect".into());
    }
    let (reference, display_name) = source_row.resolve(catalog)?;
    let source = catalog.effect_for_placement(&session.effect, reference)?;
    let (start_time, duration) =
        effect_clip_placement(pointer_time, session.playback_duration(), source.duration)
            .ok_or_else(|| "there is no room for this effect at the drop position".to_string())?;
    let clip = EffectClip::new(reference, start_time, duration);
    let clip_id = clip.id;
    let index = session.effect.effect_clips.len();
    let mut order = normalized_choreography_order(&session.effect);
    let insertion_index = insertion.map_or(order.len(), |(target, before)| {
        choreography_insertion_index(&order, target, before)
    });
    order.insert(insertion_index, ChoreographyTrackId::EffectClip(clip_id));
    if !session.execute_transaction(
        EffectTransaction::new(
            localizer.text("timeline-add-effect-clip-command"),
            vec![
                EffectCommand::AddEffectClip { clip, index },
                EffectCommand::SetChoreographyOrder { order },
            ],
        ),
        true,
    ) {
        return Err(session.status.clone());
    }
    // A drop has no Press on the destination to transfer editing focus.
    // Keep any standalone graph open, but route the next Undo to this edit.
    session.material_history_active = false;
    session.select_effect_clip(clip_id);
    let mut args = FluentArgs::new();
    args.set("name", display_name);
    session.status = localizer.text_with("timeline-effect-clip-added", &args);
    Ok(())
}

pub(super) fn finish_project_effect_drop(
    result: Result<(), String>,
    feedback: &mut Query<(&mut TimelineInvalidDropFeedback, &mut Node)>,
    commands: &mut Commands,
) {
    match result {
        Ok(()) => {
            for (mut feedback_state, mut node) in feedback {
                feedback_state.rejected = false;
                feedback_state.timer.pause();
                node.display = Display::None;
            }
        }
        Err(reason) => commands.trigger(RejectProjectEffectDrop { reason }),
    }
}

pub(super) fn reject_project_effect_drop(
    drop: On<RejectProjectEffectDrop>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
    mut feedback: Query<(&mut TimelineInvalidDropFeedback, &mut Node)>,
    mut titles: Query<
        &mut Text,
        (
            With<TimelineDropFeedbackTitle>,
            Without<TimelineDropFeedbackMessage>,
        ),
    >,
    mut messages: Query<
        &mut Text,
        (
            With<TimelineDropFeedbackMessage>,
            Without<TimelineDropFeedbackTitle>,
        ),
    >,
) {
    let mut args = FluentArgs::new();
    args.set("reason", drop.reason.as_str());
    session.status = localizer.text_with("timeline-drop-effect-rejected-status", &args);
    for (mut feedback, mut node) in &mut feedback {
        feedback.rejected = true;
        feedback.timer.reset();
        feedback.timer.unpause();
        node.display = Display::Flex;
    }
    for mut text in &mut titles {
        text.0 = localizer.text("timeline-drop-effect-rejected-title");
    }
    for mut text in &mut messages {
        text.0 = drop.reason.clone();
    }
}

pub(super) fn effect_clip_placement(
    pointer_time: f32,
    owner_duration: f32,
    source_duration: f32,
) -> Option<(f32, f32)> {
    if !owner_duration.is_finite()
        || owner_duration <= 0.0
        || !source_duration.is_finite()
        || source_duration <= 0.0
    {
        return None;
    }
    let duration = source_duration.min(owner_duration);
    let start = pointer_time.clamp(0.0, owner_duration - duration);
    Some((start, duration))
}

pub(super) fn tick_invalid_timeline_drop_feedback(
    time: Res<Time>,
    mut feedback: Query<(&mut TimelineInvalidDropFeedback, &mut Node)>,
) {
    for (mut feedback, mut node) in &mut feedback {
        if feedback.timer.is_paused() {
            continue;
        }
        feedback.timer.tick(time.delta());
        if feedback.timer.just_finished() {
            feedback.rejected = false;
            feedback.timer.pause();
            node.display = Display::None;
        }
    }
}

pub(super) fn choreography_insertion_index(
    order: &[ChoreographyTrackId],
    target: ChoreographyTrackId,
    before: bool,
) -> usize {
    order
        .iter()
        .position(|track| *track == target)
        .map_or(order.len(), |index| index + usize::from(!before))
}

#[cfg(test)]
#[path = "tests/drop_execution.rs"]
mod tests;
