# F8.1C4C — native cross-pool limits and compositor experiment decision

The previous algebraic interleaving counterexample is now a native GPU probe. Both
`Fast` and `DepthBackToFront` fail the global source-over reference. This slice adds
diagnostic tests and evidence, **not a transparency fix or smoke-art approval**.
Default, C3 and wispy show assets, public runtime settings and capacities are unchanged.

## Setup and controls

`apps/aestra-viewer/src/volume_light_images/cross_pool_limits.rs` uses ordinary compiled
effects and GPU `EffectPlayer`s with `PlaybackOnly`, not injected presentation buffers.
It shares the existing red/blue half-alpha fixture. There are no lights, bloom or
tone mapping; the target is 480×360 sRGB LDR. Central 8×8 pixels are decoded to linear
before comparison. The sprite's normal soft footprint remains in use; its central
coverage must recover the nominal alpha within .01 rather than assuming the entire
disc is flat. CPU project evaluation supplies the deterministic reference depths;
it is test tooling, not a production sorting or particle-tracking path.

Each case captures both pools, red only, blue only and both restored. Calibration
rejects missing/incorrect alpha, including black captures. Restoration must differ by
at most one byte over the whole image. Sort statistics must prove two dispatched
view/draw pairs in `DepthBackToFront`, and none in `Fast`. Both camera directions
(z=+16/-16) and both player spawn orders run in fresh apps under each mode.

Environment: Windows, Rust 1.98.1, Bevy 0.20.0, RTX 4070 SUPER, Vulkan. Raw reports,
source hashes and all 136 repeated-image hashes are archived in the companion
[`JSON evidence`](cross-pool-transparency-2026-10-23.json). Two final fresh runs have
identical corresponding PNGs. Image equality verifies repeatability only; correctness
comes from the analytic blend and positive controls below.

### Interleaving: one red pool straddling blue

The red pool bursts two particles into a thin box with z extent ±4 and negligible x/y
extent. A deterministic seed is selected by the ordinary reference sampler so one
particle is on each side of blue. Red's fixed host z=.25 and blue's z=0 avoid equal
pool-center ties; actual red reference depths are approximately +1.1310 and -3.4330.

All three particles have nominal alpha .5. Exact far-red / blue / near-red compositing
is linear `(R=.625, B=.25)` from either camera. Combining the two reds into a single
draw gives alpha .75, so either whole-draw order must be wrong. A positive control
hides the two-particle red pool and creates two ordinary single-particle players at
the reference depths. This allows the existing whole-draw sorter to reproduce the
correct interleaving without altering any shader or blend state.

| Capture | Maximum linear reference error |
| --- | ---: |
| Grouped red pool, camera +Z | .1252282 |
| Grouped red pool, camera -Z | .2487378 |
| Split-draw positive control, either camera | .0010396 |

These results hold for all eight mode/spawn/view combinations in both repetitions.
Grouped images match one of the two whole-draw predictions within .004596. Single-pool
calibration also stays within .004596. Visibility restoration is byte-exact, including
the central comparison after hiding the split controls and restoring the original pool.

The report says `probe_passed:true`, **`renderer_correct:false`**: the expected limitation
was measured successfully. Increasing per-pool sort capacity or improving its center
cannot fix this counterexample.

### Moving local depth with fixed hosts

Red's host stays at z=-2 and its particle moves at +2 units/s with zero spread, drag,
gravity and turbulence. Blue's particle/host remain at z=0. Forward fixed-step playback
captures frames 30/90/150 (0.5/1.5/2.5s), giving red reference depths -1/+1/+3. Both
players must reach the exact frame; no backward seek/replay is used for this sequence.

All eight pre-crossing captures match source-over within .003067. All **16 post-crossing
captures fail**, with maximum linear error .249842, under both modes and both cameras.
Calibration remains below the unchanged .01 threshold (maximum .008980); restoration
is byte-exact. Red-only coverage grows monotonically from +Z (14,712 → 18,924 → 25,196
pixels above red byte 100), and shrinks from -Z (18,924 → 14,712 → 11,796). This extra
control rejects a frozen/nonmoving particle as an explanation for unchanged pool order.

`GpuDrawInstance::extracted` uses the transformed `Aabb` center for the phase sort key;
the sprite bounds created by GPU preparation have center zero. They are conservative
visibility bounds, not current GPU particle centroids. Their fixed center explains
why the draw order does not follow this particle crossing. CPU readback of centroids
is neither introduced nor proposed as the global solution.

### Retained initial failure: coincident centers

The first interleaved fixture put both host centers at zero. Its split-draw reference
was correct, but four reversed-spawn cases changed whole-draw order after visibility
restoration, producing a maximum byte difference of 88. That failed report and hash
remain recorded, not relabeled as a pass. The final fixture separates centers by .25
while preserving particle interleaving, so it isolates the global-compositing failure
from equal-depth tie ordering. **No runtime tie-order stability fix is claimed.**

## Bounded diagnostic cost, not production performance

