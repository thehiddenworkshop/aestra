# F8.1C4G — HDR radiance and saved lit-smoke material

The experimental compositor passes bounded HDR radiance and lit semantic receiver
checks. No production renderer fix was needed. The public show remains opted out;
this is **not photographic smoke art, mixed-transparent or production acceptance**.

## HDR compositing before the display transform

The existing depth-interleaved red/blue fixture now also runs with authored red
radiance 8 and blue radiance 4, each alpha .5. The three-particle scene uses ordinary
compiled GPU players, forward PlaybackOnly, both camera directions/spawn orders and
no per-pool particle sorting. Two red particles straddle blue in another player.

The public `PhotographicPreview` camera profile explicitly enables HDR, fixed Reinhard
mapping, disabled dither and zero bloom. Relative display exposure cycles through
−2, 0, +2 and back to 0 stops. Screenshots remain 480×360 sRGB **display-referred SDR**;
they are not raw floating-point HDR captures. The independent central 8×8 oracle first
composites known linear radiance, then computes `c * 2^stops / (1 + c * 2^stops)`.
For the +Z camera the expected pre-display composite is `(5,0,1)`, not an SDR-clamped
color. The observed response across exposures matches that HDR prediction.

Deliberately wrong controls clamp each source to SDR before compositing, or tone-map
each layer before blending it. Both must fail. Restoring exposure zero must restore
the whole image within one byte. This tests the actual native collection/resolve,
grading and display path; an ordinary unit test separately checks the reference math.
It qualifies radiance 8/4 and these exposures, not every possible HDR magnitude/map.

## Saved material and an independent draw-order control

The second probe compiles the existing saved
`assets/test/materials/fireworks_lit_smoke.aestra.material.ron` unchanged. Its authored
albedo, radial coverage, ambient term and `ScenePointIrradiance` scattering remain in
use. The fixture freezes only emitter behavior to isolate three overlapping receiver
particles; it is not a saved-show/choreography or wispy-art test.

A gray background with linear RGB .2 makes absorption measurable: unlit alpha smoke
must **darken** it, rather than behaving like additive glow. A controlled host point
light at `(-2,1,3)`, range 16 and radius .5 cycles through intensity zero, red and blue.
The colored phases use 24,000 lumens, shadows disabled. The saved material's response
must rise by > .05 linear in the selected channel and differ from the opposite channel
by > .05. The final dark center is approximately .056–.059 against a .165 background;
the +Z red/blue selected-channel centers are approximately .185/.196.

For every phase the probe hides the original interleaved pools, disables OIT, and
spawns three one-particle draws at the same evaluated world positions. Ordinary
whole-draw sorting is sufficient for those distinct single-particle depths. This
reference uses the same saved lighting material, camera, clock and light, but an
independent alpha-blending path. Both the central patch and **every image pixel**
must agree within .01 linear RGB error. This isolates OIT/material integration; it
is not an independent physical-scattering or Bevy point-light correctness proof.

Original pools and OIT are restored after each phase; the whole image must return
within one byte. With the blue light still active, setting only the instance
`Scene light gain` to zero must reproduce dark smoke. Removing that override must
restore the blue-lit image. Compiled material identity is preserved and original
players remain paused at frame 30. All samples report no global drops, over-sort
pixels or visited invalid links. No CPU light-position transport is introduced.

Early synthetic-receiver runs passed. The first saved-material attempt at 12,000
lumens failed the minimum light-contrast check. That run is retained as failed; the
fixture light alone was strengthened to 24,000 lumens. No asset, contrast threshold,
color-error threshold or renderer policy was relaxed. This is controlled probe
calibration, not a proposed lighting setting for the public show.

## Final native results

Windows / RTX 4070 SUPER / Vulkan, Rust 1.98.1, Bevy 0.20.0:

| Measurement | Result across both final repetitions |
| --- | ---: |
| HDR cases, including exposure restoration | 16 per run |
| Saved-material lighting cases | 12 per run |
| HDR maximum linear reference error | .003593 |
| Wrong SDR-clamp control, minimum error | .299103 |
| Wrong map-before-blend control, minimum error | .141674 |
| Lit central split-draw maximum error | .003452 |
| Lit whole-image split-draw maximum error | .003738 |
| OIT, exposure and scattering restoration | byte-exact |
| Corresponding PNGs matching across final runs | 64 / 64 |

These tolerances include packed OIT color/alpha and SDR screenshot quantization.
Visual inspection confirms background darkening and local red/blue illumination.
No image repeatability claim substitutes for the analytic and split-draw controls.

The final combined run passes all five native probes: depth, healthy/lifecycle,
HDR, saved-material lighting and expected saturation degradation. Prior healthy and
opaque-depth images remain byte-identical; the tiny shared fixture helper was only
factored to accept a material program, preserving the original red/blue tests.
See [reports, source hashes, capture hashes and retained iteration notes](fragment-list-hdr-lighting-2026-10-23.json).

## Scope, validation and next gate

Only viewer test fixtures and documentation/evidence change. No production shader,
API, compiler, runtime/history, dependency, asset or default changes in this slice.
The light is deliberately host-controlled; this does not requalify authored event,
selected-star, representative-light or sound routes. It does not add shadows,
multiple scattering, fluid coupling or improve the saved smoke art.

Next: **textured/flipbook paths**, then mixed additive/trail/non-OIT interactions.
Other depth policies, resize/multiple cameras, bloom/other display transforms and
matched dense/live-paced costs remain open. Global loss and approximate-tail errors
remain known limitations. Keep the public show unchanged until the remaining gates
support adoption, revision or a different compositor.

Run native GPU work alone with a fresh absolute output directory:

```powershell
$env:AESTRA_CROSS_POOL_IMAGES = 'C:/absolute/fresh-oit-hdr'
cargo test -p aestra-viewer fragment_list_ --locked --offline -- --ignored --test-threads=1
```

The two new probes can also be selected separately with `fragment_list_hdr_` and
`fragment_list_lighting_`, using the same fresh root (distinct output subfolders).
Viewer ordinary tests: 104 pass / 35 opt-in ignores. Strict workspace all-target
Clippy (`aestra-bench/gpu`), formatting and whitespace checks pass. Existing Windows
incremental-cache hard-link fallback warnings remain. Full workspace tests, Linux,
full-show art and public HDR/live/dense performance runs are not claimed here.
