//! Production tool-button scene and action classification used throughout the editor.
use super::scenes;
use crate::feathers::action_activation::FeathersActionButton;
use bevy::{feathers::theme::ThemedText, prelude::*};

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
