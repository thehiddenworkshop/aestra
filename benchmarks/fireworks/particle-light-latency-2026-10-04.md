# F7E2 — paced fast-star registration

Measured 2026-10-04 against F7E1 (`2f16d41f`), RTX 4070 SUPER / Vulkan. The adapter
reports driver `NVIDIA`, not a numeric driver version. Two sequential native runs
reproduced approximately two frames of lag; the second run is the retained
[measurement](particle-light-latency-2026-10-04.json).

**Result: measurement complete, flagship fast-star acceptance failed.** This is
a spatial registration problem, not evidence that the bounded copy or pool is
too expensive. The portable adapter remains opt-in; it is not approved for
nearby fast-moving firework lighting.

| Star speed | p95 image lag at 60 Hz | p95 world offset | Registration budget |
| --- | --- | --- | --- |
| 25 m/s | 2.035 frames / 33.91 ms | 0.848 m | 0.417 m |
| 75 m/s | 2.061 frames / 34.35 ms | 2.576 m | 1.250 m |
| 150 m/s | 2.052 frames / 34.20 ms | 5.130 m | 2.000 m |

The explicit initial registration budget is p95 no more than one 60 Hz frame
**and** one-quarter of the authored eight-metre light range. This is a proposed
flagship engineering criterion, not a universal perceptual threshold or human
approval of the artwork. Even at the lightest selected workload, the async path
fails it. At 150 m/s, the green receiver spot visibly detaches from the red star.
The synchronized control's maximum residual error is 0.112 pixel across all
speeds, versus approximately 15 pixels of async offset.

## Measurement method

`bevy/aestra-bevy/tests/particle_light_latency.rs` is an explicit ignored native
probe. It keeps Bevy's pipelined rendering enabled and uses the canonical Aestra
GPU playback and particle-light plugins, with playback-only history. One alive
star and one selected light isolate registration from density/selection cost.

- Continuous forward playback times are supplied in 1/60-second increments.
  External deadline pacing targets real 60 Hz; no GPU polling/waiting or replay
  reconstruction is added. The source is paused only for pipeline warmup and
  screenshot callback draining. The second phase uses a later forward interval
  and a settled root translation, not a rewind.
- A red HDR GPU-rendered sprite and green shadowless light illuminate a neutral
  rough PBR plane. Orthographic top-down framing eliminates horizontal parallax.
  Fixed EV0 / Reinhard, no bloom, no ambient and no flash isolate the two signals.
  The star is eight HDR units and approximately eight pixels wide. This is a
  diagnostic scene, not production fireworks art or a smoke receiver.
- The control is one **test-only** ordinary Bevy light synchronized to the known
  straight-line trajectory at the same main-world playback time. It proves that
  normal Bevy light/sprite extraction can register without the async round trip.
  It is not a proposed CPU particle-sampling implementation.
- Each speed has 60 moving updates per phase and ten final-image observations.
  Red-only and green-only weighted centroids are compared **within the same
  tonemapped image**; neutral pixels/noise are excluded. A missing/black signal
  fails instead of producing a zero-lag result. Control mean bias is subtracted.
  The resulting offset includes the actual visible receiver response and sprite
  occlusion; it is not an exact decoded light-center coordinate.
- Screenshot request ticks label observations. Callback arrival times are never
  assigned to captured-image positions. This measures final rendered target
  registration, not monitor scanout, input-to-display latency or window vsync.
- Raw telemetry records main update intervals, wall update durations, pool
  liveness and bounded transport. Reported accepted age/frame lag are the last
  accepted snapshot's statistics, not an independently paired GPU render frame.
  These lifetime statistics can remain nonzero after disable; control summaries
  therefore leave their age/lag fields null.

Retained mean moving intervals are 16.662–16.673 ms; p95 is at most 17.315 ms.
Async main-update p95 is 1.161–1.295 ms (not total GPU time or a pool-only cost).
Main-world accepted-set age p95 is 31.66–31.95 ms, and accepted frame lag is two.
One proxy, three 64-byte staging slots, no failed maps or expired packets are
observed. These resource checks supplement, not replace, F7E1 full-show profiling.

## Reproduction

Run native GPU work alone, with no competing test, viewer or build workload:

```powershell
cargo test --locked -p aestra-bevy --test particle_light_latency paced_fast_stars -- --ignored --nocapture
./benchmarks/fireworks/validate-particle-light-latency.ps1 -ReportsDirectory target/fireworks-f7/particle-light-latency -MeasureOnly
```

The test generates registration/telemetry/metadata CSVs plus sixty PNGs under
`target/fireworks-f7/particle-light-latency`. The committed JSON retains raw CSV
hashes and the tick-28 control/async PNG hashes for each speed. The ordinary
centroid regression runs without the ignored/native test:

```powershell
cargo test --locked -p aestra-bevy --test particle_light_latency
```

The read-only validator verifies sample identities, moving image trajectories,
control registration, cadence, native setup, liveness and resource budgets.
Without `-MeasureOnly`, it deliberately exits nonzero on the failed flagship
registration budget. A passing measurement harness must not be described as a
passing perceptual gate. Raw output files are generated evidence, not committed
reference artwork.

## Next: F7E3 — same-frame GPU lighting proof

The measured blocker justifies a narrow Bevy-specific render-world prototype
which consumes the selected GPU light records without the GPU→CPU→main→GPU round
trip. First prove one fast moving selected light on this ordinary PBR receiver,
using this same registration gate. Then preserve global/source/byte budgets,
identity/layers/lifecycle and integrate bounded selection into Bevy's clustered
light representation; remeasure authored hero, volley and full-show cost.

Do not stall for current-frame readback, loosen expiry/registration thresholds to
make the test pass, silently extrapolate trajectories, or put Bevy clustering in
engine-neutral core. Keep the portable async adapter available for tolerant
workloads and representative flashes independent. Production-finale density,
additional hardware/cadences, perspective/bloom artistic acceptance, editor
lighting and lit smoke remain open.
