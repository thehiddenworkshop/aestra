//! Pointer-anchored context menus shared by editor panels.

use super::{button::FeathersActionButton, scenes};
use crate::theme;
use bevy::{
    feathers::theme::ThemedText,
    input_focus::{
        FocusCause, FocusedInput, InputFocus,
        tab_navigation::{NavAction, TabIndex},
    },
    picking::events::{Click, Over, Pointer},
    prelude::*,
    ui::RelativeCursorPosition,
    ui_widgets::{
        Activate, MenuFocusState, MenuPopup,
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

#[derive(Component)]
pub(crate) struct PointerContextSubmenuSurface;

#[derive(Component)]
struct PointerContextSubmenu(Entity);

#[derive(Component)]
struct SubmenuTrigger;

/// Spawns a window-clamped menu at a position local to `parent`.
pub(crate) fn spawn_pointer_context_menu<A: Bundle, M: Bundle>(
    parent: &mut ChildSpawnerCommands,
    position: Vec2,
    anchor_marker: A,
    marker: M,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    spawn_pointer_context_menu_sized(
        parent,
        position,
        POINTER_CONTEXT_MENU_WIDTH,
        anchor_marker,
        marker,
        build,
    );
}

/// Spawns a window-clamped menu with a caller-selected width.
pub(crate) fn spawn_pointer_context_menu_sized<A: Bundle, M: Bundle>(
    parent: &mut ChildSpawnerCommands,
    position: Vec2,
    width: f32,
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
                        width: Val::Px(width),
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

/// A flyout stays inside the root menu's focus scope, rather than becoming another modal menu.
pub(crate) fn spawn_pointer_context_submenu(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    let trigger = spawn_pointer_context_menu_custom_item(parent, label, SubmenuTrigger, |item| {
        item.spawn((
            Text::new(label),
            ThemedText,
            Node {
                flex_grow: 1.0,
                ..default()
            },
            Pickable::IGNORE,
        ));
        item.spawn((Text::new("›"), ThemedText, Pickable::IGNORE));
    });
    let mut panel = None;
    parent.commands().entity(trigger).with_children(|item| {
        panel = Some(
            item.spawn((
                PointerContextSubmenuSurface,
                super::node_graph::FeathersGraphNavigationBlocker,
                RelativeCursorPosition::default(),
                Pickable::default(),
                OverrideClip,
                GlobalZIndex(251),
                Popover {
                    positions: vec![
                        PopoverPlacement {
                            side: PopoverSide::Right,
                            align: PopoverAlign::Start,
                            gap: 0.0,
                        },
                        PopoverPlacement {
                            side: PopoverSide::Left,
                            align: PopoverAlign::Start,
                            gap: 0.0,
                        },
                    ],
                    window_margin: 8.0,
                },
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    width: Val::Px(216.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::vertical(Val::Px(4.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(theme::MENU),
                BorderColor::all(theme::BORDER_BRIGHT),
            ))
            .observe(pointer_context_submenu_key)
            .with_children(build)
            .id(),
        );
    });
    parent
        .commands()
        .entity(trigger)
        .insert(PointerContextSubmenu(panel.unwrap()))
        .observe(open_pointer_context_submenu)
        .observe(activate_pointer_context_submenu)
        .observe(pointer_context_submenu_key);
}

fn pointer_context_submenu_key(
    mut event: On<FocusedInput<bevy::input::keyboard::KeyboardInput>>,
    triggers: Query<&PointerContextSubmenu>,
    mut panels: Query<(&mut Node, &ChildOf), With<PointerContextSubmenuSurface>>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if event.input.state != bevy::input::ButtonState::Pressed || event.input.repeat {
        return;
    }
    if event.input.key_code == KeyCode::ArrowRight && triggers.contains(event.focused_entity) {
        commands.trigger(Activate {
            entity: event.focused_entity,
        });
        event.propagate(false);
    } else if event.input.key_code == KeyCode::ArrowLeft
        && let Ok((mut panel, parent)) = panels.get_mut(event.focused_entity)
    {
        panel.display = Display::None;
        focus.set(parent.parent(), FocusCause::Navigated);
        event.propagate(false);
    }
}

fn open_pointer_context_submenu(
    event: On<Pointer<Over>>,
    submenus: Query<&PointerContextSubmenu>,
    mut panels: Query<&mut Node, With<PointerContextSubmenuSurface>>,
) {
    if let Ok(submenu) = submenus.get(event.entity)
        && let Ok(mut node) = panels.get_mut(submenu.0)
    {
        node.display = Display::Flex;
    }
}

fn activate_pointer_context_submenu(
    event: On<Activate>,
    submenus: Query<&PointerContextSubmenu>,
    mut commands: Commands,
) {
    let Ok(submenu) = submenus.get(event.entity) else {
        return;
    };
    let panel = submenu.0;
    commands.queue(move |world: &mut World| {
        let Some(mut node) = world.get_mut::<Node>(panel) else {
            return;
        };
        node.display = Display::Flex;
        let Some(children) = world.get::<Children>(panel) else {
            return;
        };
        let items = children
            .iter()
            .filter(|item| world.get::<PointerContextMenuItem>(*item).is_some())
            .collect::<Vec<_>>();
        // Hidden items can have their tab indices suppressed by the visibility eligibility pass.
        for item in &items {
            world.entity_mut(*item).insert(TabIndex(0));
        }
        if let Some(first) = items.first()
            && let Some(mut focus) = world.get_resource_mut::<InputFocus>()
        {
            focus.set(*first, FocusCause::Navigated);
        }
    });
}

pub(crate) fn spawn_pointer_context_menu_shortcut_item<A: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    shortcut: &str,
    action: A,
) {
    spawn_pointer_context_menu_custom_item(parent, label, action, |item| {
        item.spawn((
            Text::new(label),
            ThemedText,
            Node {
                flex_grow: 1.0,
                ..default()
            },
            Pickable::IGNORE,
        ));
        item.spawn((
            Text::new(shortcut),
            TextColor(theme::TEXT_MUTED),
            Pickable::IGNORE,
        ));
    });
}

pub(crate) fn spawn_pointer_context_menu_custom_item<A: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: A,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) -> Entity {
    let mut item = parent.spawn_empty();
    let entity = item.id();
    item.apply_scene(scenes::feathers_menu_item())
        .insert((
            PointerContextMenuItem,
            Pickable::default(),
            Interaction::None,
            action,
            FeathersActionButton,
            AccessibleLabel(label.to_owned()),
        ))
        .observe(stop_pointer_context_menu_click_propagation)
        .observe(close_sibling_submenus)
        .with_children(build);
    entity
}

fn close_sibling_submenus(
    event: On<Pointer<Over>>,
    parents: Query<&ChildOf>,
    triggers: Query<(Entity, &ChildOf, &PointerContextSubmenu)>,
    mut panels: Query<&mut Node, With<PointerContextSubmenuSurface>>,
) {
    let Ok(parent) = parents.get(event.entity) else {
        return;
    };
    for (trigger, other_parent, submenu) in &triggers {
        if trigger != event.entity
            && parent.parent() == other_parent.parent()
            && let Ok(mut panel) = panels.get_mut(submenu.0)
        {
            panel.display = Display::None;
        }
    }
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
    use bevy::ecs::system::RunSystemOnce;
    use bevy::{
        asset::AssetPlugin,
        input_focus::{InputFocusPlugin, tab_navigation::TabNavigationPlugin},
        scene::ScenePlugin,
        text::TextPlugin,
        ui_widgets::MenuPlugin,
    };

    #[derive(Component)]
    struct TestAction;

    #[test]
    fn submenu_is_compact_and_opens_inside_the_root_focus_scope() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
            InputFocusPlugin,
            TabNavigationPlugin,
            MenuPlugin,
        ));
        app.world_mut()
            .run_system_once(|mut commands: Commands| {
                commands.spawn(Node::default()).with_children(|parent| {
                    spawn_pointer_context_menu(parent, Vec2::ZERO, (), (), |menu| {
                        spawn_pointer_context_menu_shortcut_item(
                            menu, "Copy", "Ctrl+C", TestAction,
                        );
                        spawn_pointer_context_submenu(menu, "Arrange", |submenu| {
                            for label in ["Selection", "Upstream", "Downstream"] {
                                spawn_pointer_context_menu_item(submenu, label, TestAction);
                            }
                        });
                    });
                });
            })
            .unwrap();
        app.update();
        let root = app
            .world_mut()
            .query_filtered::<Entity, With<MenuPopup>>()
            .single(app.world())
            .unwrap();
        assert_eq!(app.world().get::<Children>(root).unwrap().len(), 2);
        let (trigger, panel) = app
            .world_mut()
            .query::<(Entity, &PointerContextSubmenu)>()
            .single(app.world())
            .map(|(entity, submenu)| (entity, submenu.0))
            .unwrap();
        assert_eq!(
            app.world().get::<Node>(panel).unwrap().display,
            Display::None
        );
        assert!(app.world().get::<MenuPopup>(panel).is_none());
        app.world_mut().trigger(Activate { entity: trigger });
        app.world_mut().flush();
        app.update();
        assert_eq!(
            app.world().get::<Node>(panel).unwrap().display,
            Display::Flex
        );
        let first = app.world().get::<Children>(panel).unwrap()[0];
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(first));
        assert_eq!(
            app.world().get::<MenuFocusState>(root),
            Some(&MenuFocusState::Open)
        );
        // Reopening also restores eligibility suppressed while the flyout was hidden.
        app.world_mut().get_mut::<Node>(panel).unwrap().display = Display::None;
        app.world_mut().get_mut::<TabIndex>(first).unwrap().0 = -1;
        app.world_mut().trigger(Activate { entity: trigger });
        app.world_mut().flush();
        assert_eq!(app.world().get::<TabIndex>(first).unwrap().0, 0);
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(first));
    }

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
