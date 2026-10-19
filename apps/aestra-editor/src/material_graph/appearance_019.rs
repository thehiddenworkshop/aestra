//! Native shipping interaction change filter; selection/background policy remains shared.
use crate::material_graph::{
    MaterialGraphAction, appearance::Phase, node_controls::MaterialGraphNodeScope,
};
pub(super) use bevy::prelude::Button as NodeButton;
use bevy::{ecs::system::SystemParam, prelude::*};
#[derive(SystemParam)]
#[allow(
    clippy::type_complexity,
    reason = "Original material node background query"
)]
pub(super) struct Controls<'w, 's> {
    query: Query<
        'w,
        's,
        (
            &'static Interaction,
            &'static MaterialGraphAction,
            Option<&'static MaterialGraphNodeScope>,
            &'static mut BackgroundColor,
        ),
        (Changed<Interaction>, With<NodeButton>),
    >,
}
impl Controls<'_, '_> {
    pub(super) fn iter(
        &mut self,
    ) -> impl Iterator<
        Item = (
            Phase,
            &MaterialGraphAction,
            Option<&MaterialGraphNodeScope>,
            Mut<'_, BackgroundColor>,
        ),
    > {
        self.query
            .iter_mut()
            .map(|(phase, action, scope, background)| {
                (
                    match *phase {
                        Interaction::None => Phase::None,
                        Interaction::Hovered => Phase::Hovered,
                        Interaction::Pressed => Phase::Pressed,
                    },
                    action,
                    scope,
                    background,
                )
            })
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
