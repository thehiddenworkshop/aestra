# B20-1: actual fluid-domain births — 2026-10-10

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The isolated candidate uses
Bevy **0.20.0** / wgpu **30.0.1**. No shipping dependencies or shaders change.

## Implementation

`patches-020/coupled_trails_native.rs` now shares app setup, domain installation,
authored smoke source and two-view scene setup between the field-follow and new
domain-spawn gates. Its lowered descriptor carries the actual compiled domain
binding, and the production scheduler receives output-route wiring. No simulation,
allocation, emission, history, replay or aggregation algorithm is copied.

The new fixture compiles Secondary Emission on the actual dense smoke solver and
Spawn From Domain on a ten-slot persistent trail emitter. Ordinary rate/burst
emission is zero. Authored launch speed, gravity and turbulence are zero, and
Follow Field is absent so inherited velocity can be measured directly at birth.
Initial particle, emission and output storage is empty.

## Native contracts

- At tick one, the real domain emission list saturates its 16-record capacity.
  Only its first ten records become particles. GPU state has the exact record
  positions, half the source velocities, fresh ordinals and zero birth age;
  nonzero positions/velocities rule out an origin-seeded stand-in.
- Free count is zero and spawn count is ten. The actual GPU OnSpawn output ring
  reports ten accepted births, tick/epoch and the first three ordinal/position
  pairs. The next full-capacity tick exports zero births, not rejected requests.
- Paused playback changes neither persistent history/fields nor the output ring.
  Continued live ticks retire particles and reuse slots for later ordinals.
- The actual stateful trail observer, compaction and per-view culling produce
  candidates consumed by the installed draw queues; near/far indirect counts and
  precise consumed buffer identities are checked.
- Playback-only keeps particle/domain/trail checkpoint stores empty. Optional
  replay reconstructs the same fields/history, then restores from the common
  tick-60 checkpoint after an epoch change. Neither reconstruction exports
  duplicate births. Only discontinuity metadata is normalized for history equality.
- Despawning the effect retires persistent state, trail history and submissions.
  A native validation scope must complete without errors.

## Boundaries

This qualifies dense-smoke domain births and GPU output-ring contents, not the
complete main-world host scheduler or asynchronous delivery to host consumers.
Sparse/liquid emission, async shader readiness, pipelined rendering, visual parity
and performance remain separate gates. Tests use synchronous shader compilation.
Small readbacks and a final GPU wait exist only in the native fixture. No runtime
wait/readback, CPU particle mirror, per-particle ECS entity or mandatory replay
is introduced.

`run-020-patches.ps1 -Native` adds an eighteenth explicit serial native gate.
Existing source/executable immutability checks already include this fixture and
the full fluid extension. Final validation and evidence are recorded below.

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

- Candidate: **143 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 65 extraction/shared and **18 explicit serial native tests**.
- Shipping renderer lib: **151 passed**, three pre-existing ignored native tests,
  no filtering. Hardware-required conformance: **40 passed** — 31 stateful,
  3 general GPU/CPU, 1 trail compaction, 1 per-view culling and 4 domain-spawn/
  accepted-birth-output tests.
- Workspace all-target check and strict Clippy, candidate all-target strict Clippy,
  both formatting checks, PowerShell AST and whitespace checks pass. Configuration
  access failures were rerun with approved access; no lint is disabled.
- Actual domain births ran on NVIDIA RTX 4070 SUPER / Vulkan / driver 616.92.
  Some other gates used AMD Radeon Graphics / Vulkan / driver 26.3.1.
  Native GPU processes ran serially, including shipping regressions.
- No manifests, locks or shipping code changed in this slice. The shipping root
  manifest/lock remain unchanged across the preceding candidate slices too.

Accepted source-frozen run:
`target/bevy-020-qualification/runs/82322ce8f48a48f9addbc00d4f52900d/`.
Source hashes and executable identities stayed unchanged throughout the run.
This supersedes earlier slice evidence for the current shared fixture contents.

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `bf6670665eb8007de4954c9e76cf4a15438e535cb03df2587e4daa14f0591e8e` |
| `inputs-after.json` | `3bfea02cbaccf8c563c39126b6f496dcb4930fca1945e5d1eb9e3f9f7fa8d2d7` |
| extraction executable | `6a80f7cc20ab47eb82ea0de920e3d2b382880780a43ca954e5f8211d26bcd8f7` |
| candidate lock | `47eb58384c394fc9853d918cfdea384881cfed04fcf688c69502e3bdbd46e2ce` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
