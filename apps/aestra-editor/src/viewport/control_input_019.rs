//! Shipping preserves its original Interaction change query and pressed/reset state.
use super::{ViewportAction, actions::Control};
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
pub(super) use bevy::prelude::Button as AuditedButton;
use bevy::{ecs::system::SystemParam, prelude::*};

#[derive(SystemParam)]
#[allow(
    clippy::type_complexity,
    reason = "Original viewport action input query"
)]
pub(super) struct Controls<'w, 's> {
    query: Query<
        'w,
        's,
        (
            Entity,
            &'static Interaction,
            &'static ViewportAction,
            Option<&'static PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
}
impl Controls<'_, '_> {
    pub(super) fn iter(&mut self) -> impl Iterator<Item = Control> {
        self.query
            .iter_mut()
            .map(|(entity, interaction, action, pending)| Control {
                entity,
                pressed: *interaction == Interaction::Pressed,
                action: *action,
                pending: pending.is_some(),
            })
    }
}
pub(super) fn queue_pressed(commands: &mut Commands, entity: Entity) {
    commands
        .entity(entity)
        .insert((PendingFeathersActivation, Interaction::Pressed));
}
pub(super) fn clear_pressed(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).insert(Interaction::None);
}
#[cfg(test)]
pub(super) fn set_pressed(world: &mut World, entity: Entity, pressed: bool) {
    world.entity_mut(entity).insert(if pressed {
        Interaction::Pressed
    } else {
        Interaction::None
    });
}
#[cfg(test)]
pub(super) fn is_pressed(world: &World, entity: Entity) -> bool {
    world.get::<Interaction>(entity) == Some(&Interaction::Pressed)
}
