# F8.1A — bounded accumulating sprite-smoke prototype

Date: 2026-10-08. Adapter: NVIDIA GeForce RTX 4070 SUPER / Vulkan.
Toolchain: `1.98.1-x86_64-pc-windows-msvc`.

Implemented through the public `aestra-bevy` fireworks example, with saved assets and
normal project/compiler resolution. The viewer supplies regression instrumentation only.
This qualifies a bounded reusable prototype, **not complete F8, AAA smoke art or full-show costs**.
The existing saved show and its playback-only/unlit defaults remain unchanged.

## Authored behavior

- Shared `smoke_billow.aestra.material-function.ron`: three fixed seeded noise octaves,
  age evolution and irregular soft edges, without bitmap/resource dependencies.
- Shared `fireworks_persistent_smoke.aestra.material.ron`: density, ambient and scene-light
  gain parameters, existing isotropic scene irradiance and alpha blending.
- Saved `fireworks_smoke_persistence.aestra.ron`: one 96-slot pool, eight initial puffs plus
  20/s for four seconds (88 births), 12–14-second lifetimes, expansion, rotation and fade.
- Exposed Drift acceleration drives Motion; this is a fixed-acceleration approximation,
  not physical wind advection or fluid simulation.
- Genuine red/blue break emitters at 8/10 seconds illuminate the older smoke through
  independent representative and selected-light outputs. Their stars have no renderers,
  so star pixels cannot counterfeit the smoke response.
- Same smoke workload at all tiers; selected-light caps are 8/4/2 and representative cap is two.
  This lab uses playback-only and opt-in per-draw GPU depth ordering.

## Qualification and controls

Public example reference-interpreter tests pass at high/medium/low: exact 88 births fit the
pool, identity/count persist after emission ends, cloud-centroid drift is positive, identical
seeds repeat, different seeds vary particle origins/rotation, and particles fade/drain by 18s.
These are CPU reference assertions, **not a GPU alive-population readback**.

Two fresh native Camera3d runs pass all three tiers (480×360, SDR, no HDR/bloom,
seed `0xf83b000000000001`, host scale 0.1). All **60 corresponding PNGs match byte-for-byte**.
The saved native reports verify:

- Persistent smoke at seven seconds: 3,415 pixels gain more than 3/255 versus the empty image.
- Density zero returns the empty image; restoration and repeated frozen captures match exactly.
- Both later breaks illuminate the accumulated receiver. Each independent light family exceeds
  the unchanged 100-positive-pixel threshold at every tier.
- Reversible intensity mute/restoration matches exactly. Drain at 18 seconds and owner removal
  match empty; alpha-sort ownership retires to zero pairs/owned bytes.
- Native selected-light admission has zero CPU selected-light active/allocation/copy/submitted
  readback/staging counts, no invalid sources and reservations within the tier cap.

Positive comparisons count RGB pixel gains above 3/255. Frozen/restoration/negative controls
allow at most 1/255 absolute RGB difference; every accepted negative control measured zero.
Screenshot readback is test instrumentation, not production particle-position transport.

The first high-tier native run remains rejected in the evidence. Its restoration test used
`TransientLightSettings.enabled=false`, which intentionally retires one-shot requests.
Re-enabling admits future pulses, not resurrection of old flashes. The corrected reversible
mute uses `max_lumens=0` and restores its previous value, without weakening thresholds or
changing runtime behavior. Earlier compilation rejected private WESL helpers, so the final
function inlines fixed octaves under the existing v1 contract. An initial motion assertion
incorrectly demanded monotonic movement from every turbulent particle; the final test checks
centroid drift alongside persistent identities, bounded births and drain.

Public host captures at seven and 10.2 seconds also succeeded (1280×720, public show seed
`0xf1e0000000000001`). They use a different seed/resolution from the regression images and
are not matched comparisons. Capture shutdown logged closed readback-channel warnings;
both processes exited successfully after saving their images.

## Reproduce

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --smoke-persistence --no-audio
```

For the native regression, set `AESTRA_SMOKE_PERSISTENCE_REPORTS` to an **absolute** output
directory and run alone:

```powershell
cargo +1.98.1-x86_64-pc-windows-msvc test --locked -p aestra-viewer saved_smoke_persists_drifts_receives_later_breaks_and_drains -- --ignored --nocapture --test-threads=1
```

Other passing checks: public audio-enabled example (11 tests), viewer suite (96 passed,
22 ignored), workspace check, formatting and viewer/example Clippy with warnings denied.

[`smoke-persistence-2026-10-08.json`](smoke-persistence-2026-10-08.json) records all six accepted
reports, the rejected report, source hashes, 60 image/repeat hashes, public capture hashes
and exact commands. Raw local images/reports live under `target/fireworks-f8/persistence-*`;
the PNGs are not checked in.

## Remaining gates

Next is **F8.1B: real shell-born smoke and overlapping cohorts**, followed by authored live
overlap/full-show costs before migration. Current smoke is one draw; no cross-draw ordering,
physical advection, self-shadowing or anisotropic scattering is qualified. Sorting scales
with capacity per view/draw, and the three-octave fragment work has no large-pool cost gate.
No tier-scaled smoke-density performance, photo-approved art or production-finale claim is made.
