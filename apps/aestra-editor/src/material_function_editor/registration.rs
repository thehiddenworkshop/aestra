//! Complete function editor registration shared by the native and qualification roots.
use crate::material_function_editor::{graph, signature_input};
use bevy::prelude::*;

pub(crate) fn register(app: &mut App) {
    graph::register(app);
    signature_input::register(app);
}
