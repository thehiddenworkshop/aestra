//! Pointer-anchored context menus shared by editor panels.
#[path = "context_menu/dismissal.rs"]
mod dismissal;
pub(crate) use dismissal::should_dismiss_pointer_context_menu;
#[path = "context_menu/intent.rs"]
mod intent;
pub(crate) use intent::{keyboard_context_menu_requested, pointer_position_in_node};

#[path = "context_menu/composition.rs"]
mod composition;
#[cfg(test)]
use super::pointer::PointerClick;
#[cfg(test)]
use bevy::{
    input_focus::{InputFocus, tab_navigation::TabIndex},
    prelude::*,
    ui_widgets::{Activate, MenuFocusState, MenuPopup},
};
pub(crate) use composition::*;

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[derive(Component)]
    struct TestAction;

    #[derive(Resource, Default)]
    struct EscapedClicks(usize);

    #[test]
    fn hover_opens_flyout_and_sibling_hover_closes_it_while_clicks_stay_in_menu() {
        use super::super::pointer::tests::{click, over};
        use bevy::window::PrimaryWindow;
        let mut app = crate::feathers::text_input::tests::app();
        app.init_resource::<EscapedClicks>();
        let host = app
            .world_mut()
            .commands()
            .spawn(Node::default())
            .observe(|_: On<PointerClick>, mut escaped: ResMut<EscapedClicks>| {
                escaped.0 += 1;
            })
            .with_children(|parent| {
                spawn_pointer_context_menu(parent, Vec2::ZERO, (), (), |menu| {
                    spawn_pointer_context_submenu(menu, "Arrange", |items| {
                        spawn_pointer_context_menu_item(items, "Selection", TestAction);
                    });
                    spawn_pointer_context_menu_item(menu, "Copy", TestAction);
                });
            })
            .id();
        app.world_mut().flush();
        app.update();
        let (trigger, panel, window) = {
            let world = app.world_mut();
            let (trigger, panel) = world
                .query::<(Entity, &PointerContextSubmenu)>()
                .single(world)
                .map(|(e, submenu)| (e, submenu.0))
                .unwrap();
            let window = world
                .query_filtered::<Entity, With<PrimaryWindow>>()
                .single(world)
                .unwrap();
            (trigger, panel, window)
        };
        let root = app.world().get::<ChildOf>(trigger).unwrap().parent();
        let sibling = app.world().get::<Children>(root).unwrap()[1];
        app.world_mut().trigger(over(trigger, window));
        assert_eq!(
            app.world().get::<Node>(panel).unwrap().display,
            Display::Flex
        );
        app.world_mut().trigger(over(sibling, window));
        assert_eq!(
            app.world().get::<Node>(panel).unwrap().display,
            Display::None
        );
        app.world_mut().trigger(click(sibling, window));
        assert_eq!(app.world().resource::<EscapedClicks>().0, 0);
        // Prove the observer was installed: a click outside the menu reaches it.
        app.world_mut().trigger(click(host, window));
        assert_eq!(app.world().resource::<EscapedClicks>().0, 1);
    }

    #[test]
    fn submenu_is_compact_and_opens_inside_the_root_focus_scope() {
        // Navigation in 0.20 checks propagated visibility. Exercise the real UI
        // schedule rather than leaving all required InheritedVisibility values hidden.
        let mut app = crate::feathers::text_input::tests::app();
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

    #[test]
    fn keyboard_opens_and_returns_from_submenu_without_closing_root() {
        use crate::feathers::text_input::tests::{focus, tap};
        use bevy::input::keyboard::Key;
        let mut app = crate::feathers::text_input::tests::app();
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                spawn_pointer_context_menu(parent, Vec2::ZERO, (), (), |menu| {
                    spawn_pointer_context_submenu(menu, "Arrange", |items| {
                        spawn_pointer_context_menu_item(items, "Selection", TestAction);
                    });
                });
            });
        app.world_mut().flush();
        app.update();
        let (trigger, panel) = {
            let world = app.world_mut();
            world
                .query::<(Entity, &PointerContextSubmenu)>()
                .single(world)
                .map(|(entity, submenu)| (entity, submenu.0))
                .unwrap()
        };
        let root = app.world().get::<ChildOf>(trigger).unwrap().parent();
        focus(&mut app, trigger);
        tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight, None);
        let first = app.world().get::<Children>(panel).unwrap()[0];
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(first));
        assert_eq!(
            app.world().get::<Node>(panel).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<MenuFocusState>(root),
            Some(&MenuFocusState::Open)
        );
        tap(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft, None);
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(trigger));
        assert_eq!(
            app.world().get::<Node>(panel).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<MenuFocusState>(root),
            Some(&MenuFocusState::Open)
        );
    }
}
