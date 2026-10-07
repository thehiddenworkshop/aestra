# F8.3C — opt-in particle-smoke lighting

The portable material API and native Bevy 3D provider are implemented. **Only isolated-puff
lighting is qualified.** Dense overlapping alpha smoke failed repeatability; that attempt
is retained and is a measured prerequisite to F8.1 art/persistence, not AAA acceptance.

## Implementation

- `MaterialInput::ScenePointIrradiance`: Vec3 fragment input, appended stable key 30.
  No file-format bump or changed legacy input keys. Portable shaders and CPU graph swatches
  return neutral zero without a provider. A material graph explicitly combines this input
  with its albedo/ambient/gain; no hidden change to existing unlit programs.
- Native Bevy 3D semantic shaders use existing point-light clusters and the actual fragment
  world position (billboard corner interpolation, not particle center). Native inverse-square,
  finite-radius and smooth range attenuation is multiplied by isotropic 1/(4π). No normals,
  surface BRDF, shadows, anisotropy, directional/spot/area lights or new light pool.
- At most **32 cluster entries visited per fragment**, including inactive/retired entries.
  This is not strongest-light selection or a universal overlap budget. Mixed-host starvation,
  general render-layer/multi-view acceptance and additional hardware remain open.
- The 3D shader imports one native view binding; the portable 2D branch keeps its original
  binding and neutral provider. Composition asserts the callback/view ABI rather than
  silently accepting an unlit fallback. The capture harness fails fast on composition errors.
- The shared `fireworks_lit_smoke` material uses gray albedo × (low ambient + point irradiance
  × instance Scene light gain); alpha is particle opacity × radial mask. Its parameter changes
  do not replace the compiled shader/artifact/layout. The full show's existing material remains
  unlit and unchanged. No particle-position CPU transport or new semantic resource bindings.

## Reproduce

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --particle-smoke-lighting --no-audio
cargo run --release --locked -p aestra-bevy --example fireworks -- --particle-smoke-lighting --tier low --no-audio

# Use an absolute report folder; Rust tests run from their package directory. Run GPU tests alone.
$env:AESTRA_VOLUME_OUTPUT_REPORTS = "$PWD/target/fireworks-f8/particle-smoke-reproduction"
cargo test --locked -p aestra-viewer authored_outputs_light_particle_smoke_without_position_readback -- --ignored --nocapture --test-threads=1
```

The public host resolves the saved effect/material directly, without viewer injection or a
fluid extension. Space pauses, R restarts, L toggles both light families, Esc exits. Enabling
representative flashes after their birth does not replay old packets; restart for fresh flashes.

## Native method/results

Windows / RTX 4070 SUPER / Vulkan, development build, playback-only, 480×360, seed
`0xf83b000000000001`. One tick-zero isolated puff lives five seconds. Two material-free
32-star bursts at 1.5/1.8 seconds drive actual `FirstPerTick` representative output packets
and native same-frame selected-light outputs. No bloom, fluid domain, star draws or synthetic
scene lights can counterfeit the receiver response. All tiers compile capacity 65; this is
a bound, not an actual live-count/performance-scaling assertion.

Frame 115 is frozen for independent both/representative/off/selected controls. Positive controls
require ≥100 pixels changed by >3/255; negative/restoration controls require ≤1/255 everywhere.
Before-birth and expired family comparisons are negative. Live gain zero preserves active lights
but matches family-off; removing the override restores lighting. One-slot host selection budget
restores cleanly. A common rigid translation/rotation parent on camera and owner preserves the
image; identity restoration does too. Frame/epoch/artifact are unchanged by these controls.
Restart/rebinding and active-owner removal are tested; removal uses an empty image because
the particle receiver is already visible at tick zero. No selected-position staging/readback
or CPU proxy light enumeration is used.

| Tier | Selected host capacity / buffer bytes | Representative pixels | Selected pixels | Both pixels |
| --- | --- | --- | --- | --- |
| High | 8 / 832 | 1327 | 2331 | 2339 |
| Medium | 4 / 448 | 1327 | 2208 | 2237 |
| Low | 2 / 256 | 1327 | 1931 | 1987 |

Both complete all-tier runs pass. Negative/restoration/restart/removal differences are zero;
the low rigid-parent control has a maximum one-byte difference, within the unchanged gate.
**51 corresponding original PNGs are byte-identical between runs.** The final run also retains
12 additional gain/hierarchy PNGs. A third native run of the prior F8.3B fluid fixture passes
all three tiers after the shared harness refactor. The public Bevy host captures the colored
puff at 1.92 seconds with two actual flash lights and exits zero. Closed readback-channel warnings
appear on short-run shutdown after successful delivery, not shader/readiness failure.

## Rejected overlapping-puff attempt

The first high-tier receiver used 48 overlapping puffs: max/burst 48, Sphere(radius 12),
size curve 8→12; other timing/material/light/camera/seed settings match the isolated fixture.
Its raw report and original images remain in `target/fireworks-f8/particle-smoke/high`;
the report/configuration are included in the checked-in JSON. To reproduce that workload,
make those four receiver changes in the saved fixture (restore afterwards).

The light response is positive (3110 representative / 4030 selected / 4032 both pixels), but
gain-zero has a two-byte maximum difference, gain restoration nine, budget restoration ten,
rebind nine and expiry three; it fails the unchanged repeatability thresholds. This is consistent
with unsorted alpha overlap as GPU compaction/draw order changes, **not yet independently proven
or fixed by this slice**. The failed report's removal difference of 40 bytes used the wrong
visible tick-zero baseline; that metric is invalid as a leak diagnosis. Correcting the baseline
does not erase the other failures. The isolated-puff gate qualifies the material/provider API,
not this rejected dense workload. Next: fix/qualify dense alpha ordering before full-show migration.

Two earlier shader attempts failed (duplicate view binding; imports after lexical use). The
single-view contract and import-first composition fix those errors; unit contracts and the
subsequent native gates cover the corrected path. Thresholds were not relaxed.

## Costs and acceptance limits

Each family case records 40 warm-up plus 200 paused diagnostic updates. The JSON retains all
first/final raw pass medians/p95s/sample counts and the failed attempt. This tiny workload is
clock/noise/order sensitive. Off still runs selection, and samples are asynchronous/deduplicated,
not paired whole-frame measurements. **Do not sum/subtract medians to claim total incremental
overhead, full-show scalability, dense alpha performance or production-finale acceptance.**

Portable Sprite/Mesh/Ribbon shader source, reflection/fingerprint/resource-layout and HLSL/SPIR-V
validation pass. Native image qualification is **Sprite/default layers only**, not native Mesh,
Ribbon, 2D, arbitrary views/layers or hardware. Core/compiler/GPU tests, two provider contracts,
96 viewer tests (20 ignored), ten audio-enabled example tests, workspace check and scoped strict
Clippy pass. F8 remains in progress: dense ordering, smoke art/persistence, generic particle/fluid
injection, mixed-light prioritization and full-show/finale costs remain open.

[Raw reports, retained failure, source/capture hashes and 51 image comparisons](particle-smoke-lighting-2026-10-07.json).
