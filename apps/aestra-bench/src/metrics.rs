//! Timing accumulation, distribution statistics, and the machine-readable report
//! schema. All timing values in the emitted report are milliseconds.

use serde::Serialize;

/// Distribution summary for one timing metric. Frame-time tails matter more than
/// the mean for production games, so percentiles are first-class (strategy §10).
#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub samples: usize,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub min_ms: f64,
    pub mean_ms: f64,
    pub stddev_ms: f64,
}

impl Stats {
    /// Builds a distribution from per-frame durations expressed in nanoseconds.
    pub fn from_nanos(mut nanos: Vec<f64>) -> Self {
        if nanos.is_empty() {
            return Self {
                samples: 0,
                median_ms: 0.0,
                p95_ms: 0.0,
                p99_ms: 0.0,
                max_ms: 0.0,
                min_ms: 0.0,
                mean_ms: 0.0,
                stddev_ms: 0.0,
            };
        }
        nanos.sort_by(|a, b| a.total_cmp(b));
        let samples = nanos.len();
        let sum: f64 = nanos.iter().sum();
        let mean = sum / samples as f64;
        let variance = nanos
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / samples as f64;
        let ns_to_ms = 1.0e-6;
        Self {
            samples,
            median_ms: percentile(&nanos, 0.50) * ns_to_ms,
            p95_ms: percentile(&nanos, 0.95) * ns_to_ms,
            p99_ms: percentile(&nanos, 0.99) * ns_to_ms,
            max_ms: nanos[samples - 1] * ns_to_ms,
            min_ms: nanos[0] * ns_to_ms,
            mean_ms: mean * ns_to_ms,
            stddev_ms: variance.sqrt() * ns_to_ms,
        }
    }
}

/// Nearest-rank percentile over an ascending-sorted slice.
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    debug_assert!(!sorted.is_empty());
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[index]
}

/// Per-frame nanosecond timings, accumulated across the capture interval.
#[derive(Default)]
pub struct StageAccumulator {
    pub runtime_advance: Vec<f64>,
    pub cpu_reference_eval: Vec<f64>,
    pub artifact_update: Vec<f64>,
    pub aestra_total: Vec<f64>,
    pub alive_per_frame: Vec<u32>,
}

impl StageAccumulator {
    pub fn record(&mut self, advance_ns: f64, eval_ns: f64, artifact_ns: f64, alive: u32) {
        self.runtime_advance.push(advance_ns);
        self.cpu_reference_eval.push(eval_ns);
        self.artifact_update.push(artifact_ns);
        self.aestra_total.push(advance_ns + eval_ns + artifact_ns);
        self.alive_per_frame.push(alive);
    }

    /// Median alive-particle count across the capture, used as the steady-state
    /// occupancy numerator.
    pub fn median_alive(&self) -> u32 {
        if self.alive_per_frame.is_empty() {
            return 0;
        }
        let mut alive = self.alive_per_frame.clone();
        alive.sort_unstable();
        alive[alive.len() / 2]
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Hardware {
    pub cpu: String,
    pub os: String,
    pub arch: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Content {
    pub effects: u32,
    pub emitters: u32,
    pub capacity: u32,
    pub alive: u32,
    pub occupancy: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CpuStages {
    pub runtime_advance_ms: Stats,
    pub cpu_reference_eval_ms: Stats,
    pub artifact_update_ms: Stats,
    pub aestra_total_ms: Stats,
}

/// Normalized, machine-independent ratios (strategy §11). GPU-normalized values
/// are intentionally absent until the GPU lane exists.
#[derive(Debug, Clone, Serialize)]
pub struct Normalized {
    pub cpu_ns_per_1k_slots: f64,
    pub cpu_ns_per_1k_alive: Option<f64>,
    pub occupancy: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchReport {
    pub scenario: String,
    pub commit: String,
    pub frames: usize,
    pub warmup: usize,
    pub seed: u64,
    pub hardware: Hardware,
    pub content: Content,
    pub cpu: CpuStages,
    pub normalized: Normalized,
    /// The GPU block is deferred to the native-GPU lane; recorded explicitly as
    /// unavailable so consumers never mistake a missing measurement for zero.
    pub gpu: Option<()>,
}
