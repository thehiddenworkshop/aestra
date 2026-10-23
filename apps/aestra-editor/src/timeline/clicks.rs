//! Production timeline click/source-opening and pointer/keyboard context-menu controllers.
use super::interaction_state::*;
use super::timeline_pointer::{PointerClick, TimelineButton, TimelinePointerLocation};
use super::{ChoreographyAction, TimelineAction, TimelineState};
use crate::feathers::action_activation::{EditorNativeControl, FeathersActionButton};
use crate::feathers::context_menu::{keyboard_context_menu_requested, pointer_position_in_node};
use crate::{CurvesState, EditorSession};
use aestra_core::EmitterId;
use bevy::{
    input_focus::InputFocus, picking::pointer::PointerButton, prelude::*,
    ui_widgets::ActiveDescendant,
};
use std::time::Duration;
const REFERENCED_EMITTER_DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

pub(super) fn select_timeline_marker(
    mut click: On<PointerClick>,
    markers: Query<&TimelineMarker>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(marker) = markers.get(click.event_target()) else {
        return;
    };
    commands.trigger(TimelineAction::SelectMarker(marker.marker));
    click.propagate(false);
}

pub(super) fn select_choreography_event(
    mut click: On<PointerClick>,
    events: Query<&TimelineChoreographyEvent>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(event) = events.get(click.event_target()) else {
        return;
    };
    commands.trigger(TimelineAction::SelectChoreographyEvent(event.event));
    click.propagate(false);
}

pub(super) fn select_timeline_clip(
    click: On<PointerClick>,
    actions: Query<&ChoreographyAction, With<TimelineClipInteraction>>,
    mut commands: Commands,
) {
    let Ok(action) = actions.get(click.event_target()) else {
        return;
    };
    if click.button == PointerButton::Primary {
        commands.trigger(action.clone());
    }
}

pub(super) fn select_timeline_effect_clip(
    click: On<PointerClick>,
    actions: Query<
        (&ChoreographyAction, &TimelineEffectClipInteraction),
        With<TimelineEffectClipControl>,
    >,
    mut commands: Commands,
) {
    let Ok((action, interaction)) = actions.get(click.event_target()) else {
        return;
    };
    if click.button == PointerButton::Primary {
        commands.trigger(effect_clip_click_action(click.count, interaction, action));
    }
}

pub(super) fn effect_clip_click_action(
    click_count: u8,
    interaction: &TimelineEffectClipInteraction,
    selection: &ChoreographyAction,
) -> ChoreographyAction {
    if click_count >= 2 && interaction.kind == TimelineDragKind::Move {
        ChoreographyAction::EditEffectClipSource(interaction.clip)
    } else {
        selection.clone()
    }
}

pub(super) fn open_effect_clip_source_from_header(
    click: On<PointerClick>,
    headers: Query<&EffectClipTrackHeader>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary || click.count < 2 {
        return;
    }
    let Ok(header) = headers.get(click.event_target()) else {
        return;
    };
    commands.trigger(ChoreographyAction::EditEffectClipSource(header.clip));
}

