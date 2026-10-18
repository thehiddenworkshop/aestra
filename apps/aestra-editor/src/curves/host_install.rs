//! Existing host-curve cancellation registration, shared with the native plugin.
use super::{controls::handle_curves_actions, host_editing::cancel_drag, plugin::CurvesSet};
use bevy::prelude::*;

pub(super) fn install(app: &mut App) {
    app.add_systems(
        Update,
        cancel_drag
            .before(handle_curves_actions)
            .in_set(CurvesSet::Actions),
    );
}
