# F7E4B2A — authored GPU-lit receiver images, 2026-10-04

Base commit `75904810` (F7E4B1). Native RTX 4070 SUPER/Vulkan, optimized development
build. This closes the **high-tier authored receiver-image slice**, not all F7E4B2.
It is not a cost, paced-registration, artistic or production-finale certification.

## Method and gate

The ignored viewer test uses the normal prepared hero, overlapping secondary volley
and thirteen-clip show, with F7D2B's explicit particle-light fixture (1500 lumens,
12 m range, high-tier per-output cap 32). Materials, events, histories, transforms
and compiled choreography are unchanged. An independent representative-light pool
stays installed in both controls. This image experiment does not re-certify cue
delivery or the show admission totals; F7E4B1 retains those separate live gates.

One native app retains pipelined rendering and advances **forward** at requested
60 Hz simulation ticks in playback-only mode. At each sample it pauses, waits for
pipeline readiness, and captures **off / on / off-restored**. Only the GPU adapter
cap changes (0 / 96 / 0); global GPU selection stays enabled at 96. Each triplet
uses the same simulation time and scene, audience perspective, 960×540 HDR target,
Tony tonemapping, exposure 0 and disabled deband dither. Repeat with bloom 0 and
0.15. Screenshot readback is test instrumentation, never selected-position readback.

Test-only receiver geometry is a nonemissive rough diffuse wall, sRGB gray 0.6,
metallic 0, size 160×100×1, center (0,35,-2). No emitter brightness/range is increased
to pass. A preliminary wall at z=-12 produced no above-floor response: this fixture's
short-range lights require appropriately placed receivers. The wall can occlude
particles behind it; these are validation images, not unobstructed hero art references.

The ROI is the conservatively inset projected wall, not a hand-picked hot pixel.
At least one active sample per workload must have **100 receiver pixels and 1000
summed positive channel levels**, each pixel exceeding a fixed 3-level/channel
noise floor, both with and without bloom. With bloom disabled, additive glow cannot
produce the lit-minus-unlit wall response. Matched controls cancel the unchanged VFX.

All off-restored controls must have **zero pixels exceeding that same floor across
the entire image**, not merely the receiver ROI. Raw changed-pixel counts and maximum
restoration error remain reported. Native additive ordering can produce single-level
rounding differences; it is not labeled bit-exact when present. Before birth and at
the final cleanup sample, off/on images must match **exactly**. The image gate also
asserts native GPU presentation, both cap bounds, no invalid sources/rejections, no
portable proxies or selected-record transport, and zero slots/bytes after disabling.
Hero/volley population checks prevent replacing the authored workload with a token
particle; the busy show sample must retain multiple active source instances.

Requested samples:

- Hero: 0, 85, 110, 150, 300, 600.
- Volley: 0, 85, 110, 145, 175, 240, 600.
- Show: 0, 180, 600, 840, 1110, 1560, 1920.

Reports distinguish requested frames from actual simulation frames. `Once` playback
clamps at authored duration (hero 480, volley 540, show 1560); later requested cleanup
samples do not claim extra simulation ticks. Project-instance population observations
exclude the empty composition root and propagate unavailable measurements as null.
They remain asynchronous observations, not frame-aligned GPU population certificates.
Written-capacity bounds are **not** exact active light counts.

## Native evidence

Forty triplets / 120 final PNGs pass. Retained artifacts are in
`target/fireworks-f7/gpu-authored-images-2026-10-04/`; the adjacent JSON records the
report hash and aggregate PNG-manifest hash plus compact measurements. The read-only test reloads every
PNG, checks dimensions/metadata/basenames, recomputes all image differences and
response/restoration gates, and compares them with the report. It does not simply
trust `accepted: true`. A native rerun invalidates earlier acceptance before starting,
so a failed rerun cannot leave a stale passing report.

Representative responses (positive receiver pixels, bloom off / on): hero frame 85
**1978 / 1726**, volley frame 175 **402 / 373**, busy show frame 1110 **763 / 561**.
These observations are lighting evidence, not quality scores. No selected-record
readback, portable proxy allocation or adapter rejection; both endpoint image
controls pass exactly. Native cluster lists request F7E4B1's high-tier initial
capacities (4096 slices / 524288 indices), not an actual allocation/memory-budget
measurement. The image test does not collect or certify native cluster growth logs.

## Reproduce

Run the native test **alone**; ordinary `cargo test` keeps it ignored. Paths in the
environment override are repository-relative (or absolute), not relative to Cargo's
crate test cwd. No GUI window is opened.

```powershell
$env:AESTRA_GPU_LIGHT_IMAGE_REPORTS = 'target/fireworks-f7/gpu-authored-images-2026-10-04'
cargo test --locked -p aestra-viewer authored_gpu_lights_illuminate_receivers_and_restore_controls -- --ignored
cargo test --locked -p aestra-viewer retained_authored_receiver_images_pass_the_gate -- --ignored
```

## Next

**F7E4B2B:** paced moving-star registration under authored overlapping load. Preserve
the distinction between an analytic calibration tracer and natural authored-star
visual acceptance; paused triplets cannot establish spatial lag during live playback.
Then repeat work-matched cost runs and qualify actual native cluster allocations,
growth and overflow. Medium/low image gates, additional hardware/cadences, non-default
per-view layers/multi-view, editor lighting, lit smoke and production-finale approval
remain open. Portable async mode remains available for latency-tolerant workloads.
