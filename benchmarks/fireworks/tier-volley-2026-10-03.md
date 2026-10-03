# F5F — explicit tier budgets and bounded overlapping volley, 2026-10-03

This implements portable authored density profiles and validates overlapping births in
normal **forward GPU playback without replay checkpoints**. It is not finale certification,
moving-footage approval or a replacement for the reference-image visual target.

## API and authoring contract

`EffectAsset::particle_budgets` is an optional map of tier names to `ParticleBudgetProfile`.
Each profile provides exact `emitter_capacity`, `event_count` and `trail_capacity` maps,
keyed by stable emitter, event-link and trail-renderer IDs. Select it through the existing
`EffectCompiler::with_tier` API, including resolved projects and their dependencies.
No matching profile preserves legacy counts; high remains authored. The optional field
round-trips through both flat assets and v4 documents without rewriting the original asset.

Targets must resolve and values must be positive and no greater than their authored maxima.
Names must be nonblank/unpadded and cannot be `high`. A zero authored trail capacity means
the parent pool; a profile overrides it with an explicit owner count. Budgeting happens on a
copy before plugin migration, lowering, portable requirements and resource admission checks.
Six overcommitted high-tier event lists still fail; a bounded explicit low profile passes,
but lowering pools without lowering expansion demand cannot waive the list budget.

Profiles change capacity and event demand together, not emission-module rates/bursts,
lifetimes, velocities, material parameters, output routes or host input counts. They do not
prove that demand fits pools. Hosts must still observe admission/overflow and budget their
own live inputs. Ordinary fan-out is not automatically multiplied by `QualityTier.particles`;
extension lowerers retain their existing relative-scale behavior. Tier switching requires
normal recompilation/replacement, not a new live runtime algorithm switch.

`SetParticleBudgets` is transactional and diff-visible. Emitter/event/renderer deletion
prunes dangling entries; undo restores exact profiles, including a combined profile edit
and deletion. Missing new targets are rejected, not silently pruned. Decreasing an authored
maximum below a profile requires a matching profile edit in the transaction. A dedicated
editor profile panel and automatic duplication policies are not implemented.

## Bounded workloads

| Fixture | Compiled particle capacity high / medium / low | Main-star demand high / medium / low |
| --- | ---: | ---: |
| Multi-break | 690 / 314 / 158 | 64 / 32 / 16 |
| Crackle | 1,458 / 570 / 222 | 96 / 48 / 24 |
| Crossette | 274 / 170 / 118 | 32 / 16 / 8 |
| Strobe | 306 / 186 / 126 | 192 / 96 / 48 |
| Secondary volley | 2,760 / 1,256 / 632 | 256 / 128 / 64 |

Medium/low reduce main and burst-smoke demand by 2/4, multi-break fan-out from 8 to 6/4,
and crackle fan-out from 12 to 8/4. Carrier and each of four crossette links remain count 1.
Strobe's phase/duty/cycle controls are unchanged. Trail-owner pools scale with their owners;
record counts and retention windows remain authored, including dead-parent tails.

`fireworks_secondary_volley.aestra.ron` starts four actual rockets in **one source pool**.
Their seeded 0.8–1.4-second lifetimes stagger first breaks, feeding shared main/smoke/secondary
pools, not separate copies of the runtime or host-timed spawning. Nine seconds covers cleanup;
launch smoke retains its short original emission window. This is four overlapping shells,
not the roadmap's 8–12-shell Test B or 20–40-shell finale.

## Native evidence

Base revision `e3a14e41` plus F5F changes. Raw local JSON/PNGs are ignored under
`target/fireworks-f5/`; capture manifests record dirty paths and source hashes. Windows,
RTX 4070 SUPER/Vulkan, dev build, 960×540, seed `0xf1e0000000000001`, semantic materials,
audience camera, HDR/Tony/exposure 0/bloom 0.15, pixel floors 0/0. Each uninterrupted run
uses fast unsorted transparency, playback-only history, manual 60 Hz, 120 warm-up and 600
measured frames. Interactive mode retains wall-clock time. These are one run per tier,
not repeated target-hardware certification.

