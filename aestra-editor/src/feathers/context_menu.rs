//! Pointer-anchored context menus shared by editor panels.

use super::{button::FeathersActionButton, scenes};
use crate::theme;
use bevy::{
    feathers::theme::ThemedText,
    input_focus::tab_navigation::NavAction,
    picking::events::{Click, Pointer},
    prelude::*,
    ui::RelativeCursorPosition,
    ui_widgets::{
        MenuFocusState, MenuPopup,
        popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
    },
};

pub(crate) const POINTER_CONTEXT_MENU_WIDTH: f32 = 184.0;

#[derive(Component)]
pub(crate) struct PointerContextMenuAnchor;

#[derive(Component)]
pub(crate) struct PointerContextMenuSurface;

#[derive(Component)]
pub(crate) struct PointerContextMenuItem;

/// Spawns a window-clamped menu at a position local to `parent`.
pub(crate) fn spawn_pointer_context_menu<A: Bundle, M: Bundle>(
    parent: &mut ChildSpawnerCommands,
    position: Vec2,
    anchor_marker: A,
    marker: M,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            PointerContextMenuAnchor,
            anchor_marker,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(position.x),
                top: Val::Px(position.y),
                width: Val::Px(1.0),
                height: Val::Px(1.0),
                ..default()
            },
        ))
        .with_children(|anchor| {
            anchor
                .spawn((
                    PointerContextMenuSurface,
                    marker,
                    Pickable::default(),
                    MenuPopup::default(),
                    MenuFocusState::Opening(NavAction::First),
                    Popover {
                        positions: vec![
                            PopoverPlacement {
                                side: PopoverSide::Right,
                                align: PopoverAlign::Start,
                                gap: 4.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Left,
                                align: PopoverAlign::Start,
                                gap: 4.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Bottom,
                                align: PopoverAlign::Start,
                                gap: 4.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Top,
                                align: PopoverAlign::Start,
                                gap: 4.0,
                            },
                        ],
                        window_margin: 8.0,
                    },
                    RelativeCursorPosition::default(),
                    OverrideClip,
                    GlobalZIndex(250),
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(POINTER_CONTEXT_MENU_WIDTH),
                        padding: UiRect::axes(Val::Px(0.0), Val::Px(4.0)),
                        flex_direction: FlexDirection::Column,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(theme::MENU),
                    BorderColor::all(theme::BORDER_BRIGHT),
                    BoxShadow::new(
                        Color::srgba(0.0, 0.0, 0.0, 0.62),
                        Val::Px(0.0),
                        Val::Px(2.0),
                        Val::Px(3.0),
                        Val::Px(5.0),
                    ),
                ))
                .with_children(build);
        });
}

pub(crate) fn spawn_pointer_context_menu_item<A: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: A,
) {
    spawn_pointer_context_menu_custom_item(parent, label, action, |item| {
        item.spawn((
            Text::new(label),
            ThemedText,
            TextLayout::no_wrap(),
            Pickable::IGNORE,
        ));
    });
}

pub(crate) fn spawn_pointer_context_menu_custom_item<A: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: A,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn_empty()
        .apply_scene(scenes::feathers_menu_item())
        .insert((
            PointerContextMenuItem,
            Pickable::default(),
            Interaction::None,
            action,
            FeathersActionButton,
            AccessibleLabel(label.to_owned()),
        ))
        .observe(stop_pointer_context_menu_click_propagation)
        .with_children(build);
}

fn stop_pointer_context_menu_click_propagation(mut click: On<Pointer<Click>>) {
    click.propagate(false);
}

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

pub(crate) fn should_dismiss_pointer_context_menu(
    open: bool,
    primary_pressed: bool,
    escape_pressed: bool,
    pointer_over_surface: bool,
) -> bool {
    open && (escape_pressed || (primary_pressed && !pointer_over_surface))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_menu_dismisses_for_escape_or_primary_click_outside() {
        assert!(should_dismiss_pointer_context_menu(true, false, true, true));
        assert!(should_dismiss_pointer_context_menu(
            true, true, false, false
        ));
        assert!(!should_dismiss_pointer_context_menu(
            true, true, false, true
        ));
        assert!(!should_dismiss_pointer_context_menu(
            false, true, true, false
        ));
    }

    #[test]
    fn context_menu_keyboard_shortcuts_include_menu_key_and_shift_f10() {
        let mut menu_key = ButtonInput::default();
        menu_key.press(KeyCode::ContextMenu);
        assert!(keyboard_context_menu_requested(&menu_key));

        let mut shifted_f10 = ButtonInput::default();
        shifted_f10.press(KeyCode::ShiftLeft);
        shifted_f10.press(KeyCode::F10);
        assert!(keyboard_context_menu_requested(&shifted_f10));

        let mut plain_f10 = ButtonInput::default();
        plain_f10.press(KeyCode::F10);
        assert!(!keyboard_context_menu_requested(&plain_f10));
    }
}
