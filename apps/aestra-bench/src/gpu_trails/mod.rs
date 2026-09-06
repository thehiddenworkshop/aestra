//! Opt-in native GPU experiments. The ordinary CPU benchmark has no GPU dependency.
mod preparation;
mod rendering;
mod timestamps;

use crate::Config;
use encase::ShaderType;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub(crate) fn run(config: &Config, kind: &str) -> Result<(), String> {
    // Generate/write the report only after all count, image and timing checks pass.
    let report = match kind {
        "preparation" => preparation::run(config),
        "rendering" | "sweep" => rendering::run(config),
        _ => return Err(format!("unknown GPU trail experiment {kind:?}")),
    };
    let path = config.out.as_ref().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("benchmarks/gpu-baselines")
            .join(format!("trails-{}", report.captured_at_unix_ns))
            .join(format!("{kind}.json"))
    });
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    println!("wrote GPU baseline {}", path.display());
    Ok(())
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    timestamp_readback: &'static str,
    experiment: String,
    commit: String,
    captured_at_unix_ns: u128,
    os: &'static str,
    arch: &'static str,
    adapter: BTreeMap<&'static str, String>,
    frames: usize,
    warmup: usize,
    seed: u64,
    gpu_seed: u32,
    target_size: Option<[u32; 2]>,
    owner_capacity: u32,
    history_points: u32,
    /// Rendering only: historical samples per live owner, excluding the live head.
    retained_history_samples: Option<u32>,
    history_fill_percent: Option<f64>,
    cases: Vec<CaseReport>,
    comparisons: Vec<Comparison>,
}

impl Report {
    fn new(experiment: &str, config: &Config, adapter: wgpu::AdapterInfo) -> Self {
        Self {
            schema_version: 3,
            timestamp_readback: "separate_command_buffer",
            experiment: experiment.into(),
            commit: config.commit.clone(),
            captured_at_unix_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            adapter: BTreeMap::from([
                ("name", adapter.name),
                ("backend", format!("{:?}", adapter.backend)),
                ("driver", adapter.driver),
                ("driver_info", adapter.driver_info),
                ("device_type", format!("{:?}", adapter.device_type)),
                ("vendor_id", adapter.vendor.to_string()),
                ("device_id", adapter.device.to_string()),
            ]),
            frames: config.frames,
            warmup: config.warmup,
            seed: config.seed,
            gpu_seed: config.seed as u32,
            target_size: None,
            owner_capacity: crate::TRAIL_OWNER_CAPACITY,
            history_points: 64,
            retained_history_samples: None,
            history_fill_percent: None,
            cases: Vec::new(),
            comparisons: Vec::new(),
        }
    }
}

#[derive(Serialize)]
struct CaseReport {
    case: String,
    active_owners: u32,
    occupancy_percent: f64,
    views: usize,
    path: String,
    candidates_per_view: u32,
    submitted_per_view: u32,
    total: Option<Timing>,
    compaction: Timing,
    culling: Timing,
    drawing: Option<Timing>,
    /// None for preparation-only; true only after exact, nonblank image comparison.
    image_equivalence: Option<bool>,
}

/// Per-cell evidence, not a runtime policy or an assumed monotonic crossover.
#[derive(Serialize)]
struct Comparison {
    active_owners: u32,
    occupancy_percent: f64,
    views: usize,
    /// Positive means compaction is faster; None means a zero baseline prevents division.
    median_saving_percent: Option<f64>,
    p95_saving_percent: Option<f64>,
    paired_median_saving_ns: f64,
}

