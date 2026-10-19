//! Timestamp payloads shared by transport and main-world consumers.
use bevy::prelude::*;

/// Work encoded in the same simulation window as a GPU timestamp pair.
/// Counts describe submitted work, not asynchronously read-back live populations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuSimulationWork {
    /// Shared fixed ticks actually advanced. Unknown for independent/analytic paths.
    pub fixed_ticks: Option<u32>,
    /// All history observations, including initialization or a dirty paused frame.
    pub trail_observations: u32,
    /// Total dispatched history workgroups across every observation (not ribbons).
    pub trail_workgroups: u64,
    /// GPU bytes actually copied to particle/trail checkpoints in a coupled
    /// window. Unknown for other paths; excludes domain checkpoints and restores.
    pub checkpoint_capture_bytes: Option<u64>,
}

#[derive(Clone, Debug)]
pub(super) struct Sample {
    pub owner: Entity,
    pub token: u32,
    pub time: f32,
    pub nanoseconds: u64,
    pub sequence: u64,
    pub work: Option<GpuSimulationWork>,
}
