//! Compatibility fix for modifiers pressed and released between two rendered frames.
use super::{FocusedInput, InputFocus};
use bevy_app::{App, PreUpdate};
use bevy_ecs::prelude::*;
use bevy_input::{
    ButtonInput, InputSystems,
    keyboard::{Key, KeyCode, KeyboardFocusLost, KeyboardInput},
};
use bevy_window::PrimaryWindow;

/// A keyboard event with physical key state at that event, rather than at frame end.
/// Deferred shortcut consumers should use this message; synchronous focused observers
/// receive the same state through the usual `ButtonInput` resources.
#[derive(Message, Clone, Debug)]
pub struct KeyboardInputSnapshot {
    /// The original key event, including its repeat flag and receiving window.
    pub input: KeyboardInput,
    /// Physical held keys and transitions for this event only.
    pub key_codes: ButtonInput<KeyCode>,
}

#[derive(Resource, Default)]
pub(super) struct FrameStart {
    codes: ButtonInput<KeyCode>,
    keys: ButtonInput<Key>,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<FrameStart>()
        .add_message::<KeyboardInputSnapshot>()
        .add_systems(PreUpdate, capture_frame_start.before(InputSystems));
}

fn capture_frame_start(
    codes: Res<ButtonInput<KeyCode>>,
    keys: Res<ButtonInput<Key>>,
    mut start: ResMut<FrameStart>,
) {
    start.codes = codes.clone();
    start.keys = keys.clone();
}

pub(super) fn dispatch_keyboard_input(
    mut reader: MessageReader<KeyboardInput>,
    mut lost: MessageReader<KeyboardFocusLost>,
    start: Res<FrameStart>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut commands: Commands,
) {
    if !lost.is_empty() {
        // There is no ordering information between these two message types. Drop
        // ambiguous input on focus loss instead of issuing a stale edit/shortcut.
        lost.clear();
        reader.clear();
        commands.queue(|world: &mut World| {
            world.resource_mut::<ButtonInput<Key>>().release_all();
        });
        return;
    }
    let Ok(window) = windows.single() else {
        reader.clear();
        return;
    };
    let mut codes = start.codes.clone();
    let mut keys = start.keys.clone();
    for input in reader.read() {
        codes.clear();
        keys.clear();
        if input.state.is_pressed() {
            codes.press(input.key_code);
            keys.press(input.logical_key.clone());
        } else {
            codes.release(input.key_code);
            keys.release(input.logical_key.clone());
        }
        // Logical modifiers have two physical keys: releasing one must not release
        // the other. Keep the normal per-frame state untouched outside dispatch.
        for (logical, left, right) in [
            (Key::Control, KeyCode::ControlLeft, KeyCode::ControlRight),
            (Key::Shift, KeyCode::ShiftLeft, KeyCode::ShiftRight),
            (Key::Alt, KeyCode::AltLeft, KeyCode::AltRight),
            (Key::Super, KeyCode::SuperLeft, KeyCode::SuperRight),
        ] {
            if codes.pressed(left) || codes.pressed(right) {
                keys.press(logical);
            } else {
                keys.release(logical);
            }
        }
        let input = input.clone();
        let event_codes = codes.clone();
        let event_keys = keys.clone();
        commands.queue(move |world: &mut World| {
            world.write_message(KeyboardInputSnapshot {
                input: input.clone(),
                key_codes: event_codes.clone(),
            });
            let focused = world.resource::<InputFocus>().get();
            let focused_entity = focused
                .filter(|entity| world.get_entity(*entity).is_ok())
                .unwrap_or(window);
            if focused.is_some() && focused_entity == window {
                world.resource_mut::<InputFocus>().clear();
            }
            let frame_codes = core::mem::replace(
                &mut *world.resource_mut::<ButtonInput<KeyCode>>(),
                event_codes,
            );
            let frame_keys =
                core::mem::replace(&mut *world.resource_mut::<ButtonInput<Key>>(), event_keys);
            world.trigger(FocusedInput {
                focused_entity,
                input,
                window,
            });
            *world.resource_mut::<ButtonInput<KeyCode>>() = frame_codes;
            *world.resource_mut::<ButtonInput<Key>>() = frame_keys;
        });
    }
}
