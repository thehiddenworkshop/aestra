# F9: simulation phase sampling and event-spawn pass consolidation

The shared renderer now offers bounded, opt-in GPU timing for particle ticks,
event routing, presentation and trail recording. The first diagnostic run
identifies event routing and presentation as the largest sampled categories.
The shared `DomainSpawnPipeline` now encodes its plan and indirect spawn in one
compute pass instead of two. Both dispatches, acceptance counters, empty-work
decisions and route order remain intact. No CPU assumption suppresses GPU events.

This removes one pass boundary per event/domain/input spawn. In the repeat low
profile, call-site accounting observes 51,840 such spawn calls: the corresponding
encoder path uses 51,840 passes instead of 103,680, while preserving 103,680
dispatches. These counts cover observed latest-ledger windows, not every frame
or a driver trace. There is **no demonstrated GPU speedup** from this change.

## Reusable diagnostics and editor benefit

`aestra_bevy::gpu::GpuSimulationPhaseTimings` is shared by all native hosts and
available for editor profiling. It is disabled by default. Sampling rotates one
category per render frame, with at most 1,024 timestamp pairs and three in-flight
slots. Unsupported timestamps/busy slots skip diagnostics without blocking the
simulation; these are diagnostic bounds, not owner or particle limits. Latest
results carry phase/sequence identities for deduplication. Invalid timestamps,
readback failure and exceeded pair capacity produce unknown totals, never a
measured prefix presented as a complete frame.

The demo review runner exposes `-Cost -Instrumented -SimulationPhases`, explicitly
labels that diagnostic lane, and restores the environment afterward. Reports put
the breakdown in `profile.simulation_phase_sampling`. Ordinary image/cost lanes
explicitly disable this sampler, even when the ambient environment enables it.
The sampler adds marker passes, so its aggregate times are not performance
comparisons against runs without those markers.

One low-tier diagnostic run, including idle frames, returns:

| Sampled category | Frame-total GPU p95 (ms) | Completed windows | Partial windows |
|---|---:|---:|---:|
| Particle ticks | 2.551 | 469 | 0 |
| Event routes | 3.871 | 469 | 0 |
| Presentation | 3.642 | 470 | 0 |
| Trail recording | 1.537 | 470 | 0 |

Each total sums disjoint segments of the selected category within one frame.
Categories sample different frames: **do not add their percentiles**. These
windows exclude domain advancement, analytic/trail preparation, non-trail
checkpoints and other graph work. Zero-segment idle windows are included. The
result prioritizes routing/presentation investigation, not a complete attribution
of the whole simulation interval. Repeated sibling Bevy diagnostic labels alone
would expose only the latest individual measurement rather than this total.

Regular editor effects use the same consolidated spawning path. Rebuilt native
editor-viewport captures of Smoke Wake Lab (six times) and Smoke Material Lab
(four times) are RGB byte-identical to the preceding spawn-dispatch runtime.
The frozen smoke inputs and all 97 original showcase asset hashes are unchanged.

## Timing observations and unresolved regression

Two sequential low control/candidate observations use optimized release,
RTX 4070 SUPER/Vulkan, audience 1280×720, PlaybackOnly, HDR/bloom, audio off,
Msaa::Off, 4×4 OIT and the existing health scans. No screenshots, builds or phase
marker passes run during these comparisons. Control includes the new disabled
sampler hooks; the comparison isolates the spawn-pass change. The first candidate
predates a collector parameter-tuple refactor; its executable hash remains in the
raw manifest. The repeat and all image checks use the preserved final executable.

| Low phase | Control graph p95 range (ms) | Candidate graph p95 range (ms) | Control simulation p95 range (ms) | Candidate simulation p95 range (ms) |
|---|---:|---:|---:|---:|
| Launch / overlap | 13.922–16.026 | 14.842–14.865 | 8.494–10.141 | 8.385–9.328 |
| Active fireworks | 17.636–18.614 | 17.835–18.056 | 12.371–14.914 | 12.859–13.799 |
| Smoke drain | 13.019–13.214 | **14.774–14.991** | 11.355–12.312 | 10.303–11.014 |
| Idle | 2.221–2.233 | 2.229–2.245 | 0.004–0.004 | 0.003–0.004 |

Active CPU simulation encoding p95 is 3.634–3.920 ms for control and
3.571–3.595 ms for candidate. The observations are too few and variable to claim
a causal speedup. The broader smoke-drain graph interval is consistently higher
in these two candidate runs despite lower simulation timings. Its OIT parent
scope also rises (repeat 1.697 → 3.472 ms), without comparable changes in the
individual partition-scope percentiles. The cause is unresolved; do not infer
an end-to-end benefit from fewer pass boundaries or subtract/add nested quantiles.

All active graph observations still miss the 16.67 ms reference interval.
Presentation latency, all uploads, full-game CPU budgets, medium/high timing and
other devices remain unqualified. The scheduling change is a validated reduction
in encoded pass structure, not a passed performance milestone.

## Frozen workload and retained failures

All three fresh candidate image lanes finish 32 launches/detonations, 64 peak
owners, unrestricted admission and complete final cleanup. No source overflow,
destination rejection, expansion omission or truncated audit is observed.
Accepted event births match the preceding runtime:

| Tier | Gold ember births | Star-path smoke births | Maximum dropped fragments |
|---|---:|---:|---:|
| Low | 259,644 | 78,354 | 0 |
| Medium | 505,062 | 154,151 | 0 |
| High | 1,015,758 | 309,714 | **584,990 — failed** |

All 33 requested demo captures are retained. The strict runner rejects high's
capacity result after preserving its completed report. Node/head storage remains
47,923,200 bytes under the same 128 MiB guard; this is not total VRAM. Representative
light budgets still produce different accepted flash counts (15/28/32 in these
image runs), explicitly separate from particle-event loss. Independent forward
demo captures are not frame-exact light/flash comparisons.

## Validation and evidence

All 23 native coupled regressions pass, including the two explicitly hardware-only
field/placement cases. They cover chained births, domain/input routes, fractional
scheduled emission, event admission, cached parameters, pause, rewind and retired
trails. The extended native timestamp regression verifies slot recycling and a
successful partial readback whose omitted demand survives transport. The phase
summary regression rejects partial/invalid windows. Strict renderer/demo Clippy,
workspace formatting, PowerShell parsing and diff checks pass.

The [ledger](f9-event-passes-2026-10-30.json) fingerprints source inputs,
executables, reports/manifests, 33 demo captures and ten exact editor matches.
See the demo README for the diagnostic command. Keep smoke/workload/storage
frozen. Next investigate the broader graph/drain discrepancy and remaining
event/presentation work; high-tier fragment capacity remains a separate failure.
F9 and final F8 visual acceptance remain open.
