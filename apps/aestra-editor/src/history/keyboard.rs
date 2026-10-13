//! History shortcut dispatch from the actual layout-aware event-time key snapshots.
//! Application-wide focus/modal/palette ownership uses the actual shared context.
use crate::{history::HistoryAction, input::ShortcutKeys};
use bevy::prelude::*;

pub(crate) fn history_keyboard_input(
    mut commands: Commands,
    input: ShortcutKeys,
    palette: Res<crate::ModulePaletteState>,
    shortcuts: crate::input::ShortcutContext,
) {
    dispatch(&input, palette.open || shortcuts.blocked(), &mut commands);
}

pub(crate) fn dispatch(input: &ShortcutKeys, blocked: bool, commands: &mut Commands) {
    if blocked {
        return;
    }
    for keys in input.iter() {
        let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if control && keys.just_pressed(KeyCode::KeyZ) {
            commands.trigger(if shift {
                HistoryAction::Redo
            } else {
                HistoryAction::Undo
            });
        }
        if control && keys.just_pressed(KeyCode::KeyY) {
            commands.trigger(HistoryAction::Redo);
        }
    }
}
