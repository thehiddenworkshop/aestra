# F6A — bounded reusable show composition, 2026-10-03

Base revision `b203a88f` plus F6A changes. This verifies ordinary clip composition and
forward playback admission, not AAA art, real-time speed, full Test B or finale scale.

## Saved asset and host APIs

`assets/test/effects/fireworks_show.aestra.ron` is a normal v4 document: zero local
emitters, 13 stable-ID clips referencing four saved F5 sources. No private host timers,
fireworks-specific engine concepts or duplicated event links are needed.

| Section | Clip starts (seconds) | Sources and placement |
| --- | --- | --- |
| Solo opening | 0, 2.3 | Multi-break left, crackle right/raised |
| Pair | 4.6 | Crossette left/front, strobe right/back/raised |
| Fan | 7.9 | Three multi-breaks, left/center/right, ±0.18-radian launch rotation |
| Alternation | 12 | Crossette left/back/raised, crackle right/front/larger |
| Restrained closing section | 16, 17 | Multi-break/crackle/strobe trio, then raised/back crossette |
| Quiet tail | 24–26 | No active clips |

All clips retain the complete seven-second child window, zero source offset and
`EffectClipSeed::Inherit`. The root seed and stable clip IDs derive 13 independent
repeatable seeds. Alternating pink/blue/gold gradients override each child's exposed
`Main stars color`, leaving source defaults, hot birth color, curve times, coverage and
secondary colors unchanged. Clip transforms provide multi-site placement, fan rotation
and layered heights. Repeated clips share one compiled source but own distinct GPU pools.

Use the existing public APIs: load the asset, resolve it through
`ProjectAssetIndex::resolve_effect_project`, compile with
`EffectCompiler::with_tier(...).compile_resolved_project(...)`, then spawn
`EffectPlayer::from_project(Arc::new(project)).with_history_policy(PlaybackHistoryPolicy::PlaybackOnly)`.
No new runtime API was necessary. Hosts own sound, environment and scene integration.

At most six clip windows overlap. Conservative scheduled concurrent particle capacities
are **4,460 / 1,980 / 964** at high/medium/low. Active windows are not simultaneous bursts:
most overlap includes fading/idle tails. This remains below the production density targets.

The first high run caught one truncated launch trail in the 1.2×-scaled final crossette.
Distance sampling uses world-space travel. At 42 units/s, 0.35-second retention and
0.25-unit spacing, that scale can exceed the portable 64-point ring. F5 sources (including
their saved shared-pool volley derivative) now use 0.30-unit launch spacing:
`ceil(1.2 * 42 * 0.35 / 0.30) + 2 = 61 <= 64`. Retention and owner pools stay unchanged.
This is headroom for these bounded transforms, not automatic sampling adaptation or
removal of the point limit. The original F5F evidence remains a historical pre-adjustment run.

## Reporting and native evidence

Viewer probe `f6-show` keeps interactive wall-clock behavior. Benchmark mode uses manual
60 Hz for 120 warm-up and 1,680 measured frames: 30 seconds covers all clips and cleanup.
The JSON retains transient ECS owner → root/clip-path/source/seed/capacity/history identity
even after expiry, captures child profiles via `ProjectProfiler` and records per-child
event totals. Earlier root-only benchmark queries would measure only this empty carrier;
that blind spot is fixed without changing the root-only `EffectProfiler` API.

`project_work` records concurrent active project totals. It does not sum per-owner peaks
or per-owner timing percentiles. Last observations and peaks remain asynchronous host
observations, not same-frame certificates. Missing values stay null; estimated buffers
are not allocated/device VRAM. Identity collection includes warm-up, whereas work/timing
measurements retain the usual measured-window semantics.

Windows, RTX 4070 SUPER/Vulkan, dev build, 960×540, audience camera, root seed
`0xf1e0000000000001`, semantic materials, HDR/Tony/exposure 0/bloom 0.15, pixel floors 0/0,
fast unsorted transparency and playback-only. Raw local files are ignored under
`target/fireworks-f6/f6a-show-{high,medium,low}.json`. One uninterrupted run per tier;
not repeated performance certification.

| Tier | Accepted linked births across 13 clips | Peak concurrent live / occupied histories | Peak concurrent estimated effect buffers |
| --- | ---: | ---: | ---: |
| High | 8,413 | 1,680 / 1,728 | 9,430,016 bytes |
| Medium | 3,317 | 648 / 672 | 3,835,520 bytes |
| Low | 1,217 | 235 / 240 | 1,492,544 bytes |

Every link admits its expected tier count. All source overflow, expansion omission,
destination rejection, trail eviction/truncation and paired checkpoint-capture bytes
are measured zero. Each child has final measured zero live/occupied counters before
expiry. The final project total is also zero, with no child entities remaining.
Read-only `validate-show.ps1` gates all 13 distinct paths, source identity, seeds,
native backend, complete expected per-link births, zero drops, full lifecycle observations,
monotonic paired simulation time, no checkpoint work and decreasing concurrent estimates.
These observations certify this workload's bounded admission, not arbitrary root motion,
frame budgets, hardware or density. Paired samples can skip asynchronous maps.

High/low audience lifecycle reviews each contain 14 PNGs plus a contact sheet and endpoint
report at frame 1560. Capture uses opt-in stable ordering and advances trail-bearing child
projects one 60 Hz observation at a time; no automatic golden approval. Final reports
show measured-zero live, occupied, retired and eviction/truncation counters and only the
empty root. Inspection confirms separated sites and alternating colors, but shells occupy
a small footprint and the quiet gaps/smoke are prominent. Low is sparse/dim. These are
technical captures, not acceptance against the reference photographs or moving footage.

The saved fixture was not manually authored through the editor; its UX review is open.
Tests cover serialization/resolution, shared sources with independent seeds, packed
color overrides, exact tier capacities, scale-aware trail headroom and lifecycle boundaries.
Reporting tests cover warm-up identity retention, unavailable metrics and actual concurrent
project peaks. Viewer suite: 78 passed / seven exporters ignored; warnings-as-errors
Clippy and workspace check pass.

## Reproduce

Run from the repository root on a native GPU; use fresh output locations for new evidence.

```powershell
New-Item -ItemType Directory -Force target/fireworks-f6-new | Out-Null
foreach ($tier in @('high', 'medium', 'low')) {
    cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f6-show --camera audience --backend gpu --history playback-only --semantic-materials --tier $tier --hdr --gpu-bench "target/fireworks-f6-new/f6a-show-$tier.json"
    if ($LASTEXITCODE -ne 0) { throw "show benchmark failed: $tier" }
}
pwsh -File benchmarks/fireworks/validate-show.ps1 -ReportsDirectory target/fireworks-f6-new
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f6-show --camera audience --backend gpu --history playback-only --semantic-materials --tier high --stable-transparency --hdr --sample-frames 0,90,210,350,528,570,792,1014,1050,1110,1170,1260,1452,1560 --capture target/fireworks-f6-new/show-review-high
# Repeat with --tier low and a fresh --capture directory.
cargo test --locked -p aestra-viewer
cargo clippy --locked -p aestra-viewer --all-targets -- -D warnings
```

Next: **F6B nested spatial host-cue validation**. Check repeated/transformed clips' actual
launch/break/secondary outputs, root/path/seed identity, positioned event payloads and
delivery epochs under root placement, restart and seek. This proves the generic API for
host bindings before flagship integration; audio playback and environment remain host-owned.
Editor UX, art direction, scene lighting, lit/persistent smoke and heavy finale gates remain open.
