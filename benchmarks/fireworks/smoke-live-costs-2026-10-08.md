# F8.1C1 — live smoke costs and saved-show baseline

2026-10-08, NVIDIA GeForce RTX 4070 SUPER / Vulkan, MSVC Rust 1.98.1,
repository optimized **test** profile. Cost collection qualifies; final smoke cleanup,
show migration and full F8 acceptance do **not** qualify.

## Workload and controls

Thirty fresh apps: two repetitions × high/medium/low × five cases. Native GPU runs
are sequential; the final matrix was collected after compilation/linting finished.

- Saved `fireworks_smoke_cohorts.aestra.ron`, one root: two real shell deaths,
  32 smoke + 32 light-bearing stars per break, one 96-slot smoke pool, 64 smoke alive
  during overlap. Four coincident roots stress 256 smoke particles in four pools;
  this is synthetic cross-draw overlap, **not saved choreography or sorting approval**.
- Matched no-smoke-draw controls compile the same validated asset, then remove only
  emitter zero's compiled renderer in test tooling. Simulation, capacities, seed,
  links, output routes, lights and zero-opacity launch markers remain identical.
  A saved emitter without a renderer/output is correctly rejected by validation;
  the test does not change that rule or author a dummy workaround.
- Cohorts use depth-back-to-front ordering, root scale 0.1, the public lab camera,
  selected caps 8/4/2 and representative cap 2 globally, also for four roots. Global
  light-budget rejection is retained separately from particle-link admission.
- The 26-second saved `fireworks_show.aestra.ron` remains unchanged and unlit. Its
  audience camera, scale 1, Fast ordering and representative caps 8/4/2 match the
  public host; selected lights are disabled. This LDR baseline excludes the public
  HDR/bloom/environment/audio and is not a persistent lit-smoke candidate.

All cases use a 960×540 target, seed `0xf83b000000000001`, playback-only history,
unpaced catch-up and fixed 60 Hz. A paused screenshot awaits shader readiness;
120 paused updates warm transport. Then cohorts run 1,080 live updates (18 seconds),
show runs 1,680 (28 seconds including its cleanup tail). No screenshot, seek or replay
is used inside the live window. New clip-first-use preparation during the show is
included; not all late clip shaders are prewarmed. Tooling sleeps 2 ms per update to
allow async delivery, outside the reported `app.update` wall interval.

The shared viewer render-graph scope is installed **before** plugin cleanup transfers
the render sub-app to its pipelined thread. GPU diagnostics deduplicate fresh arrival
timestamps. Separate context-valid simulation samples deduplicate `(owner, sequence)`
and retain fixed-tick/checkpoint work. No selected-position readback, synchronous GPU
wait or replay checkpoints are introduced. The public example imports none of this
test instrumentation; its saved fixtures and normal show are unchanged.

## Observations

Render-graph GPU p95 milliseconds, repetition 1 / repetition 2. These are separate
distributions, **not paired-frame incremental deltas**.

| Tier | One pool | One, no smoke draw | Four pools | Four, no smoke draw | Existing unlit show |
| --- | --- | --- | --- | --- | --- |
| High | 0.922 / 0.794 | 0.883 / 0.930 | 2.194 / 2.216 | 2.086 / 2.186 | 5.050 / 5.339 |
| Medium | 0.921 / 0.938 | 0.870 / 0.896 | 2.199 / 2.303 | 2.087 / 2.151 | 5.297 / 5.415 |
| Low | 0.945 / 0.974 | 0.899 / 0.935 | 2.217 / 2.258 | 2.083 / 2.180 | 5.270 / 5.454 |

The four-pool drawn case's simulation-pass p95 is 1.347–1.412 ms, alpha-sort p95
0.274–0.298 ms and transparent-pass p95 0.067–0.076 ms. Removing smoke draws also
removes their sort work, **not** their simulation. The single-pool high second run's
control is slower than its drawn run, demonstrating noise/clock variation: do not
subtract independent percentiles to claim a causal cost saving. Do not sum pass or
per-source simulation percentiles. Lower tiers deliberately retain identical smoke
density; changing selected-light caps alone is not a broad workload reduction.

