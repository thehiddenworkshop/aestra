# F7D1 — portable particle-light selection prototype

Status: per-output GPU evaluation/compaction/top-K is implemented and native CPU/GPU conformance passes.
This is **not** full-show playback, global multi-output admission, asynchronous host realization,
visual light-response acceptance or a production performance claim.

Source: base commit `56acd818` plus the F7D1 changes in `crates/aestra-gpu/src/particle_lights.rs`,
`crates/aestra-gpu/src/shaders/aestra_particle_lights.wesl` and
`bevy/aestra-bevy-render/tests/particle_light_conformance.rs`. Windows/native Vulkan;
AMD Radeon(TM) Graphics integrated GPU (vendor 4098, device 5710), AMD proprietary driver
26.3.1. Do not compare these numbers to earlier RTX measurements as if hardware were identical.

## Protocol and bounds

```powershell
cargo test --locked -p aestra-gpu --lib
cargo test --locked -p aestra-bevy-render --test particle_light_conformance -- --nocapture --test-threads=1
cargo test --locked -p aestra-bevy-render --test particle_light_conformance particle_light_selection_profile -- --ignored --nocapture --test-threads=1
```

Run the profiling command three times sequentially with no concurrent conformance test. Each case
uses one warm-up and 20 GPU timestamp samples. Median is sorted sample index 10, p95 index 19
(conservative upper order statistic for this small sample). Query resolve and copy use separate
command buffers. Missing/zero/reordered timestamps fail the profile, not report zero cost.

Timestamp scope: first evaluation pass through final count pass, including intervening hierarchical
merge passes and their barriers. Excludes plan/fixture creation, particle upload, pipeline compilation,
counter clear, query resolve, selected-record/counter copy, blocking test readback and CPU validation.
No rendering, simulation, receiver lighting or resolution-dependent camera selection is timed.
The blocking readback is a test harness only; a future live adapter must not do this on its render thread.

Variant 0 uses smooth intensity decay and particle RGB, with deliberately dead/retired/wrong-emitter,
expired/negative/NaN-age, infinite-position and invalid-color slots. Variant 1 is a dense constant-color,
constant-intensity workload with all particles valid; alpha/size and even invalid particle RGB are ignored
when constant color is selected. Input pool starts at offset 3, not at buffer start. Stable ordinals run
opposite to input slot order. No candidate is sent through the host event queue.

## Three final sequential runs

These are the raw emitted summaries after removing redundant CPU fixture cloning from the profiling
loop. Times are milliseconds, selection only. GPU source buffers and capacity scratch are reused.

| Slots / cap / variant | Requested | Candidates | Selected / dropped | Two scratch buffers (bytes) | Final records (bytes) | Merge passes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 512 / 16 / 0 | 443 | 330 | 16 / 314 | 12,288 | 768 | 3 |
| 4,096 / 32 / 0 | 3,560 | 2,670 | 32 / 2,638 | 196,608 | 1,536 | 6 |
| 65,536 / 64 / 0 | 56,986 | 42,736 | 64 / 42,672 | 6,291,456 | 3,072 | 10 |
| 262,144 / 64 / 1 | 262,144 | 262,144 | 64 / 262,080 | 25,165,824 | 3,072 | 12 |

| Slots | Run 1 median / p95 | Run 2 median / p95 | Run 3 median / p95 |
| --- | ---: | ---: | ---: |
| 512 | 0.02284 / 0.02708 | 0.02276 / 0.02568 | 0.02308 / 0.02964 |
| 4,096 | 0.07388 / 0.11032 | 0.07364 / 0.11404 | 0.07352 / 0.11644 |
| 65,536 | 1.01568 / 1.54364 | 1.01608 / 2.03209 | 1.00900 / 1.36370 |
| 262,144 | 4.42092 / 6.14772 | 4.70256 / 6.72359 | 4.65500 / 8.46208 |

Earlier sequential runs of the same shader, with unnecessary CPU fixture cloning/encoding between
submissions, gave 6.51-6.68 ms medians for the dense case and 1.60-1.68 ms at 65,536 slots.
That variation is not a GPU algorithm speedup; host pacing/shared-memory/clock conditions can influence
these timestamp measurements. Preserve the tail costs and validate sustained live workloads before
choosing a production budget.

Scratch accounting excludes the existing 48-byte source particle pool, keys/160-byte plan,
32-byte per-pass uniforms, 16-byte counters and the optional readback/query buffers. No buffer is
allocated per particle beyond the reusable per-job capacity buffers. K is bounded by output quality,
host cap and source capacity. Each selected record is 48 bytes; counters add 16 bytes to a future
bounded selected-set readback. Zero budget or empty pool schedules no passes or buffers.

## Correctness evidence

The native conformance test checks eight pool/cap combinations (1, 63, 64, 65, 129, 1,025, 4,096,
65,536 slots), covering cap 1, cap below/at/above block width and odd/partial final runs. Each does
live -> empty -> reordered live on the same uncleared scratch allocation. Every selected identity,
light value and count matches the F7C reference; empty frames remove old records. Small-case full
particle byte comparisons prove no source writes (test-only readback, not an adapter design).
32-key intensity curves and 12-key gradients agree for smooth/linear/step and static/live color.

Engine-neutral tests also check the 48/32/160-byte record/key/plan ABI, WESL/WGSL validation,
device/address/dispatch rejection, authored + host cap intersection, zero-cap no-work, all 32 curve
keys, 20-key live gradients and invalid/missing/wrong-typed/empty live overrides.

## Decision / next gate

Keep the portable hierarchical selection baseline. Small workloads are inexpensive here, but
262,144 luminous particles are **not** inexpensive: final medians are 4.42-4.70 ms, with p95 up to
8.46 ms and 24 MiB scratch just for selection on this integrated GPU. Do not extrapolate AAA/finale
readiness from the bounded final count.
Measure authored hero/volley/F6 workloads after F7D2 connects live GPU presentation and applies a
global multi-output/instance cap. Then F7E can evaluate bounded async selected-set readback/pooling
and moving-light latency. Optimize or consider direct clustered integration only against those measured
resource/latency bottlenecks; no replay dependency or per-particle entity workaround is introduced.
