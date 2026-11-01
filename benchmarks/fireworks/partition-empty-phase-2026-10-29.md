# Empty partition phase retirement — 2026-10-29

An admitted view with **no queued transparent phase items** now clears its shared
heads/counter once and performs **one full-target GPU reset audit**. It skips
all per-region capture, link-sort and resolve passes, including their depth
bind-group preparation. A nonempty phase continues through the original path,
even when an asynchronous particle count or indirect draw might currently be zero.
This is a phase-ownership optimization, not density or visibility approximation.

The audit still uses the bounded asynchronous readback pool and real GPU
counters. No clean CPU result is fabricated, no blocking wait is added, and
backpressure remains unknown rather than success. `sample.empty_phase` identifies
the single full-target audit row; normal samples keep one row per region.
Fresh empty results cannot erase sticky earlier loss. The node/head allocation,
128 MiB ceiling, owner policy, admission and default ordinary rendering are unchanged.

## Correctness controls

Three explicit native executions pass all **36 exact active-image comparisons**
and additionally match their PNG pixels against the preceding binary's frozen
controls. All nonempty demand/drop checks remain unchanged. Each execution
checks three repeated hidden-phase audits from each camera direction, compares
them exactly with ordinary opaque/background rendering, and then restores visible
draws and ordinary shared lists. These are 18 repeated empty-image controls.
Six separate forced-loss → empty controls confirm that genuinely zero new GPU
counters preserve the prior observed-loss flag. Existing odd-target resize,
foreign/invalid-view rejection and overflow/recovery controls remain in place.

The frozen native comparisons are SDR captures of HDR/Reinhard rendering; they
do not replace the separate raw-HDR oracle. Live screenshots and GPU diagnostics
remain asynchronous and are not frame-paired image proofs.

## Measurement scope

The [evidence ledger](partition-empty-phase-2026-10-29.json) records three alternating
baseline/candidate profile pairs on RTX 4070 SUPER/Vulkan, high-tier tuned host
overlap, near camera, 4×4 regions, PlaybackOnly, the same 1280×720 target,
47,923,200 node/head bytes and explicit 24-owner benchmark policy. The preserved
baseline executable's SHA matches the preceding native transport review. Run
manifests hash the current runner checkout; they do not reconstruct old sources.

The aggregate GPU span includes head/counter reset, health work and result copies.
Its original name is retained for direct comparison. Per-region child spans are
absent from steady empty work; a few asynchronous observations can be classified
across phase boundaries. Ranges below are **three per-run p95s**, not a pooled
p95 or a sum of phase percentiles. Profile lanes omit event-link readbacks;
separate near/orbit image runs provide unchanged birth/route/capacity controls.

| Aggregate partition GPU p95 | Baseline, three runs | Empty-audit path, three runs |
|---|---:|---:|
| Idle | 4.737–5.622 ms | **0.157–0.163 ms** |
| Active fireworks | 13.689–14.240 ms | 13.637–13.767 ms |

Idle p95 falls approximately **97%** in each paired comparison. Active rendering
cost remains similar in these observations; no active-path speedup or full-game
performance budget is claimed. The runs retain 12 launches/detonations/flashes,
zero final live particles/gameplay rockets/VFX owners and unchanged node/head bytes.

The two new near/orbit image runs deliver ten captures and observe zero fragment
drops/invalid links. Both preserve **374,514 gold-tail ember and 115,283 star-path
smoke births**, with no source overflow, expansion omission or destination
rejection, at the same **47,923,200 node/head bytes**. Their final samples are
explicit empty audits with one `[0,0,0,0]` GPU result, and all owners/particles drain.

Validation: 187 renderer tests, 54 demo/authoring tests, the explicit native
controls, strict scoped Clippy, workspace formatting and diff checks pass.
Ordinary-suite GPU ignores remain explicit.

## Reproduction

```powershell
$env:AESTRA_CROSS_POOL_IMAGES = "$PWD/target/fireworks-f8/partition-empty-fresh"
cargo test --locked -p aestra-viewer native_partition_phase_matches_full_lists_and_rejects_unsupported_draws -- --ignored --test-threads=1

./benchmarks/fireworks/run-smoke-look-review.ps1 -OutputDirectory target/fireworks-f8/partition-empty-profile-fresh -Mode host-overlap -Camera near -Variants tuned -Tiers high -Transparency shared -Partitions 4 -Cost -Instrumented
```

The remaining active cost still repeats full-target sort/health scans and
geometry per region. Next narrow those dispatches while retaining the same
native images, live route demand and capacity checks. Broader mixed scenes,
hardware/window lifecycles, finale/whole-game budgets and final smoke art remain
separate gates. This change does not adopt partitioned transparency by default.
