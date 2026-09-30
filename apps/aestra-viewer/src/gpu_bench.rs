//! GPU-timestamp benchmark capture.
//!
//! Runs the viewer for a fixed number of frames, samples the render diagnostics
//! store each frame, and writes percentile statistics for Aestra's GPU passes to
//! JSON. Unlike the eyeballed 1-second averages of `LogDiagnosticsPlugin`, this
//! records a per-frame distribution (p50/p95/p99) so the numbers are meaningful
//! above the timestamp noise floor.
//!
//! Requires `RenderDiagnosticsPlugin` and a GPU with timestamp-query support
//! (Vulkan/DX12); diagnostics whose GPU value is unavailable simply produce no
//! samples.

use std::collections::BTreeMap;
use std::path::PathBuf;

use aestra_bevy::{EffectProfiler, ProfileValue, gpu::GpuEventLinkStatistics};
use bevy::app::AppExit;
use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use serde::Serialize;

/// Default frames measured after warm-up, chosen to sit well above the GPU
/// timestamp noise floor.
pub const DEFAULT_GPU_BENCH_FRAMES: usize = 600;
/// Default warm-up frames skipped before sampling (pipeline compilation, clock ramp).
pub const DEFAULT_GPU_BENCH_WARMUP: usize = 120;

/// Drives a fixed-length GPU-timestamp capture and writes JSON on completion.
#[derive(Resource)]
pub struct GpuBenchPlan {
    output: PathBuf,
    effect: String,
    warmup: usize,
    warmup_remaining: usize,
    frames: usize,
    remaining: usize,
    samples: BTreeMap<String, Vec<f64>>,
    work: BTreeMap<String, WorkStats>,
}

impl GpuBenchPlan {
    pub fn new(output: PathBuf, effect: String, warmup: usize, frames: usize) -> Self {
        Self {
            output,
            effect,
            warmup,
            warmup_remaining: warmup,
            frames,
            remaining: frames,
            samples: BTreeMap::new(),
            work: BTreeMap::new(),
        }
    }

    fn write_report(&self) -> Result<(), String> {
        let metrics = self
            .samples
            .iter()
            .map(|(path, values)| (path.clone(), Stats::from_samples(values)))
            .collect();
        let report = GpuBenchReport {
            effect: self.effect.clone(),
            warmup: self.warmup,
            frames: self.frames,
            metrics,
            work: &self.work,
            work_scope: "Asynchronous host observations. Peaks cover measured frames; event totals cover buffer lifetime including warm-up and can count replay/reset activity again. Missing measurements are null. Timing paths can publish only the last fixed-tick observation in a catch-up frame; do not sum stage percentiles into whole-frame costs.",
        };
        let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
        std::fs::write(&self.output, json).map_err(|error| error.to_string())
    }
}

/// Distribution of one diagnostic across the capture, in the diagnostic's native
/// unit (milliseconds for the render-pass timers).
#[derive(Serialize)]
struct Stats {
    samples: usize,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
    min: f64,
    mean: f64,
    stddev: f64,
}

