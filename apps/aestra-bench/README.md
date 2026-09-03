# aestra-bench

A headless, deterministic benchmark harness for the Aestra runtime and GPU-artifact
preparation path. It is the first slice of the
[Runtime Benchmarking & Profiling implementation plan](../../docs/aestra_runtime_benchmarking_implementation_plan.md)
(Phase 1) and its parent [strategy](../../docs/aestra_runtime_benchmarking_profiling_plan.md).

## What it measures

Per simulation frame (fixed 60 Hz step), it times three real CPU stages:

| Stage | Call | Notes |
| --- | --- | --- |
| `runtime advance` | `EffectInstance::advance` | clock + choreography |
| `cpu reference eval` | `EffectInstance::evaluate` | analytical particle reconstruction |
| `artifact update` | `GpuEffectArtifact::from_instance` | the strategy's §2.1 per-frame hotspot |

No window or GPU is created, so it runs on ordinary CI (the PR CPU lane). GPU
timings are deferred to the native-GPU lane and reported as explicitly unavailable
rather than as a misleading zero.

For each stage it reports median / p95 / p99 / max / stddev, plus measured
occupancy (`alive / capacity`) and normalized ratios (ns per 1k slots, ns per 1k
alive). Output is a human summary and, with `--out`, a JSON array (strategy §15).

## Usage

```powershell
# One scenario
cargo run -p aestra-bench -- --scenario b003_single_dense

# All canonical scenarios, writing machine-readable JSON
cargo run -p aestra-bench -- --all --out results.json

# Longer capture, fixed seed, tagged with a commit
cargo run -p aestra-bench -- --all --frames 240 --warmup 32 --seed 0xdead --commit $(git rev-parse HEAD)
```

Flags: `--scenario <name>` or `--all`; `--frames N` (default 64); `--warmup N`
(default 8); `--seed <dec-or-0xhex>`; `--out <path>`; `--commit <sha>` (also read
from `AESTRA_BENCH_COMMIT` or `GITHUB_SHA`).

## Scenarios

Scenarios are real `.aestra.ron` assets under [`scenarios/`](scenarios), embedded at
build time for determinism. This first set is a slice of the strategy's B001–B010:

- `b001_empty` — floor / harness overhead (tiny emitter).
- `b002_single_small` — 1 emitter, ~1k capacity, ~100% occupancy.
- `b003_single_dense` — 1 emitter, 100k capacity, ~100% occupancy.
- `b004_sparse_large` — 1 emitter, 500k capacity, ~1% occupancy.
- `b005_many_small` — 100 concurrent instances of a 100-particle effect (~10k total).

A `b002` → `b003` comparison (same shape, 100× capacity) isolates how CPU cost
scales with particle capacity. `b004` (sparse) shows how much of that cost is
capacity-bound rather than alive-bound, and `b005` isolates per-instance overhead
at roughly constant total particle count. A scenario's `instances` count lives in
[`src/scenario.rs`](src/scenario.rs), not the asset.

## Adding a scenario

Add a `.aestra.ron` under `scenarios/` and register it in
[`src/scenario.rs`](src/scenario.rs). The full benchmark matrix (occupancy,
emitter count, instance count, loop pressure, curves, materials, nesting, …) is
authored in later phases of the plan.
