# F7E4B3A — repeated costs and public native cluster observability, 2026-10-05

Base commit `b152c2e7` (F7E4B2B). Three high-tier repetitions of hero, overlapping
volley and thirteen-clip show, each with GPU adapter-on and selection-only control:
**18 native processes / 9 run-level pairs** on RTX 4070 SUPER / Vulkan, Windows,
optimized development build. This is qualified cost/observability evidence, **not
total resident-memory, whole-game real-time, artistic or production-finale certification**.

## Matched method

Both modes retain the existing explicit light-output fixture (1500 lumens, 12 m
range, per-output cap 32), global selection cap 96, authored sources, seeds, compiled
capacities, transforms, normal materials, event chains and histories. Adapter caps
are 96 / 0; selection stays enabled in the control. Representative flashes remain
enabled only for the show, with the same independent cap 8 in both modes.

Audience perspective, HDR/Tony/exposure 0/bloom 0.15, 960×540 headless target,
default layers, fixed sequential 60 Hz simulation ticks and playback-only history
are unchanged. The benchmark waits for shader readiness without consuming the
show, then uses 120 warmup updates and 600 measured hero/volley updates or 1680
show updates. Runs are unpaced, sequential and use fresh processes; no concurrent
native GPU/build workload. On/off order alternates between repetitions and probes.
Executable and all 96 asset-input hashes stay unchanged throughout the matrix.

The read-only gate retains the existing resource checks and all thirteen clips'
admission/cleanup checks: **8,413 accepted children in every show run**, no source
overflow, expansion omission, destination rejection, history loss or replay copies.
Stable authored paths/source IDs/seeds/capacities and event totals must match in
each pair, without requiring transient ECS IDs or asynchronous sampling peaks to
match. A composition root's unobserved event links remain **null**, not fake zeros.
All 18 native exits must succeed and report/log hashes must match the manifest.

Timings are independent asynchronous diagnostic distributions. The outer render
graph includes rendering/preparation but is **not the whole app/game**; main-world
slot/source CPU costs remain separately available in raw reports. Phase percentiles
must not be summed. Selection remains present on both sides: subtracting run means
is a useful overhead observation, not a frame-paired delta, isolated causal timer
or significance claim. Timing sample counts/tails remain visible in the adjacent
JSON; not every host update produces a fresh diagnostic result.

## Native cluster observation and the failed grid attempt

The benchmark now observes public `Buffer::size()` bindings after render preparation:
global clustered-light storage once, per-view index list and offsets/counts. A bounded
CPU mailbox preserves extracted tick labels, monotonic render sequence and lost-result
counts. It adds **no GPU copy/map or selected-position readback**. Startup observations
are retained; size-stability gates apply to measured playback, and final in-flight
render frames may be absent (at most four are allowed by this gate).

Public main-world `Clusters.last_frame_total_cluster_index_count` additionally
exposes native **asynchronous index-demand acknowledgments** and actual grid
dimensions. These can repeat/lag and are not paired with buffer/timing frames.
The gate requires the same camera, native GPU clustering, actual target matching
preallocation, a stable grid and observed demand within physical index capacity.

The first complete matrix failed: the show's offsets/counts buffer oscillated
between **2304 / 117504 bytes**, despite a constant 2 MiB index list and no resize
warnings. Locked Bevy 0.19.1 sets an independent adaptive-grid index target of
**16384** (`ViewClusterBindings::MAX_INDICES`). The previous benchmark increased
GPU initial capacity to **524288** without aligning that target. Native
`assign_objects_to_clusters` shrinks XY using asynchronous demand, independently
of the preallocated GPU buffer. Both modes now use target 524288 as well as the
same initial storage. The fix is **benchmark-host policy**, not a dependency patch,
production adapter default, smaller authored workload or relaxed gate. CPU fallback
settings are untouched, covered by a regression test.

The aligned matrix removes observed grid/size oscillation. This corroborates the
source-backed explanation; the original attempt lacks demand telemetry, so do not
claim a frame-paired causal trace. Its manifest/reports/log hashes remain retained
as **unaccepted** in the adjacent JSON.

## Retained results

Ranges below span the three independent repetitions; times are milliseconds.

| Probe | Render GPU p95, on / control | On−control render GPU mean | On−control render CPU mean |
|---|---|---|---|
| Hero | 1.341–1.479 / 1.288–1.407 | +0.038–0.070 | −0.017–+0.081 |
| Volley | 1.505–1.630 / 1.453–1.479 | +0.070–0.200 | +0.031–0.096 |
| Show | 6.882–7.449 / 6.633–7.676 | +0.098–0.214 | −0.071–+0.063 |

