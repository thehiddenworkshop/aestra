# F7F1 — explicit host lighting policy and tier qualification

Base: `e53cc097` (committed native private-lifetime milestone), plus the scoped
F7F1 worktree changes. Windows, RTX 4070 SUPER, Vulkan/NVIDIA, 960 × 540,
audience perspective, HDR/exposure 0/Tony, default layers, playback-only,
seed `0xf1e0000000000001`. **Implementation/cost gates pass; full F7F visual
qualification is partial. Low-tier full-show receiver visibility fails.**

## Host contract

`aestra_bevy::LightingQualityPolicy` is an explicit global host API. Its two
`LightQualityBudget` families independently control enable/count/lumen/range.
Example-host presets have representative caps 8/4/2 and selected caps 96/48/24.
Clamps remain 1,000,000 lm / 200 m to preserve existing authored appearance;
hosts can independently reduce either family. These are not universal device budgets.

`apply(&mut World)` validates before changing or creating any resource. It applies
the particle cap to selection, native realization and portable transport together.
Existing scratch/buffer/manifest/staging/request/age/lag budgets and authored binding
mode are preserved. It does not install plugins, switch transport/backend, recompile
effects, reset playback/history or enable shadows. A nonzero particle policy explicitly
opts the selector into authored particle-light work; realization plugins remain opt-in.
Disabled particle lighting zeros all three caps. Representative disable hides current
pulses and stops admissions; it need not destroy every reusable representative slot.
GPU/in-flight state converges through normal updates, not a blocking GPU wait.
Authored per-output caps remain upper bounds: a host policy cannot raise them.

The viewer uses this API instead of scattered count tables, retaining independent
CLI selector/GPU-control overrides and recording the requested policy in reports.
Quality name alone never changes a compiled artifact during live host switching.
The native GPU path still needs the pinned root Bevy PBR patch; it is not transitive
to external consumers. General layers/views and arbitrary-overload certification remain open.

## Repeated ordinary costs

Raw: `target/fireworks-f7/lighting-quality-costs-2026-10-05`.
Three alternating GPU/control pairs per hero/volley/show at medium and low:
**36 native processes, all exit 0**. Fixed 60 Hz, 120 warm-up frames, 600 measured
hero/volley frames and 1680 show frames. No allocator sampling in these timing runs.
Existing identity/compiled-capacity/seeds/admission/normal-render/selection/transport/
native-demand/log/cleanup gates are unchanged. The new quality gate checks the requested
policy and unchanged public index/offset fingerprints in every measured observation.
All show cleanup windows match control at one acknowledged native index. No selected
particle position readback or portable proxies are introduced.

Outer render-graph GPU p95 across the three repetitions, milliseconds:

| Tier | Workload | GPU adapter on | Matched control |
| --- | --- | ---: | ---: |
| Medium | Hero | 1.806–1.857 | 1.800–1.885 |
| Medium | Volley | 1.917–2.060 | 1.933–2.077 |
| Medium | Show | 7.476–8.231 | 8.105–8.457 |
| Low | Hero | 1.790–1.940 | 1.783–1.842 |
| Low | Volley | 1.526–1.546 | 1.521–1.540 |
| Low | Show | 7.965–8.531 | 7.660–8.083 |

Native clustering p95 is 0.127–0.150 ms; injection p95 0.012–0.022 ms.
These are independent, unpaced run distributions, not paired-frame differences,
statistical significance, whole-game frame time or a guaranteed tier speedup.
Different compiled tiers also change authored particle/event workloads; cross-tier
render differences do not isolate the light cap. The show result is not a production
finale or proof of the provisional ≤4 ms finale VFX target.

## Separate allocation qualification

Raw: `target/fireworks-f7/lighting-quality-allocations-2026-10-05`.
Another **36 sequential native processes** pass the unchanged work/cleanup/public
no-churn gates with allocator sampling enabled. These are not timing evidence.
The private gate is extended to the existing medium/low initial capacities, keeping
the fixed one-view 17×9×24 grid and exact allocation size/count checks.
Every measured census has one Z buffer (medium **24576 B**, low **12288 B**),
one scratchpad (**117504 B**), one metadata buffer (**48 B**) and **2–4** 48-byte
staging buffers, below the enforced per-view eight-slot ceiling. Missing/private
unknowns are rejected, not converted to zero. This is sampled live allocation proof,
not exact private generation IDs, driver free history, total VRAM, arbitrary-load
hard memory bounds or general multi-view/layer certification.

## Native images and the retained failed gate

The original sparse image attempt is kept at
`target/fireworks-f7/lighting-quality-images-2026-10-05`: medium accepted;
low fails full-show response. All 240 PNGs and the exit-101 log remain intact.
The early harness left a low `accepted:false` sentinel rather than full failed samples.

The expanded attempt is kept separately at
`target/fireworks-f7/lighting-quality-images-expanded-2026-10-05`.
It preserves every original frame and adds show frames 85/110/150 for **both**
new tiers, covering the first shell's authored 1.3-second death/early-star phase.
No lights, particle materials, caps, geometry or response thresholds were boosted.
The harness now writes the complete response report before asserting visibility.

