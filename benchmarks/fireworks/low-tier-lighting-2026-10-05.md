# F7F2 — explicit low-tier selected-light profile

Base: `7a183f72` plus scoped F7F2 changes. This deliberately chooses useful
selected-star lighting rather than making the low tier representative-only.
The previous output-eight failures remain in the F7F1 evidence unchanged.

## Authored choice

The opt-in viewer fixture now uses per-output caps **32 / 16 / 24** for
high / medium / low. Global host caps remain **96 / 48 / 24**, representative
caps **8 / 4 / 2**. A sparse low-tier output can use the existing global 24-slot
ceiling; multiple outputs still share that same global budget. This explicitly
changes fixture authoring, not a runtime bypass of authored limits. Per-output
caps need not be monotonic when global budgets and occupancy differ.

Intensity remains 1500 → 600 → 0 lm over normalized age, range 12 m, shadows
off. Particle density/materials/trajectories, events, seeds, clips, transforms,
renderer/history budgets and host safety budgets are unchanged. High/medium
compiled lighting caps are unchanged. This is an opt-in benchmark/showcase
fixture (`--particle-light-bench`), not an automatic rewrite of users' effects.
Hosts using the public policy still cannot raise an authored per-output ceiling.

Reports identify `f7f2_low_output24_global24`; the read-only policy validator can
require that profile and rejects old or missing identities. Unit assertions lock
the cap table, original intensity/range and preservation of normal authored fields.

## Receiver qualification

Raw: `target/fireworks-f7/low-output24-images-2026-10-05`.
RTX 4070 SUPER/Vulkan, 960×540 audience perspective, HDR/exposure 0/Tony,
playback-only seed `0xf1e0000000000001`. Same sampling, diffuse receiver geometry,
off/on/off controls and thresholds as F7F1: ≥100 receiver pixels above 3/channel
and ≥1000 positive energy in at least one active sample per workload/bloom mode.
Per-light intensity/range and geometry stayed unchanged; thresholds were not lowered.

**All six low-tier workload/bloom response gates pass**. Forty-six samples / 138
PNGs are retained; every restored control has zero significant changed pixels,
and before-birth/after-cleanup controls match exactly. Read-only recomputation
from all PNGs independently passes.
Image controls keep representative pulses enabled and identical for all workloads,
as in F7F1; only the selected-light adapter cap changes. The cost matrix keeps
representative pulses on for show and off for hero/volley, also matching F7F1.

| Workload | Bloom | Best-energy frame | Positive pixels | Positive energy |
| --- | ---: | ---: | ---: | ---: |
| Hero | 0 | 85 | 865 | 7944 |
| Hero | 0.15 | 85 | 342 | 1522 |
| Volley | 0 | 85 | 540 | 5796 |
| Volley | 0.15 | 85 | 426 | 2940 |
| Show | 0 | 85 | 431 | 2917 |
| Show | 0.15 | 85 | 372 | 1983 |

The response is not stronger at every frame: the larger eligible source run can
change deterministic admission among competing outputs. This gate establishes
useful bounded contribution in the authored workload, not balanced natural-star
tracking, uniform show lighting, AAA art approval or production-finale certification.
Capacity-bound observations are not exact live GPU-light counts.

## Repeated ordinary costs

Raw: `target/fireworks-f7/low-output24-costs-2026-10-05`.
Three alternating on/off pairs per hero/volley/show: **18 native processes, all exit 0**.
The unchanged workload/admission/transport/budget/log/demand/retirement gates pass;
the profile-aware host policy gate and public index/offset no-churn checks pass.
Every final 60-sample show cleanup window matches control at one acknowledged index.
120 warm-up frames; 600 measured hero/volley frames and 1680 show frames, fixed
60-Hz unpaced forward playback. Timing runs do not use allocation instrumentation.

Outer render-graph GPU p95, milliseconds across the three runs:

| Workload | Adapter on | Matched control |
| --- | ---: | ---: |
| Hero | 1.096–1.804 | 1.089–1.970 |
| Volley | 1.177–1.762 | 1.052–1.487 |
| Show | 5.263–8.031 | 5.172–7.606 |

Run variability is material; do not subtract independent p95s to claim a causal
light cost or guaranteed improvement over F7F1. These are not paired frames,
whole-game timing, a production finale or proof of the provisional ≤4-ms VFX target.

## Separate allocation qualification

Raw: `target/fireworks-f7/low-output24-allocations-2026-10-05`.
Another **18 sequential native processes** pass the unchanged work/cleanup/public
reuse gates and private allocation size/count gates. All reports identify the new
profile. Every census has one Z buffer (12288 B), scratchpad (117504 B), metadata
buffer (48 B) and 2–4 48-byte staging buffers, below the eight-slot ceiling.
These are separate instrumented runs, not timing evidence, exact private generation
IDs, driver free history, total VRAM or arbitrary-overload hard memory limits.

## Verification and scope

The current low authored fixture's receiver, repeated cost/cleanup and sampled
allocation gates are qualified. High/medium caps are unchanged; their F7F1/prior
native evidence remains scoped to those earlier executions. This closes the narrow
F7F2 blocker before smoke work, not all general-adapter/performance/art acceptance.

Viewer unit tests: **95 passed / 17 ignored**. Scoped all-target Clippy with
`-D warnings`, workspace all-target checks, formatting and policy/private validator
self-tests pass. Native image generation and read-only PNG recomputation pass.
All 72 matrix report/log hashes match their manifests, with binaries/assets pinned
throughout each matrix. The pre-image source/test-binary hashes are unchanged.
[Machine-readable measurements and provenance](low-tier-lighting-2026-10-05.json).

Production binary SHA-256:
`d45e5e9f8c32c6afb8511a0b77f51608f2f4912cfd9be0a6559be7da619e5fbc`.
Image test binary SHA-256:
`02d980e86da220a0ef713f06a6efc24c81b620fd3c03a0074623f3dfdc82c49b`.

## Reproduction

Use fresh output directories; native GPU jobs must run sequentially.

```powershell
cargo build --locked -p aestra-viewer
& benchmarks/fireworks/run-particle-light-costs.ps1 -ReportsDirectory target/fireworks-f7/new-low-costs -Tiers low -RequireRetirement
& benchmarks/fireworks/validate-lighting-quality.ps1 -ReportsDirectory target/fireworks-f7/new-low-costs -FixtureProfile f7f2_low_output24_global24
& benchmarks/fireworks/run-particle-light-costs.ps1 -ReportsDirectory target/fireworks-f7/new-low-allocations -Tiers low -AllocationSnapshots
& benchmarks/fireworks/validate-cluster-private-allocations.ps1 -ReportsDirectory target/fireworks-f7/new-low-allocations
$env:AESTRA_GPU_LIGHT_IMAGE_REPORTS='target/fireworks-f7/new-low-images'
cargo test --locked -p aestra-viewer --bin aestra-viewer particle_light_images::low_tier_full_budget_illuminates_receivers_and_restores_controls -- --ignored --nocapture --test-threads=1
cargo test --locked -p aestra-viewer --bin aestra-viewer particle_light_images::retained_low_tier_full_budget_receiver_images_pass_the_gate -- --ignored --nocapture --test-threads=1
```

Next after qualification: **F8.3 lighting interaction with smoke**. Representative
burst flashes must affect smoke; selected particle lighting remains bounded and
optional. General multi-view/layer support, additional hardware/cadences, manual
editor authoring, natural-star registration and finale budgets remain open.
