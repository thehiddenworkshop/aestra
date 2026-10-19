//! Per-owner preparation timing read model, independent from render-world setup.
use super::{particle_statistics::GpuParticleStatistics, simulation_timing::GpuSimulationTiming};
use aestra_runtime::{EffectInstance, EffectProfile, ProfileValue};
use bevy::prelude::*;

/// GPU trail compaction and per-view culling, excluding simulation and drawing.
#[derive(Component, Debug, Default)]
pub struct GpuPreparationTiming {
    pub(super) compaction: GpuSimulationTiming,
    pub(super) culling: GpuSimulationTiming,
}

impl GpuPreparationTiming {
    pub fn record_profile(
        &self,
        instance: &EffectInstance,
        context: &GpuParticleStatistics,
        profile: &mut EffectProfile,
    ) {
        if profile.trail_capacity.value() == Some(0) {
            profile.gpu_trail_compaction_time_ns = ProfileValue::Measured(0);
            profile.gpu_trail_culling_time_ns = ProfileValue::Measured(0);
        } else {
            profile.gpu_trail_compaction_time_ns = self.compaction.time_ns(instance, context);
            profile.gpu_trail_culling_time_ns = self.culling.time_ns(instance, context);
        }
    }
}
