//! Editor action buttons and the bridge from Feathers activation to Aestra actions.

use super::{action_activation::queue_activation, button_activation::AuditedButton, scenes};
use crate::EditorAction;
#[path = "button/tool.rs"]
mod tool;
use bevy::{prelude::*, ui_widgets::Activate};
#[path = "button/action.rs"]
mod action;
pub(crate) use action::spawn_action_button;
pub(crate) use tool::spawn_tool_button;

pub(crate) use super::action_activation::{
    EditorNativeControl, FeathersActionButton, PendingFeathersActivation,
};

pub(crate) type UnclassifiedEditorActionControl = (
    Added<EditorAction>,
    With<AuditedButton>,
    Without<FeathersActionButton>,
    Without<EditorNativeControl>,
);

pub(crate) fn queue_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<FeathersActionButton>, With<EditorAction>)>,
    commands: Commands,
) {
    queue_activation(activate, actions, commands);
}

pub(crate) fn audit_action_controls(controls: Query<Entity, UnclassifiedEditorActionControl>) {
    if cfg!(debug_assertions)
        && let Some(entity) = controls.iter().next()
    {
        panic!(
            "editor action control {entity:?} must use FeathersActionButton or be explicitly \
             marked EditorNativeControl"
        );
    }
}
