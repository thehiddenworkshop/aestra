//! Exercise the shipping menu scenes with Bevy's real activation and focus controllers.
use super::*;
use bevy::{
    input_focus::{InputFocus, tab_navigation::NavAction},
    picking::{
        backend::HitData,
        events::{PointerClick, PointerPress, PointerRelease},
        pointer::{Location, PointerButton, PointerId},
    },
    ui_widgets::MenuFocusState,
    window::PrimaryWindow,
};

fn app() -> App {
    let mut app = crate::feathers::text_input::tests::app_with(|app| {
        app.insert_resource(test_support::session_with_timing_slack())
            .insert_resource(WorkspaceLayout::default())
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_message::<bevy::input_focus::KeyboardInputSnapshot>()
            .add_plugins((
                crate::input::EditorKeyboardPlugin,
                EditorMenusPlugin::new(true),
            ));
    });
    let localizer = Localizer::new("en-US").unwrap();
    app.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|bar| {
            spawn_file_menu(bar, false, &localizer);
            spawn_edit_menu(bar, &localizer);
            spawn_view_menu(bar, &WorkspaceLayout::default(), true, &localizer);
            spawn_standard_menu(
                bar,
                "menu-help",
                MenuKind::Help,
                &[("help-about", "", EditorAction::ShowAbout)],
                &localizer,
            );
        });
    app.world_mut().flush();
    for _ in 0..3 {
        app.update();
    }
    app
}

fn button(app: &mut App, kind: MenuKind) -> Entity {
    app.world_mut()
        .query::<(Entity, &MenuAction)>()
        .iter(app.world())
        .find_map(|(entity, action)| {
            matches!(action, MenuAction::Toggle(k) if *k == kind).then_some(entity)
        })
        .unwrap()
}

fn popup(app: &mut App, kind: MenuKind) -> Entity {
    app.world_mut()
        .query::<(Entity, &MenuDropdown)>()
        .iter(app.world())
        .find_map(|(entity, dropdown)| (dropdown.0 == kind).then_some(entity))
        .unwrap()
}

fn assert_open(app: &mut App, kind: MenuKind) {
    let popup = popup(app, kind);
    assert_eq!(app.world().resource::<MenuState>().open, Some(kind));
    assert_eq!(
        app.world().get::<Node>(popup).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().get::<Visibility>(popup),
        Some(&Visibility::Visible)
    );
    assert_eq!(
        app.world().get::<MenuFocusState>(popup),
        Some(&MenuFocusState::Open)
    );
    let focus = app
        .world()
        .resource::<InputFocus>()
        .get()
        .expect("menu must own focus");
    let mut ancestor = focus;
    while ancestor != popup {
        ancestor = app
            .world()
            .get::<ChildOf>(ancestor)
            .expect("focused item must belong to the open menu")
            .parent();
    }
}

fn click(app: &mut App, entity: Entity) {
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    let pointer = bevy::picking::events::Pointer::new(
        PointerId::Mouse,
        Location {
            target: bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window))
                .normalize(Some(window))
                .unwrap(),
            position: Vec2::ZERO,
        },
    );
    app.world_mut().trigger(PointerPress {
        entity,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        count: 1,
    });
    app.world_mut().flush();
    app.world_mut().trigger(PointerClick {
        entity,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        duration: Duration::ZERO,
        count: 1,
    });
    app.world_mut().flush();
    app.world_mut().trigger(PointerRelease {
        entity,
        pointer,
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    });
    app.world_mut().flush();
    app.update();
}

#[test]
fn all_top_menus_survive_native_click_release_idle_and_reopen() {
    let mut app = app();
    for kind in [
        MenuKind::File,
        MenuKind::Edit,
        MenuKind::View,
        MenuKind::Help,
    ] {
        let button = button(&mut app, kind);
        click(&mut app, button);
        for _ in 0..3 {
            assert_open(&mut app, kind);
            app.update();
        }
        assert!(
            app.world()
                .get::<PendingFeathersActivation>(button)
                .is_none()
        );
        click(&mut app, button);
        assert!(app.world().resource::<MenuState>().open.is_none());
        let popup = popup(&mut app, kind);
        assert_eq!(
            app.world().get::<Node>(popup).unwrap().display,
            Display::None
        );
        click(&mut app, button);
        assert_open(&mut app, kind);
        app.world_mut().resource_mut::<InputFocus>().clear();
        app.update();
        app.update();
        assert!(app.world().resource::<MenuState>().open.is_none());
        assert_eq!(
            app.world().get::<Visibility>(popup),
            Some(&Visibility::Hidden)
        );
    }
}

#[test]
fn hover_switch_and_native_keyboard_close_keep_editor_state_in_sync() {
    let mut app = app();
    let file = button(&mut app, MenuKind::File);
    click(&mut app, file);
    let edit = button(&mut app, MenuKind::Edit);
    app.world_mut()
        .entity_mut(edit)
        .insert(Interaction::Hovered);
    app.update();
    app.update();
    assert_open(&mut app, MenuKind::Edit);
    let file_popup = popup(&mut app, MenuKind::File);
    assert_eq!(
        app.world().get::<MenuFocusState>(file_popup),
        Some(&MenuFocusState::Closed)
    );
    crate::feathers::text_input::tests::tap(
        &mut app,
        KeyCode::Escape,
        bevy::input::keyboard::Key::Escape,
        None,
    );
    assert!(app.world().resource::<MenuState>().open.is_none());
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(edit));

    // Arrow-key opening is a native MenuEvent, not Activate.
    app.world_mut().trigger(bevy::ui_widgets::MenuEvent {
        source: file,
        action: bevy::ui_widgets::MenuAction::Open(NavAction::Last),
    });
    app.world_mut().flush();
    app.update();
    assert_open(&mut app, MenuKind::File);
    let focused = app.world().resource::<InputFocus>().get().unwrap();
    assert_eq!(
        app.world().get::<DocumentAction>(focused),
        Some(&DocumentAction::Exit)
    );
}
