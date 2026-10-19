//! Actual viewport identity shared by graph wire layers and pointer consumers.
use bevy::prelude::*;
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FeathersGraphWireLayer {
    pub(crate) viewport: Entity,
}