| Tier | Demand = accepted: main / flash / smoke / secondary | Peak live / occupied / retired histories | Estimated effect buffer bytes | Paired simulation p50 / p95 / p99 ms |
| --- | --- | --- | ---: | --- |
| High | 256 / 4 / 192 / 2,048 | 2,012 / 2,281 / 1,177 | 10,862,400 | 1.265 / 2.091 / 2.145 |
| Medium | 128 / 4 / 96 / 768 | 766 / 885 / 437 | 4,224,820 | 1.373 / 2.073 / 2.082 |
| Low | 64 / 4 / 48 / 256 | 275 / 315 / 156 | 1,532,980 | 0.730 / 1.414 / 1.426 |

All three reports have 716 event readbacks, zero source overflow, expansion omission,
destination rejection, trail eviction/truncation, and 600 paired simulation samples with
measured zero checkpoint-capture bytes. Peaks/readbacks are asynchronous host observations,
not frame-aligned; demand includes warm-up. Buffer bytes are estimates, not total device VRAM.
Timing includes idle tail and varying dispatch work; do not sum stage percentiles or infer
monotonic speed gains, a full-frame budget, or arbitrary hardware support from these results.
`validate-tier-volley.ps1` checks this admission contract and rejects missing measurements,
fallback, wrong policy/tier/counts, drops, checkpoint work or non-decreasing estimated buffers.

The lifecycle runner passes nine cases / 72 PNGs: volley high close/audience/wide, volley
medium/low audience, and all four single shells at low/audience. These use opt-in stable
capture ordering, not the benchmark's fast mode. Expected main cohorts and four crossette
arms remain present; live/occupied/retired/eviction/truncation are measured zero at each
endpoint (frame 540 for volley, 420 for singles). Inspected audience volley high/low sheets
show pink main breaks, gold secondary clusters and later smoke; low is markedly sparse/dim.
Mechanism/cleanup checks do not establish recognizable AAA quality at every distance. No
goldens are approved. Native viewer shutdown emits readback-channel warnings after completion.

## Verification and reproduction

Core/compiler/authoring/viewer suites pass; viewer has 73 passed / seven exporters ignored.
Three budget compiler contracts cover round-trip/source preservation, invalid profiles,
legacy/high artifact equality and resource checks. Authoring has 41 command contracts.
Required native stateful conformance: 30 passed, including CPU/GPU event chains, crackle,
crossette, ordinal reuse and output delivery. Workspace check and warnings-as-errors Clippy pass.

The native single-shell host checker now derives parent demand from the compiled tier,
not high-only constants. Low multi-break/crackle/crossette checks all pass: respectively
16/24/8 secondary parent cues per initial and restarted run, distinct epochs 1/2/3, suppressed
seek reconstruction and complete remaining live cues. These are binding checks, not audio
playback or nested spatial-sound approval. Strobe's material flashes still create no cue route.

```powershell
cargo test --locked -p aestra-core -p aestra-compiler -p aestra-authoring -p aestra-viewer
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --test stateful_conformance -- --test-threads=1
# Use fresh paths; the bench viewer does not refuse overwriting existing reports.
foreach ($tier in @('high', 'medium', 'low')) {
    cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f5-secondary-volley --backend gpu --history playback-only --semantic-materials --camera audience --hdr --exposure 0 --tier $tier --gpu-bench "target/fireworks-f5/f5f-volley-$tier.json"
    if ($LASTEXITCODE -ne 0) { throw 'Viewer failed' }
}
pwsh -File benchmarks/fireworks/validate-tier-volley.ps1 -ReportsDirectory target/fireworks-f5
pwsh -File benchmarks/fireworks/capture-shell-review.ps1 -Shell secondary-volley -Camera audience -Response authored -Tier low -OutputDirectory target/fireworks-f5/volley-review-new
```

Next: **F6A**, a bounded 20–30-second `EffectClip` composition reusing these assets, with
multiple placements/seeds and the same tier/admission gates. Sound assets, audio scheduling,
skybox and host choreography integration stay host-owned. Moving-footage acceptance,
parent-oriented crossettes, nested spatial cue validation, lit/persistent smoke and true
hero/finale-scale resource/performance gates remain open. F5's bounded technical fixtures
are implemented; artistic and production-density acceptance are not complete.
