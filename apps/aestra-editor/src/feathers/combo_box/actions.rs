//! Actual compact/icon/searchable action-menu composition and filtering.
use crate::feathers::{
    combo_box::selection::{ComboOption, spawn_combo_option, spawn_lazy_action_menu},
    icon::{SvgColor, UiSvg, load_svg_icon},
    scenes,
    tooltip::EditorTooltip,
};
use crate::theme;
use bevy::{
    feathers::theme::ThemedText,
    prelude::*,
    ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
};

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
        None,
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
    categories: &[String],
) {
    spawn_icon_menu(
        parent,
        asset_server,
        icon_path,
        accessible_label,
        tooltip,
        options,
        Some(categories),
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
    use bevy::{
        input_focus::{InputFocus, tab_navigation::NavAction},
        ui_widgets::{MenuFocusState, MenuPopup},
    };

    #[derive(Component, Clone, Copy)]
    struct TestMenuAction;

    #[derive(Resource, Default)]
    struct OpenOnNextFrame(Option<Entity>);
    #[derive(Resource, Default, Debug)]
    struct MenuTrace(Vec<String>);

    fn spawn_test_action_menu(mut commands: Commands) {
        commands.spawn(Node::default()).with_children(|parent| {
            spawn_action_menu(
                parent,
                "Arrange nodes",
                &[ComboOption {
                    label: "Arrange graph".into(),
                    selected: false,
                    action: TestMenuAction,
                }],
            );
        });
    }

    #[test]
    fn action_menu_options_are_focusable() {
        // Use the real headless editor UI stack, including visibility propagation
        // and a window-owned focus context, rather than incomplete menu-only plugins.
        let mut app = crate::feathers::text_input::tests::app_with(|app| {
            app.add_systems(Startup, spawn_test_action_menu);
        });

        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<MenuPopup>>()
                .iter(app.world())
                .count(),
            0
        );
        let button = app
            .world_mut()
            .query_filtered::<Entity, With<bevy::ui_widgets::MenuButton>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .trigger(bevy::ui_widgets::Activate { entity: button });
        app.update();

        let option = {
            let mut options = app.world_mut().query_filtered::<
                (Entity, &bevy::input_focus::tab_navigation::TabIndex),
                With<bevy::ui_widgets::MenuItem>,
            >();
            let (option, tab_index) = options.single(app.world()).unwrap();
            assert_eq!(tab_index.0, 0);
            option
        };
        let popup = {
            let mut popups = app.world_mut().query_filtered::<Entity, With<MenuPopup>>();
            popups.single(app.world()).unwrap()
        };
        app.world_mut().entity_mut(popup).insert((
            Visibility::Visible,
            MenuFocusState::Opening(NavAction::First),
        ));
        app.update();

        assert_eq!(
            app.world().get::<MenuFocusState>(popup),
            Some(&MenuFocusState::Open)
        );
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(option));
    }

    #[test]
    fn lazy_arrange_icon_menu_acquires_focus_when_scene_resolves_and_reopens() {
        let mut app = crate::feathers::text_input::tests::app_with(|app| {
            app.add_plugins(crate::input::EditorKeyboardPlugin)
                .add_message::<bevy::input_focus::KeyboardInputSnapshot>()
                .init_asset::<crate::feathers::icon::SvgFile>()
                .init_resource::<OpenOnNextFrame>()
                .init_resource::<MenuTrace>()
                .add_observer(
                    |event: On<bevy::ui_widgets::MenuEvent>,
                     mut trace: ResMut<MenuTrace>,
                     focus: Res<InputFocus>| {
                        trace.0.push(format!(
                            "{:?} at {:?}, focus {:?}",
                            event.action,
                            event.source,
                            focus.get()
                        ));
                    },
                )
                .add_observer(
                    |event: On<bevy::ui_widgets::Activate>,
                     mut trace: ResMut<MenuTrace>,
                     disabled: Query<(), With<bevy::ui::InteractionDisabled>>| {
                        trace.0.push(format!(
                            "Activate {:?}, disabled {}",
                            event.entity,
                            disabled.contains(event.entity)
                        ));
                    },
                )
                .add_systems(
                    PreUpdate,
                    (|mut next: ResMut<OpenOnNextFrame>, mut commands: Commands| {
                        if let Some(entity) = next.0.take() {
                            commands.trigger(bevy::ui_widgets::Activate { entity });
                        }
                    })
                    .after(bevy::input_focus::InputFocusSystems::Dispatch),
                );
        });
        let root = app.world_mut().spawn(Node::default()).id();
        let assets = app.world().resource::<AssetServer>().clone();
        app.world_mut()
            .commands()
            .entity(root)
            .with_children(|parent| {
                spawn_icon_action_menu(
                    parent,
                    &assets,
                    "icons/chevron-down.svg",
                    "Arrange nodes",
                    "Arrange nodes",
                    &[ComboOption {
                        label: "Arrange selection".into(),
                        selected: false,
                        action: TestMenuAction,
                    }],
                );
            });
        app.update();
        app.update();
        let button = app
            .world_mut()
            .query_filtered::<Entity, With<bevy::ui_widgets::MenuButton>>()
            .single(app.world())
            .unwrap();
        let owner = app.world().get::<ChildOf>(button).unwrap().parent();
        assert!(
            app.world()
                .get::<bevy::feathers::controls::FeathersLazyMenu>(owner)
                .is_some(),
            "icon trigger must belong to the lazy menu owner"
        );
        for _ in 0..2 {
            assert_eq!(
                app.world_mut()
                    .query_filtered::<Entity, With<MenuPopup>>()
                    .iter(app.world())
                    .count(),
                0
            );
            app.world_mut().resource_mut::<MenuTrace>().0.clear();
            // Pointer/keyboard activation occurs after eligibility sync in PreUpdate.
            app.world_mut().resource_mut::<OpenOnNextFrame>().0 = Some(button);
            // Run the actual Main frame (including asset/scene phases), with
            // activation injected after the editor's tab-eligibility pass.
            app.update();
            // Native lazy menus queue a SceneListPatch asset. An in-frame
            // activation can miss this frame's asset-event publication and resolve
            // next frame; focus must be acquired as soon as the scene actually exists.
            // Do not reopen it while waiting: an accidental CloseAll must fail.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while app
                .world_mut()
                .query_filtered::<Entity, With<MenuPopup>>()
                .iter(app.world())
                .next()
                .is_none()
            {
                let trace = app.world().resource::<MenuTrace>();
                assert!(
                    !trace.0.iter().any(|event| event.starts_with("CloseAll")),
                    "popup closed while resolving: {trace:?}"
                );
                assert!(
                    std::time::Instant::now() < deadline,
                    "lazy popup did not resolve: {trace:?}"
                );
                app.update();
                std::thread::yield_now();
            }
            let popup = app
                .world_mut()
                .query_filtered::<Entity, With<MenuPopup>>()
                .single(app.world())
                .unwrap_or_else(|e| {
                    panic!(
                        "lazy popup missing: {e:?}; {:?}",
                        app.world().resource::<MenuTrace>()
                    )
                });
            let option = app
                .world_mut()
                .query_filtered::<Entity, With<bevy::ui_widgets::MenuItem>>()
                .single(app.world())
                .unwrap();
            assert_eq!(
                app.world()
                    .get::<bevy::input_focus::tab_navigation::TabIndex>(option)
                    .unwrap()
                    .0,
                0
            );
            assert_eq!(
                app.world().get::<Visibility>(popup),
                Some(&Visibility::Visible)
            );
            assert_eq!(
                app.world().get::<MenuFocusState>(popup),
                Some(&MenuFocusState::Open)
            );
            assert_eq!(app.world().resource::<InputFocus>().get(), Some(option));

            app.world_mut().trigger(bevy::ui_widgets::MenuEvent {
                source: popup,
                action: bevy::ui_widgets::MenuAction::CloseAll,
            });
            app.world_mut().flush();
            app.update();
        }
    }

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

