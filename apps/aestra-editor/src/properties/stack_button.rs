//! Existing compact action-button constructor used by properties and Curves.
use crate::feathers::{action_activation::FeathersActionButton, scenes as ui_shell};
use bevy::{feathers::theme::ThemedText, prelude::*};

pub(crate) fn stack_button<A: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: A,
    width: f32,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_tool_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.to_owned()),
            Node {
                width: Val::Px(width),
                height: Val::Px(21.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((Text::new(label), ThemedText, Pickable::IGNORE));
        });
}
