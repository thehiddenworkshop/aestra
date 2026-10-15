//! Shipping retains the exact Interaction change query.
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
use bevy::{ecs::system::SystemParam, prelude::*};

#[derive(SystemParam)]
pub(super) struct Controls<'w, 's, A: Component> {
    query: Query<
        'w,
        's,
        (
            Entity,
            &'static A,
            &'static Interaction,
            Has<PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
}
impl<A: Component> Controls<'_, '_, A> {
    pub(super) fn iter(&self) -> impl Iterator<Item = (Entity, &A, bool, bool)> {
        self.query
            .iter()
            .map(|(entity, action, interaction, pending)| {
                (
                    entity,
                    action,
                    *interaction == Interaction::Pressed,
                    pending,
                )
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