All 24 cohort cases observe the exact 64/256 peak smoke population and 128/512 total
particle peak (smoke plus stars). Every root accepts 32 children on each of four links,
with zero source overflow, expansion omission or destination rejection. Both draw
variants retain these totals. The current show reaches six active clips and measured
particle peaks 1,680/648/234 at high/medium/low; these are asynchronous observed peaks,
not proof of an instantaneous authoritative census or production-finale density.

Sort ownership peaks at 28,368 bytes/three view-draw pairs for one drawn root and
113,472 bytes/twelve pairs for four. No-draw controls retain the two zero-opacity
alpha launch markers per root: 18,912/75,648 bytes. There are no late sort allocations
after update 60 at stable capacity. All owned sort buffers/pairs retire to zero on
owner removal. These are explicit buffer bytes, not total VRAM or driver-free history.
Observed checkpoint capture bytes remain zero. Complete raw distributions, counts,
budget rejection, source hashes and raw-report hashes are in
[the evidence JSON](smoke-live-costs-2026-10-08.json).

## Exposed final-tick cleanup gap — next F8.1C2

Every cohort variant ends at player frame 1,080 but stateful time **17.999897** instead
of exactly 18. Its GPU observation retains **one smoke particle per root**, with stars
and rockets at zero. The particle has visually faded; the earlier image gates proved
empty pixels, not zero simulated populations. These live reports explicitly mark
`smoke_drained: false`; cost collection success does not relabel this as cleanup success.
No seek extends the clock or masks the retained state. Owner retirement still passes.

The Bevy stateful playback branch repeatedly adds a float tick to instance time while
the player clock advances integer frames. At the Once end, the playhead stops and the
slightly-short requested time can omit the final GPU tick. Next: reconcile live clock
and instance time without changing cue/tick/loop/epoch/replay contracts, add a CPU
regression and requalify native zero-population cleanup. Then author an opt-in saved
show candidate and compare its persistent lit smoke under authored overlap. Cross-draw
transparency, natural smoke art, full-show persistence and fluid coupling remain open.

## Limits and validation

The render-graph scope includes simulation/render GPU work, not preparation, CPU/game,
audio or presentation. Async arrivals can cross the paused warmup boundary, and final
live results can arrive after sampling. Per-source simulation distributions are
individual instance observations, not simultaneous whole-frame totals. The full-show
window contains its silent tail. `app.update` wall time is not GPU completion time,
public release frame time or a whole-game CPU budget. No <=4 ms finale or migrated-show
budget is certified here.

Viewer regressions: 97 passed, 24 explicit/hardware/export tests ignored. Final native
cost matrix: 30 runs, passed in 221.66 seconds. Warnings-denied viewer all-target Clippy,
formatting and diff checks pass. Native fresh apps retain existing Vulkan overlay,
global logger and shutdown readback warnings; no WGPU validation error is accepted.
Preliminary runs are not included in the final measurements: they found invalid
saved no-renderer controls, missing timing instrumentation installed after cleanup,
and the final-population discrepancy. These were corrected or explicitly retained;
simulation/admission was not weakened to make the cost controls differ.

## Reproduce

Use a fresh absolute report directory and run native GPU workloads alone:

```powershell
$env:AESTRA_SMOKE_LIVE_REPORTS = 'C:\absolute\fresh\smoke-live-reports'
cargo +1.98.1-x86_64-pc-windows-msvc test --locked -p aestra-viewer live_smoke_overlap_costs_and_authored_show_baseline -- --ignored --nocapture --test-threads=1
```

The runner writes `accepted.json` as false before measurement and true only after all
30 cases pass. This flag means **cost collection qualified**, not F8/physical-drain/art
acceptance. Reports for the qualified run are under `target/fireworks-f8/smoke-live-final`.
The selected evidence JSON preserves portable summaries and SHA-256 identities.
