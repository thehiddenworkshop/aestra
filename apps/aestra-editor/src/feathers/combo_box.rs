//! Data-driven editor combo boxes and compact action menus.

use super::{button::FeathersActionButton, icon::load_svg_icon, scenes, tooltip::EditorTooltip};
use crate::{EditorAction, theme};
use bevy::{
    feathers::{constants::icons, display::icon, theme::ThemedText},
    prelude::*,
    ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
};
use bevy_resvg::prelude::{SvgColor, UiSvg};

pub(crate) struct ComboOption<A = EditorAction> {
    pub(crate) label: String,
    pub(crate) selected: bool,
    pub(crate) action: A,
}

pub(crate) fn spawn_combo_control<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    value: &str,
    accessible_label: &str,
    options: &[ComboOption<A>],
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

fn spawn_combo_option<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    option: &ComboOption<A>,
) {
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

pub(crate) fn spawn_action_menu<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    accessible_label: &str,
    options: &[ComboOption<A>],
) {
    spawn_action_menu_with_trigger(parent, accessible_label, options, ActionMenuTrigger::Dots);
}

pub(crate) fn spawn_compact_action_menu<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    accessible_label: &str,
    options: &[ComboOption<A>],
) {
    spawn_action_menu_with_trigger(
        parent,
        accessible_label,
        options,
        ActionMenuTrigger::Ellipsis,
    );
}

/// Spawns a compact action menu whose trigger communicates its current semantic mode with an SVG.
pub(crate) fn spawn_icon_action_menu<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    icon_path: &'static str,
    accessible_label: &str,
    tooltip: &str,
    options: &[ComboOption<A>],
) {
    spawn_icon_menu(
        parent,
        asset_server,
        icon_path,
        accessible_label,
        tooltip,
        options,
        false,
    );
}

/// A bounded, searchable action menu for graph node catalogs.
pub(crate) fn spawn_searchable_icon_action_menu<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    icon_path: &'static str,
    accessible_label: &str,
    tooltip: &str,
    options: &[ComboOption<A>],
) {
    spawn_icon_menu(
        parent,
        asset_server,
        icon_path,
        accessible_label,
        tooltip,
        options,
        true,
    );
}

#[derive(Component)]
struct SearchableAction {
    input: Entity,
    text: String,
}

#[cfg(test)]
mod searchable_tests {
    use super::*;

    #[test]
    fn search_filters_only_its_own_menu_case_insensitively() {
        let mut app = App::new();
        let input = app.world_mut().spawn_empty().observe(filter_actions).id();
        let other = app.world_mut().spawn_empty().id();
        let matching = app
            .world_mut()
            .spawn((
                SearchableAction {
                    input,
                    text: "vector 3".into(),
                },
                Node::default(),
            ))
            .id();
        let hidden = app
            .world_mut()
            .spawn((
                SearchableAction {
                    input,
                    text: "float".into(),
                },
                Node::default(),
            ))
            .id();
        let untouched = app
            .world_mut()
            .spawn((
                SearchableAction {
                    input: other,
                    text: "float".into(),
                },
                Node::default(),
            ))
            .id();
        app.world_mut().trigger(bevy::ui_widgets::ValueChange {
            source: input,
            value: " VECTOR ".to_owned(),
            is_final: false,
        });
        assert_eq!(
            app.world().get::<Node>(matching).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(hidden).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Node>(untouched).unwrap().display,
            Display::Flex
        );
        app.world_mut().trigger(bevy::ui_widgets::ValueChange {
            source: input,
            value: String::new(),
            is_final: false,
        });
        assert_eq!(
            app.world().get::<Node>(hidden).unwrap().display,
            Display::Flex
        );
    }
}

fn filter_actions(
    change: On<bevy::ui_widgets::ValueChange<String>>,
    mut rows: Query<(&SearchableAction, &mut Node)>,
) {
    let query = change.value.trim().to_lowercase();
    for (row, mut node) in &mut rows {
        if row.input == change.source {
            node.display = if row.text.contains(&query) {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
}

fn spawn_icon_menu<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    icon_path: &'static str,
    accessible_label: &str,
    tooltip: &str,
    options: &[ComboOption<A>],
    searchable: bool,
) {
    parent
        .spawn_empty()
        .apply_scene(scenes::feathers_menu())
        .with_children(|menu| {
            menu.spawn_empty()
                .apply_scene(scenes::feathers_menu_button())
                .insert((
                    AccessibleLabel(accessible_label.to_owned()),
                    EditorTooltip::description(tooltip),
                    Node {
                        width: Val::Px(28.0),
                        height: Val::Px(28.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                ))
                .with_child((
                    Node {
                        width: Val::Px(15.0),
                        height: Val::Px(15.0),
                        ..default()
                    },
                    UiSvg(load_svg_icon(asset_server, icon_path)),
                    SvgColor(theme::TEXT),
                    Pickable::IGNORE,
                ));
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
                    if searchable {
                        let popup_entity = popup.target_entity();
                        popup
                            .commands()
                            .entity(popup_entity)
                            .insert(super::node_graph::FeathersGraphNavigationBlocker);
                        let input = super::search_field::spawn_search_field(
                            popup,
                            "",
                            "Search nodes",
                            "Clear search",
                            (),
                        );
                        popup.commands().entity(input).observe(filter_actions);
                        super::scroll::spawn_vertical_scroll_area(
                            popup,
                            crate::ScrollMemoryKey::MaterialGraphPalette,
                            Node {
                                width: Val::Px(268.0),
                                height: Val::Px(300.0),
                                flex_direction: FlexDirection::Column,
                                ..default()
                            },
                            |list| {
                                for option in options {
                                    list.spawn((
                                        SearchableAction {
                                            input,
                                            text: option.label.to_lowercase(),
                                        },
                                        Node {
                                            width: Val::Percent(100.0),
                                            flex_direction: FlexDirection::Column,
                                            ..default()
                                        },
                                    ))
                                    .with_children(|row| spawn_combo_option(row, option));
                                }
                            },
                        );
                        return;
                    }
                    for option in options {
                        spawn_combo_option(popup, option);
                    }
                });
        });
}

#[derive(Clone, Copy)]
enum ActionMenuTrigger {
    Dots,
    Ellipsis,
}

fn spawn_action_menu_with_trigger<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    accessible_label: &str,
    options: &[ComboOption<A>],
    trigger: ActionMenuTrigger,
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
                .with_children(|button| match trigger {
                    ActionMenuTrigger::Dots => {
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
                    }
                    ActionMenuTrigger::Ellipsis => {
                        button.spawn((
                            Text::new("…"),
                            TextFont {
                                font_size: FontSize::Px(14.0),
                                ..default()
                            },
                            ThemedText,
                            Pickable::IGNORE,
                        ));
                    }
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
