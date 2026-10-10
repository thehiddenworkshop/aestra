# B20-1: asynchronous extension-stage outputs — 2026-10-10

Historical twentieth-slice evidence. The subsequent
[stage lifecycle slice](stage-output-lifecycle-2026-10-10.md) replaces the legacy
mailbox with context-stamped delivery and high-water rejection; the limitations
and source hashes below describe this earlier snapshot, not the latest source.

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The isolated candidate is
Bevy **0.20.0** / wgpu **30.0.1**. This slice does not change manifests, locks,
shaders or public API exports.

## Shared delivery and a batched-completion fix

`gpu/stage_output_delivery.rs` now owns the existing `AestraEffectOutputs`,
stage-output mailbox/receiver, output edge tracker and finished-event watcher.
The production extension-stage scheduler and candidate import the same sources.
The existing fluid-impact and play-once/loop/restart unit tests move with them.
`encode_stage_outputs` shares the existing executor callback-to-mailbox wiring;
the production caller keeps its existing per-stage read-tick gate.

Qualification exposed a deferred-command bug: several first reads for one owner
could each create a fresh tracker and replace its latest-values component. A
second stage could erase the first stage's values, and repeated above-threshold
reads in the same drain could raise duplicate impacts. The receiver now shares
pending values and tracker state per owner, then inserts each component once.
It also repairs independently missing components without resetting a surviving
tracker. A new shared unit test covers all four initial component combinations,
two stages, repeated above-threshold samples and latest-values-only removal.

## Twentieth serial native gate

`stage_outputs_native::native_020_stage_outputs_reach_host_values_and_impact_messages`
uses a real headless Bevy app and render schedule. Compiler-authored 16³ smoke
domains contain box colliders above their density sources. One owner has two
separate stages with unique module identities; a second has still air and one
collider; a third has a real solver with no declared outputs.

- Actual `StageTimeline` and `StageExecutor` passes advance in bounded 20-tick
  batches to tick 120. No test writes force records or fabricates completions.
- Production `encode_stage_outputs` copies/clears real solver output resources;
  `map_buffer_on_submit` completes through normal Bevy device polling. The actual
  receiver runs in `PreUpdate`, and a normal message reader observes impacts.
- Both active plates report finite upward forces. The still owner reports near
  zero force, and the output-less owner creates no output component or readback.
  Latest values remain available through both public accessors.
- Impact messages retain actual source module, stage, output name, magnitude and
  submitted tick. Their existing legacy root semantics are preserved (empty clip
  path and no playback epoch); this test does not invent nested routing support.
- Paused ticks submit no duplicate reads or updates. After normal asynchronous
  delivery, a small blocking test-only oracle verifies the output storage cleared.
  An outstanding read for a despawned owner cannot recreate it or emit more events.
- Playback-only retains no stage checkpoints. Native validation must finish
  without GPU errors. No production waits, extra readbacks, CPU simulation mirrors
  or mandatory replay are added.

## Boundaries and next gate

This qualifies actual stage producers, executor mapping and main-world delivery,
not the complete production `run_extension_stages` orchestration or `aestra-bevy`
player clock/project/binding scheduler. Requested ticks, extraction and per-stage
read gating are controlled by the fixture. Delivery does not rely on a blocking
GPU wait; only the independent post-delivery oracle and final error-scope drain do.

The existing stage mailbox is a legacy `(entity, stage, tick, words)` transport:
it has no compiled-artifact/epoch identity or out-of-order high-water rejection.
Stale callbacks after replacement/seek and nested stage-event routing are **not**
qualified by this gate. They must be addressed explicitly when qualifying full
host lifecycle scheduling; particle-output epoch guarantees do not imply these
guarantees for stage outputs. Async pipeline readiness, pipelined rendering,
visual parity and performance also remain separate gates.

The runner hashes the new shared source and fixture, executes this native gate
serially, and rejects changed inputs or executables during qualification.

## Validation and immutable evidence

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
# AESTRA_REQUIRE_GPU_CONFORMANCE=1, restoring its previous value afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --test gpu_conformance --test stateful_conformance --test trail_compaction_conformance --test trail_culling_conformance --test domain_spawn --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc fmt --all -- --check
cargo +1.98.1-x86_64-pc-windows-msvc fmt --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml -- --check
```

- Candidate: **159 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 79 extraction/shared and **20 explicit serial native tests**.
- Shipping renderer lib: **152 passed**, three pre-existing ignored native tests,
  no filtering. Hardware-required conformance: **40 passed** — 31 stateful,
  3 general GPU/CPU, 1 trail compaction, 1 per-view culling and 4 domain-spawn/
  accepted-birth-output tests.
- Workspace all-target check and strict Clippy, candidate all-target strict Clippy,
  both formatting checks, PowerShell AST and whitespace checks pass. Formatting
  configuration access required an approved retry; no lint is disabled.
- Actual stage output delivery ran on NVIDIA RTX 4070 SUPER / Vulkan / driver
  616.92. Some other gates used AMD Radeon Graphics / Vulkan / driver 26.3.1.
  Native GPU processes ran serially, including shipping regressions.
- Root manifests and locks remain unchanged. The stage batching fix is the only
  delivery behavior change in this slice; other shipping edits are source moves
  and shared callback wiring.

Accepted source-frozen run:
`target/bevy-020-qualification/runs/130877f50ebf441f897eb50c84ce394b/`.
Source hashes and executable identities stayed unchanged throughout the run.
This supersedes earlier slice evidence for the current shared fixture contents.

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `a9941f242b1c3d0acc7e2580b4423c54d77876bf047153ee6b5ae2615493c6ab` |
| `inputs-after.json` | `27cf0eb0226e744b1a8fb018bbab60d27e0a68adfb2d508f54f7db74e1c1cb3f` |
| extraction executable | `74bec3ce514966e0b710e60063df07d3072148ec457c5d1374fa4ea7c6a8d0e5` |
| candidate lock | `47eb58384c394fc9853d918cfdea384881cfed04fcf688c69502e3bdbd46e2ce` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
