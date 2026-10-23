//! Native native action and socket markers, including state removal; no Interaction.
use super::{composition::FeathersGraphSocket, node_controls::SocketPhase};
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
use bevy::{ecs::system::SystemParam, picking::hover::Hovered, prelude::*, ui::Pressed};

#[derive(SystemParam)]
#[allow(
    clippy::type_complexity,
    reason = "Original pin/collapse query with native pressed state"
)]
pub(super) struct Controls<'w, 's, A: Component> {
    query: Query<
        'w,
        's,
        (
            Entity,
            &'static A,
            Option<&'static PendingFeathersActivation>,
        ),
        (
            Or<(Changed<Pressed>, Added<PendingFeathersActivation>)>,
            With<FeathersActionButton>,
        ),
    >,
}
impl<A: Component> Controls<'_, '_, A> {
    pub(super) fn iter(&self) -> impl Iterator<Item = (Entity, bool, &A, bool)> {
        self.query
            .iter()
            .map(|(entity, action, pending)| (entity, true, action, pending.is_some()))
    }
}
pub(super) fn clear_pressed(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).remove::<Pressed>();
}
#[derive(SystemParam)]
#[allow(
    clippy::type_complexity,
    reason = "Original socket query with native marker transitions"
)]
pub(super) struct SocketControls<'w, 's> {
    query: Query<
        'w,
        's,
        (
            Entity,
            Ref<'static, FeathersGraphSocket>,
            Option<Ref<'static, Hovered>>,
            Option<Ref<'static, Pressed>>,
            &'static Children,
        ),
    >,
    removed_hovered: RemovedComponents<'w, 's, Hovered>,
    removed_pressed: RemovedComponents<'w, 's, Pressed>,
}
impl SocketControls<'_, '_> {
    pub(super) fn iter(
        &mut self,
    ) -> impl Iterator<Item = (SocketPhase, &FeathersGraphSocket, &Children)> {
        let removed: std::collections::HashSet<_> = self
            .removed_hovered
            .read()
            .chain(self.removed_pressed.read())
            .collect();
        self.query
            .iter()
            .filter_map(move |(entity, socket, hovered, pressed, children)| {
                let changed = socket.is_added()
                    || removed.contains(&entity)
                    || hovered.as_ref().is_some_and(|s| s.is_changed())
                    || pressed.as_ref().is_some_and(|s| s.is_changed());
                changed.then(|| {
                    (
                        if pressed.is_some() {
                            SocketPhase::Pressed
                        } else if hovered.as_ref().is_some_and(|s| s.0) {
                            SocketPhase::Hovered
                        } else {
                            SocketPhase::None
                        },
                        socket.into_inner(),
                        children,
                    )
                })
            })
    }
}
#[cfg(test)]
pub(super) fn set_phase(world: &mut World, entity: Entity, phase: SocketPhase) {
    let mut entity = world.entity_mut(entity);
    entity.remove::<Pressed>();
    entity.insert(Hovered(!matches!(phase, SocketPhase::None)));
    if matches!(phase, SocketPhase::Pressed) {
        entity.insert(Pressed);
    }
}
#[cfg(test)]
pub(super) fn clear_hover(world: &mut World, entity: Entity) {
    world.entity_mut(entity).remove::<Hovered>();
}
#[cfg(test)]
pub(super) fn release_pressed(world: &mut World, entity: Entity) {
    world.entity_mut(entity).remove::<Pressed>();
}
#[cfg(test)]
pub(super) fn is_pressed(world: &World, entity: Entity) -> bool {
    world.get::<Pressed>(entity).is_some()
}
#[cfg(test)]
pub(super) fn clear_hover_while_pressed(world: &mut World, entity: Entity) {
    world.entity_mut(entity).remove::<Hovered>();
}
