//! Shared production popup positioning and keyboard request policy.
use bevy::prelude::*;

pub(crate) fn pointer_position_in_node(
    pointer: Vec2,
    node: &ComputedNode,
    transform: &UiGlobalTransform,
) -> Vec2 {
    let top_left = transform.translation.trunc() - node.size() * 0.5;
    pointer - top_left
}

pub(crate) fn keyboard_context_menu_requested(keys: &ButtonInput<KeyCode>) -> bool {
    keys.just_pressed(KeyCode::ContextMenu)
        || (keys.just_pressed(KeyCode::F10)
            && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)))
}
