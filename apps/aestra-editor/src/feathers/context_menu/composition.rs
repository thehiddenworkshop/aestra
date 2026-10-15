//! Production pointer menu composition and attached interaction observers.
use crate::feathers::{
    action_activation::FeathersActionButton,
    pointer::{PointerClick, PointerOver},
    scenes,
};
use crate::theme;
use bevy::{
    feathers::theme::ThemedText,
    input_focus::{
        FocusCause, FocusedInput, InputFocus,
        tab_navigation::{NavAction, TabIndex},
    },
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
pub(crate) struct PointerContextSubmenu(pub(crate) Entity);

#[derive(Component)]
pub(crate) struct SubmenuTrigger;

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
                crate::feathers::node_graph::FeathersGraphNavigationBlocker,
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
    event: On<PointerOver>,
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
    event: On<PointerOver>,
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

fn stop_pointer_context_menu_click_propagation(mut click: On<PointerClick>) {
    click.propagate(false);
}
