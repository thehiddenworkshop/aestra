//! Runtime profile ingestion, presentation, and Profiler workspace actions.
#[path = "profiler/control_input.rs"]
mod control_input;
mod controls;
mod plugin;
mod state;
mod workspace;
pub(crate) use plugin::{EditorProfilerPlugin, ProfilerSet};
pub(crate) use state::ProfilerState;
pub(crate) use workspace::spawn_profiler_workspace;

#[cfg(test)]
use crate::*;
#[cfg(test)]
use aestra_runtime::{EffectProfile, ProfileValue, ProfileValueSource, ProjectInstanceProfile};
#[cfg(test)]
use state::{PROFILER_HISTORY_SAMPLES, ProfilerFrameSample, ProfilerIngestOutcome};
#[cfg(test)]
use std::time::Duration;
#[cfg(test)]
use workspace::{
    ProfilerMetric, profile_source_label, profiler_metric_display, profiler_metric_source,
    project_profile_text,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn project_ingestion_tracks_active_paths_and_rebuilds_only_on_structure_changes() {
        let session = test_support::session_with_timing_slack();
        let compiled = session.preview().unwrap().effect();
        let root = ProjectInstanceProfile {
            path: Vec::new(),
            effect: compiled.source,
            name: compiled.name.clone(),
            profile: EffectProfile::from_compiled(compiled),
        };
        let mut child = root.clone();
        child.path.push(aestra_core::EffectClipId::new());
        child.profile.trail_capacity = ProfileValue::Measured(4);
        child
            .profile
            .record_trail_usage(Some(aestra_runtime::TrailUsage {
                occupied: 3,
                retired: 1,
                truncated: 1,
                evictions: 2,
            }));
        let mut state = ProfilerState::default();
        assert!(
            state
                .ingest_project(vec![root.clone(), child.clone()])
                .profile_rebuilt()
        );
        let profile = state.profile.as_ref().unwrap();
        assert_eq!(
            profile.particle_capacity.value(),
            Some(2 * compiled.max_particles as u32)
        );
        assert_eq!(profile.occupied_trails, ProfileValue::Measured(3));
        assert_eq!(profile.truncated_trails, ProfileValue::Measured(1));
        assert!(
            !state
                .ingest_project(vec![child, root.clone()])
                .profile_rebuilt()
        );
        for locale in ["en-US", "fr-FR"] {
            let localizer = Localizer::new(locale).unwrap();
            let text = project_profile_text(&state, &localizer);
            assert!(text.contains(&localizer.text("profiler-metric-live-particles")));
            assert!(text.contains(&localizer.text("profiler-metric-gpu-time")));
            assert!(text.contains(&localizer.text("profiler-metric-gpu-trail-compaction")));
            assert!(text.contains(&localizer.text("profiler-metric-gpu-trail-culling")));
            assert!(text.contains(&state.project.instances[1].path[0].to_string()));
        }
        assert!(state.ingest_project(vec![root]).profile_rebuilt());
        assert_eq!(
            state.profile.as_ref().unwrap().occupied_trails,
            ProfileValue::Measured(0)
        );
        state.ingest_project(Vec::new());
        assert_eq!(
            state.profile.as_ref().unwrap().particle_capacity,
            ProfileValue::Measured(0)
        );
    }

    #[test]
    fn point_budget_warning_is_actionable_and_clears_with_telemetry() {
        let session = test_support::session_with_timing_slack();
        let mut profile = EffectProfile::from_compiled(session.preview().unwrap().effect());
        for locale in ["en-US", "fr-FR"] {
            let localizer = Localizer::new(locale).unwrap();
            profile.record_trail_usage(Some(aestra_runtime::TrailUsage {
                truncated: 2,
                ..default()
            }));
            let (message, color) = profiler_metric_source(
                &profile,
                ProfilerMetric::TruncatedTrails,
                ProfileValueSource::Measured,
                &localizer,
            );
            assert_eq!(message, localizer.text("profiler-trail-point-warning"));
            assert_eq!(color, Color::srgb(1.0, 0.65, 0.25));
            for usage in [Some(aestra_runtime::TrailUsage::default()), None] {
                profile.record_trail_usage(usage);
                let (_, source) =
                    profiler_metric_display(&profile, ProfilerMetric::TruncatedTrails);
                let (message, _) = profiler_metric_source(
                    &profile,
                    ProfilerMetric::TruncatedTrails,
                    source,
                    &localizer,
                );
                assert_eq!(message, profile_source_label(source, &localizer));
            }
        }
    }

    #[test]
    fn frame_ingestion_preserves_preview_state_and_provenance() {
        let session = test_support::session_with_timing_slack();
        let mut baseline = session.preview().unwrap().clone();
        let mut profiled = baseline.clone();
        baseline.seek(1.25);
        profiled.seek(1.25);
        let mut baseline_samples = Vec::new();
        let mut profiled_samples = Vec::new();
        baseline.evaluate(&mut baseline_samples);
        profiled.evaluate(&mut profiled_samples);
        assert_eq!(profiled_samples, baseline_samples);

        let time_before = profiled.time();
        let seed_before = profiled.seed();
        let mut profiler = ProfilerState::default();
        assert_eq!(
            profiler.ingest(ProfilerFrameSample::new(
                profiled.effect(),
                &profiled_samples,
                Duration::from_micros(125),
            )),
            ProfilerIngestOutcome::ProfileRebuilt
        );
        assert_eq!(profiled.time(), time_before);
        assert_eq!(profiled.seed(), seed_before);
        let profile = profiler.profile.as_ref().unwrap();
        assert_eq!(
            profile.alive_particles,
            ProfileValue::Measured(profiled_samples.len() as u32)
        );
        assert_eq!(profile.gpu_time_ns, ProfileValue::Unavailable);
        assert_eq!(
            profile
                .emitters
                .iter()
                .map(|emitter| emitter.alive_particles)
                .sum::<u32>(),
            profiled_samples.len() as u32
        );
        let expected_submissions = profiled_samples.iter().fold(0_u32, |total, sample| {
            total.saturating_add(
                profiled.effect().emitters[sample.emitter_index]
                    .renderers
                    .len() as u32,
            )
        });
        assert_eq!(
            profile.submitted_instances,
            ProfileValue::Measured(expected_submissions)
        );

        baseline.advance(1.0 / 60.0);
        profiled.advance(1.0 / 60.0);
        baseline.evaluate(&mut baseline_samples);
        profiled.evaluate(&mut profiled_samples);
        assert_eq!(profiled_samples, baseline_samples);
    }

    #[test]
    fn history_is_bounded_and_resettable() {
        let session = test_support::session_with_timing_slack();
        let compiled = session.preview().unwrap().effect();
        let mut profiler = ProfilerState::default();
        for frame in 0..(PROFILER_HISTORY_SAMPLES + 12) {
            profiler.ingest(ProfilerFrameSample::new(
                compiled,
                &session.samples,
                Duration::from_micros(frame as u64 + 1),
            ));
        }
        assert_eq!(profiler.cpu_history_ns.len(), PROFILER_HISTORY_SAMPLES);
        let profile = profiler.profile.as_mut().unwrap();
        profile.alive_particles = ProfileValue::Measured(3);
        profile.peak_particles = ProfileValue::Measured(9);
        profile.emitters[0].alive_particles = 2;
        profile.emitters[0].peak_particles = 7;
        profiler.reset_peaks();
        assert!(profiler.cpu_history_ns.is_empty());
        let profile = profiler.profile.as_ref().unwrap();
        assert_eq!(profile.peak_particles, ProfileValue::Measured(3));
        assert_eq!(profile.emitters[0].peak_particles, 2);
    }

    #[test]
    fn subsequent_frames_update_the_existing_profile() {
        let session = test_support::session_with_timing_slack();
        let compiled = session.preview().unwrap().effect();
        let mut profiler = ProfilerState::default();
        profiler.ingest(ProfilerFrameSample::new(
            compiled,
            &session.samples,
            Duration::from_micros(1),
        ));
        assert_eq!(
            profiler.ingest(ProfilerFrameSample::new(
                compiled,
                &session.samples,
                Duration::from_micros(2),
            )),
            ProfilerIngestOutcome::Updated
        );
        assert_eq!(profiler.cpu_history_ns.len(), 2);
    }
}
