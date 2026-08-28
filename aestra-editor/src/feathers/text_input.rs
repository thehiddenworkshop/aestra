//! Styled single-line text input composition and value-change bridge.

use super::scenes;
use bevy::{
    ecs::relationship::Relationship,
    feathers::controls::{FeathersTextInput, FeathersTextInputContainer},
    input::{ButtonState, keyboard::KeyboardInput},
    input_focus::{FocusLost, FocusedInput},
    prelude::*,
    text::{EditableText, TextEditChange},
    ui_widgets::ValueChange,
};

pub(crate) fn spawn_text_input<M: Bundle>(
    parent: &mut ChildSpawnerCommands,
    initial_value: &str,
    accessible_label: &str,
    marker: M,
) -> Entity {
    let mut container = parent.spawn_empty();
    let entity = container.id();
    container
        .apply_scene(scenes::feathers_text_input_container())
        .insert((marker, AccessibleLabel(accessible_label.to_owned())))
        .with_children(|container| {
            container
                .spawn_empty()
                .apply_scene(scenes::feathers_text_input())
                .insert((
                    EditableText::new(initial_value),
                    AccessibleLabel(accessible_label.to_owned()),
                ));
        });
    entity
}

pub(crate) fn emit_text_change(
    change: On<TextEditChange>,
    parents: Query<&ChildOf>,
    containers: Query<(), With<FeathersTextInputContainer>>,
    inputs: Query<&EditableText, With<FeathersTextInput>>,
    mut commands: Commands,
) {
    let Ok(parent) = parents.get(change.event_target()) else {
        return;
    };
    if !containers.contains(parent.get()) {
        return;
    }
    let Ok(input) = inputs.get(change.event_target()) else {
        return;
    };
    commands.trigger(ValueChange {
        source: parent.get(),
        value: input.value().to_string(),
        is_final: false,
    });
}

pub(crate) fn submit_text_on_enter(
    input: On<FocusedInput<KeyboardInput>>,
    parents: Query<&ChildOf>,
    containers: Query<(), With<FeathersTextInputContainer>>,
    texts: Query<&EditableText, With<FeathersTextInput>>,
    mut commands: Commands,
) {
    if input.input.state != ButtonState::Pressed || input.input.key_code != KeyCode::Enter {
        return;
    }
    submit_text(
        input.event_target(),
        &parents,
        &containers,
        &texts,
        &mut commands,
    );
}

pub(crate) fn submit_text_on_focus_loss(
    lost: On<FocusLost>,
    parents: Query<&ChildOf>,
    containers: Query<(), With<FeathersTextInputContainer>>,
    texts: Query<&EditableText, With<FeathersTextInput>>,
    mut commands: Commands,
) {
    submit_text(
        lost.event_target(),
        &parents,
        &containers,
        &texts,
        &mut commands,
    );
}

fn submit_text(
    input: Entity,
    parents: &Query<&ChildOf>,
    containers: &Query<(), With<FeathersTextInputContainer>>,
    texts: &Query<&EditableText, With<FeathersTextInput>>,
    commands: &mut Commands,
) {
    let Ok(parent) = parents.get(input) else {
        return;
    };
    if !containers.contains(parent.get()) {
        return;
    }
    let Ok(text) = texts.get(input) else {
        return;
    };
    commands.trigger(ValueChange {
        source: parent.get(),
        value: text.value().to_string(),
        is_final: true,
    });
}
