// Exercise exactly the same behavioral requirements as the unpatched probe.
#[path = "../keyboard-020.rs"]
mod upstream_regression;

use bevy::{
    input::{
        ButtonState, InputPlugin,
        keyboard::{Key, KeyCode, KeyboardFocusLost, KeyboardInput},
    },
    input_focus::{
        FocusedInput, InputDispatchPlugin, InputFocus, InputFocusPlugin, InputFocusSystems,
        KeyboardInputSnapshot,
    },
    prelude::*,
    text::{EditableText, TextEdit},
    ui_widgets::{TextInput, TextInputPlugin},
    window::{Ime, PrimaryWindow},
};

#[derive(Resource, Default)]
struct Snapshots(Vec<KeyboardInputSnapshot>);

fn collect(mut events: MessageReader<KeyboardInputSnapshot>, mut snapshots: ResMut<Snapshots>) {
    snapshots.0.extend(events.read().cloned());
}

fn fixture() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        InputPlugin,
        InputFocusPlugin,
        InputDispatchPlugin,
        TextInputPlugin,
    ))
    .add_message::<Ime>()
    .init_resource::<UiScale>()
    .init_resource::<Snapshots>()
    .add_systems(PreUpdate, collect.after(InputFocusSystems::Dispatch));
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    let input = app
        .world_mut()
        .spawn((EditableText::new("Original"), TextInput))
        .id();
    app.world_mut()
        .get_mut::<EditableText>(input)
        .unwrap()
        .pending_edits
        .clear();
    app.world_mut()
        .insert_resource(InputFocus::from_entity(input));
    (app, window, input)
}

fn key(app: &mut App, window: Entity, code: KeyCode, logical_key: Key, state: ButtonState) {
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

fn tap(app: &mut App, window: Entity, code: KeyCode, logical: Key) {
    key(app, window, code, logical.clone(), ButtonState::Pressed);
    key(app, window, code, logical, ButtonState::Released);
}

fn control(app: &mut App, window: Entity, code: KeyCode, state: ButtonState) {
    key(app, window, code, Key::Control, state);
}

#[test]
fn upstream_text_input_fast_select_all_and_paste_do_not_insert_shortcut_letters() {
    let (mut app, window, input) = fixture();
    control(&mut app, window, KeyCode::ControlLeft, ButtonState::Pressed);
    tap(&mut app, window, KeyCode::KeyA, Key::Character("a".into()));
    tap(&mut app, window, KeyCode::KeyV, Key::Character("v".into()));
    control(
        &mut app,
        window,
        KeyCode::ControlLeft,
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
        "{edits:?}"
    );
    // Only queue edits: this fixture never accesses the OS clipboard.
    assert!(
        !app.world()
            .resource::<ButtonInput<Key>>()
            .pressed(Key::Control)
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::ControlLeft)
    );
    let snapshots = &app.world().resource::<Snapshots>().0;
    assert_eq!(snapshots.len(), 6);
    for snapshot in snapshots
        .iter()
        .filter(|s| s.input.key_code == KeyCode::KeyA || s.input.key_code == KeyCode::KeyV)
    {
        assert!(snapshot.key_codes.pressed(KeyCode::ControlLeft));
    }
    assert!(
        !snapshots
            .last()
            .unwrap()
            .key_codes
            .pressed(KeyCode::ControlLeft)
    );
}

