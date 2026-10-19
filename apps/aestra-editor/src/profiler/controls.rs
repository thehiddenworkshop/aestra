//! Production Profiler activation routing and peak/history reset action.
use super::{ProfilerState, control_input};
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
use crate::{EditorSession, Localizer};
use bevy::{prelude::*, ui_widgets::Activate};

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProfilerAction {
    ResetPeaks,
}

pub(super) struct Control {
    pub(super) entity: Entity,
    pub(super) pressed: bool,
    pub(super) action: ProfilerAction,
    pub(super) pending: bool,
}

pub(super) fn queue_profiler_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<ProfilerAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        control_input::queue_pressed(&mut commands, activate.entity);
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn handle_profiler_actions(
    mut commands: Commands,
    mut actions: control_input::Controls,
    mut profiler: ResMut<ProfilerState>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    for Control {
        entity,
        pressed,
        action,
        pending,
    } in actions.iter()
    {
        if !pressed || !pending {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>();
        control_input::clear_pressed(&mut commands, entity);
        match action {
            ProfilerAction::ResetPeaks => {
                profiler.reset_peaks();
                session.status = localizer.text("profiler-reset-status");
            }
        }
    }
}
