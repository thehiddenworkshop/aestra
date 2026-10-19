//! Version-specific change/consume boundary for material graph action controls.
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
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
            &'static Interaction,
            &'static A,
            Option<&'static PendingFeathersActivation>,
        ),
        (Changed<Interaction>, With<FeathersActionButton>, F),
    >,
}
impl<A: Component, F: QueryFilter + 'static> Controls<'_, '_, A, F> {
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

pub(super) fn remove_legacy_text_clip(commands: &mut Commands, entity: Entity) {
    commands
        .entity(entity)
        .remove::<bevy::ui::widget::TextScroll>();
}
#[cfg(test)]
pub(super) fn add_legacy_text_clip(world: &mut World, entity: Entity) {
    world
        .entity_mut(entity)
        .insert(bevy::ui::widget::TextScroll::default());
}
#[cfg(test)]
pub(super) fn legacy_text_clip_absent(world: &World, entity: Entity) -> bool {
    world.get::<bevy::ui::widget::TextScroll>(entity).is_none()
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
