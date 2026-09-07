use super::*;
use bevy::{
    input::{
        ButtonState, InputPlugin,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::{InputDispatchPlugin, InputFocusPlugin},
    text::TextEdit,
    ui_widgets::EditableTextInputPlugin,
    window::{Ime, PrimaryWindow},
};

pub(crate) fn keyboard_app() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        InputPlugin,
        InputFocusPlugin,
        InputDispatchPlugin,
        EditableTextInputPlugin,
        EditorKeyboardPlugin,
    ))
    .add_message::<Ime>()
    .init_resource::<UiScale>();
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    let input = app.world_mut().spawn(EditableText::new("Original")).id();
    app.world_mut()
        .get_mut::<EditableText>(input)
        .unwrap()
        .pending_edits
        .clear();
    app.world_mut()
        .insert_resource(InputFocus::from_entity(input));
    (app, window, input)
}

pub(crate) fn key(
    app: &mut App,
    window: Entity,
    code: KeyCode,
    logical_key: Key,
    state: ButtonState,
) {
    let text = match &logical_key {
        Key::Character(s) => Some(s.clone()),
        _ => None,
    };
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key,
        state,
        text,
        repeat: false,
        window,
    });
}

pub(crate) fn tap(app: &mut App, window: Entity, code: KeyCode, logical: Key) {
    key(app, window, code, logical.clone(), ButtonState::Pressed);
    key(app, window, code, logical, ButtonState::Released);
}

#[test]
fn plain_typing_before_and_after_a_chord_is_not_modified() {
    let (mut app, window, input) = keyboard_app();
    tap(&mut app, window, KeyCode::KeyA, Key::Character("a".into()));
    key(
        &mut app,
        window,
        KeyCode::ControlRight,
        Key::Control,
        ButtonState::Pressed,
    );
    tap(&mut app, window, KeyCode::KeyA, Key::Character("a".into()));
    key(
        &mut app,
        window,
        KeyCode::ControlRight,
        Key::Control,
        ButtonState::Released,
    );
    tap(&mut app, window, KeyCode::KeyV, Key::Character("v".into()));
    app.world_mut().run_schedule(PreUpdate);
    let edits = &app
        .world()
        .get::<EditableText>(input)
        .unwrap()
        .pending_edits;
    assert!(
        matches!(edits.as_slice(), [TextEdit::Insert(a), TextEdit::SelectAll, TextEdit::Insert(v)] if a == "a" && v == "v"),
        "{edits:?}"
    );
}

#[test]
fn shortcuts_follow_logical_letters_on_azerty_and_qwertz_without_changing_modifiers() {
    for (physical, logical, expected) in [
        (KeyCode::KeyW, "z", Some(KeyCode::KeyZ)),
        (KeyCode::KeyZ, "w", Some(KeyCode::KeyW)),
        (KeyCode::KeyQ, "a", Some(KeyCode::KeyA)),
        (KeyCode::KeyY, "Z", Some(KeyCode::KeyZ)),
        (KeyCode::KeyM, ";", None),
        (KeyCode::Digit2, "é", Some(KeyCode::Digit2)),
        (KeyCode::Digit1, "1", Some(KeyCode::Digit1)),
        (KeyCode::Numpad1, "1", Some(KeyCode::Numpad1)),
    ] {
        let mut physical_keys = ButtonInput::default();
        physical_keys.press(KeyCode::ControlRight);
        physical_keys.press(KeyCode::ShiftLeft);
        physical_keys.clear();
        physical_keys.press(physical);
        let snapshot = bevy::input_focus::KeyboardInputSnapshot {
            input: KeyboardInput {
                key_code: physical,
                logical_key: Key::Character(logical.into()),
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            },
            key_codes: physical_keys.clone(),
        };
        let mapped = shortcut_key_state(&snapshot);
        assert!(mapped.pressed(KeyCode::ControlRight));
        assert!(mapped.pressed(KeyCode::ShiftLeft));
        assert_eq!(
            mapped.get_just_pressed().copied().collect::<Vec<_>>(),
            expected.into_iter().collect::<Vec<_>>()
        );
        assert!(
            snapshot.key_codes.just_pressed(physical),
            "raw widget input stays unchanged"
        );
    }
}

