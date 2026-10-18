//! Production profile aggregation, peak tracking and bounded CPU history.
#[cfg(test)]
use aestra_runtime::{CompiledEffect, ParticleSample};
use aestra_runtime::{EffectProfile, ProjectInstanceProfile, ProjectProfile};
use bevy::prelude::*;
use std::collections::VecDeque;
#[cfg(test)]
use std::time::Duration;

pub(super) const PROFILER_HISTORY_SAMPLES: usize = 96;

/// One measured preview frame supplied to the Profiler without transferring ownership.
///
/// The producer owns simulation and timing. The Profiler exclusively owns aggregation,
/// compiler estimates, peak tracking, and bounded history.
#[derive(Debug, Clone, Copy)]
#[cfg(test)]
pub(crate) struct ProfilerFrameSample<'a> {
    effect: &'a CompiledEffect,
    particles: &'a [ParticleSample],
    cpu_time: Duration,
    trails: Option<aestra_runtime::TrailUsage>,
}

#[cfg(test)]
impl<'a> ProfilerFrameSample<'a> {
    pub(crate) const fn new(
        effect: &'a CompiledEffect,
        particles: &'a [ParticleSample],
        cpu_time: Duration,
    ) -> Self {
        Self {
            effect,
            particles,
            cpu_time,
            trails: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProfilerIngestOutcome {
    Updated,
    ProfileRebuilt,
}

impl ProfilerIngestOutcome {
    pub(crate) const fn profile_rebuilt(self) -> bool {
        matches!(self, Self::ProfileRebuilt)
    }
}

#[derive(Resource, Default)]
pub(crate) struct ProfilerState {
    pub(super) profile: Option<EffectProfile>,
    pub(super) project: ProjectProfile,
    pub(super) cpu_history_ns: VecDeque<u64>,
}

impl ProfilerState {
    #[cfg(test)]
    pub(crate) fn current_profile(&self) -> Option<&EffectProfile> {
        self.profile.as_ref()
    }

    pub(crate) fn ingest_project(
        &mut self,
        instances: Vec<ProjectInstanceProfile>,
    ) -> ProfilerIngestOutcome {
        let initial = self.profile.is_none();
        let changed = self.project.update(instances);
        if changed {
            self.cpu_history_ns.clear();
        }
        self.profile = Some(self.project.total.clone());
        if let Some(cpu) = self.project.total.cpu_time_ns.value() {
            self.cpu_history_ns.push_back(cpu);
            while self.cpu_history_ns.len() > PROFILER_HISTORY_SAMPLES {
                self.cpu_history_ns.pop_front();
            }
        } else {
            self.cpu_history_ns.clear();
        }
        if initial || changed {
            ProfilerIngestOutcome::ProfileRebuilt
        } else {
            ProfilerIngestOutcome::Updated
        }
    }

    #[cfg(test)]
    pub(crate) fn ingest(&mut self, sample: ProfilerFrameSample<'_>) -> ProfilerIngestOutcome {
        let mut profile = EffectProfile::from_compiled(sample.effect);
        profile.record_cpu_frame(sample.cpu_time, sample.particles);
        profile.record_submitted_frame(sample.effect, sample.particles);
        profile.record_trail_usage(sample.trails);
        self.ingest_project(vec![ProjectInstanceProfile {
            path: Vec::new(),
            effect: sample.effect.source,
            name: sample.effect.name.clone(),
            profile,
        }])
    }

    pub(super) fn reset_peaks(&mut self) {
        self.project.reset_peaks();
        if let Some(profile) = &mut self.profile {
            profile.reset_peaks();
        }
        self.cpu_history_ns.clear();
    }
}

#[cfg(test)]
#[path = "state/tests.rs"]
mod tests;
