# F8.3B — real authored light outputs on smoke (2026-10-07)

Accepted **bounded actual-output qualification**, not full-show/finale or AAA-art acceptance.
Windows / NVIDIA RTX 4070 SUPER / Vulkan, development build. Machine-readable observations:
[`output-smoke-lighting-2026-10-07.json`](output-smoke-lighting-2026-10-07.json).

## Workload and API

`assets/test/effects/fireworks_smoke_lighting.aestra.ron` persists one fluid domain and two
material-free star emitters. Red breaks at 1.5 seconds; blue at 1.8 seconds. Each emits 32
particles before compile-tier scaling, lives two seconds and expands with spherical velocity.
Actual native `OnSpawn` packets drive two saved FirstPerTick point-light bindings: 1500-lumen,
6-world-unit, 0.6-second pulses. Packet magnitude does not multiply lights. Two saved scene
outputs select the brightest stars: 4/2/1 per emitter at high/medium/low, for total 8/4/2.
The host independently caps those families at two flashes and 8/4/2 selected lights.

The public `aestra-bevy --example fireworks -- --smoke-lighting` host loads/compiles/plays
this saved asset through normal project APIs. It opts into `AestraParticleLightPlugin` and
`ParticleLightMode::SameFrameGpu`; selected positions never cross to the CPU. Playback-only
is default; replay remains explicit opt-in. No viewer implementation is imported by the host.

This is **existing-source fluid smoke**, not particle/event injection. Neither emitter has
a renderer, so bright sprites, trails, bloom, walls, sky or host-created lights cannot counterfeit
smoke illumination. Camera is (0,4,16) toward (0,3,0); root scale is 0.1. The smoke has zero
fire/directional emission, 0.05 ambient, 0.02 scene gain, and a high-tier visit budget of 16
(12/8 at medium/low). Its solver grid is 32 cells before tier scaling, 48 march steps before
presentation scaling. Quality comparison is **within** each compiled tier, never a same-grid
performance comparison across tiers.

## Native controls

Frame 80: before either birth, adapter on/off is identical. Frame 115: both pulses overlap
and the selected particles remain alive. Compare both, representative-only, neither,
selected-only and restored-off without changing frame, artifact, epoch, seed or volume.
Reduce the host native cap to one slot, verify it still illuminates smoke, restore the original
cap and require identical selected-only pixels. At frame 240 both particle cohorts and
representative pulses have expired: adapter on/off is identical. Restart must clear the
existing smoke and lights to the original frame-zero image; another forward playback must
produce exactly the first overlap image and admit exactly two new representative bindings.
Owner removal must retire representative and native outputs (written capacity becomes zero).
Both GPU-free source/quality checks and the native no-position-readback assertions pass.

Positive pixels are any channel gaining more than 3/255; acceptance requires at least 100.
Negative controls and matched restart/budget restoration allow at most 1/255 difference.

| Tier | Flashes / selected ceiling | Representative pixels | Selected pixels | Both pixels | Native adapter logical bytes |
| --- | --- | --- | --- | --- | --- |
| High | 2 / 8 | 4018 | 5751 | 5762 | 832 |
| Medium | 2 / 4 | 4166 | 5871 | 5907 | 448 |
| Low | 2 / 2 | 4427 | 6068 | 6193 | 256 |

All before-birth, off restoration, expiry, restart, budget restoration, repeated smoke and
replayed overlap maximum deltas are **zero**. Native capacity observations are bounds, not
read-back active counts. Adapter bytes exclude selector scratch, Bevy allocation granularity,
clusters, volume textures and renderer memory. Portable readback submissions/staging/selected
position bytes and CPU light proxies are zero. Source validation reports no rejection or invalid
sources. Two final executions produce 16 PNGs per tier; all 48 corresponding PNG hashes match.