Show native clustering GPU p95 is **0.312–0.331 ms on / 0.126–0.130 ms control**;
the run-mean difference is +0.040–0.045 ms. Injection GPU p95 is 0.019456 ms in
all three show runs. These do not establish a universal performance budget or
improvement; CPU differences and tail latency vary. Full distributions and phase
sample counts are retained, not cherry-picked into a single best run.

Every measured public per-view index list is **2,097,152 bytes**, offsets/counts
**117,504 bytes**, grid **17×9×24** (3672 clusters). Global light storage is
7680 bytes for enabled hero/volley, 7760–7920 for enabled show; controls retain
80 bytes for hero/volley and 80–240 for show (the native empty-light dummy and
independent flashes are real allocations, not zero memory).

Peak acknowledged native index demand is **925 / 701 / 352512** for enabled
hero/volley/show, below the 524288 capacity. No mailbox observations are lost and
no native resize/corruption/error/panic gate fails in accepted logs. Ordinary
headless/shutdown warnings remain visible; this is not a warning-free claim.

The show still acknowledges **346176** indices at the last main-world sample in
each repetition, while its independent adapter observation has zero writable
selected capacity and the existing particle/history cleanup gate passes. These
asynchronous values are not exact same-frame evidence. Main-world zero-lumen
placeholders retain max range, while injection is skipped once there is no prepared
selected frame: a **source-backed hypothesis for avoidable idle clustering work**,
not a measured causal proof. Investigate empty/source-loss/rejection transitions
before claiming clean native work retirement.

## Failures and remaining qualification

An intermediate aligned attempt completed nine successful entries, then the tenth
process wrote its report but exited **0xc0000409 / STATUS_STACK_BUFFER_OVERRUN**.
Its log contains closed readback-channel shutdown warnings, which do **not** establish
the crash cause. It stays unaccepted; none of its reports contribute the retained
cost rows. A completely fresh 18-process attempt succeeded. The runner now records
abnormal-exit report/log hashes in its failure manifest; no silent retry/substitution.

The fresh complete matrix initially hit a validator parser bug on null composition-root
event links. The corrected parser preserves null and adds a regression self-test.
Read-only revalidation passed all eighteen unchanged reports; the summary/accepted
manifest were finalized only after that pass, without replacing native measurements.

**Private Z-slice/scratchpad/metadata/staging buffers, old in-flight allocations,
allocation creation/churn, textures and allocator overhead remain unmeasured**.
The Z-slice buffer type is inside Bevy's private GPU module despite some public
fields; no private offsets/unsafe casts are used. Null means unavailable, not zero.
Stable public buffer sizes do not prove stable resident allocations or complete
overflow coverage; final asynchronous acknowledgments can still be in flight.
Resize-warning absence is a separate native-log gate, not a hard memory cap.

Next: **F7E4B3B**, remove/prove inactive-slot work retirement and qualify native
private/in-flight allocation/churn/overflow through an auditable diagnostic hook.
Medium/low repeated costs, other hardware/cadences, non-default per-view layers,
medium/low receiver images, natural-star art, lit smoke/editor lighting and finale
certification remain open. Preserve the portable async path for tolerant workloads.

## Reproduce

Run alone, after building. Use a fresh directory; existing failed attempts are never
overwritten. `-Tiers high,medium,low` expands the runner, but only **high** is retained
here. Existing workload/show validators still default to all three tiers.

```powershell
cargo build --locked -p aestra-viewer
./benchmarks/fireworks/run-particle-light-costs.ps1 -ReportsDirectory target/fireworks-f7/gpu-costs-aligned-2026-10-05-run2
./benchmarks/fireworks/validate-particle-light-costs.ps1 -ReportsDirectory target/fireworks-f7/gpu-costs-aligned-2026-10-05-run2
./benchmarks/fireworks/validate-particle-light-costs.ps1 -SelfTest
```

See [retained hashes and all nine cost pairs](particle-light-gpu-costs-2026-10-05.json).
Raw report/log hashes, ordered successful exits, executable/asset-manifest hashes,
source hashes and both failed attempts are recorded. Files under `target/` are local
evidence artifacts, not checked-in binaries or a promise that future runs reproduce timings.
