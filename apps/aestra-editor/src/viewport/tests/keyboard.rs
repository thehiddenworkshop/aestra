//! Real keyboard dispatch and production viewport action routing on both engines.
use super::*;
use crate::{
    asset_actions::AssetOperationState,
    feathers::code_editor::CodeEditor,
    input::EditorKeyboardPlugin,
    material_function_editor::graph::FunctionGraphMenuState,
    material_graph::MaterialGraphPaletteState,
    menus::{MenuKind, MenuState, TabContextMenu},
    persistence::DocumentProtectionState,
};
use bevy::{
    input::{
        ButtonState, InputPlugin,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::{InputDispatchPlugin, InputFocus, InputFocusPlugin},
    text::EditableText,
    time::TimeUpdateStrategy,
    window::PrimaryWindow,
};
use std::time::Duration;

#[derive(Resource, Default)]
struct Actions(Vec<ViewportAction>);

struct Fixture {
    app: App,
    window: Entity,
    canvas: Entity,
}

impl Fixture {
    fn new(hovered: bool) -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
            EditorKeyboardPlugin,
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
        .init_resource::<ModulePaletteState>()
        .init_resource::<Actions>()
        .add_observer(|action: On<ViewportAction>, mut actions: ResMut<Actions>| {
            actions.0.push(*action);
        });
        register(&mut app);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let canvas = app
            .world_mut()
            .spawn((
                PreviewCanvas,
                RelativeCursorPosition {
                    cursor_over: hovered,
                    normalized: None,
                },
            ))
            .id();
        app.update();
        Self {
            app,
            window,
            canvas,
        }
    }

    fn key(&mut self, code: KeyCode, logical: Key, state: ButtonState, repeat: bool) {
        self.app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: logical,
            state,
            repeat,
            text: None,
            window: self.window,
        });
    }

    fn tap(&mut self, code: KeyCode, logical: Key) {
        self.key(code, logical.clone(), ButtonState::Pressed, false);
        self.key(code, logical, ButtonState::Released, false);
    }

    fn shortcuts(&mut self) {
        self.tap(KeyCode::KeyG, Key::Character("g".into()));
        self.tap(KeyCode::F11, Key::F11);
        self.tap(KeyCode::Home, Key::Home);
        self.tap(KeyCode::Digit2, Key::Character("é".into()));
    }

    fn update(&mut self) {
        self.app.update();
    }

    fn assert_actions(&self, expected: &[ViewportAction]) {
        assert_eq!(self.app.world().resource::<Actions>().0, expected);
    }

    fn assert_blocked(&mut self) {
        self.shortcuts();
        self.update();
        self.assert_actions(&[]);
    }
}

#[test]
fn grid_and_maximize_are_global_but_frame_and_modes_need_hover() {
    let mut fixture = Fixture::new(false);
    fixture.shortcuts();
    fixture.update();
    fixture.assert_actions(&[ViewportAction::ToggleGrid, ViewportAction::ToggleMaximize]);
}

#[test]
fn hovered_canvas_routes_all_modes_and_both_frame_keys_in_event_order() {
    let mut fixture = Fixture::new(true);
    fixture.shortcuts();
    fixture.tap(KeyCode::KeyF, Key::Character("f".into()));
    fixture.tap(KeyCode::Digit1, Key::Character("&".into()));
    fixture.tap(KeyCode::Digit3, Key::Character("\"".into()));
    fixture.update();
    fixture.assert_actions(&[
        ViewportAction::ToggleGrid,
        ViewportAction::ToggleMaximize,
        ViewportAction::FramePreview,
        ViewportAction::SetTransformGizmoMode(TransformGizmoMode::Rotate),
        ViewportAction::FramePreview,
        ViewportAction::SetTransformGizmoMode(TransformGizmoMode::Translate),
        ViewportAction::SetTransformGizmoMode(TransformGizmoMode::Scale),
    ]);
}

#[test]
fn multiple_same_frame_taps_are_not_collapsed_or_replayed_next_frame() {
    let mut fixture = Fixture::new(true);
    for _ in 0..2 {
        fixture.tap(KeyCode::F11, Key::F11);
        fixture.tap(KeyCode::KeyG, Key::Character("g".into()));
    }
    fixture.update();
    let expected = [
        ViewportAction::ToggleMaximize,
        ViewportAction::ToggleGrid,
        ViewportAction::ToggleMaximize,
        ViewportAction::ToggleGrid,
    ];
    fixture.assert_actions(&expected);
    fixture.update();
    fixture.assert_actions(&expected);
}

#[test]
fn grid_and_frame_follow_logical_letters_not_qwerty_scan_codes() {
    let mut fixture = Fixture::new(true);
    fixture.tap(KeyCode::KeyG, Key::Character("p".into()));
    fixture.tap(KeyCode::KeyF, Key::Character("g".into()));
    fixture.tap(KeyCode::KeyP, Key::Character("F".into()));
    fixture.tap(KeyCode::KeyG, Key::Character("%".into()));
    fixture.update();
    fixture.assert_actions(&[ViewportAction::ToggleGrid, ViewportAction::FramePreview]);
}

