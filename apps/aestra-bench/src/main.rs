//! `aestra-bench` — a headless, deterministic benchmark harness for the Aestra
//! runtime, GPU-artifact preparation and opt-in native GPU experiments.
//!
//! The default mode measures the CPU cost of three real per-frame stages without a
//! window or GPU, so it runs on ordinary CI (the strategy's PR CPU lane):
//!
//! * `runtime advance`       — `EffectInstance::advance` (clock + choreography)
//! * `CPU reference eval`    — `EffectInstance::evaluate` (analytical particle reconstruction)
//! * `artifact update`       — `GpuEffectArtifact::dynamics_from_instance` (per-frame emitter/renderer prep)
//!
//! It records distribution statistics (median/p95/p99/max/stddev), measured
//! occupancy, and normalized ratios, then prints a summary and optionally writes
//! machine-readable JSON (strategy §15). CPU runs report GPU timings as unavailable.
//! With `--features gpu`, `--gpu-trails preparation|rendering|sweep` runs the native
//! timestamped trail experiments and writes reports under `benchmarks/gpu-baselines/`.

#[cfg(feature = "gpu")]
mod gpu_trails;
mod metrics;
mod scenario;

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use aestra_compiler::EffectCompiler;
use aestra_core::EffectAsset;
use aestra_gpu::GpuEffectArtifact;
use aestra_runtime::{EffectInstance, ParticleSample};

use metrics::{BenchReport, Content, CpuStages, Hardware, Normalized, StageAccumulator, Stats};

const TICK_HZ: f32 = 60.0;
const TRAIL_OWNER_CAPACITY: u32 = 1024;
const MAX_TRAIL_VIEWS: usize = 8;

fn main() {
    let config = match Config::from_args() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("aestra-bench: {message}");
            eprintln!(
                "usage: aestra-bench (--scenario <name> | --all | --gpu-trails preparation|rendering|sweep) [--frames N] [--warmup N] \
                 [--occupancies 1,2,5,10,25,50,75,100] [--views 1,2,4,8] \
                 [--owner-capacity 1024] [--history-points 64] [--history-fill 100] \
                 [--seed <dec-or-0xhex>] [--out results.json] [--commit <sha>]"
            );
            eprintln!("scenarios: {}", scenario::names());
            std::process::exit(2);
        }
    };

    if let Some(kind) = config.gpu_trails.as_deref() {
        #[cfg(feature = "gpu")]
        if let Err(error) = gpu_trails::run(&config, kind) {
            eprintln!("aestra-bench: {error}");
            std::process::exit(1);
        }
        #[cfg(not(feature = "gpu"))]
        {
            let _ = kind;
            eprintln!("aestra-bench: --gpu-trails requires --features gpu");
            std::process::exit(2);
        }
        #[cfg(feature = "gpu")]
        return;
    }

    let scenarios: Vec<&scenario::Scenario> = if config.all {
        scenario::SCENARIOS.iter().collect()
    } else {
        match config.scenario.as_deref().and_then(scenario::find) {
            Some(scenario) => vec![scenario],
            None => {
                eprintln!(
                    "aestra-bench: unknown scenario {:?}; available: {}",
                    config.scenario.unwrap_or_default(),
                    scenario::names()
                );
                std::process::exit(2);
            }
        }
    };

    let mut reports = Vec::new();
    let mut failures = 0;
    for scenario in scenarios {
        match run_scenario(scenario, &config) {
            Ok(report) => {
                print_summary(&report, scenario.purpose);
                reports.push(report);
            }
            Err(message) => {
                eprintln!(
                    "aestra-bench: scenario '{}' failed: {message}",
                    scenario.name
                );
                failures += 1;
            }
        }
    }

    if let Some(path) = &config.out {
        match serde_json::to_string_pretty(&reports) {
            Ok(json) => {
                if let Err(error) = std::fs::write(path, json) {
                    eprintln!("aestra-bench: could not write {path}: {error}");
                    std::process::exit(1);
                }
                println!("\nwrote {} report(s) to {path}", reports.len());
            }
            Err(error) => {
                eprintln!("aestra-bench: could not serialize reports: {error}");
                std::process::exit(1);
            }
        }
    }

    if failures > 0 {
        std::process::exit(1);
    }
}

