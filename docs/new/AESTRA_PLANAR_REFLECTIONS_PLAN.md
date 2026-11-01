# Selective planar VFX reflections

The initial additive prototype is implemented behind explicit host/editor opt-in
(2026-11-01). `preview::planar::PlanarReflection` creates a mirrored HDR camera
and reuses the existing draw buffers/trail history. It does not create a player.
The demo accepts `--scene sea --water-reflections`; the editor exposes the same
shared path in Preview settings. This is bounded prototype qualification, not
final water art or whole-show performance acceptance.

The first native Vulkan run completed five spaced rockets with a 640×360 target,
22 peak queued additive draws, 15 peak omitted transparent draws and no extra
OIT list. The target-family estimate is 6,451,200 bytes (6.15 MiB), excluding
view-local geometry/indirect buffers and driver residency. Native matched review
is recorded in [the native review](../../benchmarks/fireworks/f8-planar-reflections-2026-11-01.md).

Implemented boundaries: horizontal Y-up water, perspective source above it,
one distinct material per source, reverse-Z oblique plane clipping, proper
right-handed mirror basis, opaque scenery plus native additive sprite/trail/
flipbook presentation, independent visibility, and shared pipeline descriptors.
Targets resize in place and retire on disable/invalid configuration; views wholly
above the horizon skip capture. Finite-receiver frustum/occlusion skipping remains
open. Main-view alpha sorting and partitioned OIT are unchanged; reflection views
request neither. A nine-tap Gaussian filter is the initial prefilter, not a full
roughness mip chain. Alpha smoke is explicitly absent. This paragraph describes
the first retained prototype; the follow-up below supersedes its surface sampling.

## Wave-normal response follow-up — 2026-11-01

The mirrored image offset/blur is replaced with reflected wave-normal directions
and twelve GGX visible-normal samples, exact dielectric Fresnel and Smith masking.
A separate GPU-filtered HDR chain supports solid-angle/PDF-based LOD selection;
odd-sized mips use area-weighted reductions and preserve constant/impulse energy
in the native shader oracle. Filtering is scheduled after the mirror camera and
before the main opaque receiver draw, with cached texture views/bind groups.
The diagnostic scope is `aestra_planar_mips`. Resources follow the existing
resize/retirement policy. At 640×360 the estimated target family rises from
6.15 MiB to 8.50 MiB, within the existing configurable 16 MiB ceiling.

The surface now breaks source radiance into wave-driven streaks/glints. No height
threshold, hand-painted firework image, brightness boost or extra VFX owner is
introduced. Capture framing decides where reflections enter the image, and
source burn/cooling already determines whether light remains to reflect.
Directional sampling from a planar camera is a far-field approximation, not
depth-aware tracing: close-object parallax, finite capture coverage and wave
self-occlusion remain open, alongside smoke attenuation and broad performance
qualification. The original native image evidence remains retained separately.
The visible-normal method follows
[Heitz's GGX sampling derivation](https://jcgt.org/published/0007/04/01/paper.pdf).
See the [wave-water native review](../../benchmarks/fireworks/f8-wave-water-2026-11-01.md)
for the image controls, HDR energy oracle and separate profiling pair, including
the measured idle-capture overhead and remaining qualification gates.

In reflection mode, captured source radiance replaces analytical point specular
glints. The initial direct-water term preserves only unshadowed clustered point
diffuse. Other light kinds, shadows/indirect illumination and source-layer mask
qualification are still open. Ordinary water lighting is restored when disabled.

## Rendering approach

For one dominant water plane, render selected scene radiance from a mirrored
view into an HDR target. Start at half width and height: a 1280×720 main view
uses a 640×360 reflection image. This has one quarter of the pixels, but still
adds view preparation, vertex work and draw submissions. Measure those separately.

Reuse the authoritative GPU particle buffers and trail history. Never create a
second EffectPlayer, repeat simulation, deliver outputs twice or advance a
reflection clock. Culling, billboard orientation and any geometry/indirect draw
preparation that depends on the view must be reflection-specific. Main-view
visibility cannot decide what the reflection sees, especially for off-screen
fireworks.

The first capture should contain additive stars/embers/trails and selected
opaque occluder depth. Exclude the receiver, UI, gizmos, recursive reflections,
extra lighting/shadows, sound and per-capture bloom/tonemapping. Additive emission
does not need fragment-list OIT. Alpha smoke attenuation is a separate validated
extension; its omission must be explicit rather than presented as exact parity.
Clip to the reflecting plane and handle mirrored winding correctly.

Sample reflected HDR radiance in the water material. Use the wave normals to
distort the coordinates and roughness to prefilter/blur the image. Apply Fresnel,
then the main view's display exposure/bloom once. Avoid duplicating the captured
emissive source's energy through an analytic point-light specular highlight;
direct receiver illumination and reflected source radiance need a defined policy.

## Shared Aestra contract

- Host API declares a plane, source camera, capture filter and quality/memory
  policy; the editor water preview consumes the same API.
- Secondary presentation views share simulation ownership but keep view-specific
  draw/culling/geometry state and statistics. Despawning a view retires its caches
  without retiring the effect or creating more particle events.
- Allocate reusable HDR/depth targets and filtering resources. Resize/release
  them predictably. A quality budget controls reflection resolution and work;
  it must not become a new effect/owner-count limit.
- Run capture after authoritative GPU simulation is ready and before the main
  water draw. Skip capture when no receiver is visible. Update moving fireworks
  every frame initially; reduced-frequency updates need motion/reprojection tests.

The current experimental partition transport explicitly supports a single
full-target OIT view. Simply adding another managed OIT camera is not a qualified
solution. An additive-only reflection target should not request another OIT list,
but the existing partition guard and per-view resource lifetime still need a
deliberate secondary-view design and regression coverage. Do not remove the guard
to make a demo pass.

Bevy 0.20's current SSR path is deferred and scheduled at the opaque stage; it is
not a drop-in reflection path for Aestra's later transparent particle/trail pass.
It also cannot recover source radiance outside the main camera image. A static
environment probe cannot reproduce this moving show.

## Acceptance

1. One versus two presentation views have identical births, state, host outputs
   and playback advancement, including pause/restart and view teardown.
2. A moving rocket and trail reflect at the correct position from near/audience
   views, including off-screen sources and a barge occlusion control.
3. Calm water retains coherent shapes; rough water breaks and broadens them.
   HDR color is preserved and display processing is applied once. Resolution
   changes preserve integrated emission energy, including subpixel sprite/trail
   sampling; a coarser target must not silently brighten the reflected effect.
4. Keep alpha-smoke approximation visible in the report until validated.
5. Record capture/filter/composite GPU times, extra geometry work and owned
   bytes at multiple resolutions. Quarter pixel count is not a 75% whole-frame
   performance guarantee. Existing F9 failures remain independent gates.

Reference implementation patterns: [Vulkan planar reflections](https://docs.vulkan.org/tutorial/latest/Building_a_Simple_Engine/Advanced_Topics/Planar_Reflections.html)
and [Epic's planar-reflection performance controls](https://dev.epicgames.com/documentation/en-us/unreal-engine/planar-reflections-in-unreal-engine).
