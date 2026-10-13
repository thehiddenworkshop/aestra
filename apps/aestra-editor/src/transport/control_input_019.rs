//! Shipping control input retains its original Interaction change filter.
use crate::transport::{Control, Phase};
use crate::{
    feathers::action_activation::{FeathersActionButton, PendingFeathersActivation},
    transport::TransportAction,
};
pub(crate) use bevy::prelude::Button as AuditedButton;
use bevy::{ecs::system::SystemParam, prelude::*, ui::InteractionDisabled};

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
            &'static Interaction,
            &'static TransportAction,
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
    pub(crate) fn iter(&mut self) -> impl Iterator<Item = Control<'_>> {
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
pub(crate) fn set_phase(world: &mut World, entity: Entity, phase: Phase) {
    world.entity_mut(entity).insert(match phase {
        Phase::None => Interaction::None,
        Phase::Hovered => Interaction::Hovered,
        Phase::Pressed => Interaction::Pressed,
    });
}

#[cfg(test)]
pub(crate) fn clear_hover(world: &mut World, entity: Entity) {
    world.entity_mut(entity).insert(Interaction::None);
}

#[cfg(test)]
pub(crate) fn is_pressed(world: &World, entity: Entity) -> bool {
    world.get::<Interaction>(entity) == Some(&Interaction::Pressed)
}

#[cfg(test)]
pub(crate) fn click(world: &mut World, entity: Entity, window: Entity) {
    use bevy::{
        camera::RenderTarget,
        picking::{
            backend::HitData,
            events::{Click, Pointer},
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
    world.trigger(Pointer::new(
        PointerId::Mouse,
        location,
        Click {
            button: PointerButton::Primary,
            hit,
            duration: Duration::ZERO,
            count: 1,
        },
        entity,
    ));
}
