//! Editor shell action identity shared by generic control defaults.
use bevy::prelude::*;
#[derive(Component, Clone, Copy)]
pub(crate) enum EditorAction {
    ShowAbout,
    CloseAbout,
}
