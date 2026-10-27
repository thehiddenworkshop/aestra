//! Separate profiling lane; never interpret timestamp/readback overhead as a plain cost run.
use super::*;
use aestra_bevy::{
    AestraRenderSet, AestraSet,
    gpu::{GpuActivationTiming, GpuSimulationTiming},
};
use bevy::{diagnostic::DiagnosticsStore, render::diagnostic::RenderDiagnosticsPlugin};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Owner {
    cpu: Vec<f64>,
    gpu: Vec<f64>,
    ticks: u64,
    max_ticks: u32,
    observations: u64,
    workgroups: u64,
    checkpoint_bytes: u64,
}

#[derive(Resource, Default)]
pub(super) struct Profile {
    start: Option<Instant>,
    playback_cpu: Vec<f64>,
    encoding_cpu: Vec<f64>,
    gpu: BTreeMap<String, Vec<f64>>,
    seen_gpu: BTreeMap<String, bevy::platform::time::Instant>,
    owners: BTreeMap<String, Owner>,
    owner_sequences: BTreeMap<Entity, u64>,
    activation_seen: BTreeSet<Entity>,
    activations: Vec<serde_json::Value>,
    encoding_sequence: u64,
}

pub(super) fn install(app: &mut App) {
    app.world()
        .resource::<GpuPlaybackDiagnostics>()
        .set_profiling_enabled(true);
    app.add_plugins(RenderDiagnosticsPlugin)
        .init_resource::<Profile>()
        .add_systems(Update, begin.before(AestraSet::Playback))
        .add_systems(Update, end.after(AestraSet::Playback).before(super::record))
        .add_systems(
            Update,
            collect
                .after(AestraRenderSet::Prepare)
                .before(super::record),
        );
}

fn begin(mut profile: ResMut<Profile>, readiness: Res<Readiness>) {
    profile.start = readiness.started.then(Instant::now);
}

fn end(mut profile: ResMut<Profile>) {
    if let Some(start) = profile.start.take() {
        profile
            .playback_cpu
            .push(start.elapsed().as_secs_f64() * 1000.0);
    }
}

#[allow(clippy::type_complexity)]
fn collect(
    mut profile: ResMut<Profile>,
    readiness: Res<Readiness>,
    root: Query<&EffectPlayer, With<ShowRoot>>,
    diagnostics: Res<DiagnosticsStore>,
    ledger: Res<GpuPlaybackDiagnostics>,
    effects: Query<(
        Entity,
        &PresentedEffect,
        &GpuParticleStatistics,
        &GpuSimulationTiming,
        Option<&GpuActivationTiming>,
        Option<&aestra_bevy::EffectClipInstance>,
    )>,
) {
    if !readiness.started {
        return;
    }
    let root_time = root.single().map_or(0.0, EffectPlayer::elapsed);
    for d in diagnostics.iter() {
        let path = d.path().as_str();
        if !path.ends_with("elapsed_gpu") {
            continue;
        }
        if let Some(m) = d.measurement()
            && m.value.is_finite()
            && m.value >= 0.0
            && profile.seen_gpu.get(path).is_none_or(|time| *time < m.time)
        {
            profile.seen_gpu.insert(path.to_owned(), m.time);
            profile
                .gpu
                .entry(path.to_owned())
                .or_default()
                .push(m.value);
        }
    }
    let frame = ledger.snapshot();
    if frame.sequence > profile.encoding_sequence {
        profile.encoding_sequence = frame.sequence;
        if let Some(ns) = frame.cpu_encoding_ns {
            profile.encoding_cpu.push(ns as f64 / 1e6);
        }
        for sample in frame.samples {
            if let Some(ns) = sample.cpu_encoding_ns
                && let Ok((_, effect, _, _, _, _)) = effects.get(sample.owner)
            {
                profile
                    .owners
                    .entry(effect.effect().name.clone())
                    .or_default()
                    .cpu
                    .push(ns as f64 / 1e6);
            }
        }
    }
    for (entity, effect, stats, timing, activation, clip) in &effects {
        if let Some(activation) = activation
            && profile.activation_seen.insert(entity)
        {
            profile.activations.push(json!({"effect":effect.effect().name,"root_seconds":root_time,"cpu_preparation_ms":activation.cpu_preparation_ns as f64/1e6,"cpu_clip_construction_ms":clip.and_then(|c|c.preparation_cpu_time).map(|d|d.as_secs_f64()*1000.0)}));
        }
        if let Some(sample) = timing.frame_sample(&effect.instance, stats)
            && profile
                .owner_sequences
                .get(&entity)
                .is_none_or(|s| *s < sample.sequence)
        {
            profile.owner_sequences.insert(entity, sample.sequence);
            let owner = profile
                .owners
                .entry(effect.effect().name.clone())
                .or_default();
            owner.gpu.push(sample.nanoseconds as f64 / 1e6);
            if let Some(ticks) = sample.work.fixed_ticks {
                owner.ticks += u64::from(ticks);
                owner.max_ticks = owner.max_ticks.max(ticks);
            }
            owner.observations += u64::from(sample.work.trail_observations);
            owner.workgroups += sample.work.trail_workgroups;
            owner.checkpoint_bytes += sample.work.checkpoint_capture_bytes.unwrap_or(0);
        }
    }
    profile
        .owner_sequences
        .retain(|owner, _| effects.contains(*owner));
}

impl Profile {
    pub(super) fn summary(&self) -> serde_json::Value {
        let gpu: BTreeMap<_, _> = self.gpu.iter().map(|(k, v)| (k, distribution(v))).collect();
        let owners: BTreeMap<_,_> = self.owners.iter().map(|(k,o)|(k,json!({"cpu_encoding_ms":distribution(&o.cpu),"gpu_simulation_ms":distribution(&o.gpu),"observed_fixed_ticks":o.ticks,"max_fixed_ticks":o.max_ticks,"trail_observations":o.observations,"trail_workgroups":o.workgroups,"checkpoint_capture_bytes":o.checkpoint_bytes}))).collect();
        json!({"cpu_playback_scope_ms":distribution(&self.playback_cpu),"cpu_simulation_encoding_scope_ms":distribution(&self.encoding_cpu),"gpu_phases_ms":gpu,"owners":owners,"activations":self.activations,
            "scope":"Separate instrumented lane. GPU phases are asynchronous distinct measurement times; per-owner results are deduplicated by owner/sequence and may omit retired or budget-omitted owners. Work totals cover observed timestamp windows only. Nested GPU scopes overlap; do not add parent and child durations. CPU activation excludes render-thread allocation/submission. No replay/seek or screenshot/capacity scan."})
    }
}
