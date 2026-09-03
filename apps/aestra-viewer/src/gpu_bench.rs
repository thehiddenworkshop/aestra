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
    frames: usize,
    remaining: usize,
    samples: BTreeMap<String, Vec<f64>>,
}

impl GpuBenchPlan {
    pub fn new(output: PathBuf, effect: String, warmup: usize, frames: usize) -> Self {
        Self {
            output,
            effect,
            warmup,
            frames,
            remaining: frames,
            samples: BTreeMap::new(),
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
struct GpuBenchReport {
    effect: String,
    warmup: usize,
    frames: usize,
    metrics: BTreeMap<String, Stats>,
}

/// Samples Aestra's GPU diagnostics each frame and exits once the capture is done.
/// A no-op unless a [`GpuBenchPlan`] resource is present.
pub fn drive_gpu_bench(
    plan: Option<ResMut<GpuBenchPlan>>,
    diagnostics: Res<DiagnosticsStore>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut plan) = plan else {
        return;
    };
    if plan.warmup > 0 {
        plan.warmup -= 1;
        return;
    }
    if plan.remaining == 0 {
        return;
    }
    for diagnostic in diagnostics.iter() {
        let path = diagnostic.path().as_str();
        if (path.contains("aestra::gpu::simulate") || path.contains("main_transparent_pass_2d"))
            && let Some(value) = diagnostic.value()
        {
            plan.samples.entry(path.to_owned()).or_default().push(value);
        }
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
