//! Real generic combo construction and its public reexport on both engine graphs.
use super::*;
use bevy::{
    asset::AssetPlugin,
    input_focus::tab_navigation::TabIndex,
    scene::ScenePlugin,
    text::TextPlugin,
    ui_widgets::{MenuButton, MenuItem, MenuPopup},
};
fn app(empty: bool) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        ScenePlugin,
        TextPlugin,
    ))
    .init_asset::<Image>();
    app.add_systems(Startup, move |mut commands: Commands| {
        let options = if empty {
            Vec::new()
        } else {
            vec![
                ComboOption {
                    label: "About".into(),
                    selected: true,
                    action: EditorAction::ShowAbout,
                },
                ComboOption {
                    label: "Close".into(),
                    selected: false,
                    action: EditorAction::CloseAbout,
                },
            ]
        };
        commands.spawn(Node::default()).with_children(|parent| {
            crate::feathers::combo_box::spawn_combo_control(
                parent,
                "About",
                "Choose action",
                &options,
                160.0,
            );
        });
    });
    app.update();
    app
}
#[test]
fn public_combo_option_default_keeps_actual_shell_action_identity() {
    let option: crate::feathers::combo_box::ComboOption = ComboOption {
        label: "About".into(),
        selected: false,
        action: EditorAction::ShowAbout,
    };
    assert!(matches!(option.action, EditorAction::ShowAbout));
    let close: crate::feathers::combo_box::ComboOption = ComboOption {
        label: "Close".into(),
        selected: true,
        action: EditorAction::CloseAbout,
    };
    assert!(matches!(close.action, EditorAction::CloseAbout));
}
#[test]
fn actual_combo_uses_native_select_listbox_and_one_selected_semantic_row() {
    let mut app = app(false);
    let w = app.world_mut();
    assert_eq!(
        w.query_filtered::<Entity, With<MenuButton>>()
            .iter(w)
            .count(),
        1
    );
    assert_eq!(
        w.query_filtered::<Entity, With<MenuPopup>>()
            .iter(w)
            .count(),
        1
    );
    let actions = w
        .query::<(Entity, &EditorAction)>()
        .iter(w)
        .map(|(e, a)| (e, *a))
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 2);
    for (e, action) in actions {
        assert!(w.get::<FeathersListRow>(e).is_some());
        assert!(w.get::<FeathersActionButton>(e).is_some());
        assert_eq!(
            w.get::<Selected>(e).is_some(),
            matches!(action, EditorAction::ShowAbout)
        );
    }
    assert_eq!(
        w.query_filtered::<Entity, With<FeathersSelect>>()
            .iter(w)
            .count(),
        1
    );
    let listbox = w
        .query_filtered::<Entity, With<bevy::ui_widgets::ListBox>>()
        .single(w)
        .unwrap();
    assert_eq!(w.get::<TabIndex>(listbox).unwrap().0, 0);
}
#[test]
fn empty_options_keep_real_menu_trigger_without_dummy_actions() {
    let mut app = app(true);
    let w = app.world_mut();
    assert_eq!(
        w.query_filtered::<Entity, With<MenuItem>>().iter(w).count(),
        0
    );
    let button = w
        .query_filtered::<Entity, With<MenuButton>>()
        .single(w)
        .unwrap();
    assert_eq!(w.get::<AccessibleLabel>(button).unwrap().0, "Choose action");
    assert!(w.get::<InteractionDisabled>(button).is_some());
    let menu = w.get::<ChildOf>(button).unwrap().parent();
    let wrapper = w.get::<ChildOf>(menu).unwrap().parent();
    let node = w.get::<Node>(wrapper).unwrap();
    assert_eq!(node.width, Val::Px(160.0));
    assert_eq!(node.min_width, Val::Px(112.0));
}

