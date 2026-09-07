# Benchmark data and runners

This directory contains benchmark scenarios, focused runners and captured baselines.

- `baselines/`: CPU benchmark reports from `apps/aestra-bench`.
- `gpu-baselines/`: native GPU reports from `aestra-viewer --gpu-bench` and the
  opt-in GPU experiments in `apps/aestra-bench/src/gpu_trails/`.
- `sweep-scenarios/`: effects used by GPU workload sweeps.
- `asset-browser/`: opt-in 10,000-source editor browsing benchmark. Its harness is
  included in the editor test binary to exercise the real private UI implementation.

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

## Occupancy / view-count sweep

```powershell
$env:WGPU_BACKEND = 'vulkan' # repeat with 'dx12'
cargo +1.98.1-x86_64-pc-windows-msvc run -p aestra-bench --features gpu --locked -- --gpu-trails sweep --seed 7 --commit <revision> --out benchmarks/gpu-baselines/<run>/sweep-vulkan.json
Remove-Item Env:WGPU_BACKEND
```

`sweep` defaults to requested occupancies **1, 2, 5, 10, 25, 50, 75, 100%** and
**1, 2, 4, 8 views**, with the same 8 warmups and 64 paired samples. Override with
`--occupancies 2,3,4,5 --views 4,8` to refine a crossover. These flags also work with
`--gpu-trails rendering`; without them `rendering` retains its original 16/1,024-owner
and one/four-view matrix. The flags are rejected for preparation-only and CPU modes.

By default, occupancy means **active trail owners out of 1,024**, each with all 63 valid
segments; the shape controls below can vary this fixture. Percentages must be finite in `(0,100]`,
are rounded to the nearest owner (minimum one), and levels mapping to the same count
are rejected. Reports record actual counts and occupancy percentages, not rounded labels.
Views must be distinct integers in `[1,8]`. Owner slots are spread across the pool even
for non-divisor occupancies. Dense cases overlap colored alpha layers, so this experiment
also changes pixel workload as occupancy increases; it is not an isolated vertex-cost test.

Schema v2 adds actual occupancy to each path result and a `comparisons` row for each
occupancy/view pair. `median_saving_percent` and `p95_saving_percent` compare full-path
and compact-path total percentiles; positive means compaction saves time. Ratios are null
if the full-path percentile is zero. `paired_median_saving_ns` is the median of
`full[i] - compact[i]` from the alternating A/B pairs; it need not equal the difference
between two independently computed medians. Raw samples remain in acquisition order.

Every iteration checks the complete indirect draw command for **every view**. Each matrix
cell compares exact, nonblank images after warmup; timing queries and readback storage are
bounded for eight views. These are observations, not an automatic runtime threshold.
Treat sign changes as sampled brackets, not proof of a monotonic break-even function.

The [first six sweeps and measured crossover brackets](gpu-baselines/trails-break-even-2026-09-06/README.md)
cover three runs per backend. All 192 A/B image comparisons passed. One/two-view
medians favor full-range drawing; four/eight-view crossover ranges differ by backend
and have marginal/noisy cases. No runtime bypass is selected from these results.

## Capacity and partially filled history

Rendering and sweep modes additionally accept one fixture shape per report:

```powershell
$env:WGPU_BACKEND = 'vulkan' # repeat with 'dx12'
cargo +1.98.1-x86_64-pc-windows-msvc run -p aestra-bench --features gpu --locked -- --gpu-trails sweep --owner-capacity 128 --history-points 16 --history-fill 25 --occupancies 5,50,100 --views 1,4,8 --seed 7 --commit <revision> --out benchmarks/gpu-baselines/<run>/history-fill.json
Remove-Item Env:WGPU_BACKEND
```

- `--owner-capacity`: 1–1,024 allocated owner slots (default 1,024).
- `--history-points`: 2–64 records per owner **including the live head** (default 64).
- `--history-fill`: percentage of the remaining historical sample slots to populate,
  finite in `(0,100]` (default 100). It rounds to the nearest sample, minimum one.

These bounds match current runtime trail limits. All three flags are rejected for
CPU and preparation-only modes. Occupancy percentages are converted **after** parsing
capacity, regardless of argument order. If levels round to duplicate owner counts
(including default levels at very small capacities), provide a distinct explicit
`--occupancies` list; for capacity one, use `--occupancies 100`.

Schema v3 records `owner_capacity`, `history_points`, `retained_history_samples` and
the actual `history_fill_percent`. The last two are null for preparation-only reports.
For example, 16 points at requested 25% fill means 4 historical samples plus the head
(26.67% of the 15 sample slots). Full-range drawing submits `capacity * (points - 1)`
candidates; compacted drawing submits `active_owners * retained_history_samples`.
Actual counts, image equality, per-view indirect commands and timings are checked as before.
Every view must contain visible pixels, even for a one-segment fixture.

Partial histories retain the newest samples with unchanged sample spacing, using
the production ring-buffer indexing. Their visible tails are therefore shorter;
changing fill also changes pixel coverage, not just compaction work. Changing point
capacity changes sampling density along the same full trajectory. This is a controlled
fixture with uniform fill across live owners, flat caps, no expiry and all owners in
view—not a simulation of mixed-age histories or whole-application frame time.

The [48 capacity/history reports and findings](gpu-baselines/trails-history-fill-2026-09-06/README.md)
contain three runs per shape/backend and 432 successful image comparisons. Full owner
occupancy can favor compaction with partial histories while losing with full histories;
allocation size, point capacity and backend also change the result. No runtime rule
is selected from these single-adapter measurements.
