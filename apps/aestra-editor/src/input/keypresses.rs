//! Actual event-time keyboard collection and layout-aware shortcut key snapshots.
use bevy::{ecs::system::SystemParam, prelude::*};

pub(crate) fn register(app: &mut App) {
    app.init_resource::<KeyboardPresses>().add_systems(
        PreUpdate,
        collect_keypresses.after(bevy::input_focus::InputFocusSystems::Dispatch),
    );
}

#[derive(Resource, Default)]
pub(super) struct KeyboardPresses(
    pub(super) Vec<ButtonInput<KeyCode>>,
    pub(super) Vec<bevy::input_focus::KeyboardInputSnapshot>,
);

fn collect_keypresses(
    mut events: MessageReader<bevy::input_focus::KeyboardInputSnapshot>,
    mut presses: ResMut<KeyboardPresses>,
) {
    presses.0.clear();
    presses.1.clear();
    for event in events.read().filter(|event| event.input.state.is_pressed()) {
        if event.input.repeat {
            presses.1.push(event.clone());
        } else {
            presses.0.push(shortcut_key_state(event));
        }
    }
}

/// Menu shortcuts denote letters, not QWERTY scan-code positions. Keep the
/// event-time modifier state but translate the current letter through the OS layout.
pub(super) fn shortcut_key_state(
    event: &bevy::input_focus::KeyboardInputSnapshot,
) -> ButtonInput<KeyCode> {
    use bevy::input::keyboard::Key;
    let mut keys = event.key_codes.clone();
    if let Key::Character(text) = &event.input.logical_key {
        let letters = [
            KeyCode::KeyA,
            KeyCode::KeyB,
            KeyCode::KeyC,
            KeyCode::KeyD,
            KeyCode::KeyE,
            KeyCode::KeyF,
            KeyCode::KeyG,
            KeyCode::KeyH,
            KeyCode::KeyI,
            KeyCode::KeyJ,
            KeyCode::KeyK,
            KeyCode::KeyL,
            KeyCode::KeyM,
            KeyCode::KeyN,
            KeyCode::KeyO,
            KeyCode::KeyP,
            KeyCode::KeyQ,
            KeyCode::KeyR,
            KeyCode::KeyS,
            KeyCode::KeyT,
            KeyCode::KeyU,
            KeyCode::KeyV,
            KeyCode::KeyW,
            KeyCode::KeyX,
            KeyCode::KeyY,
            KeyCode::KeyZ,
        ];
        let logical_letter = text.len() == 1 && text.as_bytes()[0].is_ascii_alphabetic();
        // Replace letter transitions even when the layout produces punctuation,
        // but preserve physical number-row/numpad shortcuts (viewport 1/2/3).
        if logical_letter || letters.contains(&event.input.key_code) {
            keys.clear();
        }
        if logical_letter {
            let letter = letters[(text.as_bytes()[0].to_ascii_lowercase() - b'a') as usize];
            keys.reset(letter);
            keys.press(letter);
        }
    }
    keys
}

/// Discrete shortcuts use layout-aware letters and event-time held modifiers.
/// The fallback supports small ECS tests/tools that inject ButtonInput directly
/// without the native keyboard-dispatch plugin.
#[derive(SystemParam)]
pub(crate) struct ShortcutKeys<'w> {
    presses: Option<Res<'w, KeyboardPresses>>,
    keys: Res<'w, ButtonInput<KeyCode>>,
}

impl ShortcutKeys<'_> {
    pub(crate) fn repeats(&self) -> Option<&[bevy::input_focus::KeyboardInputSnapshot]> {
        self.presses.as_ref().map(|presses| presses.1.as_slice())
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = &ButtonInput<KeyCode>> {
        self.presses
            .as_ref()
            .map_or_else(
                || std::slice::from_ref(&*self.keys),
                |presses| presses.0.as_slice(),
            )
            .iter()
    }
}