#[test]
fn lazy_action_survives_popup_destruction_and_can_be_repeated_but_not_stale() {
    use crate::feathers::action_activation::{PendingFeathersActivation, queue_activation};
    use crate::feathers::text_input::tests::{focus, tap};
    use bevy::input::keyboard::Key;
    use bevy::ui_widgets::{MenuAction, MenuEvent};
    let mut app = crate::feathers::text_input::tests::app_with(|app| {
        app.add_observer(queue_activation::<With<EditorAction>>)
            .add_systems(Update, sync_action_menu_availability)
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(|parent| {
                    spawn_action_combo_control(
                        parent,
                        "Add...",
                        "Add an action",
                        &[ComboOption {
                            label: "About".into(),
                            selected: true,
                            action: EditorAction::ShowAbout,
                        }],
                        160.0,
                    );
                });
            });
    });
    let button = app
        .world_mut()
        .query_filtered::<Entity, With<MenuButton>>()
        .single(app.world())
        .unwrap();
    let owner = app.world().get::<ChildOf>(button).unwrap().parent();
    assert!(
        app.world().get::<FeathersSelect>(owner).is_none(),
        "selected toggle filters are action menus, not selects"
    );
    let target = app
        .world_mut()
        .query_filtered::<Entity, With<EditorAction>>()
        .single(app.world())
        .unwrap();
    let mut last_row = None;
    for _ in 0..2 {
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<MenuPopup>>()
                .iter(app.world())
                .count(),
            0
        );
        app.world_mut().trigger(Activate { entity: button });
        app.update();
        let row = app
            .world_mut()
            .query_filtered::<Entity, With<bevy::ui_widgets::MenuItem>>()
            .single(app.world())
            .unwrap();
        assert_ne!(
            Some(row),
            last_row,
            "visual rows are recreated, not retained"
        );
        focus(&mut app, row);
        tap(&mut app, KeyCode::Enter, Key::Enter, None);
        assert!(
            app.world().get_entity(row).is_err(),
            "native activation closes and destroys popup rows"
        );
        assert!(
            app.world()
                .get::<PendingFeathersActivation>(target)
                .is_some(),
            "semantic action outlives the visual row"
        );
        app.world_mut()
            .entity_mut(target)
            .remove::<PendingFeathersActivation>();
        last_row = Some(row);
        app.update();
    }
    app.world_mut().trigger(Activate { entity: button });
    app.update();
    let row = app
        .world_mut()
        .query_filtered::<Entity, With<bevy::ui_widgets::MenuItem>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(target)
        .insert(InteractionDisabled);
    app.update();
    assert!(app.world().get::<InteractionDisabled>(row).is_some());
    app.world_mut().trigger(Activate { entity: row });
    app.world_mut().flush();
    assert!(
        app.world()
            .get::<PendingFeathersActivation>(target)
            .is_none()
    );
    app.world_mut().trigger(MenuEvent {
        source: owner,
        action: MenuAction::CloseAll,
    });
    app.world_mut().flush();
    app.world_mut()
        .entity_mut(target)
        .remove::<InteractionDisabled>();
    app.world_mut().trigger(Activate { entity: button });
    app.update();
    let row = app
        .world_mut()
        .query_filtered::<Entity, With<bevy::ui_widgets::MenuItem>>()
        .single(app.world())
        .unwrap();
    app.world_mut().entity_mut(owner).insert(MenuContext(Some((
        aestra_core::EffectId::new(),
        1,
        1,
    ))));
    app.world_mut().trigger(Activate { entity: row });
    app.world_mut().flush();
    assert!(
        app.world()
            .get::<PendingFeathersActivation>(target)
            .is_none()
    );
    app.world_mut().trigger(MenuEvent {
        source: owner,
        action: MenuAction::CloseAll,
    });
    app.world_mut().flush();
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<MenuPopup>>()
            .iter(app.world())
            .count(),
        0
    );
}

#[derive(Resource, Default)]
struct Activations(Vec<Entity>);

