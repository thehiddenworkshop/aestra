# Benchmark data and runners

This directory contains benchmark scenarios and captured baselines, not Rust test harnesses.

- `baselines/`: CPU benchmark reports from `apps/aestra-bench`.
- `gpu-baselines/`: native GPU reports from `aestra-viewer --gpu-bench` and the
  opt-in GPU experiments in `apps/aestra-bench/src/gpu_trails/`.
- `sweep-scenarios/`: effects used by GPU workload sweeps.

## Headless trail experiments

```powershell
$env:WGPU_BACKEND = 'dx12'
cargo +1.98.1-x86_64-pc-windows-msvc run -p aestra-bench --features gpu --locked -- --gpu-trails preparation --seed 7 --commit <revision> --out benchmarks/gpu-baselines/<run>/preparation.json
cargo +1.98.1-x86_64-pc-windows-msvc run -p aestra-bench --features gpu --locked -- --gpu-trails rendering --seed 7 --commit <revision> --out benchmarks/gpu-baselines/<run>/rendering.json
Remove-Item Env:WGPU_BACKEND
```

Replace `<revision>` and `<run>` with the measured code revision and a unique run name.
Use the local native Rust toolchain on non-Windows platforms. Choose `WGPU_BACKEND` for
the target adapter; unset it to use primary backends. Timestamp queries are required.
These commands are headless and do not depend on Bevy or `aestra-bevy-render`.

Both modes default to 8 warmups and 64 measured samples; `--warmup` and `--frames`
override them. Omit `--out` to generate a timestamped directory under `gpu-baselines/`.
An explicit output file is replaced on success. `--commit` defaults to
`AESTRA_BENCH_COMMIT`, then `GITHUB_SHA`, otherwise `unknown`; mark uncommitted code
explicitly, e.g. `<revision>+worktree`.

Reports preserve nanosecond samples in acquisition order, median/p95, adapter and driver,
sampling settings, seed, submitted counts and validation outcomes. Preparation-only reports
use null for total rendering/drawing/image results, not zero. Rendering compares full-range
and compacted alpha-blended draws across sparse/dense histories and one/four views; every
image comparison must be exact and nonblank. Invalid timing/count/image observations fail
the run before writing a new report. Existing files are not removed on failure.

Run on an idle GPU and compare repeated runs on the same backend. These are controlled
workloads, not whole-application speedups. The Vulkan timestamp readback correction and
repeated Vulkan/DirectX 12 baselines are described in
[host motion tracks](../docs/host_motion_tracks.md#full-range-versus-compacted-trail-rendering).

Native renderer correctness tests remain in `crates/aestra-bevy-render/tests/`; performance
experiments are invoked with `cargo run`, not ignored `cargo test` cases. The ordinary
`aestra-bench --scenario ...` / `--all` CPU workflow does not require the `gpu` feature.

## Runner migration validation

The [2026-09-06 capture](gpu-baselines/trails-runner-2026-09-06/) records both migrated
experiments at 8 warmups / 64 samples / seed 7, measured from `fdd02ea+worktree`:

- [Preparation, Vulkan](gpu-baselines/trails-runner-2026-09-06/preparation-vulkan.json):
  four sparse/dense × one/four-view cases, with indirect count checks.
- [Rendering A/B, DirectX 12](gpu-baselines/trails-runner-2026-09-06/rendering-dx12.json):
  eight path results; all four exact, nonblank image comparisons passed.

Different backends were used deliberately because of the mixed-pipeline Vulkan timing
anomaly. Do not subtract or combine measurements between these two reports.

## Timestamp correction validation

The [corrected captures](gpu-baselines/trails-timestamp-fix-2026-09-06/) include three
8-warmup/64-sample rendering runs per backend from `8c87956+worktree`, seed 7.
All reports passed exact nonblank image comparisons, indirect-count checks and monotonic
timestamp validation. Reports identify `timestamp_readback: "separate_command_buffer"`.
Historical reports above predate that fix and are not silently replaced.

The reduced native timestamp correctness probe is opt-in:

```powershell
$env:WGPU_BACKEND = 'vulkan' # also validate 'dx12'
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bench --features gpu --locked mixed_pass_queries -- --ignored --nocapture
Remove-Item Env:WGPU_BACKEND
```

Unlike the benchmark commands, this checks correctness only and publishes no performance
report. The same-encoder resolve/copy control failed immediately with six zero timestamps;
the corrected split-copy version passes repeated mixed compute/render submissions.
