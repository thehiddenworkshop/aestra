//! Context-valid simulation timing read model, independent from timestamp transport.
use super::{
    particle_statistics::GpuParticleStatistics,
    timestamp_observation::{GpuSimulationWork, Sample},
};
use aestra_runtime::{EffectInstance, ProfileValue};
use bevy::prelude::*;

/// Latest context-valid GPU simulation observation. Includes all replay dispatches
/// for this instance in the sampled frame, but never its draw calls or CPU work.
#[derive(Component, Debug, Default)]
pub struct GpuSimulationTiming {
    pub(super) sample: Option<Sample>,
}

/// One context-valid asynchronous frame result; sequence identifies a timestamp
/// batch and lets consumers avoid counting a retained result more than once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuSimulationFrame {
    pub sequence: u64,
    pub nanoseconds: u64,
    /// Requested playback time in seconds, not proof that paced catch-up reached it.
    pub requested_time: f32,
    pub work: GpuSimulationWork,
}

impl GpuSimulationTiming {
    /// Returns no frame when timestamps or complete-window work metadata are
    /// unavailable. Legacy mixed analytic/stateful partial windows are excluded.
    pub fn frame_sample(
        &self,
        instance: &EffectInstance,
        context: &GpuParticleStatistics,
    ) -> Option<GpuSimulationFrame> {
        let sample = self.sample.as_ref().filter(|sample| {
            context.context_token(instance) == Some(sample.token) && sample.time <= instance.time()
        })?;
        Some(GpuSimulationFrame {
            sequence: sample.sequence,
            nanoseconds: sample.nanoseconds,
            requested_time: sample.time,
            work: sample.work?,
        })
    }

    pub fn time_ns(
        &self,
        instance: &EffectInstance,
        context: &GpuParticleStatistics,
    ) -> ProfileValue<u64> {
        self.sample
            .as_ref()
            .filter(|sample| {
                context.context_token(instance) == Some(sample.token)
                    && sample.time <= instance.time()
            })
            .map_or(ProfileValue::Unavailable, |sample| {
                ProfileValue::Measured(sample.nanoseconds)
            })
    }
}
