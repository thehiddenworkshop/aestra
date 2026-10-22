//! Version-specific change/consume boundary for material graph action controls.
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
use bevy::ui::Pressed;
use bevy::{
    ecs::{query::QueryFilter, system::SystemParam},
    prelude::*,
};

#[derive(SystemParam)]
#[allow(
    clippy::type_complexity,
    reason = "Original material menu input query with version-specific pressed state"
)]
pub(super) struct Controls<'w, 's, A: Component, F: QueryFilter + 'static = ()> {
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
            F,
        ),
    >,
}
impl<A: Component, F: QueryFilter + 'static> Controls<'_, '_, A, F> {
    pub(super) fn iter(&self) -> impl Iterator<Item = (Entity, bool, &A, bool)> {
        self.query
            .iter()
            .map(|(entity, action, pending)| (entity, true, action, pending.is_some()))
    }
}
pub(super) fn clear_pressed(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).remove::<Pressed>();
}

// TextScroll was removed upstream. Keep the containing input/viewport's normal clipping;
// there is no legacy private editable-text clip component to remove in 0.20.
pub(super) fn remove_legacy_text_clip(_commands: &mut Commands, _entity: Entity) {}
#[cfg(test)]
pub(super) fn add_legacy_text_clip(_world: &mut World, _entity: Entity) {}
#[cfg(test)]
pub(super) fn legacy_text_clip_absent(_world: &World, _entity: Entity) -> bool {
    true
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