#[test]
fn physical_number_row_remains_available_on_azerty_but_numpad_is_not_remapped() {
    let mut fixture = Fixture::new(true);
    fixture.tap(KeyCode::Digit1, Key::Character("&".into()));
    fixture.tap(KeyCode::Digit2, Key::Character("é".into()));
    fixture.tap(KeyCode::Digit3, Key::Character("\"".into()));
    fixture.tap(KeyCode::Numpad1, Key::Character("1".into()));
    fixture.update();
    fixture.assert_actions(&[
        ViewportAction::SetTransformGizmoMode(TransformGizmoMode::Translate),
        ViewportAction::SetTransformGizmoMode(TransformGizmoMode::Rotate),
        ViewportAction::SetTransformGizmoMode(TransformGizmoMode::Scale),
    ]);
}

#[test]
fn grid_uses_event_time_control_state_for_both_control_keys() {
    let mut fixture = Fixture::new(true);
    for control in [KeyCode::ControlLeft, KeyCode::ControlRight] {
        fixture.key(control, Key::Control, ButtonState::Pressed, false);
        fixture.tap(KeyCode::KeyG, Key::Character("g".into()));
        fixture.key(control, Key::Control, ButtonState::Released, false);
        fixture.tap(KeyCode::KeyG, Key::Character("g".into()));
    }
    fixture.update();
    fixture.assert_actions(&[ViewportAction::ToggleGrid, ViewportAction::ToggleGrid]);
}

#[test]
fn existing_modifier_policy_is_preserved_for_frame_maximize_and_modes() {
    let mut fixture = Fixture::new(true);
    fixture.key(
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Pressed,
        false,
    );
    fixture.shortcuts();
    fixture.key(
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Released,
        false,
    );
    fixture.key(KeyCode::AltLeft, Key::Alt, ButtonState::Pressed, false);
    fixture.tap(KeyCode::KeyG, Key::Character("g".into()));
    fixture.update();
    fixture.assert_actions(&[
        ViewportAction::ToggleMaximize,
        ViewportAction::FramePreview,
        ViewportAction::SetTransformGizmoMode(TransformGizmoMode::Rotate),
        ViewportAction::ToggleGrid,
    ]);
}

#[test]
fn held_keys_and_os_repeats_never_repeat_discrete_viewport_actions() {
    let mut fixture = Fixture::new(true);
    fixture.key(
        KeyCode::KeyG,
        Key::Character("g".into()),
        ButtonState::Pressed,
        false,
    );
    fixture.update();
    fixture.assert_actions(&[ViewportAction::ToggleGrid]);
    fixture.update();
    for (code, logical) in [
        (KeyCode::KeyG, Key::Character("g".into())),
        (KeyCode::F11, Key::F11),
        (KeyCode::Home, Key::Home),
        (KeyCode::Digit2, Key::Character("é".into())),
    ] {
        fixture.key(code, logical, ButtonState::Pressed, true);
    }
    fixture.update();
    fixture.assert_actions(&[ViewportAction::ToggleGrid]);
}

#[test]
fn direct_button_input_fallback_and_no_canvas_preserve_global_actions() {
    let mut app = App::new();
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::KeyG);
    keys.press(KeyCode::F11);
    keys.press(KeyCode::Home);
    app.insert_resource(keys)
        .init_resource::<ModulePaletteState>()
        .init_resource::<Actions>()
        .add_observer(|action: On<ViewportAction>, mut actions: ResMut<Actions>| {
            actions.0.push(*action)
        });
    register(&mut app);
    app.update();
    assert_eq!(
        app.world().resource::<Actions>().0,
        [ViewportAction::ToggleGrid, ViewportAction::ToggleMaximize]
    );
}

#[test]
fn unrelated_hovered_entities_do_not_own_the_viewport_and_multiple_canvases_do_not_duplicate() {
    let mut fixture = Fixture::new(false);
    fixture.app.world_mut().spawn(RelativeCursorPosition {
        cursor_over: true,
        normalized: None,
    });
    fixture.tap(KeyCode::Home, Key::Home);
    fixture.update();
    fixture.assert_actions(&[]);
    for _ in 0..2 {
        fixture.app.world_mut().spawn((
            PreviewCanvas,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: None,
            },
        ));
    }
    fixture.tap(KeyCode::Home, Key::Home);
    fixture.update();
    fixture.assert_actions(&[ViewportAction::FramePreview]);
}

#[test]
fn editable_and_code_editor_descendant_focus_own_all_viewport_shortcuts() {
    for code in [false, true] {
        let mut fixture = Fixture::new(true);
        let field = if code {
            fixture
                .app
                .world_mut()
                .spawn(CodeEditor::new("shader".into()))
                .id()
        } else {
            fixture
                .app
                .world_mut()
                .spawn(EditableText::new("Name"))
                .id()
        };
        let child = fixture.app.world_mut().spawn(ChildOf(field)).id();
        fixture.app.insert_resource(InputFocus::from_entity(child));
        fixture.assert_blocked();
    }
}

