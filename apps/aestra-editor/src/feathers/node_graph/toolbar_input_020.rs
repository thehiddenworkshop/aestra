//! Native frame-control pressed-state query and consumption only.
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
use crate::feathers::node_graph::toolbar::GraphFrameAction;
use bevy::{ecs::system::SystemParam, prelude::*, ui::Pressed};
#[derive(SystemParam)]
#[allow(
    clippy::type_complexity,
    reason = "Original frame-control query with native pressed marker"
)]
pub(super) struct Controls<'w, 's> {
    query: Query<
        'w,
        's,
        (
            Entity,
            &'static GraphFrameAction,
            Option<&'static PendingFeathersActivation>,
        ),
        (Changed<Pressed>, With<FeathersActionButton>),
    >,
}
impl Controls<'_, '_> {
    pub(super) fn iter(&self) -> impl Iterator<Item = (Entity, bool, &GraphFrameAction, bool)> {
        self.query.iter().map(|(e, a, p)| (e, true, a, p.is_some()))
    }
}
pub(super) fn clear_pressed(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).remove::<Pressed>();
}
#[cfg(test)]
pub(super) fn set_pressed(world: &mut World, entity: Entity) {
    world.entity_mut(entity).insert(Pressed);
}
