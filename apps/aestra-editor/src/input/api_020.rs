//! Bevy 0.20 keyboard API spellings; no ownership or input behavior.
use bevy::{prelude::*, ui_widgets::MenuFocusState};
pub(crate) type MenuFocusInsert<'w, 't> = On<'w, 't, Insert<MenuFocusState>>;
#[cfg(test)]
pub(crate) use bevy::ui_widgets::TextInputPlugin;

#[cfg(test)]
pub(crate) fn text_input_marker() -> impl Bundle {
    bevy::ui_widgets::TextInput
}