#[test]
fn held_modifiers_survive_frames_and_releasing_only_one_side() {
    let (mut app, window, input) = keyboard_app();
    key(
        &mut app,
        window,
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Pressed,
    );
    key(
        &mut app,
        window,
        KeyCode::ControlRight,
        Key::Control,
        ButtonState::Pressed,
    );
    app.world_mut().run_schedule(PreUpdate);
    key(
        &mut app,
        window,
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Released,
    );
    key(
        &mut app,
        window,
        KeyCode::ShiftRight,
        Key::Shift,
        ButtonState::Pressed,
    );
    tap(&mut app, window, KeyCode::ArrowLeft, Key::ArrowLeft);
    key(
        &mut app,
        window,
        KeyCode::ShiftRight,
        Key::Shift,
        ButtonState::Released,
    );
    key(
        &mut app,
        window,
        KeyCode::ControlRight,
        Key::Control,
        ButtonState::Released,
    );
    app.world_mut().run_schedule(PreUpdate);
    let edits = &app
        .world()
        .get::<EditableText>(input)
        .unwrap()
        .pending_edits;
    assert!(
        matches!(edits.as_slice(), [TextEdit::WordLeft(true)]),
        "{edits:?}"
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<Key>>()
            .pressed(Key::Control)
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::ShiftRight)
    );
}

#[test]
fn focus_loss_drops_ambiguous_chords_and_does_not_leave_control_stuck() {
    let (mut app, window, input) = keyboard_app();
    key(
        &mut app,
        window,
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Pressed,
    );
    app.world_mut().run_schedule(PreUpdate);
    tap(&mut app, window, KeyCode::KeyA, Key::Character("a".into()));
    app.world_mut()
        .write_message(bevy::input::keyboard::KeyboardFocusLost);
    app.world_mut().run_schedule(PreUpdate);
    assert!(
        app.world()
            .get::<EditableText>(input)
            .unwrap()
            .pending_edits
            .is_empty()
    );
    assert!(app.world().resource::<KeyboardPresses>().0.is_empty());
    assert!(
        !app.world()
            .resource::<ButtonInput<Key>>()
            .pressed(Key::Control)
    );
    tap(&mut app, window, KeyCode::KeyA, Key::Character("a".into()));
    app.world_mut().run_schedule(PreUpdate);
    assert!(
        matches!(app.world().get::<EditableText>(input).unwrap().pending_edits.as_slice(), [TextEdit::Insert(a)] if a == "a")
    );
}

#[test]
fn repeated_text_is_delivered_but_discrete_shortcuts_do_not_repeat() {
    let (mut app, window, input) = keyboard_app();
    key(
        &mut app,
        window,
        KeyCode::KeyA,
        Key::Character("a".into()),
        ButtonState::Pressed,
    );
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::KeyA,
        logical_key: Key::Character("a".into()),
        state: ButtonState::Pressed,
        text: Some("a".into()),
        repeat: true,
        window,
    });
    app.world_mut().run_schedule(PreUpdate);
    assert_eq!(
        app.world()
            .get::<EditableText>(input)
            .unwrap()
            .pending_edits
            .len(),
        2
    );
    assert_eq!(app.world().resource::<KeyboardPresses>().0.len(), 1);
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyA)
    );
    app.world_mut().run_schedule(PreUpdate);
    assert!(app.world().resource::<KeyboardPresses>().0.is_empty());
}

#[test]
fn fast_control_a_and_paste_keep_event_time_modifiers() {
    let (mut app, window, input) = keyboard_app();
    key(
        &mut app,
        window,
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Pressed,
    );
    key(
        &mut app,
        window,
        KeyCode::KeyA,
        Key::Character("a".into()),
        ButtonState::Pressed,
    );
    key(
        &mut app,
        window,
        KeyCode::KeyA,
        Key::Character("a".into()),
        ButtonState::Released,
    );
    key(
        &mut app,
        window,
        KeyCode::KeyV,
        Key::Character("v".into()),
        ButtonState::Pressed,
    );
    key(
        &mut app,
        window,
        KeyCode::KeyV,
        Key::Character("v".into()),
        ButtonState::Released,
    );
    key(
        &mut app,
        window,
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Released,
    );
    app.world_mut().run_schedule(PreUpdate);
    let edits = &app
        .world()
        .get::<EditableText>(input)
        .unwrap()
        .pending_edits;
    assert!(
        matches!(edits.as_slice(), [TextEdit::SelectAll, TextEdit::Paste]),
        "unexpected edits: {edits:?}"
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<Key>>()
            .pressed(Key::Control)
    );
}
