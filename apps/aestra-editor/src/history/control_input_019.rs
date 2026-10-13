//! Shipping control input retains its original Interaction change filter.
use super::controls::{Control, Phase};
use crate::{
    feathers::action_activation::{FeathersActionButton, PendingFeathersActivation},
    history::HistoryAction,
};
pub(super) use bevy::prelude::Button as AuditedButton;
use bevy::{ecs::system::SystemParam, prelude::*, ui::InteractionDisabled};

#[derive(SystemParam)]
#[allow(
    clippy::type_complexity,
    reason = "Version-specific native control query"
)]
pub(super) struct Controls<'w, 's> {
    query: Query<
        'w,
        's,
        (
            Entity,
            &'static Interaction,
            &'static HistoryAction,
            Has<FeathersActionButton>,
            Has<PendingFeathersActivation>,
            Has<InteractionDisabled>,
            &'static mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<AuditedButton>, With<FeathersActionButton>)>,
        ),
    >,
}
impl Controls<'_, '_> {
    pub(super) fn iter(&mut self) -> impl Iterator<Item = Control<'_>> {
        self.query.iter_mut().map(
            |(entity, interaction, action, feathers, pending, disabled, background)| Control {
                entity,
                phase: match *interaction {
                    Interaction::None => Phase::None,
                    Interaction::Hovered => Phase::Hovered,
                    Interaction::Pressed => Phase::Pressed,
                },
                action: *action,
                feathers,
                pending,
                disabled,
                background,
            },
        )
    }
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
pub(super) fn clear_hover(world: &mut World, entity: Entity) {
    world.entity_mut(entity).insert(Interaction::None);
}
