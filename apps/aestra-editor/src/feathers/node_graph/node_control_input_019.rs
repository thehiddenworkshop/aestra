//! Native shipping action and socket interaction-state boundaries.
use super::{composition::FeathersGraphSocket, node_controls::SocketPhase};
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
use bevy::{ecs::system::SystemParam, prelude::*};

#[derive(SystemParam)]
#[allow(
    clippy::type_complexity,
    reason = "Original pin/collapse control query"
)]
pub(super) struct Controls<'w, 's, A: Component> {
    query: Query<
        'w,
        's,
        (
            Entity,
            &'static Interaction,
            &'static A,
            Option<&'static PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>),
    >,
}
impl<A: Component> Controls<'_, '_, A> {
    pub(super) fn iter(&self) -> impl Iterator<Item = (Entity, bool, &A, bool)> {
        self.query
            .iter()
            .map(|(entity, interaction, action, pending)| {
                (
                    entity,
                    *interaction == Interaction::Pressed,
                    action,
                    pending.is_some(),
                )
            })
    }
}
pub(super) fn clear_pressed(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).insert(Interaction::None);
}
#[derive(SystemParam)]
pub(super) struct SocketControls<'w, 's> {
    query: Query<
        'w,
        's,
        (
            &'static Interaction,
            &'static FeathersGraphSocket,
            &'static Children,
        ),
        Changed<Interaction>,
    >,
}
impl SocketControls<'_, '_> {
    pub(super) fn iter(
        &mut self,
    ) -> impl Iterator<Item = (SocketPhase, &FeathersGraphSocket, &Children)> {
        self.query.iter().map(|(phase, socket, children)| {
            (
                match *phase {
                    Interaction::None => SocketPhase::None,
                    Interaction::Hovered => SocketPhase::Hovered,
                    Interaction::Pressed => SocketPhase::Pressed,
                },
                socket,
                children,
            )
        })
    }
}
#[cfg(test)]
pub(super) fn set_phase(world: &mut World, entity: Entity, phase: SocketPhase) {
    world.entity_mut(entity).insert(match phase {
        SocketPhase::None => Interaction::None,
        SocketPhase::Hovered => Interaction::Hovered,
        SocketPhase::Pressed => Interaction::Pressed,
    });
}
#[cfg(test)]
pub(super) fn clear_hover(world: &mut World, entity: Entity) {
    world.entity_mut(entity).insert(Interaction::None);
}
#[cfg(test)]
pub(super) fn release_pressed(world: &mut World, entity: Entity) {
    world.entity_mut(entity).insert(Interaction::Hovered);
}
#[cfg(test)]
pub(super) fn is_pressed(world: &World, entity: Entity) -> bool {
    world.get::<Interaction>(entity) == Some(&Interaction::Pressed)
}
#[cfg(test)]
pub(super) fn clear_hover_while_pressed(_world: &mut World, _entity: Entity) {}
