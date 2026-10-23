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
fn actual_combo_scene_has_focusable_actions_and_only_one_selection_indicator() {
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
        assert!(w.get::<MenuItem>(e).is_some());
        assert!(w.get::<FeathersActionButton>(e).is_some());
        assert_eq!(w.get::<TabIndex>(e).unwrap().0, 0);
        let indicator = w.get::<Children>(e).unwrap()[0];
        let count = w.get::<Children>(indicator).map_or(0, Children::len);
        assert_eq!(
            count,
            usize::from(matches!(action, EditorAction::ShowAbout))
        );
    }
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
    let menu = w.get::<ChildOf>(button).unwrap().parent();
    let wrapper = w.get::<ChildOf>(menu).unwrap().parent();
    let node = w.get::<Node>(wrapper).unwrap();
    assert_eq!(node.width, Val::Px(160.0));
    assert_eq!(node.min_width, Val::Px(112.0));
}
