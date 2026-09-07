//! Shared ownership checks for editor-wide keyboard shortcuts.
#[cfg(test)]
pub(crate) mod tests;
use bevy::{ecs::system::SystemParam, input_focus::InputFocus, prelude::*, text::EditableText};

pub(crate) struct EditorKeyboardPlugin;

impl Plugin for EditorKeyboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeyboardPresses>().add_systems(
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
            presses.0.push(event.key_codes.clone());
        }
    }
}

/// Discrete shortcuts use event-time held keys, never frame-end modifier state.
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
    protection: Option<Res<'w, crate::persistence::DocumentProtectionState>>,
    library: Option<Res<'w, crate::library::LibraryAssetOperationState>>,
    menus: Option<Res<'w, crate::menus::MenuState>>,
    material_palette: Option<Res<'w, crate::material_graph::MaterialGraphPaletteState>>,
    windows: Query<'w, 's, &'static Window>,
}

impl ShortcutContext<'_, '_> {
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
