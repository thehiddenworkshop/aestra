//! Compact list rows, section headers, and result-state surfaces for editor panels.

use super::button::EditorNativeControl;
use crate::theme;
use bevy::{
    input_focus::{InputFocus, InputFocusVisible},
    prelude::*,
    ui_widgets::ActiveDescendant,
};

/// Marks a headless list box that uses Aestra's shared keyboard-focus treatment.
#[derive(Component)]
pub(crate) struct KeyboardNavigableList;

/// Marks a row whose active-descendant focus should be drawn by the editor widget layer.
#[derive(Component)]
pub(crate) struct KeyboardNavigableListRow;

#[derive(Component)]
pub(crate) struct CompactListRow;

#[derive(Component)]
pub(crate) struct CompactListSectionHeader;

#[derive(Component)]
pub(crate) struct CompactListEmptyState;

pub(super) fn update_keyboard_list_focus_visuals(
    focus: Res<InputFocus>,
    focus_visible: Res<InputFocusVisible>,
    lists: Query<&ActiveDescendant, With<KeyboardNavigableList>>,
    rows: Query<(Entity, Has<Outline>), With<KeyboardNavigableListRow>>,
    mut commands: Commands,
) {
    let active = focus
        .get()
        .filter(|_| focus_visible.0)
        .and_then(|entity| lists.get(entity).ok())
        .and_then(|descendant| descendant.0);

    for (entity, has_outline) in &rows {
        if Some(entity) == active {
            if !has_outline {
                commands.entity(entity).insert(Outline {
                    color: theme::ACCENT,
                    width: Val::Px(2.0),
                    offset: Val::Px(-2.0),
                });
            }
        } else if has_outline {
            commands.entity(entity).remove::<Outline>();
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ListRowStatus<'a> {
    pub(crate) label: &'a str,
    pub(crate) color: Color,
}

pub(crate) fn spawn_action_list_row<A: Component>(
    parent: &mut ChildSpawnerCommands,
    primary: &str,
    secondary: Option<&str>,
    status: Option<ListRowStatus<'_>>,
    accessible_label: &str,
    action: A,
) -> Entity {
    let entity = spawn_list_row_content(parent, primary, secondary, status);
    parent.commands().entity(entity).insert((
        Button,
        EditorNativeControl,
        action,
        AccessibleLabel(accessible_label.to_owned()),
    ));
    entity
}

pub(crate) fn spawn_status_list_row(
    parent: &mut ChildSpawnerCommands,
    primary: &str,
    secondary: Option<&str>,
    status: ListRowStatus<'_>,
    accessible_label: &str,
) -> Entity {
    let entity = spawn_list_row_content(parent, primary, secondary, Some(status));
    parent
        .commands()
        .entity(entity)
        .insert(AccessibleLabel(accessible_label.to_owned()));
    entity
}

pub(crate) fn spawn_info_list_row(
    parent: &mut ChildSpawnerCommands,
    primary: &str,
    secondary: Option<&str>,
) -> Entity {
    let entity = spawn_list_row_content(parent, primary, secondary, None);
    parent
        .commands()
        .entity(entity)
        .insert(AccessibleLabel(primary.to_owned()));
    entity
}

fn spawn_list_row_content(
    parent: &mut ChildSpawnerCommands,
    primary: &str,
    secondary: Option<&str>,
    status: Option<ListRowStatus<'_>>,
) -> Entity {
    let mut row = parent.spawn((
        CompactListRow,
        Node {
            min_height: Val::Px(if secondary.is_some() { 42.0 } else { 34.0 }),
            width: Val::Percent(100.0),
            padding: UiRect::axes(Val::Px(9.0), Val::Px(5.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(theme::PANEL_DARK),
    ));
    let entity = row.id();
    row.with_children(|row| {
        row.spawn(Node {
            min_width: Val::Px(0.0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            ..default()
        })
        .with_children(|labels| {
            labels.spawn((
                Text::new(primary),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
                Pickable::IGNORE,
            ));
            if let Some(secondary) = secondary {
                labels.spawn((
                    Text::new(secondary),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                    Pickable::IGNORE,
                ));
            }
        });
        if let Some(status) = status {
            row.spawn((
                Text::new(status.label),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(status.color),
                Pickable::IGNORE,
            ));
        }
    });
    entity
}

pub(crate) struct ListSectionHeaderEntities {
    pub(crate) root: Entity,
    pub(crate) meta: Entity,
}

pub(crate) fn spawn_list_section_header(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    meta: &str,
) -> ListSectionHeaderEntities {
    let mut meta_entity = Entity::PLACEHOLDER;
    let mut row = parent.spawn((
        CompactListSectionHeader,
        Node {
            min_height: Val::Px(28.0),
            width: Val::Percent(100.0),
            padding: UiRect::horizontal(Val::Px(9.0)),
            align_items: AlignItems::Center,
            ..default()
        },
    ));
    let root = row.id();
    row.with_children(|row| {
        row.spawn((
            Text::new(title),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(theme::TEXT_MUTED),
        ));
        row.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        meta_entity = row
            .spawn((
                Text::new(meta),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ))
            .id();
    });
    ListSectionHeaderEntities {
        root,
        meta: meta_entity,
    }
}

pub(crate) fn spawn_list_empty_state(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    message: &str,
    color: Color,
    display: Display,
) -> Entity {
    let mut empty = parent.spawn((
        CompactListEmptyState,
        Node {
            display,
            width: Val::Percent(100.0),
            min_height: Val::Px(64.0),
            padding: UiRect::all(Val::Px(10.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        },
    ));
    let entity = empty.id();
    empty.with_children(|empty| {
        empty.spawn((
            Text::new(title),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(color),
        ));
        empty.spawn((
            Text::new(message),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
        ));
    });
    entity
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input_focus::InputFocusVisible;

    #[derive(Component)]
    struct TestAction;

    fn spawn_test_widgets(mut commands: Commands) {
        commands.spawn(Node::default()).with_children(|parent| {
            spawn_action_list_row(
                parent,
                "Prism Bloom",
                Some("assets/effects/prism_bloom.aestra.ron"),
                None,
                "Open Prism Bloom",
                TestAction,
            );
            spawn_status_list_row(
                parent,
                "Broken Effect",
                None,
                ListRowStatus {
                    label: "INVALID",
                    color: theme::ACCENT,
                },
                "Broken Effect, invalid",
            );
            spawn_list_section_header(parent, "PROJECT EFFECTS", "2 FOUND");
            spawn_list_empty_state(
                parent,
                "No matching effects",
                "Try another search.",
                theme::TEXT_MUTED,
                Display::Flex,
            );
        });
    }

    #[test]
    fn list_compositions_classify_actions_and_expose_accessible_labels() {
        let mut app = App::new();
        app.add_systems(Startup, spawn_test_widgets);
        app.update();

        let mut actions = app.world_mut().query_filtered::<
            (&AccessibleLabel, Has<Button>, Has<EditorNativeControl>),
            (With<CompactListRow>, With<TestAction>),
        >();
        let (label, button, native) = actions.single(app.world()).unwrap();
        assert_eq!(label.0, "Open Prism Bloom");
        assert!(button);
        assert!(native);
        let row_count = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<Entity, With<CompactListRow>>();
            query.iter(world).count()
        };
        let header_count = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<Entity, With<CompactListSectionHeader>>();
            query.iter(world).count()
        };
        let empty_count = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<Entity, With<CompactListEmptyState>>();
            query.iter(world).count()
        };
        assert_eq!(row_count, 2);
        assert_eq!(header_count, 1);
        assert_eq!(empty_count, 1);
    }

    #[test]
    fn keyboard_active_descendant_gets_a_visible_non_layout_focus_ring() {
        let mut app = App::new();
        app.add_systems(Update, update_keyboard_list_focus_visuals);
        let list = app.world_mut().spawn(KeyboardNavigableList).id();
        let row = app
            .world_mut()
            .spawn((KeyboardNavigableListRow, Node::default(), ChildOf(list)))
            .id();
        app.world_mut()
            .entity_mut(list)
            .insert(ActiveDescendant(Some(row)));
        app.insert_resource(InputFocus::from_entity(list));
        app.insert_resource(InputFocusVisible(true));

        app.update();

        let outline = app.world().get::<Outline>(row).unwrap();
        assert_eq!(outline.width, Val::Px(2.0));
        assert_eq!(outline.offset, Val::Px(-2.0));

        app.world_mut().resource_mut::<InputFocusVisible>().0 = false;
        app.update();
        assert!(app.world().get::<Outline>(row).is_none());
    }
}
