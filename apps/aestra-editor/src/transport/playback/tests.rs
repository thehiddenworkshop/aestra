use super::*;
use crate::test_support;

fn sample_session_with_playback_mode(mode: EffectPlaybackMode) -> EditorSession {
    test_support::session_with_playback_mode(mode)
}

#[test]
fn transport_actions_own_playback_mutations() {
    let mut app = App::new();
    let session = sample_session_with_playback_mode(EffectPlaybackMode::LoopRestart);
    assert!(session.playing);
    app.insert_resource(session)
        .add_observer(execute_transport_action);

    app.world_mut().trigger(TransportAction::TogglePlayback);
    app.update();
    assert!(!app.world().resource::<EditorSession>().playing);

    app.world_mut().trigger(TransportAction::Restart);
    app.update();
    let session = app.world().resource::<EditorSession>();
    assert!(session.playing);
    assert_eq!(session.frame(), 0);

    let original_mode = session.effect.playback_mode;
    app.world_mut().trigger(TransportAction::TogglePlaybackMode(
        EffectPlaybackMode::LoopContinuous,
    ));
    app.update();
    let session = app.world().resource::<EditorSession>();
    assert_eq!(
        session.effect.playback_mode,
        EffectPlaybackMode::LoopContinuous
    );
    assert!(session.can_undo());
    app.world_mut().resource_mut::<EditorSession>().undo();
    assert_eq!(
        app.world().resource::<EditorSession>().effect.playback_mode,
        original_mode
    );

    {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        assert!(session.set_effect_playback_mode(EffectPlaybackMode::Once));
        session.step_frame(1);
        assert_eq!(session.frame(), 1);
        assert!(!session.playing);
    }
    app.world_mut().trigger(TransportAction::TogglePlayback);
    app.update();
    let session = app.world().resource::<EditorSession>();
    assert!(session.playing);
    assert_eq!(session.frame(), 0);
}

#[test]
fn changing_playback_mode_preserves_playback_position_and_running_state() {
    let mut app = App::new();
    let mut session = sample_session_with_playback_mode(EffectPlaybackMode::LoopRestart);
    session.advance_playback(0.1);
    let frame = session.frame();
    assert!(frame > 0);
    assert!(session.playing);
    app.insert_resource(session)
        .add_observer(execute_transport_action);

    app.world_mut().trigger(TransportAction::TogglePlaybackMode(
        EffectPlaybackMode::LoopRestart,
    ));
    app.update();

    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect.playback_mode, EffectPlaybackMode::Once);
    assert!(session.playing);
    assert_eq!(session.frame(), frame);
}

#[test]
fn keyboard_input_routes_through_the_transport_action_contract() {
    let mut app = App::new();
    app.insert_resource(test_support::session_with_timing_slack())
        .insert_resource(ButtonInput::<KeyCode>::default())
        .init_resource::<ModulePaletteState>()
        .add_message::<KeyboardInput>()
        .add_observer(execute_transport_action)
        .add_systems(Update, transport_keyboard_input);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);

    app.update();

    assert!(!app.world().resource::<EditorSession>().playing);
}

#[test]
fn focused_menu_item_reserves_transport_shortcuts_for_menu_navigation() {
    let session = test_support::session_with_timing_slack();
    let initially_playing = session.playing;
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(ButtonInput::<KeyCode>::default())
        .init_resource::<ModulePaletteState>()
        .add_message::<KeyboardInput>()
        .add_observer(execute_transport_action)
        .add_systems(Update, transport_keyboard_input);
    let item = app.world_mut().spawn(bevy::ui_widgets::MenuItem).id();
    app.insert_resource(InputFocus::from_entity(item));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);

    app.update();

    assert_eq!(
        app.world().resource::<EditorSession>().playing,
        initially_playing
    );
}

#[test]
fn held_arrow_repeat_events_continue_stepping_frames() {
    let repeat = KeyboardInput {
        key_code: KeyCode::ArrowRight,
        logical_key: bevy::input::keyboard::Key::ArrowRight,
        state: ButtonState::Pressed,
        text: None,
        repeat: true,
        window: Entity::PLACEHOLDER,
    };

    assert_eq!(repeated_frame_step(&repeat, false), Some(1));
    assert_eq!(repeated_frame_step(&repeat, true), None);

    let mut released = repeat.clone();
    released.state = ButtonState::Released;
    assert_eq!(repeated_frame_step(&released, false), None);

    let mut ordinary = repeat;
    ordinary.repeat = false;
    assert_eq!(repeated_frame_step(&ordinary, false), None);
}

#[test]
fn fast_alt_navigation_does_not_step_transport_and_plain_repeats_still_do() {
    use crate::input::tests::{key, keyboard_app, tap};
    use bevy::input::keyboard::Key;
    #[derive(Resource, Default)]
    struct Actions(Vec<TransportAction>);
    let (mut app, window, _) = keyboard_app();
    app.world_mut().resource_mut::<InputFocus>().clear();
    app.init_resource::<ModulePaletteState>()
        .init_resource::<Actions>()
        .add_observer(|event: On<TransportAction>, mut actions: ResMut<Actions>| {
            actions.0.push(*event.event())
        })
        .add_systems(Update, transport_keyboard_input);
    key(
        &mut app,
        window,
        KeyCode::AltLeft,
        Key::Alt,
        ButtonState::Pressed,
    );
    tap(&mut app, window, KeyCode::ArrowLeft, Key::ArrowLeft);
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::ArrowLeft,
        logical_key: Key::ArrowLeft,
        state: ButtonState::Pressed,
        text: None,
        repeat: true,
        window,
    });
    key(
        &mut app,
        window,
        KeyCode::AltLeft,
        Key::Alt,
        ButtonState::Released,
    );
    key(
        &mut app,
        window,
        KeyCode::ArrowRight,
        Key::ArrowRight,
        ButtonState::Pressed,
    );
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::ArrowRight,
        logical_key: Key::ArrowRight,
        state: ButtonState::Pressed,
        text: None,
        repeat: true,
        window,
    });
    app.world_mut().run_schedule(PreUpdate);
    app.world_mut().run_schedule(Update);
    assert_eq!(
        app.world().resource::<Actions>().0,
        [TransportAction::StepFrame(1), TransportAction::StepFrame(1)]
    );
}

#[test]
fn editing_text_reserves_playback_and_frame_shortcuts() {
    let mut app = App::new();
    let mut session = test_support::session_with_timing_slack();
    session.seek_time(0.5);
    let frame = session.frame();
    let playing = session.playing;
    app.insert_resource(session)
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ModulePaletteState>()
        .add_message::<KeyboardInput>()
        .add_observer(execute_transport_action)
        .add_systems(Update, transport_keyboard_input);
    let text = app
        .world_mut()
        .spawn(bevy::text::EditableText::new("typing R and spaces"))
        .id();
    let child = app.world_mut().spawn(ChildOf(text)).id();
    app.insert_resource(InputFocus::from_entity(child));
    for key in [KeyCode::Space, KeyCode::KeyR, KeyCode::ArrowRight] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }
    app.update();
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.frame(), frame);
    assert_eq!(session.playing, playing);
}
