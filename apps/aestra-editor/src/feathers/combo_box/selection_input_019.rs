//! Shipping idle combo-item state; identical to the former inline initialization.
pub(super) fn idle_menu_option() -> impl bevy::prelude::Bundle {
    bevy::prelude::Interaction::None
}

#[cfg(test)]
#[test]
fn idle_option_starts_with_shipping_neutral_interaction() {
    use bevy::prelude::*;
    let mut world = World::new();
    let e = world.spawn(idle_menu_option()).id();
    assert_eq!(world.get::<Interaction>(e), Some(&Interaction::None));
}
