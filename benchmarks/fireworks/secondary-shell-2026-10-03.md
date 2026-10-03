# F5A — bounded secondary shell, 2026-10-03

Two real particle-death generations now work in one editable project effect:
rocket → 64 main stars → eight sparks per parent (512 total).
This is technical event-chain acceptance, not AAA artistic or finale-scale acceptance.

## Implementation

`assets/test/effects/fireworks_multi_break.aestra.ron` uses six ordinary emitters and four
ordinary `OnDeath` links. Root death also feeds one short flash and 48 smoke particles.
Main stars live 1.2–1.6 seconds, then create secondary sparks at their actual final positions,
with 25% inherited parent velocity plus independently sampled spherical launch velocity.
Secondary sparks live 0.45–0.85 seconds and leave short, 0.35-second history trails.
Their destination capacity and owner budget are 512; 64 records at 60 Hz cover each tail.
Total effect particle capacity is 690. The launch plume remains an independent authored
approximation. Hero material programs and exposed color/radiance controls are reused.
No special runtime concept, new trigger or hard-limit bypass was added.

Viewer probe `f5-multi-break` selects the saved asset, with the hero's whole-shell cameras.
For this probe only, `--gpu-bench` advances uninterrupted playback with a recorded manual
60 Hz step. This guarantees lifecycle coverage on fast hosts; it is not real-time throughput
measurement. Normal interactive playback remains wall-clock driven.

## Verification

- Viewer: 63 tests pass; three source exporters are intentionally ignored.
- Required GPU production-compute conformance: one translated rocket, simultaneous
  64-parent death wave and 512 live secondary sparks; compares positions by ordinal with
  the CPU reference, checks exact CPU birth positions, confirms inherited velocity changes
  later GPU motion, reverses link order and checks eventual empty state. Forward loop only:
  no seek, restart, checkpoint restore or snapshot allocation.
- Warnings-as-errors Clippy passes for viewer and render adapter, including all targets.
- Full required-GPU `stateful_conformance` suite passes: **28/28**, run serially.
- Uninterrupted report: `target/fireworks-f5/f5a-multi-break-live.json`, RTX 4070 SUPER/Vulkan,
  dev build, 960×540, seed `0xf1e0000000000001`, high tier, playback-only, stable ordering,
  HDR/Tony/exposure 0/bloom 0.15, authored floors 0/0. Warm-up 120, measured 600 render frames.
  All four links record demand = accepted: **64, 1, 48, 512**. Expansion omissions,
  destination rejections and source overflow are all zero, with 716 event readback samples.
  Observed maximum trail evictions/truncation are zero. All 600 recorded simulation frames
  report zero coupled checkpoint-capture bytes. Estimated effect buffers: 2,701,504 bytes,
  not total device memory. Asynchronous work peaks are not tick-aligned certificates.
- Exact-frame captures: `target/fireworks-f5/f5a-multi-break-review`, three cameras × eight
  frames = 24 PNGs, authored floors only. Runner records dirty source hashes on base revision
  `077a5bf2`. Native-backend, response/seed/resolution, main/secondary/smoke and endpoint checks
  pass. Captures observe main peak 64, secondary peak 512, smoke peak 48. All final metrics
  (live particles, occupied/retired histories, evictions, truncation) are measured zero at
  frame 420. The runner allows staggered secondary peaks below 512; total admission must
  come from the separate uninterrupted report, not from these seeking still captures.

Contact-sheet inspection shows pink main tails followed by distributed warm secondary
clusters and later smoke-only decay. Authored subpixel tails still fragment at distance;
the smaller secondary shell is a bounded mechanism fixture, not a replacement for the
reference hero's visual target. No golden/reference approval or moving-footage review here.
Smoke remains unlit. Idle-tail benchmark percentiles do not certify live finale cost.

## Reproduce

```powershell
cargo test --locked -p aestra-viewer
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --test stateful_conformance gpu_two_generation_death_chain_preserves_parent_positions_and_velocity -- --nocapture
# Full GPU regressions, serial to avoid unrelated device-load contention:
cargo test --locked -p aestra-bevy-render --test stateful_conformance -- --test-threads=1
cargo clippy --locked -p aestra-viewer -p aestra-bevy-render --all-targets -- -D warnings

New-Item -ItemType Directory -Force target/fireworks-f5 | Out-Null
# Choose a fresh filename; the viewer does not protect existing benchmark reports.
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f5-multi-break --camera audience --backend gpu --history playback-only --semantic-materials --stable-transparency --hdr --exposure 0 --tonemapping tony --bloom 0.15 --gpu-bench target/fireworks-f5/multi-break-live-new.json
# The capture runner rejects existing directories.
pwsh -File benchmarks/fireworks/capture-shell-review.ps1 -Shell multi-break -Response authored -OutputDirectory target/fireworks-f5/multi-break-review-new
```

Follow-up implemented: [F5B generic host-cue validation](host-cues-2026-10-03.md) for actual
launch/main/secondary particle events, bounded delivery and restart/seek epoch handling.
Audio assets/playback/mixing stay host-side. Next: bounded crackle authoring (F5C).
Remaining F5 shell styles, heavy overlapping load, F4 artistic approval and F7/F8 light/smoke
integration stay open.
