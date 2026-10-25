# F8.1C4I — mixed-pass ordering blocks compositor adoption

The new diagnostic reproduces a real mixed-rendering limitation: **foreground
additive particles, trails and mesh particles can appear behind OIT smoke**.
The test passes because it detects this known failure and verifies positive controls;
**the renderer does not pass mixed-scene acceptance**. No production render behavior
is changed. The public show remains opted out of the experimental compositor.

## Cause, not just symptoms

`bevy/aestra-bevy-render/src/gpu/render.rs::queue_gpu_sprites_3d` currently collects
only eligible alpha sprite/flipbook draws (`renderer_kind <= 1`, alpha blend,
depth-tested, no depth write, rendered mode). Additive draws, alpha trails and mesh
particles stay in the normal transparent pass. Bevy 0.20 schedules `oit_resolve`
**after** `main_transparent_pass_3d` in `bevy_core_pipeline/src/oit/mod.rs`.

The resolve consequently places collected smoke over the already-rendered mixed
source, even when that source is physically in front. Neither additional fragment
storage nor improved whole-draw pool sorting changes that pass order. The fixture
needs only one collected particle; this is not global loss or local-sort overflow.
`ExperimentalOitStatus` approves the camera and every measured diagnostic stays
healthy while the foreground pixels are wrong. Public component documentation now
states this known limitation explicitly; its status/diagnostic APIs are not changed.

## Native oracle and controls

A red semantic alpha sprite at world Z=0 overlaps a green source of one of four kinds:

- Legacy additive sprite.
- Legacy additive trail.
- Legacy alpha trail.
- Semantic alpha mesh particle using an in-memory XY rectangle.

All use ordinary compiled GPU players with `PlaybackOnly`. Green is placed at world
Z=−3 before any simulation, and pipelines are warmed with both clocks paused at frame
zero. Players then advance forward to frame 30 and pause. Each trail is a real time-sampled history (64-point capacity, one
parent, 1/60-second samples, two-second retention), spanning Y approximately −2..2
at .5 seconds at a single world Z. No trail geometry is synthesized by the viewer.
The plane is supplied through the public instance-owned mesh override; no asset
files are replaced. There are no point-light outputs or CPU light-position reads.

Each path runs both `Fast` and `DepthBackToFront`, both spawn orders, and cameras at
Z=+16 and −16. Each camera visits its initial side, the opposite side, then the initial
side again without advancing playback. Green stays at Z=−3 throughout, so its complete
trail history has a known planar depth. Every combination tests foreground/background
ordering and camera restoration; it does not assume rigid translation of world-space history.
The camera uses MSAA off and no tonemapping/bloom: these are bounded linear-color
ordering controls, not a photographic HDR/show performance test.

For every position:

1. Capture the OIT mixed result and require healthy allocation/aggregate diagnostics.
2. Hide green and capture red alone; its central red channel must be .5 within .01.
3. Hide red and capture green alone. Green must be visible (.15–.6 center contribution),
   with no other color channels. Its newly completed diagnostic must show **zero**
   attempted fragments, proving that the source did not join the list. Native trail
   age/coverage fading is calibrated from this image rather than assumed to be .5.
4. Disable OIT and require the isolated green image to remain within one byte. Then
   show red and use ordinary `DepthBackToFront` as the positive control. Both sources
   occupy distinct planar depths, so whole-draw sorting is sufficient here—unlike
   the earlier interleaved-pool scene.
5. Restore OIT and the original pool-sort mode; require byte-level restoration and
   healthy counters. Returning the camera to its initial side must also restore the image.

The independent central 8×8 oracle decodes screenshot RGB to linear. With isolated
red contribution `r` and unit-green contribution `g`, expected RGB is:

| Green position/operator | Expected linear RGB |
| --- | --- |
| Behind red, alpha or additive | `(r, g * (1-r), 0)` |
| In front, additive | `(r, g, 0)` |
| In front, alpha | `(r * (1-g), g, 0)` |

The normal sorted reference must match the correct operator/order at every patch
pixel within .01. The OIT result must match the **incorrect forced-background model**
within .01, and foreground error must exceed .07; background cases must match the
correct order. Whole-image OIT/reference agreement is also required for background
cases. Foreground whole-image error is recorded rather than accepted.

