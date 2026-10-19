//! Production preview sample and telemetry ingestion into editor profiler state.
use super::{
    presentation::PreviewPresentedEffect,
    runtime_preview::{EditorPreviewProject, PreviewEffectInstancePath, desired_preview_instances},
};
use crate::{profiler::ProfilerState, session::EditorSession, timeline::TimelineState};
use aestra_bevy_render::PresentedEffect;
use bevy::prelude::*;
use std::{sync::Arc, time::Instant};

pub(super) fn update_preview(
    mut session: ResMut<EditorSession>,
    mut profiler: ResMut<ProfilerState>,
    mut preview: ResMut<EditorPreviewProject>,
    timeline: Res<TimelineState>,
    gpu_stats: Query<
        (
            &PresentedEffect,
            Option<&aestra_bevy_render::gpu::GpuTrailStatistics>,
            &PreviewEffectInstancePath,
            Option<&aestra_bevy_render::gpu::GpuParticleStatistics>,
            Option<&aestra_bevy_render::EffectRuntimeStatus>,
            Option<&aestra_bevy_render::gpu::GpuSimulationTiming>,
            Option<&aestra_bevy_render::gpu::GpuPreparationTiming>,
        ),
        With<PreviewPresentedEffect>,
    >,
    stage_timings: Query<
        (
            &PresentedEffect,
            &PreviewEffectInstancePath,
            &aestra_bevy_render::gpu::GpuStageTiming,
        ),
        With<PreviewPresentedEffect>,
    >,
) {
    session.samples.clear();
    let desired = desired_preview_instances(
        &preview,
        &timeline,
        session.simulation_time(),
        session.preview_seed,
    );
    let mut instance_samples = Vec::new();
    let mut profiles = Vec::new();
    preview.live_particle_count = 0;
    for desired in desired {
        let mut profile = aestra_runtime::EffectProfile::from_compiled(&desired.effect);
        let observed = gpu_stats.iter().find(|(p, _, path, _, _, _, _)| {
            path.0 == desired.path && Arc::ptr_eq(p.effect(), &desired.effect)
        });
        let native_gpu = observed.is_some_and(|(_, _, _, _, runtime, _, _)| {
            runtime.is_some_and(|runtime| runtime.active == aestra_bevy_render::ActiveBackend::Gpu)
        });
        if !native_gpu {
            // Reuse the renderer's evaluation. Headless previews have no presentation entity.
            let (samples, elapsed) = if let Some((presented, _, _, _, _, _, _)) = observed {
                let elapsed = presented.cpu_evaluation_time();
                let samples = if elapsed.is_some() {
                    presented.cpu_samples()
                } else {
                    presented.gpu_samples()
                };
                (samples, elapsed)
            } else {
                let mut instance = aestra_runtime::EffectInstance::new(desired.effect.clone());
                instance.set_seed(desired.seed);
                instance.apply_compiled_parameter_overrides(&desired.parameter_overrides);
                instance.set_playback_time(desired.time);
                let started = Instant::now();
                instance.evaluate(&mut instance_samples);
                (instance_samples.as_slice(), Some(started.elapsed()))
            };
            if let Some(elapsed) = elapsed {
                profile.record_cpu_frame(elapsed, samples);
            } else {
                profile.record_particle_frame(samples);
            }
            profile.record_submitted_frame(&desired.effect, samples);
            preview.live_particle_count += samples.len();
            if desired.path.is_empty() {
                session.samples.extend_from_slice(samples);
            }
        }
        // Extension stages (a fluid) always run on the GPU, whatever the particle backend.
        if let Some((p, _, timing)) = stage_timings
            .iter()
            .find(|(p, path, _)| path.0 == desired.path && Arc::ptr_eq(p.effect(), &desired.effect))
        {
            profile.gpu_stage_time_ns = timing.time_ns(&p.instance);
        }
        profile.record_trail_usage(observed.and_then(|(p, stats, _, _, _, _, _)| {
            stats.and_then(|stats| stats.usage(&p.instance))
        }));
        if let Some((p, _, _, Some(context), Some(runtime), Some(timing), _)) = observed
            && matches!(
                runtime.active,
                aestra_bevy_render::ActiveBackend::Gpu
                    | aestra_bevy_render::ActiveBackend::GpuReadback
            )
        {
            profile.gpu_simulation_time_ns = timing.time_ns(&p.instance, context);
        }
        if let Some((p, _, _, Some(context), Some(runtime), _, Some(preparation))) = observed
            && matches!(
                runtime.active,
                aestra_bevy_render::ActiveBackend::Gpu
                    | aestra_bevy_render::ActiveBackend::GpuReadback
            )
        {
            preparation.record_profile(&p.instance, context, &mut profile);
        }
        if let Some((p, _, _, particles, Some(runtime), _, _)) = observed
            && runtime.active == aestra_bevy_render::ActiveBackend::Gpu
        {
            // Native metrics come only from GPU telemetry; no reference simulation runs here.
            profile.alive_particles = aestra_runtime::ProfileValue::Unavailable;
            profile.peak_particles = aestra_runtime::ProfileValue::Unavailable;
            profile.submitted_instances = aestra_runtime::ProfileValue::Unavailable;
            for emitter in &mut profile.emitters {
                emitter.alive_particles = 0;
                emitter.peak_particles = 0;
            }
            if let Some(particles) = particles {
                particles.record_profile(&p.instance, &mut profile);
            }
            preview.live_particle_count += profile.alive_particles.value().unwrap_or(0) as usize;
        }
        if let Some((p, _, _, Some(particles), Some(runtime), _, _)) = observed
            && matches!(
                runtime.active,
                aestra_bevy_render::ActiveBackend::Gpu
                    | aestra_bevy_render::ActiveBackend::GpuReadback
            )
        {
            particles.record_geometry_profile(&p.instance, &mut profile);
        }
        profiles.push(aestra_runtime::ProjectInstanceProfile {
            path: desired.path,
            effect: desired.effect.source,
            name: desired.effect.name.clone(),
            profile,
        });
    }
    if profiler.ingest_project(profiles).profile_rebuilt() {
        session.ui_revision += 1;
    }
}

#[cfg(test)]
#[path = "telemetry/tests.rs"]
pub(crate) mod tests;
