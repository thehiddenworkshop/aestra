# Independent host activation — 2026-10-28

The repeated launch stalls came from recompiling semantic material programs in
`PresentedEffect::new` for every independently spawned `EffectPlayer`. Nested
project clips already reused presentation templates; the host's individual
rocket wakes and detonations did not. Twenty-four constructions measured
30.2605–32.4748 ms each (mean 31.1828 ms). Main-world GPU preparation averaged
0.6554 ms and peaked at 1.8665 ms, so it was not the dominant activation cost.

`EffectPresentationCache` now shares immutable compiled programs by compiled
artifact identity. Automatic player preparation uses fresh instances from this
cache. The demo loads those same templates into its camera pipeline preparation
before starting gameplay. Each firework still has its own parameters, bindings,
resource overrides, seed, clock, output identity, simulation buffers and history.
Replacing an artifact with the same stable effect ID creates a new template.
Entries retire when the compiled artifact has no other owners; a loaded catalog
intentionally retains its prepared programs. There is no new owner-count limit.

## Native measurements

Six separate 40-second native window runs: baseline instrumented/ordinary,
shared instrumented, shared ordinary twice, and shared image review. Windows,
RTX 4070 SUPER/Vulkan, 1280×720 HDR/bloom, high tier, selected-star lights,
eroding-smoke candidate, playback-only history. The benchmark explicitly requests
24 owners; normal host admission still has no default ceiling. Native binaries
use the repository's dev profile (opt-level 1, dependencies 3), not a release
performance certification. No compilation ran concurrently with these runs.

Ordinary wall intervals include vsync. Phases follow actual host scheduling and
latest GPU population observations: launches through the last detonation,
remaining non-smoke particles, smoke/tail drain, then retired-owner idle. Unknown
populations count as active. Boundary intervals are tagged at observation time;
this is not exact GPU execution-phase attribution.

| Ordinary run | Launch p95 / p99 / max ms | Launch intervals >33.33 ms | Active sparks p95 ms | Smoke drain p95 ms | Idle p95 ms |
|---|---:|---:|---:|---:|---:|
| Baseline | 46.9901 / 48.2726 / 51.4739 | 23 / 333 | 17.5856 | 17.6461 | 17.0676 |
| Shared A | 17.6719 / 18.6051 / 19.1110 | 0 / 333 | 17.7099 | 17.5828 | 17.0473 |
| Shared B | 17.6121 / 18.6008 / 19.1078 | 0 / 333 | 17.7943 | 17.6862 | 17.0822 |

The separate instrumented run measures presentation construction at
0.0067–0.0148 ms (mean 0.01054 ms). Launch-phase CPU playback p95 falls from
31.0384 to 0.1347 ms. Main-world GPU preparation still peaks around 1.9 ms.
Observed launch-phase GPU simulation p95 is 3.5840/3.4232 ms before/after;
transparent-pass p95 is 0.9052/0.8970 ms. These asynchronous, sometimes nested
GPU scopes must not be added together or treated as frame-paired speedups.
The dominant removed work is CPU material compilation.

Startup compilation has moved to loading, not disappeared. Hosts that skip
preparation still pay first-use compilation once per distinct compiled artifact.
No simulation-buffer pooling, GPU-fence promise, replay optimization or generic
overload acceptance is implied. Whole-run ordinary maxima remain 31.5359 and
22.2768 ms; idle includes occasional pacing variation. This closes this workload's
repeated activation stalls, not a universal frame-time guarantee.

## Workload and visual checks

Every run completes twelve launches, twelve detonations and twelve representative
flashes. All star-path accepted/demand totals match exactly across all six runs;
source overflow, expansion omissions and destination rejections are zero. Final
gameplay/VFX owners, asynchronous live particles and selected capacity bound are
zero. Peak selected capacity remains 96, an admission bound rather than a live
light count. Encoding reports have no blocked owners or positive requested versus
encoded lag; this is CPU command encoding, not a GPU completion fence.

Five image requests at 4, 6.9, 12, 23 and 36 seconds were delivered. The prepared
view stays ready; the image lane records one global-cache pending update, while
ordinary/instrumented lanes record zero. Image work is excluded from ordinary
timing. Visual inspection preserves the gold/blue stars and retained wispy smoke.
Against the previous tuned review, scene-region mean absolute 8-bit channel
differences are 0.5722, 0.7855, 0.01152, 0 and 0 respectively, excluding the HUD.
These are asynchronous captures, not pixel-exact render equivalence or final
photographic approval.

Retained screenshots: [overlap](reviews/host-activation-2026-10-28/overlap-6.9s.png),
[retained smoke](reviews/host-activation-2026-10-28/overlap-12s.png),
[quiet](reviews/host-activation-2026-10-28/overlap-36s.png).
The [machine-readable summary](host-activation-2026-10-28.json) retains raw-report,
binary and source hashes, per-phase distributions, all activation timings,
admission totals and image comparison regions. Raw runs are under
`target/fireworks-f8/activation-{baseline-profile,baseline-cost,shared-profile,shared-cost-a,shared-cost-b,shared-images}-2026-10-28`.
Source snapshots explicitly distinguish native collection from later formatting,
test setup and ECS argument-grouping changes.

## Reproduction and validation

Build `aestra-demo-fireworks`, then use fresh output directories:

```powershell
& ./benchmarks/fireworks/run-smoke-look-review.ps1 -OutputDirectory target/fireworks-f8/activation-cost -Variants tuned -Cost
& ./benchmarks/fireworks/run-smoke-look-review.ps1 -OutputDirectory target/fireworks-f8/activation-profile -Variants tuned -Cost -Instrumented
& ./benchmarks/fireworks/run-smoke-look-review.ps1 -OutputDirectory target/fireworks-f8/activation-images -Variants tuned -SampleTimes '4,6.9,12,23,36'
```

The profile lane separates playback from render preparation and records each
standalone player's optional construction timing. Both profile and ordinary
reports expose phase distributions, so long quiet tails cannot hide launch p95.
Profiling is opt-in and does not add per-particle readback.

SDK tests cover shared program identity, independent runtime binding edits,
seed/clock isolation, same-ID replacement and cache retirement. The existing
camera fixture now initializes the same cache resource as `AestraPlugin`.
69 SDK tests and 49 demo/authoring tests pass, along with strict all-target Clippy
for the SDK/demo audio build and demo default build, formatting and diff checks.

Next: review and tune the default host's smoke visibility/depth at the normal
audience camera against the supplied video, including later-break illumination
of retained smoke. Keep ordinary transparency limitations and photographic
acceptance explicit. Larger finale workloads remain a separate qualification.
