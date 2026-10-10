# B20-1: analytic simulation to installed queues — 2026-10-10

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**; the independent candidate
remains locked to Bevy **0.20.0** / wgpu **30.0.1**. No dependency switch is made.

## Implementation

`gpu/simulation_pipeline.rs` now owns the production analytic pipeline resource,
eight-storage-binding layout, device-limit gate, embedded shader loading,
PipelineCache descriptors, bind-group preparation and reset/simulate/ribbon
particle dispatch. The shipping `run_simulation` calls that shared dispatch at
the same observation boundary, preserving timestamps and subsequent history,
paged-trail, checkpoint, mixed-stateful and light-readiness work.

The candidate path-imports this exact module. The native test installs its real
startup and bind-group systems alongside actual extraction, alpha-sort
preparation/compute, render preparation, queues and registered draw commands.
All 13 analytic descriptors reach PipelineCache readiness, including the ribbon,
history and paged-trail entry points; the latter are compiled, not dispatched by
this test. No duplicate reset/simulate dispatch implementation is maintained.

Inputs come from `EffectCompiler` → `EffectInstance` → `GpuEffectArtifact`.
Particle, alive, dead and counter buffers start at zero. Only authored records,
seed and requested time are supplied by the fixture: the production GPU dispatch
writes particles, compacts alive indices, resets counters and generates indirect
instance counts before the installed per-view sort and drawing.

The native contract checks:

- Five times spanning pre-emission, live particles, late survivors and complete
  expiry. Both empty and nonempty cases are required, not inferred from a pass.
- CPU-reference positions, size and normalized age; exact live/counter counts,
  compacted alive-slot membership and indirect command words/reset telemetry.
- Opposite-camera sorted permutations and unused sentinels from GPU-generated
  positions, with real prepared consumer bindings and two submitted draws.
- Same-capacity source replacement with a different seed; capacity growth from
  257 to 513 slots across another merge boundary; final effect/draw retirement.
- Explicit `PlaybackOnly` metadata, without checkpoint allocation or replay
  prerequisites in the shared dispatch.

The existing alpha observer and test-only bounded readback helper are reused.
No production GPU waits, readbacks, particle ECS entities, shader changes,
allocation-policy changes or new public APIs are introduced.

## Scope and next gate

The fixture supplies time through a **test controller**, not the shipping host
`run_simulation` system. This qualifies its shared analytic setup and particle
dispatch feeding sorting/drawing, **not the complete host scheduler**. Stateful
ticks, routed/coupled effects, trail-history scheduling, replay, lighting,
asynchronous shader readiness and pipelined rendering remain unqualified here.
The controlled initial dispatch disable is not an async-readiness test.

This uses synchronous headless rendering and the legacy sprite material path.
GPU records/commands are checked, not captured pixels, transparency quality,
performance or allocation parity. The next simulation slice should connect
actual trail-history recording and stateful dispatch without duplicating their
host scheduling logic; then qualify asynchronous/pipelined operation.

## Validation

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
# With AESTRA_REQUIRE_GPU_CONFORMANCE=1, restoring its previous value afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test gpu_conformance --test trail_compaction_conformance --test trail_culling_conformance --locked --offline -- --test-threads=1 --nocapture
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
```

- Candidate: **132 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 59 extraction/shared and **13 explicit serial native tests**.
- Shipping renderer lib: **151 passed**, three pre-existing ignored native tests,
  no filtering. Hardware-required shipping GPU/CPU/trail conformance: **5 passed**,
  including all seven showcase effects.
- Workspace all-target check and strict Clippy, candidate all-target strict
  Clippy, both formatting checks, PowerShell AST parsing and whitespace checks pass.
- Analytic integration ran on NVIDIA RTX 4070 SUPER / Vulkan / driver 616.92.
  Other candidate gates also used AMD Radeon Graphics / Vulkan / driver 26.3.1.
  All native GPU test processes were serial, including shipping renderer tests.

Accepted immutable runner evidence:
`target/bevy-020-qualification/runs/c0717c3f4a6249ac8a3ec7ae3af1ea1c/`.
The runner includes the shared analytic module in its source hashes and rejects
changed inputs/executables or logged native errors/panics.

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `e397cf745fd282d41e09dca335ec833a093fdb2d92e08a2be6beb02c64d6c010` |
| `inputs-after.json` | `c11caa1b86c4fe95a9d1cdcd3c3b56febab69c08c69e1c03316a2999bba7734d` |
| Extraction/native executable | `87a88a5ea0e883a3f254b1b9a266ab6edae3be296441b585427d6b5c4ee17d99` |
| Candidate lock | `23b7862f310867ef34697ab57f15cf043c229c793183d4c5bbe35166bf3bafdb` |
| Shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

Both lockfiles are unchanged. Earlier local single-test runs are exploratory;
the accepted evidence above is the final-source full runner execution.
