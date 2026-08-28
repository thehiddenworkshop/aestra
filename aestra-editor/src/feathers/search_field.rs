//! Clearable, accessible search-field composition built on Feathers text editing.

use super::{button::FeathersActionButton, scenes, text_input::spawn_text_input};
use crate::theme;
use bevy::{
    feathers::{controls::FeathersTextInput, theme::ThemedText},
    prelude::*,
    text::{EditableText, TextEdit},
    ui_widgets::{Activate, ValueChange},
};

#[derive(Component)]
pub(crate) struct SearchField;

#[derive(Component)]
pub(crate) struct SearchFieldInput;

#[derive(Component)]
pub(crate) struct SearchFieldClear {
    input: Entity,
}

pub(crate) fn spawn_search_field<M: Bundle>(
    parent: &mut ChildSpawnerCommands,
    initial_value: &str,
    label: &str,
    clear_label: &str,
    marker: M,
) -> Entity {
    let mut input = Entity::PLACEHOLDER;
    parent
        .spawn((
            SearchField,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(32.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
        ))
        .with_children(|field| {
            field.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
                Pickable::IGNORE,
            ));
            input = spawn_text_input(field, initial_value, label, (SearchFieldInput, marker));
            field.commands().entity(input).insert(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            });
            field
                .spawn_empty()
                .apply_scene(scenes::feathers_tool_button())
                .insert((
                    SearchFieldClear { input },
                    FeathersActionButton,
                    AccessibleLabel(clear_label.to_owned()),
                    Node {
                        display: if initial_value.is_empty() {
                            Display::None
                        } else {
                            Display::Flex
                        },
                        width: Val::Px(24.0),
                        height: Val::Px(24.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                ))
                .with_children(|button| {
                    button.spawn((Text::new("x"), ThemedText, Pickable::IGNORE));
                });
        });
    input
}

pub(crate) fn clear_search_field(
    activate: On<Activate>,
    mut clears: Query<(&SearchFieldClear, &mut Node)>,
    children: Query<&Children>,
    mut inputs: Query<&mut EditableText, With<FeathersTextInput>>,
    mut commands: Commands,
) {
    let Ok((clear, mut node)) = clears.get_mut(activate.entity) else {
        return;
    };
    let Ok(children) = children.get(clear.input) else {
        return;
    };
    for child in children.iter() {
        let Ok(mut input) = inputs.get_mut(child) else {
            continue;
        };
        input.editor_mut().set_text("");
        input.queue_edit(TextEdit::TextEnd(false));
        node.display = Display::None;
        commands.trigger(ValueChange {
            source: clear.input,
            value: String::new(),
            is_final: false,
        });
        break;
    }
}

pub(crate) fn sync_search_clear_visibility(
    children: Query<&Children>,
    inputs: Query<&EditableText, With<FeathersTextInput>>,
    mut clears: Query<(&SearchFieldClear, &mut Node)>,
) {
    for (clear, mut node) in &mut clears {
        let Ok(children) = children.get(clear.input) else {
            continue;
        };
        let has_value = children
            .iter()
            .find_map(|child| inputs.get(child).ok())
            .is_some_and(|input| !input.value().to_string().is_empty());
        node.display = if has_value {
            Display::Flex
        } else {
            Display::None
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{asset::AssetPlugin, scene::ScenePlugin, text::TextPlugin};

    #[derive(Component)]
    struct TestSearch;

    #[derive(Resource, Default)]
    struct CapturedChange {
        source: Option<Entity>,
        value: String,
    }

    fn capture_change(change: On<ValueChange<String>>, mut captured: ResMut<CapturedChange>) {
        captured.source = Some(change.source);
        captured.value.clone_from(&change.value);
    }

    fn spawn_test_search(mut commands: Commands) {
        commands.spawn(Node::default()).with_children(|parent| {
            spawn_search_field(
                parent,
                "spark",
                "Search effects",
                "Clear effect search",
                TestSearch,
            );
        });
    }

    #[test]
    fn search_composition_labels_input_and_clear_control_accessibly() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .add_systems(Startup, spawn_test_search);
        app.update();

        let input = {
            let world = app.world_mut();
            let mut query =
                world.query_filtered::<Entity, (With<SearchFieldInput>, With<TestSearch>)>();
            query.single(world).unwrap()
        };
        assert_eq!(
            app.world().get::<AccessibleLabel>(input).unwrap().0,
            "Search effects"
        );
        let clear = {
            let world = app.world_mut();
            let mut query = world
                .query_filtered::<Entity, (With<SearchFieldClear>, With<FeathersActionButton>)>();
            query.single(world).unwrap()
        };
        assert_eq!(
            app.world().get::<AccessibleLabel>(clear).unwrap().0,
            "Clear effect search"
        );
    }

    #[test]
    fn clear_activation_empties_the_input_and_emits_a_live_change() {
        let mut app = App::new();
        app.init_resource::<CapturedChange>()
            .add_observer(clear_search_field)
            .add_observer(capture_change);
        let input = app.world_mut().spawn_empty().id();
        let editable = app
            .world_mut()
            .spawn((
                ChildOf(input),
                EditableText::new("spark"),
                FeathersTextInput,
            ))
            .id();
        let clear = app
            .world_mut()
            .spawn((
                SearchFieldClear { input },
                AccessibleLabel("Clear search".into()),
                Node::default(),
            ))
            .id();

        app.world_mut().trigger(Activate { entity: clear });
        app.update();

        assert!(
            app.world()
                .get::<EditableText>(editable)
                .unwrap()
                .value()
                .to_string()
                .is_empty()
        );
        assert_eq!(
            app.world().get::<Node>(clear).unwrap().display,
            Display::None
        );
        let captured = app.world().resource::<CapturedChange>();
        assert_eq!(captured.source, Some(input));
        assert!(captured.value.is_empty());
        assert_eq!(
            app.world().get::<AccessibleLabel>(clear).unwrap().0,
            "Clear search"
        );
    }

    #[test]
    fn clear_visibility_tracks_the_current_input_value() {
        let mut app = App::new();
        app.add_systems(Update, sync_search_clear_visibility);
        let input = app.world_mut().spawn_empty().id();
        let editable = app
            .world_mut()
            .spawn((
                ChildOf(input),
                EditableText::new("spark"),
                FeathersTextInput,
            ))
            .id();
        let clear = app
            .world_mut()
            .spawn((SearchFieldClear { input }, Node::default()))
            .id();

        app.update();
        assert_eq!(
            app.world().get::<Node>(clear).unwrap().display,
            Display::Flex
        );

        app.world_mut()
            .get_mut::<EditableText>(editable)
            .unwrap()
            .editor_mut()
            .set_text("");
        app.update();
        assert_eq!(
            app.world().get::<Node>(clear).unwrap().display,
            Display::None
        );
    }
}
