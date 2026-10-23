//! Actual profile ingestion and history ownership, without panel or GPU producer stand-ins.
use super::*;
use crate::test_support;
use aestra_runtime::ProfileValue;

fn frame(effect: &CompiledEffect, cpu: Option<u64>) -> ProjectInstanceProfile {
    let mut profile = EffectProfile::from_compiled(effect);
    if let Some(cpu) = cpu {
        profile.record_cpu_frame(Duration::from_nanos(cpu), &[]);
    }
    ProjectInstanceProfile {
        path: vec![],
        effect: effect.source,
        name: effect.name.clone(),
        profile,
    }
}

#[test]
fn unavailable_cpu_measurements_clear_history_instead_of_inventing_zeroes() {
    let session = test_support::session_with_timing_slack();
    let compiled = session.preview().unwrap().effect();
    let mut state = ProfilerState::default();
    assert!(
        state
            .ingest_project(vec![frame(compiled, Some(15))])
            .profile_rebuilt()
    );
    assert_eq!(state.cpu_history_ns, [15]);
    assert!(
        !state
            .ingest_project(vec![frame(compiled, None)])
            .profile_rebuilt()
    );
    assert!(state.cpu_history_ns.is_empty());
    assert_eq!(
        state.current_profile().unwrap().cpu_time_ns,
        ProfileValue::Unavailable
    );
    state.ingest_project(vec![frame(compiled, Some(0))]);
    assert_eq!(state.cpu_history_ns, [0]);
}

#[test]
fn project_reordering_preserves_history_and_structural_changes_reset_it() {
    let session = test_support::session_with_timing_slack();
    let compiled = session.preview().unwrap().effect();
    let root = frame(compiled, Some(10));
    let mut child = frame(compiled, Some(20));
    child.path.push(aestra_core::EffectClipId::new());
    let mut state = ProfilerState::default();
    assert!(
        state
            .ingest_project(vec![root.clone(), child.clone()])
            .profile_rebuilt()
    );
    assert_eq!(state.cpu_history_ns, [30]);
    assert!(
        !state
            .ingest_project(vec![child.clone(), root.clone()])
            .profile_rebuilt()
    );
    assert_eq!(state.cpu_history_ns, [30, 30]);
    assert!(state.ingest_project(vec![root]).profile_rebuilt());
    assert_eq!(state.cpu_history_ns, [10]);
    assert_eq!(state.project.instances.len(), 1);
}

#[test]
fn partial_project_measurements_do_not_become_a_measured_total() {
    let session = test_support::session_with_timing_slack();
    let compiled = session.preview().unwrap().effect();
    let root = frame(compiled, Some(10));
    let mut child = frame(compiled, None);
    child.path.push(aestra_core::EffectClipId::new());
    let mut state = ProfilerState::default();
    state.ingest_project(vec![root, child]);
    assert_eq!(
        state.current_profile().unwrap().cpu_time_ns,
        ProfileValue::Unavailable
    );
    assert!(state.cpu_history_ns.is_empty());
    assert_eq!(
        state.current_profile().unwrap().gpu_time_ns,
        ProfileValue::Unavailable
    );
}

#[test]
fn resetting_peaks_preserves_identity_and_next_ingestion_does_not_rebuild() {
    let session = test_support::session_with_timing_slack();
    let compiled = session.preview().unwrap().effect();
    let measured = frame(compiled, Some(10));
    let mut state = ProfilerState::default();
    state.ingest_project(vec![measured.clone()]);
    let before = state
        .project
        .instances
        .iter()
        .map(|instance| (instance.path.clone(), instance.effect))
        .collect::<Vec<_>>();
    state.reset_peaks();
    assert!(state.cpu_history_ns.is_empty());
    assert_eq!(
        state
            .project
            .instances
            .iter()
            .map(|instance| (instance.path.clone(), instance.effect))
            .collect::<Vec<_>>(),
        before
    );
    assert!(!state.ingest_project(vec![measured]).profile_rebuilt());
    assert_eq!(state.cpu_history_ns, [10]);
}

#[test]
fn empty_project_retires_previous_instance_metrics_and_stabilizes() {
    let session = test_support::session_with_timing_slack();
    let compiled = session.preview().unwrap().effect();
    let mut state = ProfilerState::default();
    state.ingest_project(vec![frame(compiled, Some(500))]);
    assert!(state.ingest_project(vec![]).profile_rebuilt());
    assert!(state.project.instances.is_empty());
    assert_eq!(
        state.current_profile().unwrap().particle_capacity,
        ProfileValue::Measured(0)
    );
    assert!(!state.cpu_history_ns.contains(&500));
    assert!(!state.ingest_project(vec![]).profile_rebuilt());
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
