//! Host-owned presentation preferences, independent of extraction API versions.
use crate::capabilities::DEFAULT_GPU_PARTICLE_BUDGET;
use bevy::prelude::Resource;

/// Selects the presentation path used by the renderer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PresentationMode {
    /// Select the best supported backend and fall back without panicking.
    #[default]
    Auto,
    /// Simulate and render particles entirely on the GPU.
    Gpu,
    /// Use the deterministic CPU interpreter and pooled Bevy sprites.
    CpuReference,
    /// Simulate on the GPU, read particles back, and present them as Bevy sprites.
    GpuReadback,
}

/// Controls the ordering of stateful particles before transparent blending.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TransparentOrderMode {
    /// Skip sorting for the lowest-latency live path, without the capture sort's 4,096-live bound. The exact blend order
    /// can vary when concurrent GPU allocations place particles in different slots.
    #[default]
    Fast,
    /// Sort up to 4,096 live particles per emitter by spawn ordinal for exact visual captures.
    /// This is intentionally opt-in because the bounded GPU sort has a substantial frame cost.
    StableCapture,
    /// Per-view back-to-front GPU sorting for alpha sprite/flipbook draws, with spawn-identity
    /// tie-breaks. Paged sorting and parallel merges have no 4,096-live capture limit.
    /// Extra work/storage scales with emitter capacity and visible views. Does not interleave
    /// separate draws, sort meshes/ribbons/trails, or change additive/2D rendering.
    DepthBackToFront,
}

#[derive(Resource, Debug, Clone, Copy)]
pub struct AestraRenderSettings {
    pub presentation: PresentationMode,
    /// Application budget applied in addition to physical device limits.
    pub max_gpu_particles: u32,
    pub transparent_order: TransparentOrderMode,
}

impl Default for AestraRenderSettings {
    fn default() -> Self {
        Self {
            presentation: PresentationMode::Auto,
            max_gpu_particles: DEFAULT_GPU_PARTICLE_BUDGET,
            transparent_order: TransparentOrderMode::Fast,
        }
    }
}
