# F8.1C4K — bounded depth and camera-lifecycle qualification

Started October 25; final repetition and evidence completed October 26, 2026.

This slice adds native image/allocation tests for the experimental compositor; it
does not enable it on the public show or change production rendering. It builds on
[shared alpha/additive integration](fragment-list-unified-mixed-2026-10-25.md).

## Supported depth paths

Aestra automatically installs DepthPrepass on 3D cameras. Removing that component
before Update is not a supported no-prepass configuration: Aestra reinstalls it.
These tests exercise the actual shipping prepass path, not a test-only override of
the renderer's installation system. The upstream no-prepass resolve branch is not
qualified by this slice.

The depth-variant oracle has 48 cases: perspective/orthographic projection, opposite
camera sides, alpha/additive semantic mesh planes, Less/LessEqual material state,
and an opaque blue plane at three distinct depths. Red alpha sprites and green mesh
particles occupy separate known world depths. An independent front-to-back visibility
filter and back-to-front alpha/additive composition predict the central 8x8 patch.
Correctly sorted ordinary rendering provides an additional whole-image reference.

Every case requires analytic and whole-image linear-light error below .01, healthy
GPU diagnostics, zero attempted fragments when fully occluded, and byte-exact OIT
restoration. The fixed depths intentionally avoid equal-depth ties. Orthographic
projection uses a fixed 16-unit vertical extent; this does not certify every custom
projection or depth range.

The near-plane oracle uses a real semantic mesh rectangle rotated 45 degrees and
a perspective near distance of 1.0. Alpha and additive sources visit distances
-.5, .7, 1.0, 1.4, 1.0, .7, -.5 without advancing playback: 14 cases. Fully behind-camera
and fully near-clipped sources must contribute zero pixels and zero fragments;
the straddling and fully visible states must contain significant geometry. The
fully visible source must cover more pixels than its clipped state. Revisited
states must restore byte-exactly, detecting stale geometry/list contributions.

Each case uses isolated red/green images for a per-pixel analytic composition check,
a whole-image ordinary sorted reference below .01 linear error, and byte-exact OIT
restoration. No particle/fragment readback is introduced into the runtime; PNGs and
16-byte aggregate diagnostics remain explicit qualification tooling.

## Shared storage and camera isolation

Two full-target offscreen cameras view the same paused alpha sheets from opposite
sides. The first target changes from 480x360 to 640x240, 240x640 and back; the second
is 320x480. Camera order also reverses. Each paired image must match a same-target
solo-camera image and a reactivated paired image **byte-exactly over the whole image**.
Independent analytic center colors additionally verify the expected opposite-depth
composition. This catches cross-camera list contamination that a center-only or
repeatability-only check could miss.

The shared allocation must match the component-wise maximum target dimensions,
not a sum of separate pools: heads = width*height, nodes = four times that, storage
= 52 bytes per pixel (12-byte nodes plus 4-byte heads). A one-byte budget on either
participant must reject both, release shared node/head storage to at most 16 bytes,
and invalidate diagnostics. Restoring the budget must restore the original image.

After removing the second camera, one-camera diagnostics must resume with the
correct camera and target size. The remaining image then shrinks to 256x256, grows
to 800x360 and returns to 480x360, requiring current-size diagnostic samples and
correct allocation each time. Binding a different image of the same dimensions
and restoring the old image must preserve rendering. Removing the final opt-in
releases node/head storage and clears status/diagnostics.

Multi-camera diagnostics deliberately remain **unavailable**, not healthy: the
existing diagnostic supports exactly one full-target OIT camera. The image checks
qualify bounded two-camera reuse without inventing per-camera overflow observations.
Inactive-camera scratch tests are not a claim of general camera scheduling support.

## Evidence and remaining scope

[Machine-readable results](fragment-list-camera-depth-2026-10-25.json) record the
native reports, source/report hashes, per-probe image manifests and repeated-image
comparisons. Target: Windows, NVIDIA RTX 4070 SUPER, Vulkan, Rust 1.98.1, Bevy 0.20.0,
wgpu 30.0.1; MSAA off and no tonemapping/bloom. All player clocks remain frozen at
frame 30 after forward PlaybackOnly initialization.

The final combined run has passing reports for all ten native probes. An isolated
second run of all three new probes also passes: all **247 corresponding PNGs match
byte-exactly**. The 1,447 prior accepted alpha/depth/HDR/lighting/texture/mixed images
remain unchanged. Deliberate saturation/degradation images are not accepted output.

| New gate | Result |
| --- | ---: |
| Depth/projection cases | 48 |
| Maximum depth analytic / whole-reference error | .004152 / .005954 |
| Near-plane clipping/return cases | 14 |
| Maximum clipping analytic / whole-reference error | .005027 / .005954 |
| Camera lifecycle stages | 13 |
| Camera isolation, OIT and clipping restoration byte errors | 0 |
| Repeated new images matching | 247 / 247 |

The initial `-a` iteration used one-way near-plane transitions and basic camera
controls. `-b` added whole-image camera isolation; final `-c` adds clipping return
crossings and single-camera resizing. A mechanical Clippy enumeration fix does not
change comparison thresholds. Final repetition is `fragment-list-camera-depth-2026-10-26-a`.
No failed renderer result is reclassified as success and no bound is relaxed.

Viewer tests pass (107, with 40 explicitly ignored), along with strict workspace /
all-target Clippy, formatting and diff checks. No new production renderer, API,
dependency or asset change was required. No full workspace test or Linux GPU run
is claimed.

Run serially with a fresh absolute root:

```powershell
$env:AESTRA_CROSS_POOL_IMAGES = 'C:/absolute/fresh-oit-root'
cargo test -p aestra-viewer fragment_list_ --locked --offline -- --ignored --test-threads=1
```

This is not general no-prepass, equal-depth, arbitrary mesh/custom material, viewport
subrectangle, resolution-override, window/minimize, more-than-two-camera, multi-camera
overflow-diagnostic, Linux GPU, HDR/bloom mixed-scene or dense/live-performance acceptance.
The prior restrictions on non-participating transparent draws and saturated fragment
lists still apply. Photographic smoke art remains unapproved.

Next compare matched dense/live-paced compositor costs and capacity at relevant
show resolutions. Use those measurements to decide whether the bounded compositor
is viable before enabling it on the example or resuming smoke art tuning.
