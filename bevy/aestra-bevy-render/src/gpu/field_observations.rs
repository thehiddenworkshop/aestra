//! Main-world field and trail observations, independent from device preparation.
use aestra_runtime::{EffectInstance, ProfileValue};
use bevy::prelude::*;

/// Asynchronously observed owner-pool counters, invalidated by history discontinuities.
#[derive(Component, Debug, Default)]
pub struct GpuTrailStatistics {
    pub(super) epoch: Option<u32>,
    pub(super) usage: aestra_runtime::TrailUsage,
}

impl GpuTrailStatistics {
    pub fn usage(
        &self,
        instance: &aestra_runtime::EffectInstance,
    ) -> Option<aestra_runtime::TrailUsage> {
        (self.epoch == Some(instance.history_epoch())).then_some(self.usage)
    }
}

/// The latest measured GPU time of an instance's extension stages (fluid F1): every tick simulated in
/// the most recent frame that advanced them, catch-up and replay included.
#[derive(Component, Debug, Default, Clone)]
pub struct GpuStageTiming {
    pub(super) sample: Option<(f32, u64)>,
}

impl GpuStageTiming {
    /// The measured time, when it was taken at or before the instance's current time.
    pub fn time_ns(&self, instance: &EffectInstance) -> ProfileValue<u64> {
        self.sample
            .filter(|(time, _)| *time <= instance.time())
            .map_or(ProfileValue::Unavailable, |(_, nanoseconds)| {
                ProfileValue::Measured(nanoseconds)
            })
    }
}