/// Shared categorized, scrollable search results for toolbar and pointer graph menus.
pub(crate) fn spawn_searchable_action_list<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    input: Entity,
    options: &[ComboOption<A>],
    categories: &[String],
) {
    parent.commands().entity(input).observe(filter_actions);
    parent
        .spawn(Node {
            width: Val::Px(280.0),
            height: Val::Px(360.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|body| {
            crate::feathers::scroll::spawn_vertical_scroll_area(
                body,
                crate::ScrollMemoryKey::MaterialGraphPalette,
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                |list| {
                    let mut groups = Vec::<(&str, Vec<usize>)>::new();
                    for (index, category) in categories.iter().enumerate() {
                        if let Some((_, items)) =
                            groups.iter_mut().find(|(name, _)| *name == category)
                        {
                            items.push(index);
                        } else {
                            groups.push((category, vec![index]));
                        }
                    }
                    for (category, items) in groups {
                        list.spawn((
                            SearchableAction {
                                input,
                                text: items
                                    .iter()
                                    .map(|index| {
                                        format!("{} {}", category, options[*index].label)
                                            .to_lowercase()
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" "),
                            },
                            Text::new(category.to_uppercase()),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(theme::ACCENT),
                            Node {
                                padding: UiRect::all(Val::Px(8.0)),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                        for index in items {
                            let option = &options[index];
                            list.spawn((
                                SearchableAction {
                                    input,
                                    text: format!("{} {}", category, option.label).to_lowercase(),
                                },
                                Node {
                                    width: Val::Percent(100.0),
                                    flex_direction: FlexDirection::Column,
                                    ..default()
                                },
                            ))
                            .with_children(|row| spawn_combo_option(row, option));
                        }
                    }
                },
            );
        });
}

fn spawn_icon_menu<A: Component + Copy>(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    icon_path: &'static str,
    accessible_label: &str,
    tooltip: &str,
    options: &[ComboOption<A>],
    categories: Option<&[String]>,
) {
    let owner = if categories.is_none() {
        spawn_lazy_action_menu(parent, options)
    } else {
        parent
            .spawn_empty()
            .apply_scene(scenes::feathers_menu())
            .id()
    };
    parent.commands().entity(owner).with_children(|menu| {
        let mut search_button = Entity::PLACEHOLDER;
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
            ))
            .with_children(|button| {
                search_button = button.target_entity();
            });
        if categories.is_none() {
            return;
        }
        menu.spawn_empty()
            .apply_scene(scenes::feathers_menu_popup())
            .insert((
                Popover {
                    positions: vec![
                        PopoverPlacement {
                            side: PopoverSide::Bottom,
                            align: if categories.is_some() {
                                PopoverAlign::Start
                            } else {
                                PopoverAlign::End
                            },
                            gap: 2.0,
                        },
                        PopoverPlacement {
                            side: PopoverSide::Top,
                            align: if categories.is_some() {
                                PopoverAlign::Start
                            } else {
                                PopoverAlign::End
                            },
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
                if let Some(categories) = categories {
                    let popup_entity = popup.target_entity();
                    popup
                        .commands()
                        .entity(popup_entity)
                        .insert(crate::feathers::node_graph::FeathersGraphNavigationBlocker);
                    let input = crate::feathers::search_field::spawn_search_field(
                        popup,
                        "",
                        "Search nodes",
                        "Clear search",
                        (),
                    );
                    // A search popup owns focus itself; ordinary menus assume focusable
                    // items directly beneath the popup and can immediately close this tree.
                    popup
                        .commands()
                        .entity(search_button)
                        .remove::<bevy::ui_widgets::MenuButton>()
                        .remove::<bevy::ui_widgets::ActivateOnPress>()
                        .observe(
                            move |_: On<bevy::ui_widgets::Activate>, mut commands: Commands| {
                                commands.queue(move |world: &mut World| {
                                    let visible = world.get::<Visibility>(popup_entity)
                                        == Some(&Visibility::Visible);
                                    if let Ok(mut entity) = world.get_entity_mut(popup_entity) {
                                        entity.insert((
                                            if visible {
                                                Visibility::Hidden
                                            } else {
                                                Visibility::Visible
                                            },
                                            if visible {
                                                bevy::ui_widgets::MenuFocusState::Closed
                                            } else {
                                                bevy::ui_widgets::MenuFocusState::Open
                                            },
                                        ));
                                    }
                                    if !visible {
                                        world
                                            .resource_mut::<bevy::input_focus::InputFocus>()
                                            .set(input, bevy::input_focus::FocusCause::Navigated);
                                    }
                                });
                            },
                        );
                    spawn_searchable_action_list(popup, input, options, categories);
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
    let owner = spawn_lazy_action_menu(parent, options);
    parent
        .commands()
        .entity(owner)
        .insert(Pickable::IGNORE)
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
        });
}
