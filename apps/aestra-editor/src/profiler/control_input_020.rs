//! Candidate maps changed native pressed markers to the original profiler action contract.
use super::controls::{Control, ProfilerAction};
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
#[cfg(test)]
pub(super) use bevy::ui_widgets::Button as NativeButton;
use bevy::{ecs::system::SystemParam, prelude::*, ui::Pressed};

#[derive(SystemParam)]
#[allow(
    clippy::type_complexity,
    reason = "Version-specific profiler action input query"
)]
pub(super) struct Controls<'w, 's> {
    query: Query<
        'w,
        's,
        (
            Entity,
            &'static ProfilerAction,
            Option<&'static PendingFeathersActivation>,
        ),
        (Changed<Pressed>, With<FeathersActionButton>),
    >,
}
impl Controls<'_, '_> {
    pub(super) fn iter(&mut self) -> impl Iterator<Item = Control> {
        self.query
            .iter_mut()
            .map(|(entity, action, pending)| Control {
                entity,
                pressed: true,
                action: *action,
                pending: pending.is_some(),
            })
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
    if pressed {
        world.entity_mut(entity).insert(Pressed);
    } else {
        world.entity_mut(entity).remove::<Pressed>();
    }
}
#[cfg(test)]
pub(super) fn is_pressed(world: &World, entity: Entity) -> bool {
    world.get::<Pressed>(entity).is_some()
}
