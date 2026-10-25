# F8.1C4F — opaque-depth occlusion oracle

The experimental fragment-list compositor passes the new **opaque StandardMaterial /
default Aestra depth-prepass** gate. No production rendering fix was needed for this
path. This adds tests/evidence, not a public-show opt-in or smoke-art approval.

## Independent depth and color controls

The existing three-layer red/blue fixture runs through ordinary compiled GPU players,
forward PlaybackOnly at frozen frame 30. The target remains 480×360 sRGB LDR, MSAA off,
no lights/bloom/tone mapping and no per-pool sorting. A standard opaque, unlit green
box covers world X < 0 and leaves the other half of the image unobstructed. Both camera
directions and player spawn orders are tested. Camera reversal swaps the covered side.

The box is moved non-monotonically through four depths in the same app:

| Wall center Z | Visible layers on covered side, +Z camera | Visible layers, −Z camera |
| --- | ---: | ---: |
| −6 | 3 | 0 |
| −1 | 2 | 1 |
| .5 | 1 | 2 |
| 6 | 0 | 3 |

The oracle uses the box's camera-facing surface (center ± .1), not its center, to
filter particle world depths. It sorts only the remaining layers and computes
linear source-over against the opaque green background `(0,.25,0)`. The open side
independently retains the original three-layer blend against black. Coplanar ties,
contact softness and edge antialiasing are deliberately outside this test.

Color patches are two 3×8 regions inside the original calibrated central 8×8 particle
plateau, with a two-pixel gap across the wall edge. An initial attempt sampled farther
off-axis and failed the clear-image calibration because sprite coverage fades there.
That failed run is retained separately in the evidence. Correcting patch placement
does not relax the unchanged **< .01 maximum linear-color error** threshold.

Each app checks the no-wall image first, then hides both players to independently
calibrate the wall's green and the open half's black. Both players are restored for
all four wall positions. Finally, hiding the wall must restore the original whole
image within one byte and restore the original GPU fragment count. The no-wall image
is also compared against each occluded reference as a deliberate wrong control: a
probe that ignored opaque depth must fail, not pass by repeatability.

The camera must have Aestra's `DepthPrepass`, and OIT approval must remain valid.
Newly completed aggregate samples must report zero drops, over-sort pixels and visited
invalid links. Fragment attempts must stay positive on the open half, fall when the
wall occludes particles, and equal the baseline when all particles are in front.
This verifies rejection before list collection on this prepass path; it is not an
opaque-depth performance benchmark or a guarantee for other material configurations.

## Native results

Windows / RTX 4070 SUPER / Vulkan, Rust 1.98.1, Bevy 0.20.0:

- All **16 depth/view/spawn cases** pass in two final runs.
- Covered-side maximum linear error: **.003067**; open-side maximum: **.001040**.
- Wall-only calibration error: .000159; clear-particle calibration: .001040.
- Missing-occlusion controls fail by at least **.123960** linear error.
- Hiding the wall restores every image **byte-exactly**.
- All **28 corresponding depth-probe PNGs match byte-for-byte** across repetitions.

For the +Z view, attempted fragments change from 105,173 with the wall behind all
particles to 52,588 with the wall in front of them; the −Z view changes from 129,524
to 64,762. Intermediate depths retain the expected intermediate counts. No global
loss or local-sort saturation occurs. These are GPU-derived aggregate observations,
not CPU particle enumeration or timing measurements.

The final combined native run also reruns the existing healthy/lifecycle and expected
saturation probes sequentially. All three tests pass. The healthy images retain the
previous result; degraded saturation remains explicitly unaccepted. See the
[machine-readable reports, capture hashes and source hashes](fragment-list-depth-2026-10-23.json).
Visual inspection confirms partial occlusion, the uncovered half and full foreground
occlusion rather than a scene-wide masking effect.

## Scope and next gate

Only the existing test harness and roadmap/evidence change. The portable compiler,
particle ABI, runtime, camera guard, diagnostic transport, Bevy resolve shaders,
saved fireworks assets and public defaults remain unchanged.

This does **not** qualify opaque custom materials that opt out of the prepass,
alpha-masked geometry, coplanar/near-plane cases, arbitrary depth policies, lit/HDR or
textured/flipbook materials, mixed transparent draws, multi-camera/resize behavior,
dense/live performance or photographic smoke. In particular, the presence of a
camera prepass does not prove every custom opaque material contributes to it.

Next: **lit semantic smoke and HDR radiance**, followed by textured/flipbook and mixed
additive/trail/non-OIT interactions. Keep OIT experimental while these gates, other
depth paths, resize/multiple cameras and matched dense/live-paced costs remain open.

## Reproduce and validation

Run native GPU work alone with a fresh absolute directory:

```powershell
$env:AESTRA_CROSS_POOL_IMAGES = 'C:/absolute/fresh-oit-depth'
cargo test -p aestra-viewer fragment_list_depth_ --locked --offline -- --ignored --test-threads=1

# Depth, healthy/lifecycle and saturation probes, sequentially:
$env:AESTRA_CROSS_POOL_IMAGES = 'C:/absolute/fresh-oit-regression'
cargo test -p aestra-viewer fragment_list_ --locked --offline -- --ignored --test-threads=1
```

The analytic depth/color reference has an ordinary unit test. Viewer ordinary tests:
102 pass / 33 opt-in ignores. Strict all-target workspace Clippy (`aestra-bench/gpu`),
formatting and whitespace checks pass. Existing Windows incremental-cache hard-link
fallback warnings remain. Full workspace tests, Linux and public HDR/live/dense runs
were not repeated for this test-only slice.
