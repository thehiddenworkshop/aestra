//! Native native marker transitions, including removal, without deprecated Interaction.
use crate::material_graph::{
    MaterialGraphAction, appearance::Phase, node_controls::MaterialGraphNodeScope,
};
pub(super) use bevy::ui_widgets::Button as NodeButton;
use bevy::{ecs::system::SystemParam, picking::hover::Hovered, prelude::*, ui::Pressed};
#[derive(SystemParam)]
#[allow(
    clippy::type_complexity,
    reason = "Original material node background query with native state markers"
)]
pub(super) struct Controls<'w, 's> {
    query: Query<
        'w,
        's,
        (
            Entity,
            Ref<'static, MaterialGraphAction>,
            Option<Ref<'static, Pressed>>,
            Option<Ref<'static, Hovered>>,
            Option<&'static MaterialGraphNodeScope>,
            &'static mut BackgroundColor,
        ),
        With<NodeButton>,
    >,
    removed_pressed: RemovedComponents<'w, 's, Pressed>,
    removed_hovered: RemovedComponents<'w, 's, Hovered>,
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
        let removed: std::collections::HashSet<_> = self
            .removed_pressed
            .read()
            .chain(self.removed_hovered.read())
            .collect();
        self.query.iter_mut().filter_map(
            move |(entity, action, pressed, hovered, scope, background)| {
                let changed = action.is_added()
                    || background.is_added()
                    || removed.contains(&entity)
                    || pressed.as_ref().is_some_and(|s| s.is_changed())
                    || hovered.as_ref().is_some_and(|s| s.is_changed());
                changed.then(|| {
                    (
                        if pressed.is_some() {
                            Phase::Pressed
                        } else if hovered.as_ref().is_some_and(|s| s.0) {
                            Phase::Hovered
                        } else {
                            Phase::None
                        },
                        action.into_inner(),
                        scope,
                        background,
                    )
                })
            },
        )
    }
}
#[cfg(test)]
pub(super) fn set_phase(world: &mut World, entity: Entity, phase: Phase) {
    let mut entity = world.entity_mut(entity);
    entity.remove::<Pressed>();
    entity.insert(Hovered(!matches!(phase, Phase::None)));
    if matches!(phase, Phase::Pressed) {
        entity.insert(Pressed);
    }
}
#[cfg(test)]
pub(super) fn clear_hover(world: &mut World, entity: Entity) {
    world.entity_mut(entity).remove::<Hovered>();
}
