//! Production generic selection-combo construction, shared with editor consumers.
use crate::feathers::{action_activation::FeathersActionButton, scenes};
use crate::{EditorAction, theme};
use bevy::{
    feathers::{constants::icons, display::icon, theme::ThemedText},
    prelude::*,
};
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

pub(super) fn spawn_combo_option<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    option: &ComboOption<A>,
) {
    parent
        .spawn_empty()
        .apply_scene(scenes::feathers_menu_item())
        .insert((
            super::selection_input::idle_menu_option(),
            option.action,
            FeathersActionButton,
            AccessibleLabel(option.label.clone()),
            // `FeathersMenuPopup` opens a modal tab group and focuses the first
            // indexed item. Without an explicit tab index the popup reports
            // `NoFocusableEntities` and closes before the action can run.
            bevy::input_focus::tab_navigation::TabIndex(0),
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

#[cfg(test)]
#[path = "selection/tests.rs"]
mod tests;
