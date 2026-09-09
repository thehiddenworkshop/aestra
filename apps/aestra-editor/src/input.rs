//! Shared ownership checks for editor-wide keyboard shortcuts.
mod tab_navigation;
#[cfg(test)]
pub(crate) mod tests;
use bevy::{ecs::system::SystemParam, input_focus::InputFocus, prelude::*, text::EditableText};

pub(crate) struct EditorKeyboardPlugin;

impl Plugin for EditorKeyboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeyboardPresses>()
            .add_systems(
                PreUpdate,
                tab_navigation::sync_tab_eligibility
                    .before(bevy::input_focus::InputFocusSystems::Dispatch),
            )
            .add_systems(
                PreUpdate,
                collect_keypresses.after(bevy::input_focus::InputFocusSystems::Dispatch),
            );
    }
}

#[derive(Resource, Default)]
struct KeyboardPresses(
    Vec<ButtonInput<KeyCode>>,
    Vec<bevy::input_focus::KeyboardInputSnapshot>,
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
fn shortcut_key_state(event: &bevy::input_focus::KeyboardInputSnapshot) -> ButtonInput<KeyCode> {
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

#[derive(SystemParam)]
pub(crate) struct ShortcutContext<'w, 's> {
    focus: Option<Res<'w, InputFocus>>,
    editable: Query<'w, 's, (), With<EditableText>>,
    parents: Query<'w, 's, &'static ChildOf>,
    menu_items: Query<'w, 's, (), With<bevy::ui_widgets::MenuItem>>,
    asset_surfaces: Query<'w, 's, (), With<crate::asset_browser::BrowserSurface>>,
    protection: Option<Res<'w, crate::persistence::DocumentProtectionState>>,
    library: Option<Res<'w, crate::library::LibraryAssetOperationState>>,
    menus: Option<Res<'w, crate::menus::MenuState>>,
    material_palette: Option<Res<'w, crate::material_graph::MaterialGraphPaletteState>>,
    windows: Query<'w, 's, &'static Window>,
}

impl ShortcutContext<'_, '_> {
    /// Panel-local shortcuts must not also edit the timeline behind Assets.
    /// Keep global history/save shortcuts available by not folding this into blocked().
    pub(crate) fn asset_browser_focused(&self) -> bool {
        let mut focused = self.focus.as_ref().and_then(|focus| focus.get());
        while let Some(entity) = focused {
            if self.asset_surfaces.contains(entity) {
                return true;
            }
            focused = self.parents.get(entity).ok().map(ChildOf::parent);
        }
        false
    }

    pub(crate) fn blocked(&self) -> bool {
        if self
            .protection
            .as_ref()
            .is_some_and(|state| state.is_open())
            || self.library.as_ref().is_some_and(|state| state.is_open())
            || self
                .material_palette
                .as_ref()
                .is_some_and(|state| state.is_open())
            || self.menus.as_ref().is_some_and(|state| {
                state.open.is_some()
                    || state.panels_open
                    || state.tab_context.is_some()
                    || state.show_about
            })
            || (!self.windows.is_empty() && !self.windows.iter().any(|window| window.focused))
        {
            return true;
        }
        let mut focused = self.focus.as_ref().and_then(|focus| focus.get());
        while let Some(entity) = focused {
            if self.editable.contains(entity) || self.menu_items.contains(entity) {
                return true;
            }
            focused = self.parents.get(entity).ok().map(ChildOf::parent);
        }
        false
    }
}
