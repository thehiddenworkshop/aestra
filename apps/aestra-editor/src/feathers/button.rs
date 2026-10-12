//! Editor action buttons and the bridge from Feathers activation to Aestra actions.

use super::{action_activation::queue_activation, button_activation::AuditedButton, scenes};
use crate::EditorAction;
use bevy::{feathers::theme::ThemedText, prelude::*, ui_widgets::Activate};

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
    #[cfg(debug_assertions)]
    if let Some(entity) = controls.iter().next() {
        panic!(
            "editor action control {entity:?} must use FeathersActionButton or be explicitly \
             marked EditorNativeControl"
        );
    }
}

pub(crate) fn spawn_action_button<A: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: A,
    primary: bool,
) {
    let mut button = parent.spawn_empty();
    if primary {
        button.apply_scene(scenes::feathers_primary_button());
    } else {
        button.apply_scene(scenes::feathers_button());
    }
    button
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.to_owned()),
        ))
        .with_children(|button| {
            button.spawn((Text::new(label), ThemedText, Pickable::IGNORE));
        });
}

pub(crate) fn spawn_tool_button<A: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: A,
) -> Entity {
    let mut button = parent.spawn_empty();
    button.apply_scene(scenes::feathers_tool_button());
    let entity = button.id();
    button
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.to_owned()),
            Node {
                width: Val::Px(28.0),
                height: Val::Px(24.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((Text::new(label), ThemedText, Pickable::IGNORE));
        });
    entity
}
