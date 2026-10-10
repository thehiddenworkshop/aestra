# B20-1: fluid coupling and joint stateful trail history — 2026-10-10

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The isolated candidate uses
Bevy **0.20.0** / wgpu **30.0.1**. No shipping dependency upgrade is activated.

## Implementation

The candidate path-imports the actual `gpu/stateful_trails.rs` observer,
`gpu/trail_checkpoints.rs`, `gpu/trail_replay.rs` and `host_transform.rs`.
Explicit observer imports replace its parent wildcard without changing history
recording, snapshot storage or restoration behavior. The candidate's direct
engine-neutral `aestra-extension` and `aestra-fluid` dev-dependencies add only the
fluid path package to its lockfile; the shipping manifest and lockfile stay unchanged.

`coupled_trails_native.rs` supplies authored inputs and a small render controller,
not replacement simulation/history/replay algorithms. `EffectCompiler` with the
installed fluid extension compiles a real dense smoke domain (16³ cells, six-unit
cell size) and three persistent trail heads. The source/grid are centered on the
heads, away from the closed floor, so Follow Field must produce vertical motion
despite zero authored vertical velocity and gravity.

The native test installs actual extraction, simulation pipeline preparation,
trail compaction, per-view culling and draw queues. It invokes the production
lockstep scheduler, actual stage executor and actual stateful trail observer on
empty particle/history storage. Two image-target cameras exercise different
culling decisions while installed commands consume the precise produced buffers.

## Native contracts

- Density and velocity evolve through the real solver. Live heads move vertically
  through the actual compiled Follow Field binding. Fluid, particles and trail
  history clocks agree at every encoded update.
- Skipped-frame advances converge through bounded preview budgets; the observer
  records the canonical intermediate ticks. Repeated target time changes neither
  the fluid fields, persistent particles/history nor compacted candidate indices.
- Dead heads retain renderable tails, then trails expire. Per-view indirect counts
  match the compacted candidate count near the effect and zero in the distant view.
- Playback-only particle/domain/trail checkpoint stores stay empty. Switching to
  optional replay leaves the live simulation unchanged. All three stores contain
  tick 60; seeking back from tick 100 to tick 70 across an epoch change restores
  bit-identical fluid fields and persistent particle/history data through the
  common checkpoint. Only the deliberately rebased epoch tag is normalized.
- Source replacement at the same capacity and growth from 70 to 1,025 owners
  exercise reset/rebinding and inline/paged history. Paused paged sort/scan scratch
  is temporary work storage, not persistent history; equality excludes it.
- Removing the draw/effect retires submissions, persistent state and history.
  Native validation scope must finish without GPU errors.

## Scope

This qualifies dense-smoke field following and joint trail history in the real
renderer schedule, not domain-emitted particle spawning, sparse/liquid coupling,
the complete main-world host scheduler, asynchronous output delivery, async shader
readiness, pipelined rendering, pixels or performance. Those remain distinct gates.
No production readback/wait, particle mirror, per-particle ECS entity, shader change
or mandatory replay is added. Small bounded readbacks and a final wait are test-only.

The runner adds a seventeenth explicit serial native gate and hashes the full fluid
extension source/manifests plus the imported historical pose/replay code. It rejects
native errors/panics and changes to source hashes or executable identity during a run.

## Validation

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
# AESTRA_REQUIRE_GPU_CONFORMANCE=1, restoring its previous value afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --test gpu_conformance --test stateful_conformance --test trail_compaction_conformance --test trail_culling_conformance --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc fmt --all -- --check
cargo +1.98.1-x86_64-pc-windows-msvc fmt --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml -- --check
```

- Candidate: **142 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 65 extraction/shared and **17 explicit serial native tests**.
- Shipping renderer lib: **151 passed**, three pre-existing ignored native tests,
  no filtering. Hardware-required shipping conformance: **36 passed** — 31 stateful,
  3 general GPU/CPU, 1 trail compaction and 1 per-view trail culling.
- Workspace all-target check and strict Clippy, candidate all-target strict Clippy,
  both formatting checks, PowerShell AST and whitespace checks pass.
- Fluid/joint-history integration ran on NVIDIA RTX 4070 SUPER / Vulkan / driver
  616.92. Some other candidate gates used AMD Radeon Graphics / Vulkan / driver
  26.3.1. Native GPU processes ran serially, including shipping regressions.
- The candidate lock adds the engine-neutral fluid path package. The shipping
  manifest and lockfile are unchanged. Configuration-access failures for formatting
  and Clippy were rerun with approved access. No lint is disabled.

Accepted final source-frozen evidence:
`target/bevy-020-qualification/runs/7cb8f472bd864a4fa5b4a4bbab94b01c/`.
The final run includes the obsolete-parent-import cleanup found by shipping Clippy.
Source hashes and all executable identities stayed unchanged throughout this run.

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `f19761a8ecb70d21bb37b5e661c8a1e602f113badd240a23e0891ed0bbd9d760` |
| `inputs-after.json` | `f0c167b09273e1fb16d38ef7133bfb1c7250626e08a3d2e277faf90e9ee7c4b3` |
| extraction executable | `ce3ef3b4c5f43edc4d4e89a3dbcc693ca14621a236730c6565b4a2ea5a08f45f` |
| candidate lock | `47eb58384c394fc9853d918cfdea384881cfed04fcf688c69502e3bdbd46e2ce` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
