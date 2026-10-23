//! Native idle combo-item state; the scene installs native Pressed behavior.
pub(super) fn idle_menu_option() -> impl bevy::prelude::Bundle {
    bevy::picking::hover::Hovered(false)
}

#[cfg(test)]
#[test]
fn idle_option_starts_unhovered_and_unpressed() {
    use bevy::{picking::hover::Hovered, prelude::*, ui::Pressed};
    let mut world = World::new();
    let e = world.spawn(idle_menu_option()).id();
    assert!(!world.get::<Hovered>(e).unwrap().0);
    assert!(world.get::<Pressed>(e).is_none());
}
