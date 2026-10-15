//! Candidate uses the equivalent native Pressed insertion/change query.
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
use bevy::ui::Pressed;
use bevy::{ecs::system::SystemParam, prelude::*};
#[derive(SystemParam)]
pub(super) struct Controls<'w, 's, A: Component> {
    query: Query<
        'w,
        's,
        (
            Entity,
            &'static A,
            &'static Pressed,
            Has<PendingFeathersActivation>,
        ),
        (Changed<Pressed>, With<FeathersActionButton>),
    >,
}
impl<A: Component> Controls<'_, '_, A> {
    pub(super) fn iter(&self) -> impl Iterator<Item = (Entity, &A, bool, bool)> {
        self.query
            .iter()
            .map(|(entity, action, _pressed, pending)| (entity, action, true, pending))
    }
}
pub(super) fn queue_pressed(commands: &mut Commands, entity: Entity) {
    commands
        .entity(entity)
        .insert((PendingFeathersActivation, Pressed));
}
pub(super) fn clear_pressed(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).remove::<Pressed>();
}
#[cfg(test)]
pub(super) fn set_pressed(world: &mut World, entity: Entity, pressed: bool) {
    let mut entity = world.entity_mut(entity);
    if pressed {
        entity.insert(Pressed);
    } else {
        entity.remove::<Pressed>();
    }
}
#[cfg(test)]
pub(super) fn is_pressed(world: &World, entity: Entity) -> bool {
    world.get::<Pressed>(entity).is_some()
}