impl Comparison {
    fn new(
        active: u32,
        capacity: u32,
        views: usize,
        full: &[[u64; 4]],
        compact: &[[u64; 4]],
    ) -> Self {
        assert_eq!(full.len(), compact.len());
        let full_total = Timing::new(full.iter().map(|s| s[0]).collect());
        let compact_total = Timing::new(compact.iter().map(|s| s[0]).collect());
        let saving = |base: u64, candidate: u64| {
            (base > 0).then(|| 100.0 * (base as f64 - candidate as f64) / base as f64)
        };
        let mut paired: Vec<_> = full
            .iter()
            .zip(compact)
            .map(|(a, b)| i128::from(a[0]) - i128::from(b[0]))
            .collect();
        paired.sort_unstable();
        Self {
            active_owners: active,
            occupancy_percent: f64::from(active) * 100.0 / f64::from(capacity),
            views,
            median_saving_percent: saving(full_total.median_ns, compact_total.median_ns),
            p95_saving_percent: saving(full_total.p95_ns, compact_total.p95_ns),
            paired_median_saving_ns: paired[paired.len() / 2] as f64,
        }
    }
}

#[derive(Serialize)]
struct Timing {
    median_ns: u64,
    p95_ns: u64,
    /// Preserve acquisition order to allow later outlier/order analysis.
    samples_ns: Vec<u64>,
}

impl Timing {
    fn new(samples_ns: Vec<u64>) -> Self {
        assert!(!samples_ns.is_empty());
        let mut sorted = samples_ns.clone();
        sorted.sort_unstable();
        Self {
            median_ns: sorted[sorted.len() / 2],
            p95_ns: sorted[(sorted.len() as f64 * 0.95) as usize],
            samples_ns,
        }
    }
}

fn encode<T: ShaderType + encase::internal::WriteInto>(value: &T) -> Vec<u8> {
    let mut buffer = encase::StorageBuffer::new(Vec::new());
    buffer.write(value).unwrap();
    buffer.into_inner()
}

fn group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffers: &[&wgpu::Buffer],
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &buffers
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_accepts_one_sample_and_preserves_order() {
        let single = Timing::new(vec![7]);
        assert_eq!((single.median_ns, single.p95_ns), (7, 7));
        let stats = Timing::new((0..64).rev().collect());
        assert_eq!((stats.median_ns, stats.p95_ns), (32, 60));
        assert_eq!(stats.samples_ns[0], 63);
    }

    #[test]
    fn preparation_report_does_not_claim_rendering_measurements() {
        let case = CaseReport {
            case: "sparse".into(),
            active_owners: 16,
            occupancy_percent: 1.5625,
            views: 1,
            path: "compact".into(),
            candidates_per_view: 64512,
            submitted_per_view: 1008,
            compaction: Timing::new(vec![123]),
            culling: Timing::new(vec![45]),
            total: None,
            drawing: None,
            image_equivalence: None,
        };
        let json = serde_json::to_value(case).unwrap();
        assert!(json["total"].is_null());
        assert!(json["drawing"].is_null());
        assert!(json["image_equivalence"].is_null());
        assert_eq!(json["compaction"]["samples_ns"][0], 123);
    }

    #[test]
    fn comparisons_preserve_signed_savings_and_unavailable_ratios() {
        let result = Comparison::new(512, 1024, 4, &[[100, 0, 0, 0]], &[[150, 0, 0, 0]]);
        assert_eq!(result.occupancy_percent, 50.0);
        assert_eq!(result.median_saving_percent, Some(-50.0));
        assert_eq!(result.paired_median_saving_ns, -50.0);
        let result = Comparison::new(1, 1024, 1, &[[0; 4]], &[[0; 4]]);
        assert_eq!(result.median_saving_percent, None);
        assert_eq!(result.p95_saving_percent, None);
        let result = Comparison::new(
            1,
            1024,
            1,
            &[[1, 0, 0, 0], [2, 0, 0, 0], [100, 0, 0, 0]],
            &[[1, 0, 0, 0], [50, 0, 0, 0], [51, 0, 0, 0]],
        );
        assert_eq!(result.paired_median_saving_ns, 0.0);
        assert_eq!(result.median_saving_percent, Some(-2400.0));
    }
}
