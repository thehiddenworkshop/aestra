//! Candidate action controls use the native pressed marker, not deprecated Interaction.
pub(super) use bevy::ui_widgets::Button as AuditedButton;
pub(super) fn pressed_state() -> impl bevy::prelude::Bundle {
    bevy::ui::Pressed
}

pub(super) fn clear_pressed(entity: &mut bevy::prelude::EntityCommands) {
    entity.remove::<bevy::ui::Pressed>();
}

#[cfg(test)]
pub(super) fn is_pressed(world: &bevy::prelude::World, entity: bevy::prelude::Entity) -> bool {
    world.get::<bevy::ui::Pressed>(entity).is_some()
}
