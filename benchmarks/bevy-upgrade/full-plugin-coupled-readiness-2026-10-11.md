# Bevy 0.20 — Full-plugin coupled readiness

## Implementation

The isolated candidate now exercises one authored, persistent particle emitter
following a real dense fluid domain and producing paged trails through the actual
`AestraPlugin` and renderer bootstrap. It uses the same threaded app constructor
as the nested host/light gate, with an optional fixture setup callback before
Bevy moves the render app to its render thread. The authored effect is shared
with the existing independent coupled/trail gates, not a second simulation.

`patches-020/pipelined_coupled_native.rs` reserves an absent shader asset, copies
the production stateful presentation pipeline descriptor, and withholds only
that shader dependency. The descriptor is copied after Bevy registers its queue,
before any effect owner exists. Shader compilation and production dispatch stay
asynchronous; the fixture does not substitute a graph, allocator or scheduler.

The gate requires:

- At least eight render observations with allocated particle/domain state but
  no readiness, joint ticks, spawn carry, particle records, density, velocity or
  trail candidates consumed while the presentation shader is missing.
- Real shader insertion resumes the same production allocation at tick 30.
  Actual GPU fields are nonzero, fluid following changes the heads, three heads
  are alive, and paged history produces candidates.
- The real host receives live counts and nonzero submitted trail primitives.
  After seeking to tick 70, heads are dead but living tails still reach actual
  draw submissions. An offscreen target also receives nonblack presentation.
- Paused frames are idempotent. Backward seek and restart reproduce particle,
  persistent trail metadata, density, velocity and candidate headers bit-for-bit
  at tick 30; only the intentional trail discontinuity tag is normalized.
- Exact host frame boundaries produce context-valid telemetry. PlaybackOnly
  retains no particle/domain checkpoints and reports zero capture bytes through
  production asynchronous timing delivery.
- Teardown retires production stateful/domain runtimes and trail compaction/
  per-view culling entries. Clock/shader transitions reuse main asset identities.

Only the final bounded test oracles read back particle/field bytes or wait for
GPU completion. Production transport and rendering remain nonblocking.

## Regression found and fixed on both engine graphs

The stronger submitted-geometry check initially failed after an exact-frame
forward seek to tick 70: particle/trail/domain buffers were populated correctly,
but host live counts and geometry remained unavailable. GPU telemetry had the
correct token and epoch, but its time was **1.1666667461395264** while the host
clock rounded the same tick to **1.1666666269302368**. The strict future-frame
filter correctly rejected that packet.

`stateful_simulation::stateful_tick_time` now reuses the renderer-neutral
`PlaybackClock` conversion for coupled presentation and the trail observer's
clock upload. Fixed-dt physics integration and existing internal checkpoint
lookup times are unchanged. This fixes shipping 0.19 and the candidate from the
same production source; it does not introduce an epsilon into timestamp checks
or accept stale context. A shared unit regression checks all 36,001 boundaries
from zero through ten minutes and the specific tick-70 reciprocal-rounding case.
The existing bounded-catch-up trail regression now also requires the exact host
clock boundary, rather than expecting the old reciprocal multiplication result.

## Scope and next step

The full-plugin coupled readiness requirement now has an explicit native gate
in `run-020-patches.ps1 -Native`, alongside all existing independent compute,
replay, output, queue, volume and lighting checks. It is intentionally opt-in for
ordinary CPU-only test runs, but required by the native qualification runner.

This is not a smoke art-direction, volume-light scattering or performance parity
approval. B20-2 editor API/theme compatibility, the coherent dependency/adapter
switch, renderer/cache identity review and final native visual/performance parity
remain open. Shipping stays **Bevy 0.19.1**; the isolated candidate stays
**Bevy 0.20.0 / wgpu 30.0.1**. No dependencies or visual references are changed.

## Validation

Shipping renderer **164** and host **63** library tests pass; the three existing
renderer ignores are unchanged. Workspace all-target checking and strict Clippy
with `aestra-bench/gpu`, strict all-target candidate Clippy in both configurations,
both formatting checks and `git diff --check` pass.

Accepted source-frozen native run:
`target/bevy-020-qualification/runs/c51d74d004a9486fa8d89221b4ddbb0f/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **260 checks** pass
(9 keyboard, 40 upstream focus, 4 cluster units, 4 shader, 3 storage,
166 shared/candidate checks and **34 explicit native gates**). All **463 input
hashes and five executable hashes** remain unchanged during qualification.
The new full-plugin coupled gate passes in **4.13 seconds** on NVIDIA
RTX 4070 SUPER / Vulkan 616.92. Low-level device/shader/storage/timestamp gates
also run on AMD integrated Vulkan 26.3.1. These are correctness measurements,
not a performance baseline or visual parity approval.

Sandbox-denied lint/format checks pass with approved access. Existing
incremental-cache hard-link warnings fall back to copying. GPU suites run
serially; no lint suppression or assertion removal is introduced.

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render -p aestra-bevy --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --features async-qualification --locked --offline -- -D warnings
```

SHA-256 evidence:

| Artifact | SHA-256 |
| --- | --- |
| Synchronous extraction executable | `5f818a69b0178a542469fd00c8a8923e268ce87c0482521e0dbaa20d38b69af3` |
| Asynchronous extraction executable | `9aa8d85cc3c5be25bd037e0ac9cd529f1a817896e2925539cf1aec1adf0e2e32` |
| Shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
