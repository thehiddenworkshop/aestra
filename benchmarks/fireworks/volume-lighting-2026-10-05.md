# F8.3A — opt-in fluid scene lighting

Base: `8fd72073` plus scoped F8.3A changes. **Narrow point-light controls pass; F8 remains in
progress.** This is a renderer capability slice, not AAA smoke art or a production-finale claim.

## Contract and host use

In the fluid extension's `Volume Look`, set `scene_light_intensity` above zero to opt into scene
illumination. The default is **zero**, preserving existing source/artifact looks. The authored
ambient/directional light, its self-shadowing, opacity integral and blackbody fire remain intact.
`scene_light_limit` defaults to 8, scales to 6/4 at medium/low using the existing fluid presentation
tier, and accepts 0–32. Zero gain or zero limit disables the scene contribution. These inputs change
presentation constants, not the solver's execution block. Live presentation tests retain the same
volume entity, material and field texture handles. Old artifacts with 21 constant words get zero
gain/budget through uniform padding; no artifact version or six-field ray ABI change is needed.

The generic GPU volume interface exposes
`aestra_volume_scene_lighting(uvw, pixel, max_lights) -> vec3<f32>`. The portable default returns zero.
A backend can supply its implementation with `volume_interface_wgsl_with_scene_lighting`; it owns
world conversion and view bindings. No extension-specific fluid dependency is added to the backend.

Bevy reads its existing per-view clustered point-light buffers at **each occupied march sample's
world position and view depth**, not the rendered box's back-face depth. Conversion uses the complete
mesh/parent transform. No source-particle CPU enumeration/readback, new particle light pool or new
CPU light-position upload is introduced. Screenshot readback below is explicit test tooling only.

The model is unshadowed isotropic single scattering (phase `1/(4*pi)`), with Bevy's smooth range
falloff and a finite source-radius distance floor. Scene directional/spot/area lights, dynamic
opaque/volume shadows and anisotropic phase are not supported. Nonpositive-range/inactive slots are
neutral. Each sample visits at most 32 native point-light cluster entries, **including inactive
slots**; this is a shader cost ceiling, not a global host/source-particle limit or strongest-light
selection. Other host lights can consume the prefix budget; starvation/ordering needs qualification.

## Native controls

Raw directories: `target/fireworks-f8/volume-light-images-final` and
`target/fireworks-f8/volume-light-images-final-repeat`; twelve PNGs and one JSON report each.
All twelve PNGs and the report are byte-identical in the fresh repeat. Source, binary and raw-file
SHA-256 hashes for both accepted executions are retained in the adjacent evidence JSON.

- RTX 4070 SUPER / Vulkan; 480×360 perspective, black background, no bloom/sky/surface receiver.
- Real fluid-extension density, 32³ grid; emitter list cleared, so no emissive particles can
  counterfeit a positive result. Authored ambient 0.05; directional intensity/shadow steps zero.
- Paused playback-only player frame 90, seed `0x0000000000000000`, initialized by an explicit seek.
  This is not forward cadence/cost/replay performance evidence or a GPU-stage-frame readback.
- Scene gain 1, requested limit 8; one ordinary Bevy red point light, 1500 lm, range 6 m,
  physical radius 0.1 m. Runtime light buffers/cluster imports are exercised, not mocked.
- Readiness waits for all cached render pipelines to succeed and settle before screenshots.
- Baseline → light on → light moved 500 m away → intensity zero; then a nested rigid hierarchy
  triplet, then a separately rotated/nonuniformly scaled volume triplet.
- Identity, player frame and playback epoch remain unchanged through the lighting/transform
  triplets. The last zero-gain/zero-budget controls **recompile the look at the same frame**;
  they do not claim a new live parameter-binding API.

Unchanged gates: at least 100 pixels with red response above 3/255 in each active geometry case;
out-of-range, removal and disabled-look maximum channel delta ≤1/255. Rigidly transforming the
whole camera/volume/light hierarchy must preserve the on image within 1/255.

Results:

| Control | Result |
| --- | --- |
| Ordinary light response | 11,684 positive red pixels |
| Nested rigid hierarchy response | 11,684 positive red pixels |
| Rotated/nonuniform volume response | 11,542 positive red pixels |
| Out-of-range / all three removal controls | Exact baseline, maximum delta 0 |
| Rigidly transformed on image | Maximum delta 1 |
| Zero gain / zero budget | Exact authored baseline, maximum delta 0 |

The captured on image was visually inspected: the density column, not a surface receiver, becomes
red near the light. This is a diagnostic smoke column, **not artistic acceptance**.

## Rejected attempts retained

`target/fireworks-f8/volume-light-images`: the first native run caught WGSL's reserved identifier
`smooth`. Shader compilation failed and the image gate correctly rejected zero response. The helper
now has a GPU-free Naga validation test in addition to the real native composition gate.

`target/fireworks-f8/volume-light-images-attempt2`: a fixed 24-frame warmup captured a black baseline
before pipelines were ready; removal/range deltas were 51. Rejected, not normalized away. The test now
waits for actual pipeline readiness. Attempt3 passed basic controls; the transforms and controls
directories retain intermediate expanded gates. The final run includes the inactive-slot guard.

## Verification and reproduction

GPU-free: 34 fluid contract tests; four backend volume tests; three portable volume tests; 95 viewer
unit tests (18 ignored native/export tests). Scoped all-target Clippy with `-D warnings` and workspace
formatting pass. These tests cover legacy defaults, finite/nonnegative gain, light-budget validation,
quality scaling, unchanged solver blocks, reusable presentation handles, neutral portable composition
and the real adapter helper's WGSL syntax/types. Native tests remain explicit/ignored for GPU-free CI.

Run native captures alone, without concurrent builds/benchmarks, using a fresh evidence directory:

```powershell
$env:AESTRA_VOLUME_LIGHT_IMAGE_REPORTS = 'target/fireworks-f8/my-volume-light-images'
cargo test --locked -p aestra-viewer volume_light_images -- --ignored --nocapture --test-threads=1
```

## Next gates

**F8.3B:** actual Aestra representative pulses and native same-frame selected-particle lights on
smoke under authored overlapping forward playback, with disable/range/removal/retirement controls
and matched cost measurements. The native selected-light adapter's default-layer restrictions still
apply; consuming its buffer format is not qualification of its smoke contribution.

**F8.3C:** explicitly lit particle-smoke material support or an intentional, documented unlit-sprite
decision. F8.1 persistent smoke art, F8.2 generic particle/event-to-fluid injection, sparse-volume
scene-light images, multiple views/render layers, additional hardware and production-finale behavior
remain open. Ordinary clustered point-light proof does not close those gates.
