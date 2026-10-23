//! Production timeline activation dispatch, control audit and track-color preview/commit.
use super::control_input::{Controls, clear_pressed, queue_pressed};
use super::interaction_state::*;
use super::timeline_pointer::TimelineButton;
use super::{ChoreographyAction, TimelineAction};
use crate::feathers::action_activation::{
    EditorNativeControl, FeathersActionButton, PendingFeathersActivation,
};
use crate::feathers::list_row::KeyboardNavigableList;
use crate::{EditorSession, localization::Localizer, menus::MenuState};
use bevy::{
    prelude::*,
    ui_widgets::{Activate, ValueChange},
};

pub(super) fn queue_timeline_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<TimelineAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        queue_pressed(&mut commands, activate.entity);
    }
}

pub(super) fn queue_choreography_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<ChoreographyAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        queue_pressed(&mut commands, activate.entity);
    }
}

pub(super) fn activate_timeline_track_entry(
    change: On<ValueChange<Entity>>,
    lists: Query<(), With<KeyboardNavigableList>>,
    actions: Query<
        &ChoreographyAction,
        Or<(
            With<EmitterTrackHeader>,
            With<EffectClipTrackHeader>,
            With<ReferencedEmitterTrackHeader>,
        )>,
    >,
    mut commands: Commands,
) {
    if lists.contains(change.source)
        && let Ok(action) = actions.get(change.value)
    {
        commands.trigger(action.clone());
    }
}

pub(super) fn handle_timeline_track_color_change(
    change: On<ValueChange<Option<[f32; 4]>>>,
    controls: Query<&EmitterTrackColorPicker>,
    mut swatches: Query<(&EmitterTrackColorSwatch, &mut BackgroundColor)>,
    mut clips: Query<(&TimelineClip, &mut BackgroundColor), Without<EmitterTrackColorSwatch>>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    if let Some([red, green, blue, alpha]) = change.value {
        let preview = Color::srgba(red, green, blue, alpha);
        for (swatch, mut color) in &mut swatches {
            if swatch.emitter == control.emitter {
                color.0 = preview;
            }
        }
        for (clip, mut color) in &mut clips {
            if clip.emitter == control.emitter {
                color.0 = preview;
            }
        }
    }
    if !change.is_final {
        return;
    }
    if session.set_emitter_display_color(control.emitter, change.value) {
        session.status = localizer.text("timeline-emitter-color-updated");
    }
}

pub(super) fn handle_timeline_action_buttons(
    mut commands: Commands,
    buttons: Controls<TimelineAction>,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, action, pressed, pending) in buttons.iter() {
        if !pressed || !pending {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>();
        clear_pressed(&mut commands, entity);
        menu.open = None;
        menu.panels_open = false;
        if menu.tab_context.take().is_some() {
            session.ui_revision += 1;
        }
        commands.trigger(*action);
    }
}

pub(super) fn handle_choreography_action_buttons(
    mut commands: Commands,
    buttons: Controls<ChoreographyAction>,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, action, pressed, pending) in buttons.iter() {
        if !pressed || !pending {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>();
        clear_pressed(&mut commands, entity);
        menu.open = None;
        menu.panels_open = false;
        if menu.tab_context.take().is_some() {
            session.ui_revision += 1;
        }
        commands.trigger(action.clone());
    }
}

pub(super) fn audit_timeline_controls(
    controls: Query<Entity, UnclassifiedTimelineControl>,
    choreography: Query<Entity, UnclassifiedChoreographyControl>,
) {
    if cfg!(debug_assertions)
        && let Some(entity) = controls.iter().next()
    {
        panic!(
            "timeline control {entity:?} must use FeathersActionButton or be explicitly marked \
             EditorNativeControl"
        );
    }
    if cfg!(debug_assertions)
        && let Some(entity) = choreography.iter().next()
    {
        panic!(
            "choreography control {entity:?} must use FeathersActionButton or be explicitly \
             marked EditorNativeControl"
        );
    }
}

type UnclassifiedTimelineControl = (
    Added<TimelineAction>,
    With<TimelineButton>,
    Without<FeathersActionButton>,
    Without<EditorNativeControl>,
);

type UnclassifiedChoreographyControl = (
    Added<ChoreographyAction>,
    With<TimelineButton>,
    Without<FeathersActionButton>,
    Without<EditorNativeControl>,
);

#[cfg(test)]
#[path = "tests/action_controls.rs"]
mod tests;
