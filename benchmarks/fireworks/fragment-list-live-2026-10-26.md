# F8.1C4L — paced compositor costs and capacity

Follow-up: [C4M2 ordering and repeated costs](fragment-list-order-2026-10-26.md).
This dated report retains the earlier resolver and local-overflow interpretation.

This is a measured adoption gate, not a public-show migration or realistic smoke-art
approval. The benchmark compares ordinary sorted rendering with the existing bounded
fragment-list prototype using identical compiled inputs, camera, seed and forward
PlaybackOnly windows. Production renderer defaults and saved assets are unchanged.

## Method

The 24 configurations comprise two saved workloads, two resolutions, three rendering
modes and two repetitions. The second repetition reverses mode order:

- High-tier saved persistent-smoke show, audience camera, unchanged representative
  lights and selected-particle lights disabled as in its prior matched show benchmark.
- Four deliberately coincident copies of the saved shell-born smoke cohort effect,
  shared seed and root scale .1. This is synthetic overlap stress, not choreography.
- 960x540 and 1920x1080 full offscreen sRGB targets, MSAA off, no HDR/bloom.
- Ordinary DepthBackToFront; default OIT (average four, local sort eight, 32 MiB);
  explicitly provisioned OIT (average four, local sort 32, 128 MiB).

Shader readiness and forward initialization to frame 360 occur outside measurement.
The players then run frames 360..600, one player frame per update, with 240 host updates
paced at 60 Hz by absolute deadlines. Native simulation ticks are recorded separately
(up to two observed ticks per window in this run). No seek, checkpoint replay, time compression or
frame skip is used. Deadlines, wall intervals and pending-pipeline updates are recorded,
not hidden. This is not a window/vsync/presentation or complete game/audio measurement.

Every admitted OIT configuration has two independently initialized lanes:

1. **Cost:** OIT aggregate scans disabled, standard GPU timestamp instrumentation
   enabled. Record whole-render-graph, transparent-pass, OIT-resolve and per-source
   simulation distributions; app.update time is separate from GPU completion time.
2. **Capacity:** identical independently initialized/paced playback with bounded OIT
   aggregate scans enabled. These scans add GPU work, so their timing is deliberately
   excluded from the production-cost comparison. Require at least 120 newly completed
   observations and zero invalid links, but report real drops/over-sort as degradation.

Async timestamp/count messages are deduplicated by their own sequence/time, not joined
to frame IDs. Warmup/final results may straddle measurement boundaries. Population
peaks are asynchronous observations. Do not sum pass/source percentiles or treat a
difference of separate-run medians as a frame-aligned isolated-pass measurement.
Playback-only checkpoint bytes are reported as **observed** bytes with an explicit
unknown-window count; unknown metadata is not silently certified as zero work.

Allocation rejection is a distinct result. Its ordinary-rendering fallback is never
timed or described as successful OIT. At average four the full target requires
52 bytes per pixel: 26,956,800 bytes at 960x540 and 107,827,200 at 1920x1080. The default
32 MiB guard therefore rejects 1080p. Provisioning is explicit and does not remove
the local exact-sort bound or prove capacity is adequate.

## Result and adoption decision

The machine-readable [reports and provenance](fragment-list-live-2026-10-26.json)
contain separate timing/capacity lanes, source/report hashes, matched configuration
summaries and per-repetition results. Raw outputs remain under the recorded fresh
target directory. Benchmark completion means valid observations were collected;
it does **not** mean the renderer passes dense compositing acceptance.

The native matrix completes in 224.32 seconds: 20 admitted cost lanes, 12 independent
capacity lanes and four explicit default-budget rejections. All cost windows have zero
pending-pipeline updates, all root players end at frame 600, and observed population
peaks match between lanes (617 in the show window, 256 in dense cohorts). No checkpoint
capture bytes or unknown checkpoint windows are observed. These are matched workloads,
not empty-carrier or reduced-population controls.

Whole-render-graph GPU times below are ranges of **separate run medians/p95s across
two repetitions**, in milliseconds—not confidence intervals or summed pass timings:

