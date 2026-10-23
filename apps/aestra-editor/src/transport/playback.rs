//! Actual keyboard transport, action execution and playback advancement.
use crate::{ModulePaletteState, session::EditorSession};
use aestra_core::EffectPlaybackMode;
use bevy::{
    input::{ButtonState, keyboard::KeyboardInput},
    input_focus::InputFocus,
    prelude::*,
};

pub(super) fn register(app: &mut App) {
    app.add_observer(execute_transport_action).add_systems(
        Update,
        (
            transport_keyboard_input.in_set(TransportSet::Input),
            advance_playback.in_set(TransportSet::Playback),
        ),
    );
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TransportSet {
    Input,
    Actions,
    Playback,
    Sync,
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransportAction {
    TogglePlayback,
    Stop,
    TogglePlaybackMode(EffectPlaybackMode),
    Restart,
    StepFrame(i8),
}

pub(super) fn execute_transport_action(
    action: On<TransportAction>,
    mut session: ResMut<EditorSession>,
) {
    match *action {
        TransportAction::TogglePlayback => {
            if session.playing {
                session.playing = false;
            } else if session.effect.playback_mode.is_looping() {
                session.playing = true;
            } else {
                session.restart();
            }
        }
        TransportAction::Stop => session.stop(),
        TransportAction::TogglePlaybackMode(mode) => {
            let mode = if session.effect.playback_mode == mode {
                EffectPlaybackMode::Once
            } else {
                mode
            };
            session.set_effect_playback_mode(mode);
        }
        TransportAction::Restart => session.restart(),
        TransportAction::StepFrame(direction) => session.step_frame(direction),
    }
}

type FocusedKeyboardControls = Or<(
    With<bevy::ui_widgets::MenuItem>,
    With<bevy::ui_widgets::Button>,
)>;

pub(super) fn transport_keyboard_input(
    mut commands: Commands,
    mut keyboard_events: MessageReader<KeyboardInput>,
    input: crate::input::ShortcutKeys,
    palette: Res<ModulePaletteState>,
    focus: Option<Res<InputFocus>>,
    controls: Query<
        (
            Has<bevy::ui_widgets::MenuItem>,
            Has<bevy::ui_widgets::Button>,
        ),
        FocusedKeyboardControls,
    >,
    shortcuts: crate::input::ShortcutContext,
) {
    let repeated_steps = if let Some(events) = input.repeats() {
        keyboard_events.clear();
        events
            .iter()
            .filter_map(|event| {
                repeated_frame_step(
                    &event.input,
                    event.key_codes.pressed(KeyCode::AltLeft)
                        || event.key_codes.pressed(KeyCode::AltRight),
                )
            })
            .collect::<Vec<_>>()
    } else {
        let alt = input
            .iter()
            .any(|keys| keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight));
        keyboard_events
            .read()
            .filter_map(|event| repeated_frame_step(event, alt))
            .collect::<Vec<_>>()
    };
    let (menu_item_focused, button_focused) = focus
        .as_deref()
        .and_then(InputFocus::get)
        .and_then(|entity| controls.get(entity).ok())
        .unwrap_or_default();
    if palette.open || menu_item_focused || shortcuts.blocked() {
        return;
    }
    for keys in input.iter() {
        let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
        // Headless buttons already turn focused Space into Activate. Do not also
        // toggle global playback; R and arrow shortcuts retain their existing policy.
        if keys.just_pressed(KeyCode::Space) && !button_focused {
            commands.trigger(TransportAction::TogglePlayback);
        }
        if keys.just_pressed(KeyCode::KeyR) {
            commands.trigger(TransportAction::Restart);
        }
        if !alt && keys.just_pressed(KeyCode::ArrowLeft) {
            commands.trigger(TransportAction::StepFrame(-1));
        }
        if !alt && keys.just_pressed(KeyCode::ArrowRight) {
            commands.trigger(TransportAction::StepFrame(1));
        }
    }
    for direction in repeated_steps {
        commands.trigger(TransportAction::StepFrame(direction));
    }
}

fn repeated_frame_step(event: &KeyboardInput, alt: bool) -> Option<i8> {
    if alt || !event.repeat || event.state != ButtonState::Pressed {
        return None;
    }
    match event.key_code {
        KeyCode::ArrowLeft => Some(-1),
        KeyCode::ArrowRight => Some(1),
        _ => None,
    }
}

fn advance_playback(time: Res<Time>, mut session: ResMut<EditorSession>) {
    session.advance_playback(time.delta_secs());
}

#[cfg(test)]
#[path = "playback/qualification.rs"]
mod qualification;
#[cfg(test)]
#[path = "tests/playback.rs"]
mod tests;
