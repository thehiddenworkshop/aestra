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

#[derive(Default)]
struct EncodingOperation {
    frames_ms: Vec<f64>,
    calls: u64,
    buffers: u64,
    buffer_bytes: u64,
    initialized_bytes: u64,
    bind_groups: u64,
    dispatches: u64,
    copy_bytes: u64,
    queue_write_bytes: u64,
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
    operations: BTreeMap<&'static str, EncodingOperation>,
    scratch_peak: [u64; 3],
    scratch_last: [u64; 3],
    parameter_peak: [u64; 5],
    parameter_last: [u64; 5],
    preparation_sequence: u64,
    preparation_cpu: Vec<f64>,
    preparation_births: Vec<serde_json::Value>,
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
    let preparation = ledger.stateful_preparation_snapshot();
    if preparation.sequence > profile.preparation_sequence {
        profile.preparation_sequence = preparation.sequence;
        profile
            .preparation_cpu
            .push(preparation.cpu_ns as f64 / 1e6);
        if preparation.allocated_emitters > 0 {
            profile.preparation_births.push(json!({"observed_root_seconds":root_time,
                "allocated_emitters":preparation.allocated_emitters,"cpu_ms":preparation.cpu_ns as f64/1e6}));
        }
    }
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
        for sample in frame.encoding_operations.into_iter().flatten() {
            let op = profile.operations.entry(sample.name).or_default();
            op.frames_ms.push(sample.cpu_ns as f64 / 1e6);
            op.calls += sample.calls;
            op.buffers += sample.buffers;
            op.buffer_bytes += sample.buffer_bytes;
            op.initialized_bytes += sample.initialized_bytes;
            op.bind_groups += sample.bind_groups;
            op.dispatches += sample.dispatches;
            op.copy_bytes += sample.copy_bytes;
            op.queue_write_bytes += sample.queue_write_bytes;
        }
        if let Some(scratch) = frame.event_scratch {
            profile.scratch_last = [scratch.links, scratch.buffers, scratch.bytes];
            for i in 0..3 {
                profile.scratch_peak[i] = profile.scratch_peak[i].max(profile.scratch_last[i]);
            }
        }
        if let Some(s) = frame.parameter_scratch {
            profile.parameter_last = [
                s.destinations,
                s.destination_bytes,
                s.bind_groups,
                s.upload_pages,
                s.upload_bytes,
            ];
            for i in 0..5 {
                profile.parameter_peak[i] =
                    profile.parameter_peak[i].max(profile.parameter_last[i]);
            }
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
        let operations: BTreeMap<_, _> = self.operations.iter().map(|(name, op)| (name, json!({
            "observed_frame_cpu_ms": distribution(&op.frames_ms), "calls":op.calls,
            "buffers":op.buffers,"requested_buffer_bytes":op.buffer_bytes,
            "initialized_bytes":op.initialized_bytes,"bind_groups":op.bind_groups,"dispatches":op.dispatches,
            "copy_bytes":op.copy_bytes,"queue_write_bytes":op.queue_write_bytes
        }))).collect();
        json!({"cpu_playback_scope_ms":distribution(&self.playback_cpu),"cpu_simulation_encoding_scope_ms":distribution(&self.encoding_cpu),"gpu_phases_ms":gpu,"owners":owners,"activations":self.activations,
            "encoding_operations":operations,
            "stateful_render_preparation":{"cpu_ms":distribution(&self.preparation_cpu),"observed_allocation_windows":self.preparation_births,"scope":"Latest CPU stateful preparation system windows only; excludes ShaderBuffer asset preparation, other render systems, submission, driver allocation and GPU execution. May skip frames."},
            "parameter_scratch_retained":{"order":["destinations","destination_bytes","bind_groups","upload_pages","upload_bytes"],"observed_peak":self.parameter_peak,"last":self.parameter_last,"scope":"CPU-visible GPU handle ownership, not driver residency."},
            "event_scratch_retained":{"order":["links","buffers","bytes"],"observed_peak":self.scratch_peak,"last":self.scratch_last,"scope":"Render-world cache ownership, not driver residency or completed-submission frees."},
            "encoding_operations_scope":"Instrumented simulation-thread API call sites in observed latest-ledger frames only. Buffer bytes are requested sizes; initialized bytes are create-with-data payloads, not measured PCIe traffic. CPU scopes exclude parameter-byte construction and GPU execution. Initial render-world preparation, uninstrumented helpers, driver allocations and skipped ledger frames are not counted. Gather/spawn scopes include command encoding; other scopes measure allocation/bind-group calls.",
            "scope":"Separate instrumented lane. GPU phases are asynchronous distinct measurement times; per-owner results are deduplicated by owner/sequence and may omit retired or budget-omitted owners. Work totals cover observed timestamp windows only. Nested GPU scopes overlap; do not add parent and child durations. CPU activation excludes render-thread allocation/submission. No replay/seek or screenshot/capacity scan."})
    }
}