#[test]
fn native_select_keyboard_navigation_commits_once_and_escape_restores_trigger_focus() {
    use crate::feathers::{
        action_activation::{PendingFeathersActivation, queue_activation},
        text_input::tests::{app_with, focus, tap},
    };
    use bevy::{input::keyboard::Key, input_focus::InputFocus, ui_widgets::ListBox};
    let mut app = app_with(|app| {
        app.add_observer(queue_activation::<With<EditorAction>>)
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(|parent| {
                    spawn_combo_control(
                        parent,
                        "About",
                        "Choose action",
                        &[
                            ComboOption {
                                label: "About".into(),
                                selected: true,
                                action: EditorAction::ShowAbout,
                            },
                            ComboOption {
                                label: "Close".into(),
                                selected: false,
                                action: EditorAction::CloseAbout,
                            },
                        ],
                        160.0,
                    );
                });
            });
    });
    let button = app
        .world_mut()
        .query_filtered::<Entity, With<MenuButton>>()
        .single(app.world())
        .unwrap();
    let popup = app
        .world_mut()
        .query_filtered::<Entity, With<MenuPopup>>()
        .single(app.world())
        .unwrap();
    let listbox = app
        .world_mut()
        .query_filtered::<Entity, With<ListBox>>()
        .single(app.world())
        .unwrap();
    let target = app
        .world_mut()
        .query::<(Entity, &EditorAction)>()
        .iter(app.world())
        .find_map(|(e, a)| matches!(a, EditorAction::CloseAbout).then_some(e))
        .unwrap();
    focus(&mut app, button);
    tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown, None);
    assert_eq!(
        app.world().get::<Visibility>(popup),
        Some(&Visibility::Visible)
    );
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(listbox));
    tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown, None);
    assert!(
        app.world()
            .get::<PendingFeathersActivation>(target)
            .is_none()
    );
    tap(&mut app, KeyCode::Enter, Key::Enter, None);
    assert!(
        app.world()
            .get::<PendingFeathersActivation>(target)
            .is_some()
    );
    assert_eq!(
        app.world().get::<Visibility>(popup),
        Some(&Visibility::Hidden)
    );
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(button));
    app.world_mut()
        .entity_mut(target)
        .remove::<PendingFeathersActivation>();
    tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown, None);
    tap(&mut app, KeyCode::Escape, Key::Escape, None);
    assert_eq!(
        app.world().get::<Visibility>(popup),
        Some(&Visibility::Hidden)
    );
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(button));
    assert!(
        app.world()
            .get::<PendingFeathersActivation>(target)
            .is_none()
    );
}

#[test]
fn native_select_dispatches_once_and_rejects_disabled_foreign_and_stale_rows() {
    let mut app = app(false);
    app.init_resource::<Activations>().add_observer(
        |event: On<Activate>, mut seen: ResMut<Activations>| seen.0.push(event.entity),
    );
    let owner = app
        .world_mut()
        .query_filtered::<Entity, With<FeathersSelect>>()
        .single(app.world())
        .unwrap();
    let row = app
        .world_mut()
        .query_filtered::<Entity, With<EditorAction>>()
        .iter(app.world())
        .next()
        .unwrap();
    let foreign = app
        .world_mut()
        .spawn((EditorAction::ShowAbout, FeathersActionButton))
        .id();
    for (source, value, is_final) in [
        (row, row, true),
        (owner, foreign, true),
        (owner, row, false),
        (owner, row, true),
    ] {
        app.world_mut().trigger(ValueChange {
            source,
            value,
            is_final,
        });
        app.world_mut().flush();
    }
    assert_eq!(app.world().resource::<Activations>().0, vec![row]);
    app.world_mut()
        .entity_mut(owner)
        .insert(InteractionDisabled);
    app.world_mut().trigger(ValueChange {
        source: owner,
        value: row,
        is_final: true,
    });
    app.world_mut().flush();
    app.world_mut()
        .entity_mut(owner)
        .remove::<InteractionDisabled>();
    app.world_mut().entity_mut(row).insert(InteractionDisabled);
    app.world_mut().trigger(ValueChange {
        source: owner,
        value: row,
        is_final: true,
    });
    app.world_mut().flush();
    app.world_mut()
        .entity_mut(row)
        .remove::<InteractionDisabled>();
    // Change the captured owner epoch without rebuilding the row.
    app.world_mut().entity_mut(owner).insert(MenuContext(Some((
        aestra_core::EffectId::new(),
        1,
        1,
    ))));
    app.world_mut().trigger(ValueChange {
        source: owner,
        value: row,
        is_final: true,
    });
    app.world_mut().flush();
    assert_eq!(app.world().resource::<Activations>().0, vec![row]);
}
