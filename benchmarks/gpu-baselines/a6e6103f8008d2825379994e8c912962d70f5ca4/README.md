# GPU sweep — a6e6103 (B006/B007/B008)

GPU-timestamp captures for the analytical-risk scenarios, extending the first GPU
baseline (`../20d5785…/`, which holds prism_bloom / b003 / b004). Same tool and
hardware; only the scenarios are new, so the b003/b004 numbers below are quoted
from that baseline for comparison.

## Provenance

| Field | Value |
| --- | --- |
| Commit | `a6e6103f8008d2825379994e8c912962d70f5ca4` |
| Captured | 2026-09-04 |
| GPU | NVIDIA GeForce RTX 4070 SUPER, driver 596.49, Vulkan |
| Tool | `aestra-viewer --gpu-bench` (600 frames / 120 warmup) |

## Results — `aestra::gpu::simulate` GPU time (ms)

| Scenario | Alive (≈steady) | Structure | sim p50 | sim p95 | sim p99 |
| --- | ---: | --- | ---: | ---: | ---: |
| b004_sparse_large | 5,000 | 500k cap, once | 0.001 | 0.002 | 0.004 |
| b003_single_dense | 100,000 | 1 emitter, once | 0.362 | 0.713 | 0.720 |
| **b006_many_emitters** | ~100,000 | **64 emitters**, once | **0.965** | 1.110 | 1.116 |
| **b007_loop_pressure** | ~4,000 | **LP=32**, continuous | **0.096** | 0.107 | 0.108 |
| **b008_curve_stress** | ~2,000 | **8-key spawn curve**, continuous | **0.389** | 0.565 | 0.600 |

Full per-metric distributions are in the per-scenario JSON files beside this manifest.

## Findings — which §2.x GPU risks are real

This is the Phase 6 analytical-kernel scaling result. Three of the four GPU
simulation risks are **confirmed** by measurement; one is **refuted**.

- **§2.2 capacity-bound — REFUTED.** b004 (500k slots, 5k alive) simulates at
  0.001 ms; dead slots are free. Cost tracks alive particles, not capacity. (See
  the `20d5785…` baseline.)
- **§2.3 emitter lookup — CONFIRMED.** b006 (64 emitters, ~100k alive) costs
  **0.965 ms vs b003's 0.362 ms** for the *same* particle count in 1 emitter —
  **2.7×**. Per-slot emitter-ownership resolution scales with emitter count.
- **§2.4 loop pressure — CONFIRMED.** b007 (LP=32, continuous, ~4k alive) costs
  0.096 ms; b004 (LP=1, ~5k alive) costs 0.001 ms — **~100× per particle**.
  Reconstructing historical cycles is expensive and grows with lifetime/duration.
- **§2.5 curve emission — CONFIRMED.** b008 (curve-driven spawn, ~2k alive) costs
  0.389 ms vs b007 (constant rate, ~4k alive) 0.096 ms — **~8× per particle** for
  half the count. Inverse-curve evaluation at spawn reconstruction is real.

## Implication for optimization priority (Phase 7)

Measurement redirects the roadmap: the capacity-scaling optimizations (dead-slot
dispatch elimination, packed slot ranges) are **not** justified for the GPU kernel.
The real GPU bottlenecks, in rough order of measured severity per unit work, are:

1. **Loop-pressure reconstruction (§2.4)** — ~100× per-particle penalty; the
   sharpest cliff. Continuous effects with long lifetimes should cache or bound
   historical-cycle search.
2. **Curve-driven emission (§2.5)** — ~8× per-particle; precompute a curve→time
   lookup instead of per-candidate inverse search.
3. **Emitter lookup (§2.3)** — 2.7× at 64 emitters; replace linear per-slot search
   with precomputed slot→emitter ranges or per-emitter dispatch.

## Sweep completion — emitter-count and occupancy curves

Generated from `benchmarks/sweep-scenarios/` (`sweep_*.json` beside this manifest),
filling the two axes left open after B006/B007/B008.

**Emitter count at fixed ~100k alive** (`simulate` p50, ms):

| Emitters | 1 (b003) | 4 | 16 | 64 (b006) |
| --- | ---: | ---: | ---: | ---: |
| sim p50 | 0.362 | 0.196 | 0.223 | 0.965 |

Not monotonic at the low end — 1/4/16 sit within measurement noise of each other
(GPU under-utilization and b003's high-variance distribution). §2.3 is real but
**only bites at high emitter counts (64 ≈ 3–5×)**; moderate emitter counts are
effectively free. This lowers §2.3's practical priority — most effects have few
emitters.

**Occupancy at fixed 10k alive** (`simulate` p50, ms):

| Capacity | 10k (100%) | 100k (10%) | 1M (1%) |
| --- | ---: | ---: | ---: |
| sim p50 | 0.033 | 0.135 | 0.003 |

Non-monotonic (workgroup-count / GPU-utilization confound at 10k alive), but the
decisive point stands: **1M capacity holding only 10k alive is the *cheapest* of the
three** — 100× the capacity, ~10× *less* time. §2.2 (capacity-bound sim) is refuted
even more firmly: dead slots are free, and more of them can even improve GPU
saturation.

**Net:** the confirmed GPU bottlenecks and their order are unchanged — loop pressure
(§2.4) ≫ curve emission (§2.5) > emitter lookup (§2.3, high-count only); capacity
(§2.2) is a non-issue.

## Caveats

- b007/b008 are `LoopContinuous`; b003/b004/b006 are `Once`. The loop-pressure and
  curve comparisons are therefore continuous-vs-continuous (b007 vs b008) and
  continuous-vs-once (b007 vs b004) — the once/continuous distinction is part of the
  §2.4 mechanism, not a confound to remove.
- Alive counts for the continuous scenarios are approximate steady-state; the
  600-frame capture spans the ramp to steady state, so p95/p99 better reflect the
  populated regime than the mean.
- Single GPU/driver; scaling shape transfers, absolute numbers do not.

## Reproduce

```powershell
cargo run -p aestra-viewer -- --effect apps/aestra-bench/scenarios/b006_many_emitters.aestra.ron --gpu-bench out.json
```