struct Config {
    gpu_trails: Option<String>,
    #[cfg(feature = "gpu")]
    trail_owners: Vec<u32>,
    #[cfg(feature = "gpu")]
    trail_views: Vec<usize>,
    #[cfg(feature = "gpu")]
    trail_shape: [u32; 3], // Owner capacity, points including head, retained history samples.
    scenario: Option<String>,
    all: bool,
    frames: usize,
    warmup: usize,
    seed: u64,
    out: Option<String>,
    commit: String,
}

impl Config {
    fn from_args() -> Result<Self, String> {
        Self::parse(std::env::args().skip(1))
    }

    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut gpu_trails = None;
        let mut trail_owners = None;
        let mut trail_views = None;
        let mut owner_capacity = None;
        let mut history_points = None;
        let mut history_fill = None;
        let mut scenario = None;
        let mut all = false;
        let mut frames = 64usize;
        let mut warmup = 8usize;
        let mut seed = 0xa357_2a11_5eed_0001u64;
        let mut out = None;
        let mut commit = default_commit();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--gpu-trails" => {
                    let kind = next_value(&mut args, "--gpu-trails")?;
                    if !matches!(kind.as_str(), "preparation" | "rendering" | "sweep") {
                        return Err("--gpu-trails expects preparation, rendering or sweep".into());
                    }
                    gpu_trails = Some(kind);
                }
                "--occupancies" => {
                    trail_owners = Some(next_value(&mut args, "--occupancies")?);
                }
                "--owner-capacity" => {
                    owner_capacity = Some(next_value(&mut args, "--owner-capacity")?)
                }
                "--history-points" => {
                    history_points = Some(next_value(&mut args, "--history-points")?)
                }
                "--history-fill" => history_fill = Some(next_value(&mut args, "--history-fill")?),
                "--views" => {
                    trail_views = Some(parse_views(&next_value(&mut args, "--views")?)?);
                }
                "--scenario" => {
                    scenario = Some(next_value(&mut args, "--scenario")?);
                }
                "--all" => all = true,
                "--frames" => {
                    frames = parse_usize(&next_value(&mut args, "--frames")?, "--frames")?;
                }
                "--warmup" => {
                    warmup = parse_usize(&next_value(&mut args, "--warmup")?, "--warmup")?;
                }
                "--seed" => {
                    seed = parse_seed(&next_value(&mut args, "--seed")?)?;
                }
                "--out" => out = Some(next_value(&mut args, "--out")?),
                "--commit" => commit = next_value(&mut args, "--commit")?,
                other => return Err(format!("unexpected argument {other:?}")),
            }
        }

        if gpu_trails.is_some() && (all || scenario.is_some()) {
            return Err("--gpu-trails cannot be combined with --scenario or --all".into());
        }
        if (trail_owners.is_some()
            || trail_views.is_some()
            || owner_capacity.is_some()
            || history_points.is_some()
            || history_fill.is_some())
            && !matches!(gpu_trails.as_deref(), Some("rendering" | "sweep"))
        {
            return Err("trail geometry, occupancy and view options require --gpu-trails rendering or sweep".into());
        }
        if !all && scenario.is_none() && gpu_trails.is_none() {
            return Err("expected --scenario <name>, --all or --gpu-trails <kind>".into());
        }
        if frames == 0 {
            return Err("--frames must be greater than zero".into());
        }
        if frames.checked_add(warmup).is_none() {
            return Err("--frames plus --warmup is too large".into());
        }
        let trail_shape = parse_trail_shape(
            owner_capacity.as_deref(),
            history_points.as_deref(),
            history_fill.as_deref(),
        )?;
        let trail_owners = parse_occupancies(
            trail_owners
                .as_deref()
                .unwrap_or(if gpu_trails.as_deref() == Some("sweep") {
                    "1,2,5,10,25,50,75,100"
                } else {
                    "1.5625,100"
                }),
            trail_shape[0],
        )?;
        #[cfg(not(feature = "gpu"))]
        let _ = trail_owners;
        Ok(Self {
            #[cfg(feature = "gpu")]
            trail_owners,
            #[cfg(feature = "gpu")]
            trail_shape,
            #[cfg(feature = "gpu")]
            trail_views: trail_views.unwrap_or_else(|| {
                if gpu_trails.as_deref() == Some("sweep") {
                    vec![1, 2, 4, 8]
                } else {
                    vec![1, 4]
                }
            }),
            gpu_trails,
            scenario,
            all,
            frames,
            warmup,
            seed,
            out,
            commit,
        })
    }
}

