//! Editor action buttons and the bridge from Feathers activation to Aestra actions.

use super::scenes;
use crate::EditorAction;
use bevy::{feathers::theme::ThemedText, prelude::*, ui_widgets::Activate};

/// Marks a standard editor action implemented by a Bevy Feathers button.
#[derive(Component)]
pub(crate) struct FeathersActionButton;

/// Marks an intentional editor-native interaction with no Feathers equivalent.
#[derive(Component)]
pub(crate) struct EditorNativeControl;

pub(crate) type UnclassifiedEditorActionControl = (
    Added<EditorAction>,
    With<Button>,
    Without<FeathersActionButton>,
    Without<EditorNativeControl>,
);

#[derive(Component)]
pub(crate) struct PendingFeathersActivation;

pub(crate) fn queue_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<EditorAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
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
