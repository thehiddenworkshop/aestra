# F8.1C4R — ordered stateful parameter reuse

Implemented; the bounded CPU encoding/allocation gate passes. Whole-host pacing,
universal single-tick registration and photographic smoke approval remain open.

## Change and correctness

Coupled emitters retain one parameter destination and a stateful bind group keyed
by the layout and all eleven other bound buffer identities. Each tick/presentation
appends its bytes to a distinct range in reusable upload pages and records a copy
to its destination immediately before the consuming compute work. Page writes are
queued before any referencing frame commands submit, after the previous frame has
submitted. **Queue writes never overwrite live tick destinations directly.** This
preserves historical placement, burst/cutoff inputs and intermediate trail observations.

Independent tick batches still use separate ephemeral parameters for each tick in
their shared compute pass; they are not redirected through the sequential destination.
Their final presentation can reuse its destination. No shader ABI, event order,
catch-up budget, checkpoint policy or authored source changes.

Packed physics data is uploaded only when its words change. State/free-list/event,
render/world/physics resource or layout changes rebuild the relevant group; parameter
size changes replace the destination. Emitter replacement and owner retirement drop
their caches. Upload pages retain the busy period's high water and release it on an
encoded frame without parameter uploads. This extends working-memory lifetime; it
is not a global hard memory budget or proof of driver allocation/free behavior.

The tradeoff is explicit GPU copies plus queue uploads. Payload traffic is not
eliminated, and the queue may allocate internal staging memory. The optimization
removes repeated buffer/bind-group API creation, not all allocations in the driver.

## Native measurements

Same Windows RTX 4070 SUPER/Vulkan, Rust 1.98.1, Bevy 0.20.0, wgpu 30.0.1 and dev
application opt-level 1/dependencies opt-level 3 as C4Q. Public 1280×720 HDR/Tony/bloom
0.15, Off MSAA, experimental OIT average4/K32/128 MiB guard. PlaybackOnly, full
37-second shows plus ten cleanup updates. `fireworks-audio` compiled, `--no-audio`.
No compilation or other native GPU probes ran during the measurement windows.

The baseline is the previously recorded C4Q final binary/report, preserved with its
hashes and source inventory. There is one profile per implementation and different
real-time observation counts; timing differences are observations, not an isolated
universal speedup ratio. New stateful preparation profiling exists only in the final.

| Instrumented wispy high OIT | C4Q baseline | Parameter reuse |
| --- | ---: | ---: |
| CPU simulation encoding p95 | 3.330 ms | **1.729 ms** |
| CPU simulation encoding maximum | 8.533 ms | 3.616 ms |
| Observed parameter buffer creations, including upload pages | 207,247 | 87 |
| Observed stateful bind-group creations | 207,247 | 82 |
| Observed physics buffer creations | 13,676 | 12 |
| Outer GPU simulation p95 | 7.107 ms | 6.895 ms |
| Maximum sampled encoding lag | 33.333 ms | 16.667 ms |

The playback window observes 82 destination/5 page allocations. Six initial
destinations/groups and one page are prepared while the initial clock is paused,
outside that window. Observed owned peaks are **88 destinations / 67,232 bytes /
88 groups**, and **six upload pages / 393,216 bytes**; every reported ownership
column returns to zero. These are handle/capacity observations, not total VRAM.
The new profile records 210,993 ordered parameter copies and 3,562 page writes,
each accounting for 161,198,652 bytes of API payload in the observed frames.
Do not interpret the buffer-creation reduction as equivalent transfer reduction.

Stateful render-world preparation p95 is **0.0132 ms**, maximum **0.4961 ms**.
Five observed allocation windows take 0.1923–0.4961 ms. This optional CPU system
scope excludes ShaderBuffer asset preparation, other render systems, submission
and GPU execution. Its latest-frame mailbox can skip windows; it is not a complete
activation inventory or proof that all render-thread activation is cheap.

| Separate final plain control | Host p95 | Host maximum | Max sampled encoding lag |
| --- | ---: | ---: | ---: |
| Wispy repetition 1 | 26.038 ms | 73.382 ms | 50.000 ms |
| Wispy repetition 2 | 25.127 ms | 51.560 ms | 16.667 ms |
| Persistent | 24.529 ms | 42.176 ms | 16.667 ms |

All four new runs finish with **13 accepted flashes, zero late pipelines, no
missing/blocked scheduled owners, zero clip scheduling error and empty final
observed populations**. The profile observes zero PlaybackOnly checkpoint capture.
One plain repetition still reaches three ticks of sampled lag and a 73 ms host
interval. Stable 60 FPS, universal single-tick latency and visible registration
are **not accepted**. Host intervals include vsync and asynchronous observation.
No screenshots or fragment-capacity scans were taken in these cost/profile lanes.

## Checks and next work

Renderer library: **182 passed, three ignored**, native GPU required. Host library:
**65 passed**; audio-enabled example: **29 passed**. Total **276 passed**. Strict
workspace Clippy, formatting and diff checks pass; full workspace tests were not rerun.

Native tests compare actual particle/trail bytes against fresh buffers, exercise
empty-event ticks and existing multi-tick/rewind/mixed cohorts, validate upload-page
rollover and consecutive submissions without an intervening CPU wait, verify group
reuse/resource invalidation and unchanged/changed physics data, and retire owners.

The profiling report adds copy/queue-write byte counters, parameter ownership and
`stateful_render_preparation`. Normal playback/plain controls leave these timers off.
[Machine-readable evidence](reused-stateful-parameters-2026-10-27.json) retains
current/baseline reports, source inventories and hashes. Authored `.ron` hashes match.

Next resume continuous star-path smoke and bright/quiet-phase public HDR art review
with separate paced cost controls. CPU encoding now has more headroom; whole-host/GPU
tail latency stays a separate gate. Do not hide it by raising catch-up budgets or
replace the default show before its art/performance acceptance.
