//! Data-driven editor combo boxes and compact action menus.

use super::{button::FeathersActionButton, scenes};
use crate::{EditorAction, theme};
use bevy::{
    feathers::{constants::icons, display::icon, theme::ThemedText},
    prelude::*,
    ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
};

pub(crate) struct ComboOption {
    pub(crate) label: String,
    pub(crate) selected: bool,
    pub(crate) action: EditorAction,
}

pub(crate) fn spawn_combo_control(
    parent: &mut ChildSpawnerCommands,
    value: &str,
    accessible_label: &str,
    options: &[ComboOption],
    width: f32,
) {
    parent
        .spawn(Node {
            width: Val::Px(width),
            min_width: Val::Px(112.0),
            ..default()
        })
        .with_children(|wrapper| {
            wrapper
                .spawn_empty()
                .apply_scene(scenes::feathers_menu())
                .with_children(|menu| {
                    menu.spawn_empty()
                        .apply_scene(scenes::feathers_menu_button())
                        .insert((
                            AccessibleLabel(accessible_label.to_owned()),
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(28.0),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(Val::Px(8.0)),
                                ..default()
                            },
                        ))
                        .with_children(|button| {
                            button.spawn((
                                Text::new(value),
                                ThemedText,
                                Pickable::IGNORE,
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                            ));
                            button
                                .spawn_empty()
                                .apply_scene(icon(icons::CHEVRON_DOWN))
                                .insert(Pickable::IGNORE);
                        });
                    menu.spawn_empty()
                        .apply_scene(scenes::feathers_menu_popup())
                        .with_children(|popup| {
                            for option in options {
                                spawn_combo_option(popup, option);
                            }
                        });
                });
        });
}

fn spawn_combo_option(parent: &mut ChildSpawnerCommands, option: &ComboOption) {
    parent
        .spawn_empty()
        .apply_scene(scenes::feathers_menu_item())
        .insert((
            Interaction::None,
            option.action,
            FeathersActionButton,
            AccessibleLabel(option.label.clone()),
        ))
        .with_children(|item| {
            item.spawn((
                Node {
                    width: Val::Px(18.0),
                    height: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|indicator| {
                if option.selected {
                    indicator.spawn((
                        Node {
                            width: Val::Px(6.0),
                            height: Val::Px(6.0),
                            border_radius: BorderRadius::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(theme::ACCENT),
                        Pickable::IGNORE,
                    ));
                }
            });
            item.spawn((
                Text::new(option.label.clone()),
                ThemedText,
                Pickable::IGNORE,
            ));
        });
}

pub(crate) fn spawn_action_menu(
    parent: &mut ChildSpawnerCommands,
    accessible_label: &str,
    options: &[ComboOption],
) {
    parent
        .spawn_empty()
        .apply_scene(scenes::feathers_menu())
        .with_children(|menu| {
            menu.spawn_empty()
                .apply_scene(scenes::feathers_menu_button())
                .insert((
                    AccessibleLabel(accessible_label.to_owned()),
                    Node {
                        width: Val::Px(28.0),
                        height: Val::Px(28.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                ))
                .with_children(|button| {
                    button
                        .spawn((
                            Node {
                                width: Val::Px(4.0),
                                height: Val::Px(16.0),
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::SpaceBetween,
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|dots| {
                            for _ in 0..3 {
                                dots.spawn((
                                    Node {
                                        width: Val::Px(3.0),
                                        height: Val::Px(3.0),
                                        border_radius: BorderRadius::all(Val::Px(2.0)),
                                        ..default()
                                    },
                                    BackgroundColor(theme::TEXT_MUTED),
                                    Pickable::IGNORE,
                                ));
                            }
                        });
                });
            menu.spawn_empty()
                .apply_scene(scenes::feathers_menu_popup())
                .insert((
                    Popover {
                        positions: vec![
                            PopoverPlacement {
                                side: PopoverSide::Bottom,
                                align: PopoverAlign::End,
                                gap: 2.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Top,
                                align: PopoverAlign::End,
                                gap: 2.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Left,
                                align: PopoverAlign::Start,
                                gap: 2.0,
                            },
                        ],
                        window_margin: 8.0,
                    },
                    OverrideClip,
                ))
                .with_children(|popup| {
                    for option in options {
                        spawn_combo_option(popup, option);
                    }
                });
        });
}
