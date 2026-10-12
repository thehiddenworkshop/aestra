//! History action identity and activation bridge, independent of session/catalog execution.
use crate::feathers::action_activation::{FeathersActionButton, queue_activation};
use bevy::{prelude::*, ui_widgets::Activate};

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryAction {
    Undo,
    Redo,
}

pub(crate) fn queue_history_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<FeathersActionButton>, With<HistoryAction>)>,
    commands: Commands,
) {
    queue_activation(activate, actions, commands);
}