fn run_scenario(scenario: &scenario::Scenario, config: &Config) -> Result<BenchReport, String> {
    let asset = EffectAsset::from_ron(scenario.ron)
        .map_err(|error| format!("could not parse scenario asset: {error}"))?;
    let compiled = EffectCompiler::default()
        .compile(&asset)
        .map_err(|error| format!("could not compile scenario: {error}"))?;
    let effect = Arc::new(compiled);

    let instance_count = scenario.instances.max(1);
    let dt = 1.0 / TICK_HZ;

    // Independent instances with distinct seeds model N concurrent effects. Each
    // stage is timed across the whole set, mirroring how the ECS systems advance
    // and prepare every player before moving to the next stage.
    let mut instances: Vec<EffectInstance> = (0..instance_count)
        .map(|index| {
            EffectInstance::with_seed(Arc::clone(&effect), config.seed.wrapping_add(index as u64))
        })
        .collect();

    // Per-instance analytical slot count; total capacity scales with instance count.
    let per_instance_capacity = GpuEffectArtifact::dynamics_from_instance(&instances[0])
        .map_err(|error| format!("GPU artifact is unavailable ({error}); cannot size capacity"))?
        .total_slots;
    let capacity =
        per_instance_capacity.saturating_mul(instance_count.min(u32::MAX as usize) as u32);

    // A single reused buffer keeps CPU-reference timing focused on reconstruction
    // work rather than the caller's output allocation.
    let mut samples: Vec<ParticleSample> = Vec::new();

    for _ in 0..config.warmup {
        for instance in &mut instances {
            instance.advance(dt);
            samples.clear();
            instance.evaluate(&mut samples);
            black_box(GpuEffectArtifact::dynamics_from_instance(instance).ok());
        }
    }

    let mut accumulator = StageAccumulator::default();
    for _ in 0..config.frames {
        let start = Instant::now();
        for instance in &mut instances {
            instance.advance(dt);
        }
        let advance_ns = start.elapsed().as_nanos() as f64;

        let mut alive_total = 0usize;
        let start = Instant::now();
        for instance in &instances {
            samples.clear();
            instance.evaluate(&mut samples);
            alive_total += samples.len();
        }
        let eval_ns = start.elapsed().as_nanos() as f64;
        let alive = alive_total.min(u32::MAX as usize) as u32;

        let start = Instant::now();
        for instance in &instances {
            black_box(
                GpuEffectArtifact::dynamics_from_instance(instance)
                    .map(|dynamics| dynamics.total_slots)
                    .ok(),
            );
        }
        let artifact_ns = start.elapsed().as_nanos() as f64;

        accumulator.record(advance_ns, eval_ns, artifact_ns, alive);
    }

    let alive = accumulator.median_alive();
    let occupancy = if capacity > 0 {
        f64::from(alive) / f64::from(capacity)
    } else {
        0.0
    };

    let cpu = CpuStages {
        runtime_advance_ms: Stats::from_nanos(accumulator.runtime_advance),
        cpu_reference_eval_ms: Stats::from_nanos(accumulator.cpu_reference_eval),
        artifact_update_ms: Stats::from_nanos(accumulator.artifact_update),
        aestra_total_ms: Stats::from_nanos(accumulator.aestra_total),
    };
    let total_median_ns = cpu.aestra_total_ms.median_ms * 1.0e6;
    let normalized = Normalized {
        cpu_ns_per_1k_slots: if capacity > 0 {
            total_median_ns / (f64::from(capacity) / 1000.0)
        } else {
            0.0
        },
        cpu_ns_per_1k_alive: (alive > 0).then(|| total_median_ns / (f64::from(alive) / 1000.0)),
        occupancy,
    };

    Ok(BenchReport {
        scenario: scenario.name.to_owned(),
        commit: config.commit.clone(),
        frames: config.frames,
        warmup: config.warmup,
        seed: config.seed,
        hardware: host_hardware(),
        content: Content {
            effects: instance_count.min(u32::MAX as usize) as u32,
            emitters: (effect.emitters.len() as u64 * instance_count as u64)
                .min(u64::from(u32::MAX)) as u32,
            capacity,
            alive,
            occupancy,
        },
        cpu,
        normalized,
        gpu: None,
    })
}