Each tier has **46 samples / 138 PNGs**: hero, volley and show, off/on/off triplets,
with bloom 0 and 0.15. Existing requirements remain ≥100 diffuse receiver pixels
with >3/channel positive response and ≥1000 summed positive energy in at least
one active sample per workload/bloom mode. Before-birth/after-cleanup images match;
every restored control has zero significant changed pixels (>3/channel).

Medium passes all six workload/bloom response gates. Low hero and volley pass,
but the low show fails both modes even with the added early samples. Best low-show
energy without bloom is **790 / 152 pixels at frame 180**; with bloom it is
**335 / 32 pixels at frame 1110**. This is measured light contribution below the
unchanged visibility gate, not absent rendering or a successful AAA/art gate.
The second native image test also exits **101**, and its low report explicitly
records `accepted:false` with all samples. A third, provenance-qualified reproduction
at `target/fireworks-f7/lighting-quality-images-final-2026-10-05` reproduces the same
responses and failure, with source/test-binary hashes recorded before execution.
It retains another 276 receiver PNGs, seven live-host PNGs and the passing read-only
PNG/report integrity log. All three low-show attempts remain rejected.

A separate live-host test passes at `.../host-switch`: one compiled high-tier hero
at frame 85, policies 0 → 96 → 48 → 24 → 0 → 96 → 0, representative budget unchanged.
The compiled Arc, frame/epoch/seed/history and observed alive/trail profiles remain
identical. All three nonzero caps illuminate the receiver; disable returns to the
baseline and re-enable restores the first high image without significant changes.
Seven PNGs are retained. The 24-cap image contributes **342 pixels / 1522 energy**.
All same-frame samples have no selected-position readback or portable proxy work.

The final live-host reproduction yields the same response values. Its test binary
is SHA-256 `fa339a94608ef00fa6dc10de5c0e01e6eae017a1b9a64391dc3afe7d0b717bb4`;
the ordinary and allocation matrices pin the production binary
`382294451d5df74d7045019f0cac6396e3cc0aa772088460a08f053cc971c882`.
Earlier image binary hashes were not recorded. The final source snapshot is not
retroactively claimed as a pre-cost-build snapshot. [Machine-readable evidence](lighting-quality-2026-10-05.json)
records response gates, source hashes, raw-tree digests and manifest/report/log hashes.
All 144 matrix report/log hashes still match their retained manifests, and the final
12 source hashes/test-binary hash still match their pre-image input manifest.

Thus the host API can lower lighting independently of particle density. The compiled
low-show fixture combines sparse particles and authored per-output light caps of eight
(versus 32/16 at high/medium), under a global 24 cap. The live-switch result does not
establish a single cause or justify silently overriding those authored bounds. General
natural-star tracking, additional hardware/cadences, editor lighting, lit smoke and
finale art/cost acceptance are still separate gates.

## Verification

Bevy unit tests: **60 passed**; viewer unit tests: **95 passed / 15 ignored**;
Bevy doctests: **2 passed**. Scoped all-target Clippy with `-D warnings`, workspace
all-target checking, formatting and both validator self-tests pass. Both 36-run
matrix gates, native live-host switching and retained PNG/report integrity pass.
The tier-image generator fails with exit 101 on all three attempts specifically
because low-show receiver visibility is below the unchanged gate. The integrity
test confirms that rejection; it does not convert it into visual acceptance.

## Reproduction

Run native jobs sequentially on an idle GPU; use fresh directories and retain failures.

```powershell
cargo build --locked -p aestra-viewer
& benchmarks/fireworks/run-particle-light-costs.ps1 -ReportsDirectory target/fireworks-f7/new-quality-costs -Tiers medium,low -RequireRetirement
& benchmarks/fireworks/validate-lighting-quality.ps1 -ReportsDirectory target/fireworks-f7/new-quality-costs
& benchmarks/fireworks/validate-lighting-quality.ps1 -SelfTest
& benchmarks/fireworks/run-particle-light-costs.ps1 -ReportsDirectory target/fireworks-f7/new-quality-allocations -Tiers medium,low -AllocationSnapshots
& benchmarks/fireworks/validate-cluster-private-allocations.ps1 -ReportsDirectory target/fireworks-f7/new-quality-allocations
$env:AESTRA_GPU_LIGHT_IMAGE_REPORTS='target/fireworks-f7/new-quality-images'
cargo test --locked -p aestra-viewer --bin aestra-viewer particle_light_images::quality_tiers_illuminate_receivers_and_restore_controls -- --ignored --nocapture --test-threads=1
# Current low-show visibility failure is expected; full failed report is retained.
cargo test --locked -p aestra-viewer --bin aestra-viewer particle_light_images::live_host_quality_policy_preserves_particles_and_recovers_lighting -- --ignored --nocapture --test-threads=1
# Integrity only: recomputes both accepted medium and rejected low reports from PNGs.
cargo test --locked -p aestra-viewer --bin aestra-viewer particle_light_images::retained_quality_receiver_images_match_the_reported_gates -- --ignored --nocapture --test-threads=1
```

## Next

F7F2: choose and validate the low-tier authored lighting profile—either explicitly
representative-only lighting, or a useful selected-star contribution within an explicit
global budget. Do not mask failure by loosening image thresholds or silently increasing
range/lumens/counts. Any profile change needs fresh work-matched costs and receiver controls.
Then F8.3: make smoke consume scene lighting. Optional shadows remain off and are not a
prerequisite. F7F1 implements the host API; **F7F as a whole is not marked complete**.