The two-draw/three-particle interleaved scene is frozen after its original capture.
After 40 warmup updates, each repetition collects 200 distinct GPU timestamp samples
for the representative +Z/nonreversed case, before adding split controls.

| Metric | Fast, runs b/c | DepthBackToFront, runs b/c |
| --- | --- | --- |
| Transparent pass median / p95, ms | .003072 / .004096, both | .003072 / .004096, both |
| Alpha-sort median / p95, ms | no dispatch | .031744 / .033792; .032768 / .034816 |
| Sort-owned buffers | 0 | 18,912 bytes, two view/draw pairs |

The minimum sort pages make this deliberately tiny workload a poor scaling benchmark.
Timestamp quantization, empty-light infrastructure and capture pumping remain present.
This measures the existing path only: it is **not** a full-show frame percentile, live
paced cost, OIT comparison, or evidence that any replacement compositor is faster.

## Decision: prototype bounded fragment-list compositing, do not adopt yet

The native measurements rule out whole-pool sorting as the correctness solution.
Keep the current opt-in per-pool mode as a control; do not spend another smoke-art
iteration tuning bounds centers or substituting additive smoke to hide the failure.

The next experiment should integrate Aestra's 3D Alpha sprite/flipbook path with
**Bevy 0.20's bounded per-pixel fragment-list OIT**, behind an explicit opt-in, and run
this same oracle. This is an experiment choice, not a production architecture or
performance acceptance. Source inspection of the pinned Bevy implementation shows:

- `bevy_core_pipeline/src/oit/mod.rs`: linked-list collection plus fullscreen resolve,
  configurable exact-sort count and average fragment capacity; no MSAA support.
- `oit/draw.wesl`: bounded global node allocation drops fragments on exhaustion;
  packed RGB9E5 color and 24-bit depth / 8-bit alpha add quantization.
- `oit/resolve/oit_resolve.wesl`: fragments beyond the local exact-sort count are
  blended approximately. It is **not weighted blended OIT** and not universally exact.
- Bevy's 0.20 material migration guide requires explicit shader participation and
  premultiplied input. Aestra's custom pipeline currently emits its own ordinary color
  target and has no OIT specialization. A camera component alone will not integrate it.

At Bevy's default average four fragments/pixel, 12-byte nodes plus 4-byte heads alone
cost roughly 8.57 MiB at 480×360 and 102.83 MiB at 1080p, before ancillary buffers.
This is a source-derived allocation estimate, **not a measured Aestra OIT allocation**.
Memory, saturation and overdraw therefore need explicit budgets; do not enable it on
all cameras by default or replace an existing hard limit with an unbounded allocation.

Required prototype gates:

1. Both native oracles match source-over within the existing .01 tolerance when the
   fragment budget is sufficient, including restoration, spawn orders and cameras.
   Change the expected-failure assertions only when the opt-in implementation exists;
   retain baseline failure measurements.
2. Exercise insufficient global capacity and the local-sort threshold deliberately;
   expose allocation/overflow or another trustworthy saturation signal. Report loss
   or approximation, not a false correctness pass.
3. Check ordinary opaque-depth occlusion, HDR/lit semantic materials, resize, multiple
   cameras and explicit MSAA compatibility. Define how non-OIT sparks/trails/additive
   draws interact with smoke; a post-pass cannot silently claim correct mixed ordering.
4. Compare baseline/prototype GPU time and memory on identical frozen **and live paced**
   overlapping smoke workloads, then on the unchanged saved public show. No CPU particle
   enumeration/readback or unbounded per-particle draw expansion in the runtime.
5. Decide to retain, revise or reject that compositor from those measurements. Weighted
   OIT is an approximate fallback experiment if fragment-list costs are unacceptable;
   exact global GPU ordering across heterogeneous draws is a larger alternative, not
   an automatic consequence of increasing the existing per-draw sort budget.

After the compositor experiment, resume continuous star-path smoke and shared-wind
haze art using the generic distance-emission API, then review public HDR bright/quiet
phases and matched paced costs. Existing rocket deposition remains intact. None of
this promotes the wispy draft to the default or completes F8.

## Reproduce

Use a **fresh absolute output directory** for each repetition, and run native GPU
workloads alone/sequentially:

```powershell
$env:AESTRA_CROSS_POOL_IMAGES = 'C:/absolute/fresh-cross-pool-run'
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-viewer cross_pool_ --locked --offline -- --ignored --test-threads=1

$env:AESTRA_CROSS_DRAW_IMAGES = 'C:/absolute/fresh-separated-run'
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-viewer separate_alpha_pools_match_source_over_from_both_views_and_spawn_orders --locked --offline -- --ignored --test-threads=1
```

Final checks: viewer ordinary suite 101 passed / 30 ignored; new native probes passed
in two fresh runs; existing separated oracle passed; strict viewer all-target Clippy
and workspace formatting passed. The sandbox initially denied an unrelated temporary
fixture operation; the full viewer suite passed with fixture access. Workspace-wide
tests, Linux and public HDR show/art runs were not repeated for this test-only slice.
