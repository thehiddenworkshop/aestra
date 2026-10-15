//! Actual top-level timeline/choreography observers and emitter deletion preparation.
use super::automation_execution::handle_automation_action;
use super::referenced_execution::handle_referenced_effect_action;
use super::region_execution::{
    delete_selected_emitter_regions, duplicate_selected_emitter_regions, merge_selected_regions,
    split_selected_region_at_playhead,
};
use super::{
    TimelineState,
    actions::{ChoreographyAction, TimelineAction},
    document_actions, motion_execution,
};
use crate::{
    CurvesState, EditorSession, ProjectEffectCatalog, localization::Localizer,
    persistence::DocumentAction,
};
use aestra_authoring::{EffectCommand, EffectTransaction};
use aestra_core::EmitterId;
use bevy::prelude::*;

pub(super) fn execute_timeline_action(
    action: On<TimelineAction>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    localizer: Option<Res<Localizer>>,
) {
    match *action {
        TimelineAction::AdjustEffectDuration(delta) => {
            session.adjust_effect_duration(delta);
        }
        TimelineAction::SetSnap(mode) => {
            if state.set_snap(mode) {
                session.ui_revision += 1;
            }
        }
        TimelineAction::FrameAll => state.frame_all(motion_execution::timeline_duration(&session)),
        TimelineAction::AddMarker => {
            state.clear_emitter_selection();
            document_actions::apply(*action, &mut session, localizer.as_deref());
        }
        TimelineAction::SelectMarker(_) => {
            state.clear_emitter_selection();
            document_actions::apply(*action, &mut session, localizer.as_deref());
            state.inspected_child = None;
        }
        TimelineAction::DeleteMarker(_) => {
            document_actions::apply(*action, &mut session, localizer.as_deref());
        }
        TimelineAction::AddChoreographyEvent => {
            state.clear_emitter_selection();
            document_actions::apply(*action, &mut session, localizer.as_deref());
        }
        TimelineAction::SelectChoreographyEvent(_) => {
            state.clear_emitter_selection();
            document_actions::apply(*action, &mut session, localizer.as_deref());
            state.inspected_child = None;
        }
        TimelineAction::DeleteChoreographyEvent(_) => {
            document_actions::apply(*action, &mut session, localizer.as_deref());
        }
        TimelineAction::SplitEmitterRegion => {
            split_selected_region_at_playhead(&mut session, &mut state);
        }
        TimelineAction::JoinEmitterRegion => {
            merge_selected_regions(&mut session, &mut state);
        }
    }
}

