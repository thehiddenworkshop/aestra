# F8.1C4Q — reuse measured event-link scratch

Implemented and qualified for the bounded public fireworks shows. Whole-host
60 FPS, photographic smoke approval and default OIT adoption remain open.

## Finding and change

A separate instrumented high-tier wispy show located repeated allocation in the
production lockstep encoder. Its observed frames requested 56,359 event-list
buffers, 242,790 gather buffers and 161,860 spawn buffers: **461,009 requests**.
Gather/spawn CPU scopes included their buffer creation, bind groups and dispatch
encoding. Their frame p95 costs were 1.286/1.438 ms respectively.

Each effect now retains one scratch slot per resolved event link, owned by its
first persistent emitter. A slot holds the emission list, three gather buffers
(parameters, order, indirect dispatch), and two spawn buffers (parameters, plan).
Only the list's count is cleared; gather and spawn regenerate their live headers
before consumption. The list no longer receives a CPU zero-filled initialization
payload. Per-tick stateful parameters and current-resource bind groups stay fresh.
No queue writes replace historical tick inputs or alter command ordering.

Gather trigger/count/capacity/counter changes invalidate the affected slot.
Spawn inheritance, acceptance and birth-output parameter changes rebuild its spawn
buffers. Buffers bind current particle states/events/counters at every dispatch;
there is no cached group retaining an old source or target. Removed links are
truncated even when removal switches to independent scheduling. Owner removal,
GPU extraction retirement and simulation-state replacement release cache ownership.
Standalone public gather/spawn signatures and their ephemeral behavior remain.

The tradeoff is retained working memory between frames/pauses. It is bounded by
the owner's link descriptors and existing list capacities, not a global GPU memory
budget. There is no cross-owner cache, forced destruction of in-flight buffers,
particle readback, replay checkpoint or catch-up policy change.

## Measured comparison

Windows, RTX 4070 SUPER/Vulkan, Rust 1.98.1, Bevy 0.20.0, wgpu 30.0.1.
Development application opt-level 1/dependencies opt-level 3. Actual public
1280×720 HDR/Tony/bloom 0.15 show, `fireworks-audio` compiled but `--no-audio`.
Experimental OIT retains Off MSAA, average 4, K32 and its 128 MiB guard.
PlaybackOnly, 37 authored seconds and ten cleanup updates. No compilation or other
native GPU probes ran during these measurement windows.

| Instrumented wispy high OIT | Fresh baseline | Reused scratch |
| --- | ---: | ---: |
| Observed event-link buffer creation requests | 461,009 | 450 |
| Observed event-link create-with-data payload bytes | 209,443,008 | 3,000 |
| CPU simulation encoding p95 | 5.612 ms | 3.330 ms |
| CPU simulation encoding maximum | 9.666 ms | 8.533 ms |
| Outer GPU simulation p95 | 8.726 ms | 7.107 ms |
| Host interval p95, including vsync | 33.064 ms | 26.656 ms |
| Maximum sampled encoding lag | 66.669 ms | 33.333 ms |

The reused run observes 75 link preparations: 75 list buffers, 225 gather buffers
and 150 spawn buffers. Steady gather/spawn scopes request zero buffers. Observed
owned cache peaks at 75 links / 450 buffers / **589,044 bytes**, then returns to
zero. This establishes cache ownership retirement, not driver frees or total VRAM.

The buffer-request reduction exceeds 99.9% in this observed workload. Counts and
payload sizes are CPU API accounting, not allocator traces or measured PCIe traffic.
There is one instrumented run per implementation; actual frame/tick distribution
and asynchronous observations differ. GPU and host timing changes are observations,
not a universal speedup ratio. Do not sum category or nested GPU percentiles.

| Separate final plain control | Host p95 | Host maximum | Max sampled encoding lag |
| --- | ---: | ---: | ---: |
| Wispy repetition 1 | 26.894 ms | 60.634 ms | 16.667 ms |
| Wispy repetition 2 | 26.674 ms | 52.811 ms | 16.667 ms |
| Persistent | 24.874 ms | 52.285 ms | 16.667 ms |

All five runs finish with **13 accepted flashes, zero late pipelines, zero missing
or blocked scheduled owners, zero clip schedule error, and zero final observed
particle population**. Final plain controls observe at most one fixed tick of lag
within f32 tolerance. These are sampled encoding results, not universal latency
bounds or visible-pixel registration proof. Their p95/tails still fail stable 60 FPS.
The instrumented final run observes zero PlaybackOnly checkpoint capture bytes.
No authored `.ron` source changes between baseline and final inventories.

## Diagnostics and checks

`AESTRA_EXAMPLE_PROFILE=1` now adds fixed-category, simulation-thread CPU accounting
to the existing latest-frame ledger. It covers list/gather/spawn creation and
encoding, tick/presentation parameter creation, stateful bind-group creation, physics
uploads and trail-observation uploads. It excludes parameter-byte serialization,
initial render-preparation-system allocations, other uninstrumented helpers and
driver behavior. The bounded mailbox may skip frames. Timers and accounting remain
off in ordinary interactive/plain cost runs; no new GPU timestamps or readbacks.

Renderer library: **180 passed, three ignored**, with native GPU conformance required.
Standalone domain-spawn/stateful conformance: **4 + 31 passed**, native GPU required.
Host library/audio-enabled example: **65 + 29 passed**. Total **309 passed**.
Strict workspace Clippy, formatting and diff checks pass; full workspace tests
were not rerun. The production-loop regression compares GPU particle/trail bytes
against fresh allocation after real source deaths and empty-event ticks, checks
stable identity and no repeated preparation, capacity invalidation, paused frames,
independent-scheduling transition and owner retirement. Existing rewind, chained
birth, host-output and full-cohort trail tests also pass.

[Machine-readable evidence](reused-event-scratch-2026-10-27.json) retains all five
reports, binary/report/log hashes and before/after source inventories.

Next measure remaining per-tick/presentation parameter and bind-group churn plus
render-world activation outside the simulation scope. Keep bounded live catch-up
and flash delivery; continuous star-path smoke and paced HDR art remain unfinished.