| Workload / resolution | Sorted median / p95 | Provisioned OIT median / p95 | OIT node/head MiB |
| --- | --- | --- | ---: |
| Show, 960x540 | 3.47–3.56 / 8.05–8.12 | 3.71–3.90 / 9.16–9.36 | 25.71 |
| Show, 1920x1080 | 3.47–3.57 / 8.11–9.80 | 4.07–4.18 / 9.04–9.21 | 102.83 |
| Four cohorts, 960x540 | 2.48–2.50 / 4.30–4.38 | 3.47–3.49 / 5.86–5.89 | 25.71 |
| Four cohorts, 1920x1080 | 2.61–2.68 / 4.51–5.05 | 5.39–5.42 / 7.31–7.45 | 102.83 |

Default eight-slot OIT at 960x540 is cheaper (show median 3.59–3.66 ms, dense median
2.74–2.75 ms), but also exceeds its exact-sort capacity. At 1080p it is rejected,
so there is no purported default-OIT timing there. Main-transparent and resolve
distributions are recorded separately; the transparent pass is not an isolated list
insertion measurement. Provisioned resolve p95 is .60–.75 ms for the show and
1.97–2.03 ms for dense cohorts. Small show p95 differences vary between repetitions
and are not evidence of an OIT speedup.

The 12 capacity lanes collect **2,828 observations**: zero global drops, zero invalid
links, and over-sort pixels in **all 2,828**. At 32 local slots, peak over-sort pixels
are 4,567/18,246 for the show and 4,580/18,359 for dense cohorts at 540p/1080p; peaks
repeat identically. At eight slots, the 540p peaks are 8,140 and 5,082. Global node
capacity is not the limiting resource in these observations. This is not an exact
frame-aligned maximum-overdraw measurement: traversal deliberately caps at the local
threshold plus one, and arrivals are asynchronous.

Absolute deadline pacing retains all 240 updates. The show cost lanes miss 8–15
deadlines per run, while dense lanes miss none; app.update and start-interval
distributions are included. Four-second elapsed windows do not imply guaranteed
60 FPS or isolate the cause of those missed deadlines. No production-cost timing
is taken from the instrumented capacity lanes.

The current adoption blocker is local-sort capacity, not global storage admission.
More node storage is not a fix for prematurely blended overflow or incorrect handling
of already premultiplied emission. Keep the public show opted out of OIT. Ordinary
sorting is only a cost baseline here, not a correctness reference for interleaved
transparent effects.

## Reproduction and scope

Run alone, with no other intentional GPU workloads, using a fresh absolute directory:

```powershell
$env:AESTRA_OIT_LIVE_REPORTS = 'C:/absolute/fresh-oit-live-root'
cargo test -p aestra-viewer oit_live_paced_matched_cost_and_capacity --locked --offline -- --ignored --test-threads=1
```

The benchmark is deliberately outside the `fragment_list_` image-regression filter.
Its unit test checks the 24-case schedule, counterbalanced order and default/provisioned
memory distinction. No external benchmark package or runtime dependency is added.
Viewer tests pass (108, with 41 explicitly ignored), as do strict workspace/all-target
Clippy, formatting and diff checks. The benchmark implementation/results are separate
from the preceding compositor/depth/CRLF commit `70176c4c`.

This is a four-second high-tier workload window on one native GPU, not the entire
37-second show, a dense finale, other tiers/devices, HDR/bloom, multi-camera cost,
image-quality or photographic smoke approval. Timestamp scopes exclude game/audio,
host preparation and presentation. Desktop/driver/overlay activity is not controlled
by the test. Standard Vulkan overlay warnings and repeated-headless-app logging
warnings, if emitted, are environment/harness notices rather than shader validation
failures; the measured pipeline-readiness observations are recorded separately.

Next establish a dense overflow image-error oracle and correct the compositor's
overflow representation/order handling, or select another bounded compositor.
Do not silently raise the local limit and declare exact blending. Repeat the matched
cost/capacity gate after that decision, then resume continuous wind-dispersed smoke
and public HDR art review.
