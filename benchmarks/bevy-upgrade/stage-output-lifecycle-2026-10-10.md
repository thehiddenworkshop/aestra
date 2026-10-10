# B20-1: stage-output lifecycle safety — 2026-10-10

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**; the isolated candidate remains
Bevy **0.20.0** / wgpu **30.0.1**. No dependency version, shader, authored format,
public output accessor or message payload changes are part of this slice.

## Implementation

`gpu/stage_output_delivery.rs` now captures a `StageOutputStamp` when the executor
submits a readback. Its owner, stage and tick travel with immutable identity:

- The compiled artifact's `Arc` identity, not just structurally equivalent blocks.
- A private presentation token allocated by `PresentedEffect::new`. Extraction
  and presentation clones retain it; replacing a presentation with the same
  compiled artifact still changes it.
- The GPU seed, playback history epoch, simulation history revision and host-input
  epoch, copied from the actual extracted stage snapshot.

`gpu/stage_inputs.rs`, `sync_stage_inputs` and the extraction regression carry
and verify the presentation token. The production `run_extension_stages` read gate
compares the complete stamp rather than only the tick: otherwise a paused stage
could never send a fresh same-tick result after a context change.

The shared receiver validates identity against the current presentation **before**
decoding values or mutating trackers. On each receiver pass it also invalidates
old-context latest values and edge/high-water state, even without a new callback.
This prevents an old sibling stage's values surviving beside fresh-context data.
Idle components are not marked changed merely by the identity check.

Reads in one drain are sorted by tick. Independent per-stage high-water marks
then reject duplicate or older completions across drains, preventing latest-value
rollback and false edge rearming. Earlier pending-component batch merging and
independent component repair are preserved. An output-values-only removal does
not reset the surviving edge/high-water tracker.

This remains asynchronous copy/map delivery and checkpoint-free playback-only
remains supported. No production GPU wait, extra readback, CPU particle mirror,
shader modification or compulsory replay is introduced.

## Coverage

Three additional shared tests exercise the actual production receiver on both
Bevy versions:

1. Actual/extracted identity equivalence; independent presentation, artifact,
   seed, epoch, revision and host-epoch changes; clone identity preservation.
2. Restart, seek, seed changes above the GPU's lower 32 bits, history invalidation,
   recompiled artifact replacement and same-artifact presentation replacement.
   Existing values clear without fresh results; old reads are rejected before
   and after new reads; new contexts reset edges and accept lower ticks without
   mixing stale sibling-stage values.
3. Deliberately shuffled completions within a batch, older/duplicate completions
   across batches, independent stage high-water marks and legitimate later edges.

The existing native stage-output gate is extended, not replaced or duplicated.
It uses the real compiled fluid force producers, executor async callback and
main-world receiver. A fixture switch holds **only receiver scheduling**, while
normal Bevy updates/polling finish real GPU callbacks into the production mailbox.
After both callbacks are demonstrably queued, restart or same-artifact replacement
changes the owner context. Releasing delivery rejects those callbacks and clears
old values. Fresh same-tick callbacks are accepted; advancing again produces two
new legitimate impacts. Existing producer, pause, batch merging, output-less owner,
copy-and-clear, owner isolation, playback-only and despawn assertions remain.

## Explicit boundaries

This is callback transport/delivery qualification, not the full host/player clock
or production stage reset/reconstruction scheduler. The native fixture still
controls requested ticks and timelines. Resetting delivery state does not itself
reset a running solver. Host rebind identity propagation is checked field-by-field;
a full binding-to-stage scheduling integration remains for the host lifecycle gate.

Stage messages retain their legacy root/empty-clip/no-playback-epoch payload.
Nested stage-event routing and suppression of newly generated events during seek
reconstruction are not implemented or claimed by this slice. A host that directly
replaces the public `instance` inside a surviving presentation with an otherwise
identical unmarked instance must signal a history discontinuity; supported
restart/seek/history-invalidating APIs do so, and a new presentation gets a new
token automatically.

High-water rejection intentionally drops superseded old stage samples; it is not
an unbounded, lossless event journal. Within-drain sorting preserves available
chronological edges, but a genuinely later callback from an earlier tick is not
replayed once a newer tick has been accepted. Async shader readiness, render
pipelining, editor compatibility, coherent dependency switching and final native
visual/performance parity remain open upgrade gates.

## Validation

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows;
native GPU processes ran serially.

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

- Candidate: **162 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 82 extraction/shared and **20 explicit serial native tests**.
- Shipping renderer lib: **155 passed**, three pre-existing ignored native tests,
  no filtering. Hardware-required conformance: **40 passed** — 31 stateful,
  3 general GPU/CPU, 1 trail compaction, 1 per-view culling and 4 domain-spawn/
  accepted-birth-output tests. No GPU-unavailable skip is allowed in that run.
- Workspace all-target check and strict Clippy, candidate all-target strict Clippy,
  both formatting checks, PowerShell AST and whitespace checks pass. Formatting
  and Clippy configuration access required approved retries; no lint is disabled.
- The final extended native stage-output gate passed in 8.81 seconds on NVIDIA
  RTX 4070 SUPER / Vulkan / driver 616.92. Some other native gates used AMD Radeon
  Graphics / Vulkan / driver 26.3.1. This is not new visual/performance parity evidence.
- Root manifests and lock remain unchanged; no commit was requested or created.

Accepted source-frozen run:
`target/bevy-020-qualification/runs/d4c98b83976e4df798281929624a6d1b/`.
Source hashes and executable identities remained unchanged throughout the run.
This supersedes the previous slice's hashes for the current shared source contents.

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `6a81cf6cd563f477c03e37f4a6ed8f83326986a58ccf0c2048fe878c824ca41b` |
| `inputs-after.json` | `7be492e12ee1913ea5f79d129da7bcba907161d22d6a4a4ea312fc6244dc9f5a` |
| extraction executable | `cdad77c38d66f504d43b612e88635d1c2ab826ddf028f536df480ee805aae228` |
| candidate lock | `47eb58384c394fc9853d918cfdea384881cfed04fcf688c69502e3bdbd46e2ce` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
