//! Production discrete viewport shortcuts; navigation and rendered action execution stay separate.
use crate::{
    ModulePaletteState,
    viewport::{PreviewCanvas, ViewportAction, ViewportSet},
};
use bevy::{gizmos::transform_gizmo::TransformGizmoMode, prelude::*, ui::RelativeCursorPosition};

#[cfg(test)]
#[path = "tests/keyboard.rs"]
mod tests;

pub(crate) fn register(app: &mut App) {
    app.add_systems(Update, viewport_keyboard_input.in_set(ViewportSet::Input));
}

pub(super) fn viewport_keyboard_input(
    mut commands: Commands,
    input: crate::input::ShortcutKeys,
    palette: Res<ModulePaletteState>,
    canvases: Query<&RelativeCursorPosition, With<PreviewCanvas>>,
    shortcuts: crate::input::ShortcutContext,
) {
    if palette.open || shortcuts.blocked() {
        return;
    }
    for keys in input.iter() {
        let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        if keys.just_pressed(KeyCode::KeyG) && !control {
            commands.trigger(ViewportAction::ToggleGrid);
        }
        // Enlarge/reduce works from anywhere, not just with the cursor over the canvas: once
        // maximized the canvas fills the editor, and the shortcut is how you get back.
        if keys.just_pressed(KeyCode::F11) {
            commands.trigger(ViewportAction::ToggleMaximize);
        }
        if !canvases.iter().any(RelativeCursorPosition::cursor_over) {
            continue;
        }
        if keys.just_pressed(KeyCode::KeyF) || keys.just_pressed(KeyCode::Home) {
            commands.trigger(ViewportAction::FramePreview);
        }
        if keys.just_pressed(KeyCode::Digit1) {
            commands.trigger(ViewportAction::SetTransformGizmoMode(
                TransformGizmoMode::Translate,
            ));
        }
        if keys.just_pressed(KeyCode::Digit2) {
            commands.trigger(ViewportAction::SetTransformGizmoMode(
                TransformGizmoMode::Rotate,
            ));
        }
        if keys.just_pressed(KeyCode::Digit3) {
            commands.trigger(ViewportAction::SetTransformGizmoMode(
                TransformGizmoMode::Scale,
            ));
        }
    }
}
