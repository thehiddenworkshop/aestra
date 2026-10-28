# Partitioned capture/resolve feasibility — 2026-10-28

The isolated native prototype proves that disjoint screen regions can reuse one
fragment-node pool without changing the final HDR image. The measured tradeoff is
substantial: sixteen regions use **82.73% fewer node/head bytes than a tightly
sized full-screen control**, but take approximately **5.5–5.7× its GPU time** in
this small fixture. Four regions roughly halve those bytes at about twice the
time. This is a memory-policy candidate, not a blanket optimization or runtime
adoption. Normal fireworks playback and public renderer settings are unchanged.

## Scope and implementation

The harness is `cfg(test)` only, under the renderer's experimental OIT module.
It submits 48 flat, partially overlapping alpha/additive layers to a **257×193
Rgba16Float** target on RTX 4070 SUPER/Vulkan. An opaque-depth patch, black
absorbers, emission with zero extinction, partial quads and image-edge coverage
exercise compositing and seams. Peak reference RGB is 5.49, so HDR is actually
present. The independent nine-patch linear-color oracle has maximum error
0.003409 against the 0.01 budget.

Capture keeps the full projection and pixel addressing. Only the raster scissor
changes between regions. Each region captures, sorts and resolves before the next
reuses the pool. Heads retain full-target addressing; the node pool is reused.
Dispatches cover only the current region, including non-multiple-of-eight edges.
No particle count, opacity, resolution or shader-quality reduction is involved.

The harness includes the **production sort/resolve algorithm bodies**, substituting
fixture view types/bindings, a fixed local threshold of eight, and the no-prepass
depth path. The sort entry gains a region origin/extent guard. Test-only codecs
follow Bevy 0.20 RGB9E5/alpha8 packing. This does **not** qualify Bevy's full view
bindings, phase scheduling or a new production shader mode. CPU coverage tests
verify odd targets are partitioned exactly once and reject invalid extents,
overflowing pixel addresses and declared unsupported-view/foreign-draw inputs.
That guard is private fixture admission, not automatic auditing of host draws.

## Native correctness and controls

Three final native executions pass all five configurations. Every configuration
attempts exactly **1,228,172 fragments** across its regions. All healthy images
match the adequately provisioned full-list reference **byte for byte across all
49,601 pixels and four half-float channels**, including reversed submission order,
repeated frames and the timed iterations.

The deliberately undersized full-screen control drops **1,029,768 fragments** and
fails image equivalence in every run. The sixteen-region case uses that same
physical node capacity and produces the complete image with zero loss. Faster
execution of the lossy control is rejected as a produced-work performance result.

Healthy pools are then deliberately limited to eight nodes, observed overflowing,
restored in place, and checked against the reference again. Empty frames have
zero attempts and exactly the uniform background across the entire target. Every
frame checks that heads and the allocation counter are zero after resolve.
Node/head sizes stay constant through reuse. Buffers are explicitly destroyed
after completed submissions; a newly created pool produces the reference image.
These are ownership/reset checks, not a driver VRAM/free-history trace.

## Storage and GPU costs

Each final run has five warmup frames and **30 measured frames per lane and
configuration**. Table timing ranges are the three independently computed p95s;
they are not pooled percentiles or sums of phase percentiles.

| Configuration | Node/head bytes | Draws / submitted vertices | Drops | Exact HDR match | Aggregate GPU p95 range |
|---|---:|---:|---:|---|---:|
| Full reference, conservative 48-node/pixel capacity | 28,768,580 | 48 / 288 | 0 | Yes | 0.283–0.313 ms |
| Full screen, capacity fitted to this fixture's observed demand | 14,936,468 | 48 / 288 | 0 | Yes | 0.271–0.307 ms |
| Full screen, small-pool negative control | 2,579,252 | 48 / 288 | 1,029,768 | **No** | 0.048–0.053 ms; rejected |
| Four regions, worst-region fixture capacity | 7,405,892 | 192 / 1,152 | 0 | Yes | 0.544–0.607 ms |
| Sixteen regions, same small pool as negative control | 2,579,252 | 768 / 4,608 | 0 | Yes | 1.535–1.696 ms |

The tight full-screen control avoids overstating savings against the conservative
reference. It is fitted to this immutable fixture's measured demand, not a safe
general host capacity planner. Likewise, the four-region allocation covers this
fixture's known 48-layer worst-region bound; no automatic planning policy is added.
These bytes include nodes and full-target heads, not all GPU memory. The prototype
also owns textures, uniforms, bind groups, queries and bounded test readbacks.

Detailed profiling writes six timestamps and copies the attempt count per region.
Its capture/sort/resolve samples remain in the ledger for attribution. A separate
**aggregate-only lane writes exactly two timestamps per frame for every mode and
omits per-region counter copies**. The table uses that lane. It still checks exact
images and final resets, but missing attempt samples are not a zero-loss verdict;
capacity is separately qualified on the identical immutable workload.

The aggregate span starts at first capture and ends after the last resolve,
including inter-region counter clears and pass transitions. Background/depth
fixture setup, post-pass readback, mapping/fences, CPU work and pipeline/resource
creation are excluded. Per-phase percentiles are not added together. Readbacks
and completion waits occur between frames: this is a blocking test harness,
not a live game frame-time measurement. More submitted geometry and pass work
remains expensive even after reducing instrumentation overhead. Small-region
occupancy/dispatch effects are included; no single dominant bottleneck is claimed.

## Visual witnesses and reproduction

Reference fixture:

![Reference](reviews/partition-probe-2026-10-28/reference.png)

Same small node pool without partitioning, deliberately rejected:

![Lossy full-screen control](reviews/partition-probe-2026-10-28/full-small-negative.png)

The sixteen-region [witness](reviews/partition-probe-2026-10-28/sixteen-regions.png)
matches the reference. Raw `.rgba16` bytes establish exactness; PNGs use identical
Reinhard display mapping followed by linear-to-sRGB conversion. These are oracle
layers, not photographic fireworks artwork.

```powershell
cargo test --locked -p aestra-bevy-render --lib
$env:AESTRA_PARTITION_PROBE = "$PWD/target/fireworks-f8/partition-probe-fresh"
cargo test --locked -p aestra-bevy-render partitioned_capture_reuses_nodes_and_matches_full_list_without_seams -- --ignored --test-threads=1
```

Use a fresh absolute output directory and run the native probe alone. It requires
a native GPU with timestamp-query support and fails instead of silently skipping
when unavailable. The regular CPU test validates shader substitutions with Naga.
The renderer suite passes **185 tests** with four explicit native tests ignored;
the new native test is separately executed three times. Strict all-target renderer
Clippy, workspace formatting and whitespace checks pass. Existing environment
hard-link/cache-copy warnings remain.

The [JSON ledger](partition-probe-2026-10-28.json) records all three final reports,
source/binary/raw-image hashes, detailed and aggregate timing lanes, and earlier
detailed-only pilots. Pilots remain recorded separately; their profiler-heavy
timings are not substituted for the final aggregate lane.

**Next:** integrate a narrow, explicitly enabled partition transport for one
approved full-target host view, with real phase ordering, readiness, per-region
overflow reporting and unsupported-view/foreign-draw rejection. Start with fewer
regions and compare actual high-tier near/orbit workloads under the unchanged
memory ceiling, including real material, trail geometry and repeated draw costs.
This synthetic result cannot establish the best host partition size, generic
camera/layer support, total VRAM bounds or production-finale performance. Keep
ordinary rendering as the default and the previous high-tier failures visible.
