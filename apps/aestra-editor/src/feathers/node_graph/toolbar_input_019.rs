//! Native frame-control pressed-state query and consumption only.
use super::toolbar::GraphFrameAction;
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
use bevy::{ecs::system::SystemParam, prelude::*};
#[derive(SystemParam)]
#[allow(clippy::type_complexity, reason = "Original frame-control query")]
pub(super) struct Controls<'w, 's> {
    query: Query<
        'w,
        's,
        (
            Entity,
            &'static Interaction,
            &'static GraphFrameAction,
            Option<&'static PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
}
impl Controls<'_, '_> {
    pub(super) fn iter(&self) -> impl Iterator<Item = (Entity, bool, &GraphFrameAction, bool)> {
        self.query
            .iter()
            .map(|(e, i, a, p)| (e, *i == Interaction::Pressed, a, p.is_some()))
    }
}
pub(super) fn clear_pressed(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).insert(Interaction::None);
}
#[cfg(test)]
pub(super) fn set_pressed(world: &mut World, entity: Entity) {
    world.entity_mut(entity).insert(Interaction::Pressed);
}
