# Baseline — bfe29d8

First archived Aestra runtime benchmark baseline (implementation-plan Phase 2).
This is the **CPU-headless lane** captured by `aestra-bench`; it has no render
world, so there is no GPU/driver dimension. GPU-stage timings await the native
GPU lane and are recorded as unavailable in the JSON.

## Provenance

| Field | Value |
| --- | --- |
| Commit | `bfe29d8011ec826aba3204623372b21f35ec1a44` |
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
> other `--release` captures.

## Results (median aestra total CPU, per frame)

| Scenario | Capacity | Occupancy | Median | p95 | p99 |
| --- | ---: | ---: | ---: | ---: | ---: |
| b001_empty | 64 | 12.5% | 0.0013 ms | 0.0024 ms | 0.0081 ms |
| b002_single_small | 1,024 | 100% | 0.1153 ms | 0.1217 ms | 0.1554 ms |
| b003_single_dense | 100,000 | 100% | 12.2739 ms | 12.5780 ms | 12.9802 ms |

Full per-stage distributions (runtime advance / cpu reference eval / artifact
update) and normalized ratios are in [`baseline.json`](baseline.json).

### Observations to drive later phases

- **b002 → b003 (100× capacity):** aestra-total CPU scales ~106× — cost is
  strongly capacity-bound in the analytical path, as the strategy predicts.
- **Artifact update** (`GpuEffectArtifact::from_instance`) scales worse than the
  particle-reconstruction stage across the same jump — the Phase 3 target (§2.1).

## Reproduce

```powershell
cargo run -p aestra-bench --release -- --all --frames 240 --warmup 32 `
  --commit bfe29d8011ec826aba3204623372b21f35ec1a44 `
  --out benchmarks/baselines/bfe29d8011ec826aba3204623372b21f35ec1a44/baseline.json
```

Absolute timings are machine-dependent; prefer the normalized ratios and the
b002→b003 scaling shape when comparing across hosts.
