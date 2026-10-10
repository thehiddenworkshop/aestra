//! Host-selected asynchronous light transport budgets and frame identity.
use bevy::prelude::Resource;
use std::time::Duration;

/// Host-controlled transport budgets, independent of authored/per-output selection.
/// All storage is bounded by these budgets, not by the number of source particles.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct ParticleLightReadbackSettings {
    pub max_lights: u32,
    pub max_in_flight: usize,
    pub max_staging_bytes: u64,
    /// Maximum manifest bytes per in-flight snapshot (including clip-path data).
    pub max_manifest_bytes: usize,
    pub max_age: Duration,
    pub max_frame_lag: u64,
}
impl Default for ParticleLightReadbackSettings {
    fn default() -> Self {
        Self {
            max_lights: 96,
            max_in_flight: 3,
            max_staging_bytes: 1024 * 1024,
            max_manifest_bytes: 1024 * 1024,
            max_age: Duration::from_millis(100),
            max_frame_lag: 8,
        }
    }
}
#[derive(Resource, Clone, Copy, Default)]
pub struct ParticleLightReadbackFrame(pub u64);