fn print_summary(report: &BenchReport, purpose: &str) {
    println!("── {} ──", report.scenario);
    println!("   {purpose}");
    println!(
        "   content: {} effect(s), {} emitter(s), capacity {}, alive {} ({:.2}% occupancy)",
        report.content.effects,
        report.content.emitters,
        report.content.capacity,
        report.content.alive,
        report.content.occupancy * 100.0,
    );
    println!(
        "   frames {} (warmup {}), seed {:#018x}",
        report.frames, report.warmup, report.seed
    );
    print_stat("runtime advance   ", &report.cpu.runtime_advance_ms);
    print_stat("cpu reference eval", &report.cpu.cpu_reference_eval_ms);
    print_stat("artifact update   ", &report.cpu.artifact_update_ms);
    print_stat("aestra total (cpu)", &report.cpu.aestra_total_ms);
    print!(
        "   normalized: {:.1} ns / 1k slots",
        report.normalized.cpu_ns_per_1k_slots
    );
    match report.normalized.cpu_ns_per_1k_alive {
        Some(value) => println!(", {value:.1} ns / 1k alive"),
        None => println!(", n/a ns / 1k alive (no alive particles)"),
    }
    println!("   gpu: unavailable (native GPU lane)");
}

fn print_stat(label: &str, stats: &Stats) {
    println!(
        "   {label}  median {:.4} ms  p95 {:.4}  p99 {:.4}  max {:.4}  stddev {:.4}",
        stats.median_ms, stats.p95_ms, stats.p99_ms, stats.max_ms, stats.stddev_ms
    );
}

fn host_hardware() -> Hardware {
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(0);
    Hardware {
        cpu: format!("{cores} logical cores"),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        backend: "cpu-headless".to_owned(),
    }
}

fn default_commit() -> String {
    std::env::var("AESTRA_BENCH_COMMIT")
        .or_else(|_| std::env::var("GITHUB_SHA"))
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_usize(value: &str, flag: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|_| format!("{flag} expects a non-negative integer, got {value:?}"))
}

fn parse_seed(value: &str) -> Result<u64, String> {
    let parsed = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(|| value.parse::<u64>(), |hex| u64::from_str_radix(hex, 16));
    parsed.map_err(|_| format!("--seed expects a decimal or 0x-prefixed hex value, got {value:?}"))
}

fn parse_trail_shape(
    owners: Option<&str>,
    points: Option<&str>,
    fill: Option<&str>,
) -> Result<[u32; 3], String> {
    let owners = parse_usize(owners.unwrap_or("1024"), "--owner-capacity")?;
    let points = parse_usize(points.unwrap_or("64"), "--history-points")?;
    if !(1..=TRAIL_OWNER_CAPACITY as usize).contains(&owners) || !(2..=64).contains(&points) {
        return Err("--owner-capacity must be 1–1024 and --history-points must be 2–64 (including the head)".into());
    }
    let samples = parse_occupancies(fill.unwrap_or("100"), (points - 1) as u32)
        .map_err(|error| error.replace("--occupancies", "--history-fill"))?;
    if samples.len() != 1 {
        return Err("--history-fill expects one percentage in (0,100]".into());
    }
    Ok([owners as u32, points as u32, samples[0]])
}

fn parse_occupancies(value: &str, capacity: u32) -> Result<Vec<u32>, String> {
    let mut owners = Vec::new();
    for entry in value.split(',') {
        let percent: f64 = entry
            .trim()
            .parse()
            .map_err(|_| "--occupancies expects percentages in (0,100]".to_owned())?;
        if !percent.is_finite() || percent <= 0.0 || percent > 100.0 {
            return Err("--occupancies expects finite percentages in (0,100]".into());
        }
        let count = (percent * f64::from(capacity) / 100.0).round().max(1.0) as u32;
        if owners.contains(&count) {
            return Err("--occupancies contains levels mapping to the same owner count".into());
        }
        owners.push(count);
    }
    Ok(owners)
}

