# F8.1C4J — shared alpha/additive fragment lists

The bounded mixed-draw oracle now checks **correct rendering**, rather than the
expected failure recorded in [C4I](fragment-list-mixed-2026-10-25.md). Eligible
Aestra alpha/additive draws share depth-ordered fragment lists. This fixes the
previous foreground sprite/trail/mesh ordering error inside the experimental opt-in.
The public fireworks example and editor defaults remain unchanged.

## Integration and restrictions

The native 3D queue admits rendered sprites, flipbooks, trails and semantic mesh
particles. Semantic render state must be alpha/additive, Less/LessEqual and not
depth-writing. The pipeline selects the additive shader variant using the **effective
semantic blend state**, not the legacy draw blend. Legacy alpha/additive fragment
entry points and the final semantic fragment are wrapped after ordinary material,
texture, coverage and lighting evaluation. Portable shader bodies and buffer ABI
are unchanged.

Each list node stores coverage-weighted RGB. Alpha is extinction: ordinary alpha
uses material coverage, while additive emission uses **zero extinction**. Front-to-back
source-over therefore attenuates light behind smoke, but additive light does not
dim the layer behind it. The existing packed RGB9E5/24-bit-depth/8-bit-alpha node
layout suffices; no extra operator lane or CPU fragment/particle transport is added.

This is not universal transparency. Ribbons, multiply, wireframe, overlay/disabled
depth, depth-writing, legacy meshes, custom/foreign draws and fluid rendering remain
outside the qualified set. Bevy's final resolve still overlays ordinary transparent
non-participants irrespective of depth. Equal-depth tie behavior, negative radiance,
arbitrary custom material side effects and all host-material combinations are not
certified. Unit tests exercise explicit inclusion/exclusion and semantic overrides.

## Positive native oracle

Six green-source paths overlap a red half-alpha semantic sprite:

- Legacy additive sprite, additive trail and alpha trail.
- Semantic alpha mesh, additive sprite and additive mesh.

The real compiled GPU players use PlaybackOnly; no history/replay or synthesized
trail geometry is introduced. Both sources are placed before recording. Green stays
at world Z=-3 and red at 0; pipelines warm at frame zero, players advance to frame 30,
then pause. Both Fast/DepthBackToFront and both spawn orders run from each camera
side, followed by opposite-side and initial-side restoration: **144 cases**.

Each case requires an independently calibrated isolated source, newly completed
nonzero fragment-list diagnostics for green, the analytic central 8x8 blend oracle,
and a correctly sorted ordinary two-plane reference. Analytic and whole-image
linear-light errors must stay below .01. Foreground cases must reject the wrong
forced-background model; additive cases also reject ordinary alpha absorption.
Camera/OIT restoration remains byte-strict (at most one byte, measured separately
from ordinary-pass quantization). Invisible or uncollected green is not a pass.

The first two local iterations (`-a`, `-b`) stopped at an overly strict **new** isolated
OIT/ordinary byte comparison. Diagnostic iteration `-b` measured maximum linear
error .005954, centers .502886 versus .496933, and maximum seven bytes at low-coverage
edges. Ordinary LDR blending and packed-list resolve are not bit-identical color
paths. The isolated control now uses the same existing .01 linear-light bound as
the analytic/whole-image reference; those bounds and identical-path restoration
were not relaxed. Iteration `-c` passes all 144 mixed cases before additional controls.
Final evidence uses fresh roots `-d` and `-e`.

## Occlusion and expected degradation

One default DepthPrepass opaque-wall control per path places a calibrated unlit
blue wall in front of both sources. All six require zero attempted fragments, a
byte-identical wall-only image and byte-identical unoccluded restoration.

A semantic additive foreground case also deliberately exhausts global node storage,
then local exact-sort capacity. Both must visibly disagree with the healthy result
and publish the corresponding drop/over-sort diagnostics. Restoring capacity must
restore the healthy image. These are **expected-degradation controls**, not accepted
blending. Bevy's local overflow approximation multiplies already weighted RGB by
alpha again, so zero-extinction additive emission can disappear. No upstream resolve
patch or larger budget is represented as a correctness fix. Allocation approval and
zero global drops alone remain insufficient.

## Evidence and validation

[Machine-readable results](fragment-list-unified-mixed-2026-10-25.json) record source
hashes, raw report hashes, per-path extrema, all nine new controls, image-manifest
digests and repeated-image comparisons. The target is Windows / NVIDIA RTX 4070 SUPER /
Vulkan, Rust 1.98.1, Bevy 0.20.0 / wgpu 30.0.1, 480x360 sRGB, MSAA off, no tonemapping
or bloom. This is not photographic HDR mixed-scene or production performance evidence.

The combined native run repeats the existing alpha/lifecycle, opaque-depth, HDR,
lit-smoke, texture/flipbook and saturation probes alongside the positive mixed test.
The real Bevy ShaderCache qualification separately compiles legacy, semantic unlit
and saved lit-smoke alpha/additive variants with OIT enabled/disabled and checks a
single view binding. GPU image probes exercise the actual production pipeline.

Both final runs pass 144 cases (72 foreground) and all nine additional controls:

| Measurement | Result |
| --- | ---: |
| Maximum analytic mixed patch error | .004696 |
| Maximum whole-image ordinary-reference error | .005954 |
| Minimum foreground wrong-order error | .221098 |
| Camera/OIT restoration maximum byte error | 0 |
| Non-degraded mixed/control PNGs matching across repetitions | 883 / 883 |
| Prior accepted alpha/depth/HDR/lighting/texture PNGs unchanged | 564 / 564 |

The combined run passes seven native probes. Of 885 mixed/control PNGs per run,
two deliberately degraded capacity images are excluded from repeatability acceptance.
Only the global-exhaustion image differs across runs; both reports require 93,124
drops from 93,296 attempts with 172 nodes. Local capacity one reports 37,108 over-sort
pixels, no global drops, and .502887 maximum linear error. Restoring capacity is
byte-exact. This is evidence of diagnosed degradation, not acceptable smoke output.

Ordinary tests pass: renderer 170 (3 ignored), viewer 106 (37 ignored), real shader
composition 8 (1 ignored). Strict workspace/all-target and qualification-target
Clippy, workspace/qualification formatting and diff checks pass. Existing incremental
hard-link fallback warnings are environmental. No full workspace test or Linux GPU
run is claimed.

Reproduce in a fresh absolute root, alone and serially:

```powershell
$env:AESTRA_CROSS_POOL_IMAGES = 'C:/absolute/fresh-oit-root'
cargo test -p aestra-viewer fragment_list_ --locked --offline -- --ignored --test-threads=1
```

Next qualify non-default depth/near-plane paths and resize/multiple-camera lifecycle,
then matched dense/live-paced costs. Unsupported ordinary draws remain an adoption
restriction. Continuous wind-dispersed star-path smoke, local relighting and public
HDR art review remain open; this integration does not claim realistic smoke approval.
