# B20-1: canonical host clock and stage reset scheduling — 2026-10-10

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The isolated candidate uses
Bevy **0.20.0** / wgpu **30.0.1**. Root manifests/lock and authored formats are
unchanged. This slice qualifies a root player-to-stage bridge, not the full plugin.

## Implementation

- Extract the canonical `EffectPlayer`, clock system, choreography dispatch and
  presentation preparation/synchronization into `aestra-bevy/src/playback.rs`.
  Preserve public re-exports. Separate clock advancement from profiling so a
  missing profiler/runtime-status component cannot prevent a player from ticking.
  The shipping plugin still orders advancement before project reconciliation
  and final presentation synchronization.
- Share the actual stage runtime preparer in `gpu/stage_runtimes.rs` and derive
  extracted inputs through `ExtractedStages::from_presented`. Carry the actual
  history discontinuity start time through both extraction adapters.
- Rebuild stages for a new presentation token, changed simulation revision, or
  an epoch change beginning at zero. A restart is detected even when its first
  requested live tick has already exceeded the previous simulation tick.
  Seed/block/layout changes retain their existing rebuild rules. Compatible
  positive seeks retain timelines for checkpoint restoration or bounded catch-up.
  Constant-only edits retain the live solver; host rebinds invalidate checkpoints
  on that path too, not only on the identical-block path.
- Suppress reconstructed stage impacts without hiding latest force values.
  The stage timeline uses the existing default 60 Hz policy. An aggregate must
  finish beyond the instance's discontinuity boundary and have a preceding
  accepted read at/after that boundary before it may arm output-event edges.
  At a zero boundary, live tick one may emit normally. Reconstructed values do
  not arm edge tracking, allowing the first wholly live impact to be delivered.

No production GPU wait, extra readback, shader change, particle CPU mirror or
mandatory replay/checkpoint retention is introduced.

## Native gate

`host_stage_native::native_020_host_clock_drives_stage_reset_and_suppressed_seek_outputs`
uses the real compiled dense-smoke force producer and these production modules:
canonical player clock and presentation systems, stage input builder, Bevy 0.20
component extraction, stage allocation/context preparation, stage executor/timeline,
output callback encoder and main-world mailbox receiver.

The test's only scheduling driver is a narrow render-graph system with a four-tick
budget. It derives target time from extracted presentation state; there is no
fixture tick-request map or manually constructed stage runtime. Deterministic
Bevy time controls the actual player. Async callbacks complete through ordinary
app updates, not synchronous test reads or synthetic completion events.

It checks:

1. A 20-frame host advance reaches frame/tick 20 exactly once, without profiler
   or runtime-status components.
2. Restart followed by a 30-frame first live advance resets the GPU runtime and
   starts catch-up at tick 4 rather than continuing at tick 24.
3. Forward seek to tick 90 and backward seek to tick 30 use bounded reconstruction
   and update force values without emitting impacts; live tick 91 resumes impacts.
4. Explicit history invalidation and same-artifact presentation replacement
   rebuild before advancing. Despawn releases the runtime.
5. Playback-only retains zero checkpoint bytes throughout, and the validation
   error scope remains empty on native Vulkan hardware.

Two new shared regressions test reset classification and seek-event suppression,
including a first aggregate that overshoots the seek boundary. Existing extraction
tests now verify the boundary field, and context-delivery tests verify a silent
seek snapshot followed by a fresh live edge.

## Boundaries and tradeoffs

Aggregate stage outputs have no per-event source ticks. Conservatively suppressing
the first boundary-straddling read can discard a legitimate impact that happened
in that same mixed interval. This avoids replayed gameplay; it is not lossless event
journaling. Latest force values remain available to polling hosts. The read gate's
identity does not include the advancing sample window, so paused frames stay deduplicated.

This gate does **not** install the complete `AestraPlugin`, root binding capture,
nested clip reconciliation/forwarding, volume presentation, profiler, production
outer stage scheduler, asynchronous shader readiness or pipelined rendering.
Existing shipping host tests still cover project/binding/profile behavior on 0.19.
Nested stage messages retain their legacy root/empty-clip/no-epoch payload; routing
is not newly qualified. Compatible compiled-artifact edits intentionally preserve
live stages unless their context is explicitly invalidated. Direct unmarked
replacement of a public instance is not an automatic history-discontinuity API.

Replay-enabled restoration remains covered by earlier timeline/coupled gates;
this new host gate focuses on checkpoint-free gameplay playback. Editor changes,
the coherent dependency switch and native visual/performance parity remain open.

## Validation

All commands use pinned MSVC Rust 1.98.1 and locked offline dependencies;
native GPU processes run sequentially.

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --lib --locked --offline
# AESTRA_REQUIRE_GPU_CONFORMANCE=1, restoring its previous value afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --test gpu_conformance --test stateful_conformance --test trail_compaction_conformance --test trail_culling_conformance --test domain_spawn --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc fmt --all -- --check
cargo +1.98.1-x86_64-pc-windows-msvc fmt --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml -- --check
```

- Candidate: **165 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 84 extraction/shared and **21 explicit serial native gates**.
- Shipping host library: **61 passed**, without filtering.
- Shipping renderer library: **157 passed**, three pre-existing ignored native
  tests. Hardware-required conformance: **40 passed** — 31 stateful, 3 general,
  1 trail compaction, 1 culling and 4 domain/accepted-birth-output checks.
- Workspace all-target check, both strict all-target Clippy checks, both formatting
  checks, PowerShell AST validation and whitespace checks pass. Formatting and
  Clippy configuration access required approved retries outside the sandbox;
  no lint was disabled.
- Root manifests/lock remain unchanged. Changes are not committed.

Accepted final source-frozen run:
`target/bevy-020-qualification/runs/56999bfd63dd4849b7a93b79c21c41c7/`.
Source and executable hashes remained unchanged throughout the run. The new host
gate passed in **3.40 seconds** on NVIDIA RTX 4070 SUPER / Vulkan (driver 616.92
reported by the companion gates). Some other gates selected AMD Radeon Graphics
/ Vulkan / driver 26.3.1. This is correctness evidence, not visual/performance parity.

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `a7f6f0931603e0dae5c91ea7d96c75e28698ccd85a7094bae60740ad03bf53d9` |
| `inputs-after.json` | `a34d67b49373132e4e73fdad04766ca98fe58618e270c64e5897461a8a95223e` |
| extraction executable | `446aa769d1d5de5d17b15c897dffc784efe7c03241ec7a5ba019f376570fa953` |
| candidate lock | `47eb58384c394fc9853d918cfdea384881cfed04fcf688c69502e3bdbd46e2ce` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
