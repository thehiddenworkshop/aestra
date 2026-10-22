//! Production editor scheduling identities shared by compatibility gates.
use bevy::prelude::*;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EditorSet {
    Setup,
    PreViewport,
    MainUpdate,
    UiRebuild,
    UiSync,
}
