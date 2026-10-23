//! Native control input uses native marker transitions, including marker removal.
use super::controls::{Control, Phase};
use crate::{
    feathers::action_activation::{FeathersActionButton, PendingFeathersActivation},
    history::HistoryAction,
};
pub(super) use bevy::ui_widgets::Button as AuditedButton;
use bevy::{
    ecs::system::SystemParam,
    picking::hover::Hovered,
    prelude::*,
    ui::{InteractionDisabled, Pressed},
};

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
            Ref<'static, HistoryAction>,
            Option<Ref<'static, Pressed>>,
            Option<Ref<'static, Hovered>>,
            Has<FeathersActionButton>,
            Option<Ref<'static, PendingFeathersActivation>>,
            Has<InteractionDisabled>,
            &'static mut BackgroundColor,
        ),
        Or<(With<AuditedButton>, With<FeathersActionButton>)>,
    >,
    removed_pressed: RemovedComponents<'w, 's, Pressed>,
    removed_hovered: RemovedComponents<'w, 's, Hovered>,
}
impl Controls<'_, '_> {
    pub(super) fn iter(&mut self) -> impl Iterator<Item = Control<'_>> {
        let removed: std::collections::HashSet<_> = self
            .removed_pressed
            .read()
            .chain(self.removed_hovered.read())
            .collect();
        self.query.iter_mut().filter_map(
            move |(entity, action, pressed, hovered, feathers, pending, disabled, background)| {
                let changed = action.is_added()
                    || pending.as_ref().is_some_and(|pending| pending.is_added())
                    || background.is_added()
                    || removed.contains(&entity)
                    || pressed.as_ref().is_some_and(|state| state.is_changed())
                    || hovered.as_ref().is_some_and(|state| state.is_changed());
                changed.then(|| Control {
                    entity,
                    phase: if pending.is_some() || pressed.is_some() {
                        Phase::Pressed
                    } else if hovered.as_ref().is_some_and(|state| state.0) {
                        Phase::Hovered
                    } else {
                        Phase::None
                    },
                    action: *action,
                    feathers,
                    pending: pending.is_some(),
                    disabled,
                    background,
                })
            },
        )
    }
}
#[cfg(test)]
pub(super) fn set_phase(world: &mut World, entity: Entity, phase: Phase) {
    let mut entity = world.entity_mut(entity);
    entity.remove::<Pressed>();
    entity.insert(Hovered(phase != Phase::None));
    if phase == Phase::Pressed {
        entity.insert(Pressed);
    }
}

#[cfg(test)]
pub(super) fn clear_hover(world: &mut World, entity: Entity) {
    world.entity_mut(entity).remove::<Hovered>();
}
