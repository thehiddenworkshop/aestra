//! Existing application action dispatcher still consumes the legacy pressed state.
pub(super) use bevy::prelude::Button as AuditedButton;
pub(super) fn pressed_state() -> impl bevy::prelude::Bundle {
    bevy::prelude::Interaction::Pressed
}
