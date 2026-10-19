//! CPU-side draw observation payload, independent from GPU draw resources.
use bevy::prelude::*;

/// CPU-side observation payload, independent from the readback transport.
#[derive(Clone, Debug, Default)]
pub(super) struct GeometrySample {
    pub owner: Option<Entity>,
    pub token: u32,
    pub time: f32,
    pub instances: u32,
    pub vertices: u64,
    pub primitives: u64,
    pub draws: u32,
}
