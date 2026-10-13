//! Candidate control input uses native marker transitions, including marker removal.
use crate::transport::{Control, Phase};
use crate::{
    feathers::action_activation::{FeathersActionButton, PendingFeathersActivation},
    transport::TransportAction,
};
pub(crate) use bevy::ui_widgets::Button as AuditedButton;
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
pub(crate) struct Controls<'w, 's> {
    query: Query<
        'w,
        's,
        (
            Entity,
            Ref<'static, TransportAction>,
            Option<Ref<'static, Pressed>>,
            Option<Ref<'static, Hovered>>,
            Has<FeathersActionButton>,
            Has<PendingFeathersActivation>,
            Has<InteractionDisabled>,
            &'static mut BackgroundColor,
        ),
        Or<(With<AuditedButton>, With<FeathersActionButton>)>,
    >,
    removed_pressed: RemovedComponents<'w, 's, Pressed>,
    removed_hovered: RemovedComponents<'w, 's, Hovered>,
}
impl Controls<'_, '_> {
    pub(crate) fn iter(&mut self) -> impl Iterator<Item = Control<'_>> {
        let removed: std::collections::HashSet<_> = self
            .removed_pressed
            .read()
            .chain(self.removed_hovered.read())
            .collect();
        self.query.iter_mut().filter_map(
            move |(entity, action, pressed, hovered, feathers, pending, disabled, background)| {
                let changed = action.is_added()
                    || background.is_added()
                    || removed.contains(&entity)
                    || pressed.as_ref().is_some_and(|state| state.is_changed())
                    || hovered.as_ref().is_some_and(|state| state.is_changed());
                changed.then(|| Control {
                    entity,
                    phase: if pressed.is_some() {
                        Phase::Pressed
                    } else if hovered.as_ref().is_some_and(|state| state.0) {
                        Phase::Hovered
                    } else {
                        Phase::None
                    },
                    action: *action,
                    feathers,
                    pending,
                    disabled,
                    background,
                })
            },
        )
    }
}
#[cfg(test)]
pub(crate) fn set_phase(world: &mut World, entity: Entity, phase: Phase) {
    let mut entity = world.entity_mut(entity);
    entity.remove::<Pressed>();
    entity.insert(Hovered(phase != Phase::None));
    if phase == Phase::Pressed {
        entity.insert(Pressed);
    }
}

#[cfg(test)]
pub(crate) fn clear_hover(world: &mut World, entity: Entity) {
    world.entity_mut(entity).remove::<Hovered>();
}

#[cfg(test)]
pub(crate) fn is_pressed(world: &World, entity: Entity) -> bool {
    world.get::<Pressed>(entity).is_some()
}

#[cfg(test)]
pub(crate) fn click(world: &mut World, entity: Entity, window: Entity) {
    use bevy::{
        camera::RenderTarget,
        picking::{
            backend::HitData,
            events::{Pointer, PointerClick},
            pointer::{Location, PointerButton, PointerId},
        },
        window::WindowRef,
    };
    use std::time::Duration;
    let location = Location {
        target: RenderTarget::Window(WindowRef::Entity(window))
            .normalize(None)
            .unwrap(),
        position: Vec2::ZERO,
    };
    let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
    world.trigger(PointerClick {
        entity,
        pointer: Pointer::new(PointerId::Mouse, location),
        button: PointerButton::Primary,
        hit,
        duration: Duration::ZERO,
        count: 1,
    });
}
