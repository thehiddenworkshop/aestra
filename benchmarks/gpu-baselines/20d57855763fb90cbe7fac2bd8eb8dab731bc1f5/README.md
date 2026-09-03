# GPU baseline — 20d5785

First archived **GPU-timestamp** baseline, captured with `aestra-viewer --gpu-bench`
(600 measured frames after 120 warm-up, per-frame distribution). These are real
Vulkan timestamp-query results on discrete hardware — the native GPU lane the
strategy calls for, run locally.

## Provenance

| Field | Value |
| --- | --- |
| Commit | `20d57855763fb90cbe7fac2bd8eb8dab731bc1f5` |
| Captured | 2026-09-04 |
| GPU | NVIDIA GeForce RTX 4070 SUPER |
| Driver | 596.49 |
| Backend | Vulkan |
| Frames / warmup | 600 / 120 |
| Tool | `aestra-viewer --gpu-bench` |

## Results — `aestra::gpu::simulate` GPU time (ms)

| Effect | Capacity | Alive | sim p50 | sim p95 | sim p99 | render p50 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| prism_bloom (4 emitters, continuous) | 360 | ~100 | 0.074 | 0.082 | 0.083 | 0.037 |
| b003_single_dense | 100,000 | 100,000 | 0.362 | 0.713 | 0.720 | 0.389 |
| b004_sparse_large | 500,000 | 5,000 | **0.001** | 0.002 | 0.004 | 0.043 |

Full per-metric distributions (p50/p95/p99/max/min/mean/stddev, plus transparent-pass
pipeline stats) are in the per-effect JSON files beside this manifest.

## Headline finding — §2.2 is refuted

The strategy's central §2.2 worry was that GPU simulation cost scales with **total
particle capacity**. The measured reality contradicts it:

- **b004 has 5× the capacity of b003 (500k vs 100k slots) but simulates ~360× faster**
  (0.001 ms vs 0.362 ms p50). Dead slots are essentially free — the analytical kernel
  is driven by **alive particles**, not allocated capacity.
- Dense throughput: **100k live particles = 0.36 ms p50** (~3.6 ns/particle),
  comfortably inside a 2 ms GPU budget on this hardware.

This means the feared capacity-bound GPU simulation does **not** materialize here, and
the analytical kernel looks viable for sparse large-capacity effects — reframing the
priority of the strategy's later capacity-scaling milestones (M5–M8) for the GPU path.
(The CPU-side capacity cost was real and separate — the per-frame artifact allocation,
already removed in Phase 3.)

## Secondary observations

- **Frame-time tail (§10):** b003 sim p99 (0.72 ms) is ~2× its p50 (0.36 ms) — real
  per-frame variance, likely GPU clock ramp plus the t=0 burst of 100k particles.
- **Loop-pressure cost (§2.4):** prism_bloom (only ~100 alive, but `LoopContinuous`
  and 4 curve-driven emitters) costs 0.074 ms — more than b004's 5k alive — reflecting
  historical-cycle reconstruction and per-particle complexity, not particle count.

## Caveats

- The `bXXX` scenarios are `playback_mode: Once` with an 8 s duration; a 600-frame
  capture outlasts the effect, so **mean/min are contaminated by empty tail frames**.
  Use **p50** as the robust statistic (the effect is populated for the majority of the
  capture). prism_bloom is continuous, so its distribution is clean.
- Small workloads (few workgroups) are affected by GPU under-utilization, so absolute
  sub-0.05 ms values are not directly comparable across effects; the b003↔b004 capacity
  comparison is the reliable one.
- Single GPU, single driver. Absolute numbers are hardware-specific; the *scaling
  shape* is the transferable result.

## Reproduce

```powershell
cargo run -p aestra-viewer -- --effect apps/aestra-bench/scenarios/b003_single_dense.aestra.ron --gpu-bench out.json
```
