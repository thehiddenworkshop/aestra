//! `aestra-bench` — a headless, deterministic benchmark harness for the Aestra
//! runtime and GPU-artifact preparation path.
//!
//! This first slice measures the CPU cost of three real per-frame stages without a
//! window or GPU, so it runs on ordinary CI (the strategy's PR CPU lane):
//!
//! * `runtime advance`       — `EffectInstance::advance` (clock + choreography)
//! * `CPU reference eval`    — `EffectInstance::evaluate` (analytical particle reconstruction)
//! * `artifact update`       — `GpuEffectArtifact::from_instance` (the §2.1 per-frame hotspot)
//!
//! It records distribution statistics (median/p95/p99/max/stddev), measured
//! occupancy, and normalized ratios, then prints a summary and optionally writes
//! machine-readable JSON (strategy §15). GPU timings are deferred to the native
//! GPU lane and reported as explicitly unavailable.

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

fn main() {
    let config = match Config::from_args() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("aestra-bench: {message}");
            eprintln!(
                "usage: aestra-bench (--scenario <name> | --all) [--frames N] [--warmup N] \
                 [--seed <dec-or-0xhex>] [--out results.json] [--commit <sha>]"
            );
            eprintln!("scenarios: {}", scenario::names());
            std::process::exit(2);
        }
    };

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
        let mut scenario = None;
        let mut all = false;
        let mut frames = 64usize;
        let mut warmup = 8usize;
        let mut seed = 0xa357_2a11_5eed_0001u64;
        let mut out = None;
        let mut commit = default_commit();

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
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

        if !all && scenario.is_none() {
            return Err("expected --scenario <name> or --all".into());
        }
        if frames == 0 {
            return Err("--frames must be greater than zero".into());
        }
        Ok(Self {
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
    let per_instance_capacity = GpuEffectArtifact::from_instance(&instances[0])
        .map_err(|error| format!("GPU artifact is unavailable ({error}); cannot size capacity"))?
        .total_slots;
    let capacity =
        per_instance_capacity.saturating_mul(instance_count.min(u32::MAX as usize) as u32);

    // A single reused buffer keeps CPU-reference timing focused on reconstruction
    // work rather than the caller's output allocation. `from_instance` allocates
    // internally on purpose — that allocation is exactly what we want to measure.
    let mut samples: Vec<ParticleSample> = Vec::new();

    for _ in 0..config.warmup {
        for instance in &mut instances {
            instance.advance(dt);
            samples.clear();
            instance.evaluate(&mut samples);
            black_box(GpuEffectArtifact::from_instance(instance).ok());
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
                GpuEffectArtifact::from_instance(instance)
                    .map(|artifact| artifact.total_slots)
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
