//! Existing application action dispatcher still consumes the legacy pressed state.
pub(super) use bevy::prelude::Button as AuditedButton;
pub(super) fn pressed_state() -> impl bevy::prelude::Bundle {
    bevy::prelude::Interaction::Pressed
}

pub(super) fn clear_pressed(entity: &mut bevy::prelude::EntityCommands) {
    entity.insert(bevy::prelude::Interaction::None);
}

#[cfg(test)]
pub(super) fn is_pressed(world: &bevy::prelude::World, entity: bevy::prelude::Entity) -> bool {
    world.get::<bevy::prelude::Interaction>(entity) == Some(&bevy::prelude::Interaction::Pressed)
}
