# Baseline — c09393c (post Phase 3)

Aestra runtime benchmark baseline captured after the Phase 3 optimization that
removes the per-frame particle-buffer allocation from GPU effect updates. This is
the **CPU-headless lane** (`aestra-bench`); GPU-stage timings await the native GPU
lane and are unavailable in the JSON. Supersedes `../903d453…/`.

## Provenance

| Field | Value |
| --- | --- |
| Commit | `c09393ce50d6f7a9c3ad71753ead7915966c5ffd` |
| Captured | 2026-09-03 |
| Lane | cpu-headless (no window, no GPU) |
| Build profile | `--release` |
| Frames / warmup | 240 / 32 |
| Seed | `0xa3572a115eed0001` (harness default) |
| CPU | 16 logical cores |
| OS / arch | windows / x86_64 |
| Bevy | 0.19.1 |
| wgpu | 29.0.4 |

> The `artifact update` stage now measures `GpuEffectArtifact::dynamics_from_instance`
> — the particle-free builder the runtime uses on the per-frame update path — rather
> than the full `from_instance` measured in the pre-Phase-3 baseline. That change in
> what the runtime does per frame *is* the optimization.

## Results

Per-frame CPU medians (ms):

| Scenario | Capacity | Alive | artifact update | aestra total |
| --- | ---: | ---: | ---: | ---: |
| b001_empty | 64 | 8 | 0.0005 | 0.0012 |
| b002_single_small | 1,024 | 1,024 | 0.0005 | 0.1009 |
| b003_single_dense | 100,000 | 100,000 | 0.0024 | 11.6474 |
| b004_sparse_large | 500,000 | 5,000 | 0.0006 | 0.5575 |
| b005_many_small (×100) | 10,000 | 10,000 | 0.0277 | 1.1813 |

## Phase 3 before / after

`artifact update` stage, median ms — pre-Phase-3 (`from_instance`, baseline
`903d453`) vs post-Phase-3 (`dynamics_from_instance`, this baseline):

| Scenario | before | after | change |
| --- | ---: | ---: | ---: |
| b002_single_small | 0.0194 | 0.0005 | ~39× faster |
| b003_single_dense | 0.7412 | 0.0024 | ~309× faster |
| b004_sparse_large | 3.5438 | 0.0006 | ~5900× faster |
| b005_many_small | 0.0399 | 0.0277 | ~1.4× faster |

The stage is now effectively **capacity-independent**: b004 (500k slots) and b002
(1k slots) both prepare in ~0.0005 ms. The remaining b003/b004 CPU cost is the
analytical particle reconstruction in `evaluate`, which is a CPU-reference-only
cost — on the GPU path that work happens in-shader.

## Reproduce

```powershell
cargo run -p aestra-bench --release -- --all --frames 240 --warmup 32 `
  --commit c09393ce50d6f7a9c3ad71753ead7915966c5ffd `
  --out benchmarks/baselines/c09393ce50d6f7a9c3ad71753ead7915966c5ffd/baseline.json
```