#[test]
fn module_palette_and_document_or_asset_modals_block_the_actual_consumer() {
    let mut fixture = Fixture::new(true);
    fixture
        .app
        .world_mut()
        .resource_mut::<ModulePaletteState>()
        .open = true;
    fixture.assert_blocked();
    fixture
        .app
        .world_mut()
        .resource_mut::<ModulePaletteState>()
        .open = false;
    for state in DocumentProtectionState::blocking_states_for_test() {
        fixture.app.insert_resource(state);
        fixture.assert_blocked();
    }
    fixture
        .app
        .insert_resource(DocumentProtectionState::default());
    fixture
        .app
        .insert_resource(AssetOperationState::open_for_test());
    fixture.assert_blocked();
}

#[test]
fn material_and_function_transient_menus_block_the_actual_consumer() {
    let mut fixture = Fixture::new(true);
    for state in MaterialGraphPaletteState::blocking_states_for_test() {
        fixture.app.insert_resource(state);
        fixture.assert_blocked();
    }
    fixture
        .app
        .insert_resource(MaterialGraphPaletteState::default());
    for state in FunctionGraphMenuState::blocking_states_for_test() {
        fixture.app.insert_resource(state);
        fixture.assert_blocked();
    }
}

#[test]
fn top_level_panel_tab_context_about_and_menu_item_focus_block() {
    let mut fixture = Fixture::new(true);
    for state in [
        MenuState {
            open: Some(MenuKind::File),
            ..default()
        },
        MenuState {
            panels_open: true,
            ..default()
        },
        MenuState {
            tab_context: Some(TabContextMenu {
                panel: crate::docking::ToolPanel::Viewport,
                position: [0.0, 0.0],
            }),
            ..default()
        },
        MenuState {
            show_about: true,
            ..default()
        },
    ] {
        fixture.app.insert_resource(state);
        fixture.assert_blocked();
    }
    fixture.app.insert_resource(MenuState::default());
    let menu_item = fixture
        .app
        .world_mut()
        .spawn(bevy::ui_widgets::MenuItem)
        .id();
    fixture
        .app
        .insert_resource(InputFocus::from_entity(menu_item));
    fixture.assert_blocked();
}

#[test]
fn lost_window_focus_blocks_until_any_editor_window_is_focused() {
    let mut fixture = Fixture::new(true);
    fixture
        .app
        .world_mut()
        .get_mut::<Window>(fixture.window)
        .unwrap()
        .focused = false;
    fixture.assert_blocked();
    fixture.app.world_mut().spawn(Window::default());
    fixture.tap(KeyCode::F11, Key::F11);
    fixture.update();
    fixture.assert_actions(&[ViewportAction::ToggleMaximize]);
}

#[test]
fn closing_a_modal_does_not_replay_blocked_keys_but_new_keys_work() {
    let mut fixture = Fixture::new(true);
    fixture
        .app
        .world_mut()
        .resource_mut::<ModulePaletteState>()
        .open = true;
    fixture.assert_blocked();
    fixture
        .app
        .world_mut()
        .resource_mut::<ModulePaletteState>()
        .open = false;
    fixture.update();
    fixture.assert_actions(&[]);
    fixture.shortcuts();
    fixture.update();
    fixture.assert_actions(&[
        ViewportAction::ToggleGrid,
        ViewportAction::ToggleMaximize,
        ViewportAction::FramePreview,
        ViewportAction::SetTransformGizmoMode(TransformGizmoMode::Rotate),
    ]);
}

#[test]
fn losing_hover_and_despawning_the_canvas_stop_local_shortcuts_only() {
    let mut fixture = Fixture::new(true);
    fixture
        .app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(fixture.canvas)
        .unwrap()
        .cursor_over = false;
    fixture.tap(KeyCode::Home, Key::Home);
    fixture.update();
    fixture.assert_actions(&[]);
    fixture.app.world_mut().despawn(fixture.canvas);
    fixture.shortcuts();
    fixture.update();
    fixture.assert_actions(&[ViewportAction::ToggleGrid, ViewportAction::ToggleMaximize]);
}

#[test]
fn flight_keys_do_not_dispatch_discrete_actions_or_mutate_the_held_key_resource() {
    let mut fixture = Fixture::new(true);
    for (code, text) in [
        (KeyCode::KeyW, "w"),
        (KeyCode::KeyA, "a"),
        (KeyCode::KeyS, "s"),
        (KeyCode::KeyD, "d"),
    ] {
        fixture.key(
            code,
            Key::Character(text.into()),
            ButtonState::Pressed,
            false,
        );
    }
    fixture.update();
    fixture.assert_actions(&[]);
    let keys = fixture.app.world().resource::<ButtonInput<KeyCode>>();
    for code in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD] {
        assert!(keys.pressed(code));
    }
}
