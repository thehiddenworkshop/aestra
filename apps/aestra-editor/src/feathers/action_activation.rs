//! Shared Feathers Activate queue and event dispatch; host effects remain in their handlers.
use super::button_activation;
use bevy::{ecs::query::QueryFilter, prelude::*, ui_widgets::Activate};

/// Marks a standard editor action implemented by a Bevy Feathers button.
#[derive(Component)]
pub(crate) struct FeathersActionButton;

/// Marks an intentional editor-native interaction with no Feathers equivalent.
#[derive(Component)]
pub(crate) struct EditorNativeControl;

#[derive(Component)]
pub(crate) struct PendingFeathersActivation;

/// Only explicitly classified actions in the caller's domain are queued.
pub(crate) fn queue_activation<F: QueryFilter>(
    activate: On<Activate>,
    actions: Query<(), (With<FeathersActionButton>, F)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands.entity(activate.entity).insert((
            PendingFeathersActivation,
            button_activation::pressed_state(),
        ));
    }
}

/// Dispatches a domain event after the caller has checked its interaction/availability.
/// Feathers actions require an explicit Activate, not merely a pressed pointer state.
/// Returns whether the host should perform its associated menu/focus updates.
pub(crate) fn dispatch_action<A: Event + Copy>(
    commands: &mut Commands,
    entity: Entity,
    action: A,
    feathers: bool,
    pending: bool,
    disabled: bool,
) -> bool
where
    for<'a> A::Trigger<'a>: Default,
{
    if disabled || (feathers && !pending) {
        return false;
    }
    if feathers {
        let mut entity = commands.entity(entity);
        entity.remove::<PendingFeathersActivation>();
        button_activation::clear_pressed(&mut entity);
    }
    commands.trigger(action);
    true
}

#[cfg(test)]
#[path = "action_activation/tests.rs"]
mod tests;
