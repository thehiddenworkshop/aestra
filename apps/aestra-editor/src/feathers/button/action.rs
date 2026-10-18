//! Production generic Feathers action-button composition.
use crate::feathers::{action_activation::FeathersActionButton, scenes};
use bevy::{feathers::theme::ThemedText, prelude::*};
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