pub(super) fn open_timeline_track_context_menu(
    mut click: On<PointerClick>,
    headers: Query<(&EmitterTrackHeader, &ComputedNode, &UiGlobalTransform)>,
    parents: Query<&ChildOf>,
    mut session: ResMut<EditorSession>,
    mut curves: ResMut<CurvesState>,
    mut state: ResMut<TimelineState>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let mut entity = click.event_target();
    let (emitter, position) = loop {
        if let Ok((header, node, transform)) = headers.get(entity) {
            break (
                header.emitter,
                pointer_position_in_node(
                    click.timeline_pointer_location().position,
                    node,
                    transform,
                ),
            );
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let revision = session.ui_revision;
    if session
        .effect
        .emitters
        .iter()
        .any(|candidate| candidate.id == emitter)
    {
        if !state.selected_emitters.contains(&emitter) {
            state.select_only_emitter(emitter);
        }
        session.select_emitter(emitter);
        curves.clear();
    }
    state.color_picker_emitter = None;
    state.automation_menu_emitter = None;
    state.context_effect_clip = None;
    state.context_emitter = Some(emitter);
    state.context_menu_position = position;
    if session.ui_revision == revision {
        session.ui_revision += 1;
    }
    click.propagate(false);
}

pub(super) fn open_focused_timeline_context_menu(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    active_descendants: Query<&ActiveDescendant>,
    emitter_headers: Query<(&EmitterTrackHeader, &ComputedNode)>,
    clip_headers: Query<(&EffectClipTrackHeader, &ComputedNode)>,
    parents: Query<&ChildOf>,
    mut session: ResMut<EditorSession>,
    mut curves: ResMut<CurvesState>,
    mut state: ResMut<TimelineState>,
) {
    if !keyboard_context_menu_requested(&keys)
        || state.context_emitter.is_some()
        || state.context_effect_clip.is_some()
    {
        return;
    }
    let Some(mut entity) = focus.as_deref().and_then(InputFocus::get) else {
        return;
    };
    if let Ok(active) = active_descendants.get(entity)
        && let Some(descendant) = active.0
    {
        entity = descendant;
    }
    loop {
        if let Ok((header, node)) = emitter_headers.get(entity) {
            let revision = session.ui_revision;
            if session
                .effect
                .emitters
                .iter()
                .any(|candidate| candidate.id == header.emitter)
            {
                if !state.selected_emitters.contains(&header.emitter) {
                    state.select_only_emitter(header.emitter);
                }
                session.select_emitter(header.emitter);
                curves.clear();
            }
            state.color_picker_emitter = None;
            state.automation_menu_emitter = None;
            state.context_effect_clip = None;
            state.context_emitter = Some(header.emitter);
            state.context_menu_position =
                Vec2::new((node.size().x - 8.0).max(0.0), node.size().y * 0.5);
            if session.ui_revision == revision {
                session.ui_revision += 1;
            }
            return;
        }
        if let Ok((header, node)) = clip_headers.get(entity) {
            let revision = session.ui_revision;
            if session
                .effect
                .effect_clips
                .iter()
                .any(|candidate| candidate.id == header.clip)
            {
                state.clear_emitter_selection();
                session.select_effect_clip(header.clip);
                curves.clear();
            }
            state.inspected_child = None;
            state.context_emitter = None;
            state.color_picker_emitter = None;
            state.automation_menu_emitter = None;
            state.context_effect_clip = Some(header.clip);
            state.context_menu_position =
                Vec2::new((node.size().x - 8.0).max(0.0), node.size().y * 0.5);
            if session.ui_revision == revision {
                session.ui_revision += 1;
            }
            return;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    }
}

pub(super) fn open_effect_clip_track_context_menu(
    mut click: On<PointerClick>,
    headers: Query<(&EffectClipTrackHeader, &ComputedNode, &UiGlobalTransform)>,
    parents: Query<&ChildOf>,
    mut session: ResMut<EditorSession>,
    mut curves: ResMut<CurvesState>,
    mut state: ResMut<TimelineState>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let mut entity = click.event_target();
    let (clip, position) = loop {
        if let Ok((header, node, transform)) = headers.get(entity) {
            break (
                header.clip,
                pointer_position_in_node(
                    click.timeline_pointer_location().position,
                    node,
                    transform,
                ),
            );
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let revision = session.ui_revision;
    if session
        .effect
        .effect_clips
        .iter()
        .any(|candidate| candidate.id == clip)
    {
        state.clear_emitter_selection();
        session.select_effect_clip(clip);
        curves.clear();
    }
    state.inspected_child = None;
    state.context_emitter = None;
    state.color_picker_emitter = None;
    state.automation_menu_emitter = None;
    state.context_effect_clip = Some(clip);
    state.context_menu_position = position;
    if session.ui_revision == revision {
        session.ui_revision += 1;
    }
    click.propagate(false);
}

pub(super) fn configure_timeline_track_action_control(mut commands: Commands, entity: Entity) {
    commands
        .entity(entity)
        .remove::<TimelineButton>()
        .remove::<FeathersActionButton>()
        .insert((TimelineTrackActionControl, EditorNativeControl))
        .observe(activate_timeline_track_action_control);
}

pub(super) fn activate_timeline_track_action_control(
    mut click: On<PointerClick>,
    controls: Query<&ChoreographyAction, With<TimelineTrackActionControl>>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(action) = controls.get(click.event_target()) else {
        return;
    };
    commands.trigger(action.clone());
    click.propagate(false);
}

pub(super) fn collapse_emitter_multi_selection_on_primary_click(
    click: On<PointerClick>,
    headers: Query<&EmitterTrackHeader>,
    parents: Query<&ChildOf>,
    action_controls: Query<
        (),
        Or<(
            With<TimelineTrackActionControl>,
            With<EmitterTrackReorderHandle>,
        )>,
    >,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<TimelineState>,
    mut session: ResMut<EditorSession>,
    mut curves: ResMut<CurvesState>,
) {
    let target_is_action_control = action_controls.contains(click.event_target());
    if click.button != PointerButton::Primary || target_is_action_control {
        return;
    }
    let mut entity = click.event_target();
    let emitter = loop {
        if let Ok(header) = headers.get(entity) {
            break header.emitter;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if !should_collapse_emitter_multi_selection(
        click.button,
        target_is_action_control,
        control,
        shift,
        state.selected_emitters.len(),
        session.selection.emitter(&session.effect),
        emitter,
    ) {
        return;
    }

    state.select_only_emitter(emitter);
    session.select_emitter(emitter);
    session.ui_revision += 1;
    curves.clear();
}

pub(super) fn should_collapse_emitter_multi_selection(
    button: PointerButton,
    target_is_action_control: bool,
    control: bool,
    shift: bool,
    selected_count: usize,
    primary: Option<EmitterId>,
    clicked: EmitterId,
) -> bool {
    button == PointerButton::Primary
        && !target_is_action_control
        && !control
        && !shift
        && selected_count > 1
        && primary == Some(clicked)
}

pub(super) fn open_timeline_region_context_menu(
    mut click: On<PointerClick>,
    targets: Query<&TimelineClipInteraction>,
    headers: Query<(&EmitterTrackHeader, &ComputedNode, &UiGlobalTransform)>,
    mut session: ResMut<EditorSession>,
    mut curves: ResMut<CurvesState>,
    mut state: ResMut<TimelineState>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let Ok(target) = targets.get(click.event_target()) else {
        return;
    };
    let selection = (target.emitter, target.region);
    if !state.selected_emitter_regions.contains(&selection) {
        state.select_only_emitter_region(target.emitter, target.region);
    } else {
        state.selected_emitter_region = Some(selection);
        state.emitter_region_selection_anchor = Some(selection);
    }
    session.select_emitter_region(target.emitter, target.region);
    curves.clear();

    let position = headers
        .iter()
        .find(|(header, _, _)| header.emitter == target.emitter)
        .map_or(Vec2::ZERO, |(_, node, transform)| {
            pointer_position_in_node(click.timeline_pointer_location().position, node, transform)
        });
    state.color_picker_emitter = None;
    state.automation_menu_emitter = None;
    state.context_effect_clip = None;
    state.context_emitter = Some(target.emitter);
    state.context_menu_position = position;
    session.ui_revision += 1;
    click.propagate(false);
}

pub(super) fn referenced_emitter_click_action(
    state: &mut TimelineState,
    now: Duration,
    click_count: u8,
    selection: &ChoreographyAction,
) -> ChoreographyAction {
    let ChoreographyAction::SelectEffectClipEmitter { path, emitter } = selection else {
        state.referenced_emitter_click = None;
        return selection.clone();
    };
    let repeated = state
        .referenced_emitter_click
        .as_ref()
        .is_some_and(|previous| {
            previous.path == *path
                && previous.emitter == *emitter
                && now.saturating_sub(previous.at) <= REFERENCED_EMITTER_DOUBLE_CLICK_INTERVAL
        });
    if click_count >= 2 || repeated {
        state.referenced_emitter_click = None;
        ChoreographyAction::EditEffectClipEmitterSource {
            path: path.clone(),
            emitter: *emitter,
        }
    } else {
        state.referenced_emitter_click = Some(ReferencedEmitterClick {
            path: path.clone(),
            emitter: *emitter,
            at: now,
        });
        selection.clone()
    }
}

pub(super) fn select_timeline_referenced_emitter(
    click: On<PointerClick>,
    actions: Query<&ChoreographyAction, With<TimelineReferencedEmitterControl>>,
    mut state: ResMut<TimelineState>,
    time: Res<Time<Real>>,
    mut commands: Commands,
) {
    let Ok(action) = actions.get(click.event_target()) else {
        return;
    };
    if click.button == PointerButton::Primary {
        commands.trigger(referenced_emitter_click_action(
            &mut state,
            time.elapsed(),
            click.count,
            action,
        ));
    }
}

pub(super) fn open_referenced_emitter_source_from_header(
    click: On<PointerClick>,
    headers: Query<&ChoreographyAction, With<ReferencedEmitterTrackHeader>>,
    mut state: ResMut<TimelineState>,
    time: Res<Time<Real>>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(action) = headers.get(click.event_target()) else {
        return;
    };
    let resolved = referenced_emitter_click_action(&mut state, time.elapsed(), click.count, action);
    if matches!(
        resolved,
        ChoreographyAction::EditEffectClipEmitterSource { .. }
    ) {
        commands.trigger(resolved);
    }
}

#[cfg(test)]
#[path = "tests/clicks.rs"]
mod tests;