#[test]
fn upstream_text_input_typing_before_and_after_same_frame_chord_is_plain() {
    let (mut app, window, input) = fixture();
    tap(&mut app, window, KeyCode::KeyA, Key::Character("a".into()));
    control(
        &mut app,
        window,
        KeyCode::ControlRight,
        ButtonState::Pressed,
    );
    tap(&mut app, window, KeyCode::KeyA, Key::Character("a".into()));
    control(
        &mut app,
        window,
        KeyCode::ControlRight,
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
fn upstream_text_input_keeps_other_control_side_and_shift_across_frames() {
    let (mut app, window, input) = fixture();
    control(&mut app, window, KeyCode::ControlLeft, ButtonState::Pressed);
    control(
        &mut app,
        window,
        KeyCode::ControlRight,
        ButtonState::Pressed,
    );
    app.world_mut().run_schedule(PreUpdate);
    control(
        &mut app,
        window,
        KeyCode::ControlLeft,
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
    control(
        &mut app,
        window,
        KeyCode::ControlRight,
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
}

#[test]
fn focus_loss_drops_ambiguous_edits_and_snapshots_then_recovers() {
    let (mut app, window, input) = fixture();
    control(&mut app, window, KeyCode::ControlLeft, ButtonState::Pressed);
    app.world_mut().run_schedule(PreUpdate);
    app.world_mut().resource_mut::<Snapshots>().0.clear();
    tap(&mut app, window, KeyCode::KeyA, Key::Character("a".into()));
    app.world_mut().write_message(KeyboardFocusLost);
    app.world_mut().run_schedule(PreUpdate);
    assert!(
        app.world()
            .get::<EditableText>(input)
            .unwrap()
            .pending_edits
            .is_empty()
    );
    assert!(app.world().resource::<Snapshots>().0.is_empty());
    assert!(
        !app.world()
            .resource::<ButtonInput<Key>>()
            .pressed(Key::Control)
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::ControlLeft)
    );
    tap(&mut app, window, KeyCode::KeyA, Key::Character("a".into()));
    app.world_mut().run_schedule(PreUpdate);
    assert!(
        matches!(app.world().get::<EditableText>(input).unwrap().pending_edits.as_slice(), [TextEdit::Insert(a)] if a == "a")
    );
}

#[test]
fn repeat_text_and_snapshot_repeat_flags_are_preserved() {
    let (mut app, window, input) = fixture();
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
    let edits = &app
        .world()
        .get::<EditableText>(input)
        .unwrap()
        .pending_edits;
    assert!(
        matches!(edits.as_slice(), [TextEdit::Insert(a), TextEdit::Insert(b)] if a == "a" && b == "a")
    );
    let snapshots = &app.world().resource::<Snapshots>().0;
    assert_eq!(snapshots.len(), 2);
    assert!(!snapshots[0].input.repeat);
    assert!(snapshots[1].input.repeat);
    assert!(!snapshots[1].key_codes.just_pressed(KeyCode::KeyA));
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyA)
    );
}

#[test]
fn upstream_text_input_layout_aware_select_all_uses_logical_key() {
    let (mut app, window, input) = fixture();
    control(&mut app, window, KeyCode::ControlLeft, ButtonState::Pressed);
    // AZERTY physical Q, logical A; the dispatcher must not remap raw widget keys.
    tap(&mut app, window, KeyCode::KeyQ, Key::Character("a".into()));
    control(
        &mut app,
        window,
        KeyCode::ControlLeft,
        ButtonState::Released,
    );
    app.world_mut().run_schedule(PreUpdate);
    assert!(matches!(
        app.world()
            .get::<EditableText>(input)
            .unwrap()
            .pending_edits
            .as_slice(),
        [TextEdit::SelectAll]
    ));
    assert_eq!(
        app.world().resource::<Snapshots>().0[1].input.key_code,
        KeyCode::KeyQ
    );
}

#[derive(Resource, Default)]
struct Targets(Vec<Entity>);

#[test]
fn missing_focused_entity_falls_back_to_primary_window_and_clears_focus() {
    let (mut app, window, input) = fixture();
    app.init_resource::<Targets>().add_observer(
        |event: On<FocusedInput<KeyboardInput>>, mut targets: ResMut<Targets>| {
            targets.0.push(event.focused_entity);
        },
    );
    app.world_mut().despawn(input);
    tap(&mut app, window, KeyCode::KeyA, Key::Character("a".into()));
    app.world_mut().run_schedule(PreUpdate);
    assert_eq!(app.world().resource::<Targets>().0, [window, window]);
    assert_eq!(app.world().resource::<InputFocus>().get(), None);
}
