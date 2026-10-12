//! Candidate action controls use the native pressed marker, not deprecated Interaction.
pub(super) use bevy::ui_widgets::Button as AuditedButton;
pub(super) fn pressed_state() -> impl bevy::prelude::Bundle {
    bevy::ui::Pressed
}