pub(super) fn execute_choreography_action(
    action: On<ChoreographyAction>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
    mut curves: ResMut<CurvesState>,
    mut state: ResMut<TimelineState>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
) {
    if handle_automation_action(
        &action,
        &mut commands,
        &mut session,
        &mut curves,
        &mut state,
        &localizer,
    ) {
        return;
    }

    if let ChoreographyAction::ToggleEmitterColorPicker(emitter) = action.clone() {
        let revision = session.ui_revision;
        if session
            .effect
            .emitters
            .iter()
            .any(|item| item.id == emitter)
        {
            session.select_emitter(emitter);
            state.select_only_emitter(emitter);
            curves.clear();
        }
        state.context_emitter = None;
        state.color_picker_emitter =
            (state.color_picker_emitter != Some(emitter)).then_some(emitter);
        if session.ui_revision == revision {
            session.ui_revision += 1;
        }
        return;
    }

    let revision = session.ui_revision;
    state.restore_context_emitter_focus = state.context_emitter;
    state.restore_context_effect_clip_focus = state.context_effect_clip;
    let closed_context_menu = state.context_emitter.take().is_some()
        | state.color_picker_emitter.take().is_some()
        | state.context_effect_clip.take().is_some()
        | state.automation_menu_emitter.take().is_some();
    if handle_referenced_effect_action(
        &action,
        &mut commands,
        &mut session,
        &mut curves,
        &mut state,
        &catalog,
        &localizer,
    ) {
        if closed_context_menu && session.ui_revision == revision {
            session.ui_revision += 1;
        }
        return;
    }
    match action.clone() {
        ChoreographyAction::SelectEmitterRegion { emitter, region } => {
            state.selected_automation_key = None;
            if let Some(selected_emitter) = session
                .effect
                .emitters
                .iter()
                .find(|item| item.id == emitter)
                .filter(|item| item.timeline_region(region).is_some())
            {
                let control = keys.as_deref().is_some_and(|keys| {
                    keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
                });
                let shift = keys.as_deref().is_some_and(|keys| {
                    keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)
                });
                let primary = state.select_emitter_region(selected_emitter, region, control, shift);
                if let Some(primary) = primary {
                    session.select_emitter_region(emitter, primary);
                } else {
                    session.select_emitter(emitter);
                }
                state.inspected_child = None;
                curves.clear();
                session.ui_revision += 1;
            }
        }
        ChoreographyAction::SelectEmitter(emitter) => {
            state.selected_automation_key = None;
            if session
                .effect
                .emitters
                .iter()
                .any(|item| item.id == emitter)
            {
                let control = keys.as_deref().is_some_and(|keys| {
                    keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
                });
                let shift = keys.as_deref().is_some_and(|keys| {
                    keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)
                });
                let current = session.selection.emitter(&session.effect);
                let primary =
                    state.select_emitter(&session.effect, current, emitter, control, shift);
                session.select_emitter(primary);
                state.inspected_child = None;
                curves.clear();
            }
        }
        ChoreographyAction::SelectEffectClip(clip) => {
            state.selected_automation_key = None;
            if session
                .effect
                .effect_clips
                .iter()
                .any(|item| item.id == clip)
            {
                state.clear_emitter_selection();
                let changed = session.select_effect_clip(clip);
                let had_child = state.inspected_child.take().is_some();
                state.inspected_child = None;
                curves.clear();
                if had_child && !changed {
                    session.ui_revision += 1;
                }
            }
        }
        ChoreographyAction::ToggleEffectClipMuted(clip) => {
            if !state.muted_effect_clips.remove(&clip) {
                state.muted_effect_clips.insert(clip);
            }
            session.status = localizer.text("timeline-effect-clip-preview-updated");
            session.ui_revision += 1;
        }
        ChoreographyAction::ToggleEffectClipSolo(clip) => {
            let next_solo = (state.solo_effect_clip != Some(clip)).then_some(clip);
            if next_solo.is_some()
                && let Some(emitter) = session.solo_emitter
            {
                session.toggle_preview_solo(emitter);
            }
            state.solo_effect_clip = next_solo;
            session.status = localizer.text("timeline-effect-clip-preview-updated");
            session.ui_revision += 1;
        }
        ChoreographyAction::DeleteEffectClip(clip) => {
            if session.execute(
                localizer.text("timeline-delete-effect-clip-command"),
                EffectCommand::RemoveEffectClip { id: clip },
                true,
            ) {
                state
                    .expanded_effect_clips
                    .retain(|path| path.root() != clip);
                state.muted_effect_clips.remove(&clip);
                if state.solo_effect_clip == Some(clip) {
                    state.solo_effect_clip = None;
                }
                state.context_effect_clip = None;
                state.inspected_child = None;
                curves.clear();
            }
        }
        ChoreographyAction::EditEffectClipSource(clip) => {
            if let Some(source) = session
                .effect
                .effect_clips
                .iter()
                .find(|candidate| candidate.id == clip)
                .map(|clip| clip.source)
            {
                state.context_effect_clip = None;
                session.ui_revision += 1;
                commands.trigger(DocumentAction::OpenSource(source));
            }
        }
        ChoreographyAction::AddEmitter => {
            session.add_layer();
            if let Some(emitter) = session.selection.emitter(&session.effect) {
                state.select_only_emitter(emitter);
            }
            curves.clear();
        }
        ChoreographyAction::DuplicateEmitter(target) => {
            if select_choreography_target(&mut session, target) {
                session.duplicate_selected_layer();
                if let Some(emitter) = session.selection.emitter(&session.effect) {
                    state.select_only_emitter(emitter);
                }
                curves.clear();
            }
        }
        ChoreographyAction::DuplicateSelectedEmitterRegions => {
            duplicate_selected_emitter_regions(&mut session, &mut state, &mut curves);
        }
        ChoreographyAction::DeleteEmitter(target) => {
            if select_choreography_target(&mut session, target)
                && preview_selected_emitter_deletion(&mut session, &localizer)
            {
                curves.clear();
            }
        }
        ChoreographyAction::DeleteSelectedEmitterRegions => {
            delete_selected_emitter_regions(&mut session, &mut state, &mut curves, &localizer);
        }
        ChoreographyAction::SetEmitterEnabled { emitter, enabled } => {
            if select_choreography_target(&mut session, Some(emitter)) {
                session.set_selected_emitter_enabled(enabled);
                curves.clear();
            }
        }
        ChoreographyAction::ToggleEmitterSolo(emitter) => {
            if session.toggle_preview_solo(emitter) {
                if session.solo_emitter.is_some() {
                    state.solo_effect_clip = None;
                }
                curves.clear();
            }
        }
        ChoreographyAction::ToggleEmitterColorPicker(_) => unreachable!(),
        ChoreographyAction::ToggleEmitterAutomation(_)
        | ChoreographyAction::SetEmitterAutomationVisibility { .. }
        | ChoreographyAction::SetAutomationLaneVisibility { .. }
        | ChoreographyAction::SelectAutomationKey(_)
        | ChoreographyAction::AddAutomationKey(_)
        | ChoreographyAction::AddAutomationKeyAt { .. }
        | ChoreographyAction::DeleteAutomationKey(_)
        | ChoreographyAction::SelectEffectClipEmitter { .. }
        | ChoreographyAction::SelectReferencedEffectClip(_)
        | ChoreographyAction::ToggleEffectClipExpanded(_)
        | ChoreographyAction::EditEffectClipEmitterSource { .. } => unreachable!(),
    }
    if closed_context_menu && session.ui_revision == revision {
        session.ui_revision += 1;
    }
}

pub(super) fn select_choreography_target(
    session: &mut EditorSession,
    target: Option<EmitterId>,
) -> bool {
    let Some(target) = target.or_else(|| session.selection.emitter(&session.effect)) else {
        session.status = "The selected timeline item is not an emitter".into();
        return false;
    };
    if !session
        .effect
        .emitters
        .iter()
        .any(|emitter| emitter.id == target)
    {
        session.status = "Emitter no longer exists".into();
        return false;
    }
    session.select_emitter(target);
    true
}

pub(super) fn preview_selected_emitter_deletion(
    session: &mut EditorSession,
    localizer: &Localizer,
) -> bool {
    let Some(selected_layer) = session.selected_layer() else {
        return false;
    };
    if session.effect.emitters.len() <= 1 {
        session.status = localizer.text("assets-status-minimum-emitter");
        return false;
    }
    let id = selected_layer.id;
    session.preview_transaction(EffectTransaction::single(
        localizer.text("assets-change-delete-emitter"),
        EffectCommand::RemoveEmitter { id },
    ))
}
