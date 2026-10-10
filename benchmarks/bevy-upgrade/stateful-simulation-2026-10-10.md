# B20-1: independent stateful simulation to installed queues — 2026-10-10

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The independently locked
candidate remains Bevy **0.20.0** / wgpu **30.0.1**; dependencies are not switched.

## Implementation

`gpu/stateful_simulation.rs` now owns the production persistent-state lifecycle,
twelve-binding pipeline setup, parameter packing, independent fixed-tick scheduler
and presentation/compaction. Shipping and candidate import the same implementation.
Metadata fingerprint and host-history helpers move beside their descriptors in
`effect_inputs.rs`. Event-buffer sizing uses the existing engine-neutral formula;
the public embedded shader path and host APIs are preserved.

The new native controller installs real state preparation, extraction, alpha
sorting and draw queues. Authored inputs pass through `EffectCompiler` →
`EffectInstance` → `GpuEffectArtifact`. Particle/persistent buffers begin empty;
there are no seeded GPU results, copied simulation kernels or duplicate tick loops.
The controller supplies time and clears the shared live counter before calling
the production scheduler; it does not emulate the full host scheduler.

Three generations (129 slots/seed 7, 129/11, 257/13) advance through ticks
0, 3, 20, 44, 80 and 120, each with repeated identical-time presentation:

- GPU positions and ordinals match the CPU stateful reference within 0.004.
  Compacted indices, live counts and indirect commands agree exactly.
- Death frees slots and later births reuse them. Free count plus live count
  remains capacity; after the tick-70 stop, all particles eventually expire.
- Preview catch-up is bounded per frame. Persistent allocation stays stable
  across advances; reseeding and resizing invalidate it.
- Installed alpha sorting produces exact opposite-camera permutations, and
  installed commands consume the prepared output bindings.
- A separate five-particle burst advanced over three ticks spawns exactly once.
- `PlaybackOnly` captures no checkpoints; teardown removes persistent state,
  alpha entries and submitted draws.

## Bugs found and corrected

Before the fix, every bind group in an independent cadence segment encoded
`persistent.last_tick` before that tick advanced. This reused burst/cutoff/trace
inputs across the segment. The native cutoff check reproduced **64 live particles
instead of the CPU reference's 44**. Advancing the tick after encoding each group
fixes it while preserving pass batching and checkpoint boundaries. The separate
burst-once regression also passes. Coupled scheduling already advanced per tick
and is unchanged.

The startup capability gate formerly required nine storage buffers although the
layout has twelve. It now requires twelve; the stale layout comment is corrected.

## Scope and remaining gates

This qualifies independent stateful scheduling and GPU output consumption, not
the full host/coupled/routed scheduler, replay/checkpoint restoration, asynchronous
shader readiness or pipelined rendering. It observes records and submitted
commands, not pixels or performance. No production wait/readback, particle mirror,
per-particle ECS entity, shader change or new public API is introduced. B20-1
remains open; coupled/routed scheduling and full host integration are next.

## Validation

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
# AESTRA_REQUIRE_GPU_CONFORMANCE=1, restoring its previous value afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --test gpu_conformance --test stateful_conformance --test trail_compaction_conformance --test trail_culling_conformance --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
```

- Candidate: **134 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 59 extraction/shared and **15 explicit serial native tests**.
- Shipping renderer lib: **151 passed**, three pre-existing ignored native tests,
  no filtering. Hardware-required shipping conformance: **36 passed** — 31 stateful,
  3 general GPU/CPU (including all seven showcase effects), 1 trail compaction,
  1 per-view trail culling.
- Workspace all-target check and strict Clippy, candidate all-target strict
  Clippy, both formatting checks, PowerShell AST and whitespace checks pass.
- Stateful integration ran on NVIDIA RTX 4070 SUPER / Vulkan / driver 616.92.
  Other candidate gates also used AMD Radeon Graphics / Vulkan / driver 26.3.1.
  Native GPU processes ran serially, including shipping renderer tests.

Accepted immutable runner evidence:
`target/bevy-020-qualification/runs/4df3e08ea21246f694775122e239f4a1/`.
The runner hashes the shared stateful source and rejects changed source inputs,
executables or logged native errors/panics. An earlier successful run predates
removal of a stranded documentation comment; the final evidence below covers
the strict-Clippy-clean source.

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `a0728b466b10fd03a7f60ee4c2272610440c8edf76c49a4b6dc1fff95158d2a2` |
| `inputs-after.json` | `0a7a193b5b579c17fc4cb966c596e070590d9c68c3ba3683f84518582a9dca78` |
| extraction executable | `3bb45227ec896fe1db0cd34018eb7130916e61e6db7d2d405cb57cb099193e21` |
| candidate lock | `23b7862f310867ef34697ab57f15cf043c229c793183d4c5bbe35166bf3bafdb` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