fn parse_views(value: &str) -> Result<Vec<usize>, String> {
    let mut views = Vec::new();
    for entry in value.split(',') {
        let count = parse_usize(entry.trim(), "--views")?;
        if !(1..=MAX_TRAIL_VIEWS).contains(&count) || views.contains(&count) {
            return Err("--views expects distinct counts between 1 and 8".into());
        }
        views.push(count);
    }
    Ok(views)
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn trail_shape_options_are_bounded_and_order_independent() {
        assert_eq!(parse_trail_shape(None, None, None).unwrap(), [1024, 64, 63]);
        assert_eq!(
            parse_trail_shape(Some("128"), Some("16"), Some("25")).unwrap(),
            [128, 16, 4]
        );
        assert_eq!(
            parse_trail_shape(Some("1"), Some("2"), Some("0.01")).unwrap(),
            [1, 2, 1]
        );
        for owners in ["0", "1025", "-1", "1.5", ""] {
            assert!(parse_trail_shape(Some(owners), None, None).is_err());
        }
        for points in ["0", "1", "65", "-1", ""] {
            assert!(parse_trail_shape(None, Some(points), None).is_err());
        }
        for fill in ["0", "101", "NaN", "inf", "25,50", ""] {
            assert!(parse_trail_shape(None, None, Some(fill)).is_err());
        }
        for flag in ["--owner-capacity", "--history-points", "--history-fill"] {
            assert!(config(&["--all", flag, "16"]).is_err());
            assert!(config(&["--gpu-trails", "preparation", flag, "16"]).is_err());
        }
        let _first = config(&[
            "--gpu-trails",
            "sweep",
            "--occupancies",
            "25,100",
            "--owner-capacity",
            "128",
            "--history-points",
            "16",
            "--history-fill",
            "25",
        ])
        .unwrap();
        let _last = config(&[
            "--owner-capacity",
            "128",
            "--history-points",
            "16",
            "--history-fill",
            "25",
            "--gpu-trails",
            "sweep",
            "--occupancies",
            "25,100",
        ])
        .unwrap();
        #[cfg(feature = "gpu")]
        {
            assert_eq!(_first.trail_owners, vec![32, 128]);
            assert_eq!(_first.trail_owners, _last.trail_owners);
            assert_eq!(_first.trail_shape, [128, 16, 4]);
            assert_eq!(_first.trail_shape, _last.trail_shape);
            assert_eq!(
                config(&["--gpu-trails", "rendering"]).unwrap().trail_owners,
                vec![16, 1024]
            );
        }
        // Percentages must remain distinct after rounding at the chosen capacity.
        assert!(
            config(&[
                "--gpu-trails",
                "sweep",
                "--owner-capacity",
                "1",
                "--occupancies",
                "25,100"
            ])
            .is_err()
        );
    }

    fn config(args: &[&str]) -> Result<Config, String> {
        Config::parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn gpu_experiments_use_common_sampling_and_output_options() {
        let cfg = config(&[
            "--gpu-trails",
            "rendering",
            "--frames",
            "1",
            "--warmup",
            "0",
            "--seed",
            "7",
            "--out",
            "benchmarks/gpu-baselines/test.json",
            "--commit",
            "test",
        ])
        .unwrap();
        assert_eq!(cfg.gpu_trails.as_deref(), Some("rendering"));
        assert_eq!((cfg.frames, cfg.warmup, cfg.seed), (1, 0, 7));
        assert_eq!(
            cfg.out.as_deref(),
            Some("benchmarks/gpu-baselines/test.json")
        );
        assert_eq!(cfg.commit, "test");
        assert!(config(&["--gpu-trails", "preparation"]).is_ok());
    }

    #[test]
    fn reject_invalid_or_mixed_gpu_experiments() {
        for args in [
            vec!["--gpu-trails"],
            vec!["--gpu-trails", "unknown"],
            vec!["--gpu-trails", "rendering", "--all"],
            vec!["--gpu-trails", "rendering", "--scenario", "B001"],
            vec!["--gpu-trails", "rendering", "--frames", "0"],
        ] {
            assert!(config(&args).is_err());
        }
        assert!(config(&["--all"]).unwrap().gpu_trails.is_none());
    }

    #[test]
    fn sweep_arguments_are_bounded_and_unambiguous() {
        assert_eq!(
            parse_occupancies("1,5,50,100", 1024).unwrap(),
            vec![10, 51, 512, 1024]
        );
        assert_eq!(parse_occupancies("0.01", 1024).unwrap(), vec![1]);
        assert_eq!(parse_views("1,2,4,8").unwrap(), vec![1, 2, 4, 8]);
        for value in ["", "0", "-1", "101", "NaN", "inf", "1,1", "1,1.01", "1,"] {
            assert!(parse_occupancies(value, 1024).is_err(), "{value}");
        }
        for value in ["", "0", "9", "1,1", "1.5", "1,"] {
            assert!(parse_views(value).is_err(), "{value}");
        }
        assert!(config(&["--all", "--views", "1"]).is_err());
        assert!(config(&["--gpu-trails", "preparation", "--occupancies", "50"]).is_err());
        let _cfg = config(&["--gpu-trails", "sweep"]).unwrap();
        #[cfg(feature = "gpu")]
        {
            assert_eq!(_cfg.trail_views, vec![1, 2, 4, 8]);
            assert_eq!(
                _cfg.trail_owners,
                vec![10, 20, 51, 102, 256, 512, 768, 1024]
            );
        }
    }
}
