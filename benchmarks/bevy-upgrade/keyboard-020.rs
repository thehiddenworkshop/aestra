//! Unpatched upstream qualification, not a replacement for editor regressions.
//! The same-frame test must pass before dropping Aestra's modifier patch.
#[cfg(test)]
mod tests {
    use bevy_app::{App, PreUpdate};
    use bevy_ecs::prelude::*;
    use bevy_input::{
        ButtonInput, ButtonState, InputPlugin,
        keyboard::{Key, KeyCode, KeyboardInput},
    };
    use bevy_input_focus::{FocusedInput, InputDispatchPlugin, InputFocus, InputFocusPlugin};
    use bevy_window::{PrimaryWindow, Window};

    #[derive(Resource, Default)]
    struct ObservedControl(Vec<bool>);

    fn observe_shortcut(
        input: On<FocusedInput<KeyboardInput>>,
        keys: Res<ButtonInput<Key>>,
        mut observed: ResMut<ObservedControl>,
    ) {
        if input.input.key_code == KeyCode::KeyA && input.input.state.is_pressed() {
            observed.0.push(keys.pressed(Key::Control));
        }
    }

    fn fixture() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((InputPlugin, InputFocusPlugin, InputDispatchPlugin))
            .init_resource::<ObservedControl>()
            .add_observer(observe_shortcut);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let input = app.world_mut().spawn_empty().id();
        app.world_mut()
            .insert_resource(InputFocus::from_entity(input));
        (app, window)
    }

    fn key(app: &mut App, window: Entity, code: KeyCode, logical: Key, state: ButtonState) {
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: logical,
            state,
            text: None,
            repeat: false,
            window,
        });
    }

    #[test]
    fn held_control_is_visible_to_focused_shortcut_observers() {
        let (mut app, window) = fixture();
        key(
            &mut app,
            window,
            KeyCode::ControlLeft,
            Key::Control,
            ButtonState::Pressed,
        );
        app.world_mut().run_schedule(PreUpdate);
        key(
            &mut app,
            window,
            KeyCode::KeyA,
            Key::Character("a".into()),
            ButtonState::Pressed,
        );
        app.world_mut().run_schedule(PreUpdate);
        assert_eq!(app.world().resource::<ObservedControl>().0, [true]);
    }

    #[test]
    fn same_frame_chord_preserves_event_time_modifiers() {
        let (mut app, window) = fixture();
        for (code, logical, state) in [
            (KeyCode::ControlLeft, Key::Control, ButtonState::Pressed),
            (
                KeyCode::KeyA,
                Key::Character("a".into()),
                ButtonState::Pressed,
            ),
            (
                KeyCode::KeyA,
                Key::Character("a".into()),
                ButtonState::Released,
            ),
            (KeyCode::ControlLeft, Key::Control, ButtonState::Released),
        ] {
            key(&mut app, window, code, logical, state);
        }
        app.world_mut().run_schedule(PreUpdate);
        assert_eq!(
            app.world().resource::<ObservedControl>().0,
            [true],
            "Focused observers must see Ctrl held at A-down, not the frame-end released state"
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<Key>>()
                .pressed(Key::Control)
        );
    }
}