These thresholds cannot be satisfied by an invisible mixed source, a missing red
particle, stale transforms, toggling only image visibility, or a normal-reference
ordering error. Passing the expected-failure assertion means an adoption blocker
was reproduced. It must be replaced with a positive gate when the integration changes.

## Repeated results

Windows / NVIDIA RTX 4070 SUPER / Vulkan, Rust 1.98.1, Bevy 0.20.0, 480×360 sRGB:

| Measurement | Both final repetitions |
| --- | ---: |
| Paths / cases per path | 4 / 24 |
| Total cases | 96 per run |
| Foreground cases failing correct compositing | 48 per run |
| Foreground minimum linear patch error | .220131 |
| Correct background maximum linear patch error | .003127 |
| Normal sorted positive-control maximum linear error | .005954 |
| OIT forced-background-model maximum linear error | .003127 |
| OIT/camera restoration maximum byte error | 0 |
| Drops / over-sort pixels / visited invalid links | 0 / 0 / 0 |
| Corresponding PNGs matching | 480 / 480 |

Final roots are `target/fireworks-f8/fragment-list-mixed-2026-10-25-f` and `-g`.
The first also passes all six prior compositor probes. Prior accepted healthy,
depth, HDR, lighting and texture images remain unchanged (564 PNGs); deliberate
saturation images are excluded from repeatability acceptance because their loss
can depend on fragment arrival order.

Initial `-a` through `-d` iterations are **not accepted evidence**: they moved the
owner after recording world-space trail samples, then assumed the whole history had
that new depth. The `-b`/`-c` pair passed central checks but 60/480 images differed,
by up to 187 bytes/channel across the trail. Full-image comparison exposed this bad
fixture assumption. Warming pipelines alone (`-d`) did not repair that contract.
The final fixture fixes source placement before recording and changes the camera
instead. This is a corrected qualification fixture, not a runtime trail-motion or
cold-start fix; no tolerance is relaxed to hide the rejected iterations.
See [reports, hashes and validation](fragment-list-mixed-2026-10-25.json).

Ordinary viewer tests pass (106 passed, 37 intentionally ignored), as do strict
workspace/all-target Clippy with `aestra-bench/gpu`, formatting and diff checks.
Existing incremental-cache hard-link fallback-to-copy warnings are not source lint
failures. No full workspace test or Linux run is claimed.

## Next decision / remaining scope

**F8.1C4J must address the mixed-compositor boundary before adoption or more smoke art.**
Prototype unified, depth-ordered collection of eligible Aestra alpha/additive sprites,
trails and mesh particles, or select another bounded compositor. Additive emission
must not extinguish the layers behind it; treating it as ordinary alpha is not a fix.
Recheck packed color/alpha, exact-sort/global saturation semantics and diagnostic
budgets with all participating geometry. Preserve explicit restrictions for multiply,
depth-writing/overlay/wireframe/custom draws and qualify host-material interoperability
instead of claiming that all transparent objects are covered automatically.

Turn the foreground cases into positive analytic/reference checks, retain toggling,
opposite-camera-depth and isolated-source controls, and rerun existing OIT regressions. Merely
removing the opt-in is not a general solution: the earlier whole-pool interleaving
limitation still exists, even though ordinary sorting is valid for this simple probe.

This slice adds viewer tests, API warning documentation and evidence only. It does
not change schemas, shaders, ABI, dependencies, assets, runtime history or defaults.
It does not qualify arbitrary Bevy/custom host transparent materials, ribbon/multiply
operators, opaque depth variants, near-plane intersections, HDR/bloom mixed scenes,
resize/multiple cameras, Linux, dense/live-paced costs or realistic smoke art.

To reproduce, use a fresh absolute root and run the native probe alone:

```powershell
$env:AESTRA_CROSS_POOL_IMAGES='C:/absolute/fresh/mixed-probe'
cargo test -p aestra-viewer fragment_list_mixed_draws --locked --offline -- --ignored --test-threads=1
```

Filter `fragment_list_` runs the full seven-probe regression sequentially. The mixed
native test is intentionally ignored in ordinary CI; its fixture/operator unit test
runs normally. Do not interpret a green test suite as mixed-scene visual acceptance.
