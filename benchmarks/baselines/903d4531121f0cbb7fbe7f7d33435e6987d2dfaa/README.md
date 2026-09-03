# Baseline — 903d453

Aestra runtime benchmark baseline for the five-scenario suite (B001–B005). This is
the **CPU-headless lane** captured by `aestra-bench`; it has no render world, so
there is no GPU/driver dimension. GPU-stage timings await the native GPU lane and
are recorded as unavailable in the JSON. Supersedes the three-scenario baseline at
`../bfe29d8…/`.

## Provenance

| Field | Value |
| --- | --- |
| Commit | `903d4531121f0cbb7fbe7f7d33435e6987d2dfaa` |
| Captured | 2026-09-03 |
| Lane | cpu-headless (no window, no GPU) |
| Build profile | `--release` |
| Frames / warmup | 240 / 32 |
| Seed | `0xa3572a115eed0001` (harness default) |
| CPU | 16 logical cores |
| OS / arch | windows / x86_64 |
| Bevy | 0.19.1 |
| wgpu | 29.0.4 |

> Build profile matters: these numbers are `--release`. Compare only against
> other `--release` captures. Absolute timings are machine-dependent; prefer the
> normalized ratios and cross-scenario shape when comparing across hosts.

## Results

Per-frame CPU medians (ms):

| Scenario | Capacity | Alive | Occupancy | artifact update | aestra total |
| --- | ---: | ---: | ---: | ---: | ---: |
| b001_empty | 64 | 8 | 12.5% | 0.0006 | 0.0014 |
| b002_single_small | 1,024 | 1,024 | 100% | 0.0194 | 0.1205 |
| b003_single_dense | 100,000 | 100,000 | 100% | 0.7412 | 12.3497 |
| b004_sparse_large | 500,000 | 5,000 | 1% | 3.5438 | 4.1161 |
| b005_many_small (×100) | 10,000 | 10,000 | 100% | 0.0399 | 1.2117 |

Full per-stage distributions and normalized ratios are in
[`baseline.json`](baseline.json).

### Observations to drive later phases

- **Capacity-bound, not alive-bound (§2.1):** b004 has 20× *fewer* live particles
  than b003 (5k vs 100k) yet its `artifact update` stage costs ~4.8× *more* (3.54 vs
  0.74 ms). `GpuEffectArtifact::from_instance` clearly tracks allocated capacity,
  not occupancy. This is the primary Phase 3 target.
- **Per-instance overhead (§2.7):** b005 spends ~1.21 ms of CPU on 100 instances
  totalling 10k particles — materially more than a single effect of comparable
  total particle count would. Instance count is a real cost axis.
- **Runtime advance** remains negligible across all scenarios; CPU cost is
  dominated by analytical reconstruction and artifact preparation.

## Reproduce

```powershell
cargo run -p aestra-bench --release -- --all --frames 240 --warmup 32 `
  --commit 903d4531121f0cbb7fbe7f7d33435e6987d2dfaa `
  --out benchmarks/baselines/903d4531121f0cbb7fbe7f7d33435e6987d2dfaa/baseline.json
```