impl Stats {
    fn from_samples(values: &[f64]) -> Self {
        if values.is_empty() {
            return Self {
                samples: 0,
                p50: 0.0,
                p95: 0.0,
                p99: 0.0,
                max: 0.0,
                min: 0.0,
                mean: 0.0,
                stddev: 0.0,
            };
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let count = sorted.len();
        let mean = sorted.iter().sum::<f64>() / count as f64;
        let variance = sorted.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;
        Self {
            samples: count,
            p50: percentile(&sorted, 0.50),
            p95: percentile(&sorted, 0.95),
            p99: percentile(&sorted, 0.99),
            max: sorted[count - 1],
            min: sorted[0],
            mean,
            stddev: variance.sqrt(),
        }
    }
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[derive(Serialize)]
struct GpuBenchReport<'a> {
    effect: String,
    warmup: usize,
    frames: usize,
    metrics: BTreeMap<String, Stats>,
    /// Host-observed asynchronous root/child metrics during the measured window.
    /// Counts can lag. Event totals cover buffer lifetime, including warm-up;
    /// they are not deltas for this window or authoritative replay accounting.
    work: &'a BTreeMap<String, WorkStats>,
    work_scope: &'static str,
}

#[derive(Default, Serialize)]
struct WorkStats {
    peak_live_particles: Option<u32>,
    peak_occupied_trails: Option<u32>,
    peak_retired_trails: Option<u32>,
    max_trail_evictions: Option<u32>,
    max_truncated_trails: Option<u32>,
    estimated_buffer_memory_bytes: Option<u64>,
    event_readback_samples: Option<u64>,
    source_event_overflow: Option<u64>,
    links: Option<Vec<LinkStats>>,
}

#[derive(Serialize)]
struct LinkStats {
    captured_demand: u64,
    expansion_omitted: u64,
    accepted: u64,
    destination_rejected: u64,
}

impl WorkStats {
    fn record(&mut self, profiler: &EffectProfiler, events: Option<&GpuEventLinkStatistics>) {
        let profile = &profiler.0;
        fn peak(target: &mut Option<u32>, value: ProfileValue<u32>) {
            if let ProfileValue::Measured(value) = value {
                *target = Some(target.unwrap_or(0).max(value));
            }
        }
        peak(&mut self.peak_live_particles, profile.alive_particles);
        peak(&mut self.peak_occupied_trails, profile.occupied_trails);
        peak(&mut self.peak_retired_trails, profile.retired_trails);
        peak(&mut self.max_trail_evictions, profile.trail_evictions);
        peak(&mut self.max_truncated_trails, profile.truncated_trails);
        self.estimated_buffer_memory_bytes = profile.buffer_memory_bytes.value();
        if let Some(events) = events.filter(|events| events.readback_samples > 0) {
            self.event_readback_samples = Some(events.readback_samples);
            self.source_event_overflow = Some(events.source_overflow);
            self.links = Some(
                events
                    .links
                    .iter()
                    .map(|link| LinkStats {
                        captured_demand: link.captured_demand,
                        expansion_omitted: link.expansion_omitted,
                        accepted: link.accepted,
                        destination_rejected: link.destination_rejected,
                    })
                    .collect(),
            );
        }
    }
}

/// Samples Aestra's GPU diagnostics each frame and exits once the capture is done.
/// A no-op unless a [`GpuBenchPlan`] resource is present.
pub fn drive_gpu_bench(
    plan: Option<ResMut<GpuBenchPlan>>,
    diagnostics: Res<DiagnosticsStore>,
    effects: Query<(Entity, &EffectProfiler, Option<&GpuEventLinkStatistics>)>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut plan) = plan else {
        return;
    };
    if plan.warmup_remaining > 0 {
        plan.warmup_remaining -= 1;
        return;
    }
    if plan.remaining == 0 {
        return;
    }
    for diagnostic in diagnostics.iter() {
        let path = diagnostic.path().as_str();
        // Aestra's passes, and the transparent passes its particles and volumes draw in.
        if [
            "aestra::gpu::simulate",
            "aestra::gpu::trail_history",
            "aestra::gpu::trail_particles",
            "aestra::gpu::trail_compaction",
            "aestra::gpu::extension_stages",
            "main_transparent_pass_2d",
            "main_transparent_pass_3d",
        ]
        .iter()
        .any(|pass| path.contains(pass))
            && let Some(value) = diagnostic.value()
        {
            plan.samples.entry(path.to_owned()).or_default().push(value);
        }
    }
    for (entity, profiler, events) in &effects {
        plan.work
            .entry(entity.to_string())
            .or_default()
            .record(profiler, events);
    }
    plan.remaining -= 1;
    if plan.remaining == 0 {
        match plan.write_report() {
            Ok(()) => {
                info!(
                    "aestra-viewer: wrote GPU benchmark to {}",
                    plan.output.display()
                );
                exit.write(AppExit::Success);
            }
            Err(error) => {
                eprintln!("aestra-viewer: GPU benchmark failed: {error}");
                exit.write(AppExit::error());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_report_keeps_missing_measurements_distinct_from_zero_and_counts_admitted_work() {
        let compiled = aestra_bevy::EffectCompiler::default()
            .compile(&aestra_bevy::EffectAsset::new("report", 6.0))
            .unwrap();
        let mut profile = EffectProfiler(aestra_bevy::EffectProfile::from_compiled(&compiled));
        profile.0.alive_particles = ProfileValue::Estimated(9000);
        let mut work = WorkStats::default();
        work.record(&profile, Some(&GpuEventLinkStatistics::default()));
        assert_eq!(work.peak_live_particles, None);
        assert!(work.links.is_none());
        profile.0.alive_particles = ProfileValue::Measured(7200);
        profile.0.occupied_trails = ProfileValue::Measured(10400);
        profile.0.retired_trails = ProfileValue::Measured(3200);
        let events = GpuEventLinkStatistics {
            links: vec![aestra_bevy::gpu::GpuEventLinkCounts {
                captured_demand: 12800,
                accepted: 12800,
                ..default()
            }],
            readback_samples: 100,
            ..default()
        };
        work.record(&profile, Some(&events));
        profile.0.alive_particles = ProfileValue::Measured(0);
        work.record(&profile, Some(&events));
        let json = serde_json::to_value(&work).unwrap();
        assert_eq!(json["peak_live_particles"], 7200);
        assert_eq!(json["peak_occupied_trails"], 10400);
        assert_eq!(json["peak_retired_trails"], 3200);
        assert_eq!(json["links"][0]["accepted"], 12800);
        assert_eq!(json["source_event_overflow"], 0);
        assert!(json["max_trail_evictions"].is_null());
    }

    #[test]
    fn report_preserves_configured_warmup_after_it_is_consumed() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("report.json");
        let mut plan = GpuBenchPlan::new(output.clone(), "fixture".into(), 120, 2);
        plan.warmup_remaining = 0;
        plan.samples.insert("gpu".into(), vec![0.2, 0.3]);
        plan.write_report().unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        assert_eq!(report["warmup"], 120);
        assert_eq!(report["frames"], 2);
    }
}