A third, stronger all-tier retirement run replays a fresh overlap, requires two active
representative lights and the identical combined image, then removes the owner while both
families are enabled. The resulting image matches the empty initial image exactly at every
tier, and native written capacity/representative activity are zero. Earlier runs removed an
already-disabled owner; they are not used as evidence for active-owner removal. The stronger
run produces 17 PNGs per tier in `target/fireworks-f8/output-smoke-active-retirement/` and
is also preserved in JSON. Compiled source-particle capacity is 64 at every tier. Within a
tier every lighting control uses one artifact; changing host light caps does not alter
its authored particle simulation. Capacity is not a live-particle count or cross-tier work proof.

## Matched costs, with explicit limits

Each case uses 40 settling updates followed by 200 paused updates, deduplicating asynchronous
GPU diagnostics by timestamp. The camera, compiled artifact, simulation frame, fluid field,
selector settings and smoke presentation are matched. Only native realization cap and
representative admission change. The selector stays enabled even in the off control: this
measures realization/presentation controls, **not total lighting-off savings**. Measurements
are milliseconds; do not add stage percentiles or treat them as whole-frame durations.

Second execution's transparent-pass median / p95 (roughly 197–200 unique observations):

| Tier | Neither | Representative | Selected | Both |
| --- | --- | --- | --- | --- |
| High | 0.071680 / 0.076800 | 0.043008 / 0.073728 | 0.123904 / 0.139264 | 0.060416 / 0.067584 |
| Medium | 0.075776 / 0.079872 | 0.074752 / 0.082944 | 0.105472 / 0.110592 | 0.071680 / 0.080896 |
| Low | 0.072704 / 0.077824 | 0.064512 / 0.082944 | 0.092160 / 0.100352 | 0.071680 / 0.078848 |

These tiny costs are strongly clock/order/noise-sensitive (including a lit case faster than
its unlit control). First execution's high both median was 0.084992 ms; second was 0.060416 ms.
The first run overlapped host build/window verification; the second ran after the example
exited. **No incremental overhead, production budget or speedup claim is accepted.** JSON
also preserves selector, injection and clustering observations. Dense live smoke, many-shell
overlap, mixed host lights and whole-renderer GPU costs still need their own matched gates.

## Failure that informed the fixture

An initial scene visit limit of 8 was smaller than the high-tier overlap's 10 active lights.
Smoke off/restart controls were exact, but the second combined lit image differed by up to
70/255 because cluster order could choose a different first eight lights after pool retirement.
Covering this fixture's complete overlap with visit limit 16 restores exact repeatability; the
thresholds and emitters were not weakened. This is **not a fix for general strongest-light
selection or mixed-host-light starvation**. The current marcher visits the cluster's first entries,
including inactive entries, up to the bounded budget. Larger shows must author lower selected
caps or implement/qualify a better bounded selection policy rather than blindly raising limits.
The gain was also reduced from 1 to 0.02 to avoid saturation hiding the individual response.

## Reproduce

```powershell
cargo test --locked -p aestra-bevy --example fireworks --features fireworks-audio
cargo test --locked -p aestra-viewer
$env:AESTRA_VOLUME_OUTPUT_REPORTS = 'target/fireworks-f8/output-smoke'
cargo test --locked -p aestra-viewer volume_light_images::outputs::authored_outputs_light_smoke_and_retire_without_position_readback -- --ignored --nocapture --test-threads=1
cargo run --release --locked -p aestra-bevy --example fireworks -- --smoke-lighting --no-audio
```

GPU-free suites: 96 viewer tests and 9 audio-enabled example tests pass; 5 example tests pass
without audio. Strict scoped Clippy and formatting pass. Public native 1280×720 window capture
at 1.92 seconds shows actual GPU playback and two active representative lights; evidence lives
in `target/fireworks-f8/bevy-smoke-lighting.png`. Expected short-run shutdown can emit Bevy
readback-channel-closed warnings after screenshot delivery; the process exits successfully.
Raw final/repeat images and reports are in `target/fireworks-f8/output-smoke-final/` and
`target/fireworks-f8/output-smoke-repeat/`. Images are not approval of final smoke/star art.

F8.3C explicitly lit **particle-smoke sprites**, F8.1 smoke art/persistence, F8.2 generic injection,
full-show selected-light authoring and dense mixed-light performance remain open. Default-layer
native adapter restrictions and unshadowed isotropic point-light scattering remain unchanged.
