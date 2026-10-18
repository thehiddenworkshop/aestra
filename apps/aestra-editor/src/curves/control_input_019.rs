//! Shipping retains the original Interaction change query and activation reset.
use super::{
    CurvesAction,
    controls::{Control, Phase},
};
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
pub(super) use bevy::prelude::Button as NativeButton;
use bevy::{ecs::system::SystemParam, prelude::*};

#[derive(SystemParam)]
#[allow(
    clippy::type_complexity,
    reason = "Original production Curves control query"
)]
pub(super) struct Controls<'w, 's> {
    query: Query<
        'w,
        's,
        (
            Entity,
            &'static Interaction,
            &'static CurvesAction,
            Option<&'static FeathersActionButton>,
            Option<&'static PendingFeathersActivation>,
            &'static mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<NativeButton>, With<FeathersActionButton>)>,
        ),
    >,
}
impl Controls<'_, '_> {
    pub(super) fn iter(&mut self) -> impl Iterator<Item = Control<'_>> {
        self.query.iter_mut().map(
            |(entity, interaction, action, feathers, pending, background)| Control {
                entity,
                phase: match *interaction {
                    Interaction::None => Phase::None,
                    Interaction::Hovered => Phase::Hovered,
                    Interaction::Pressed => Phase::Pressed,
                },
                action: *action,
                feathers: feathers.is_some(),
                pending: pending.is_some(),
                background,
            },
        )
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
pub(super) fn set_phase(world: &mut World, entity: Entity, phase: Phase) {
    world.entity_mut(entity).insert(match phase {
        Phase::None => Interaction::None,
        Phase::Hovered => Interaction::Hovered,
        Phase::Pressed => Interaction::Pressed,
    });
}
#[cfg(test)]
pub(super) fn is_pressed(world: &World, entity: Entity) -> bool {
    world.get::<Interaction>(entity) == Some(&Interaction::Pressed)
}
