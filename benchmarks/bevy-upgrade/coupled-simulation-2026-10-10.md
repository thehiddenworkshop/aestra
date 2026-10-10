# B20-1: coupled particle events/routes to installed queues — 2026-10-10

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The independently locked
candidate remains Bevy **0.20.0** / wgpu **30.0.1**; dependencies are not switched.

## Implementation

`gpu/coupled_simulation.rs` extracts the production lockstep loop and the
`run_stateful_dispatches` caller unchanged in scheduling behavior. Shipping and
candidate share event-link expansion/spawning, host-input bursts, output-ring
aggregation, shared live-counter reset, telemetry stamping and pacing selection.
Route wiring and event-counter offsets live beside the extraction descriptors.

A small `CoupledHistory` interface exposes the coordination operations already
provided by the shipping stateful trail observer. Its implementation delegates
to the existing observer; no history ABI, storage or restoration policy changes.
The candidate's particle-only test passes no history observer or fluid domains.

The candidate path-imports the actual engine-neutral `execution.rs`. Its two
mapped-view sites use the existing version-selected helper: wgpu 29 preserves
its infallible view behavior; wgpu 30 handles unavailable views without panicking.
The existing asynchronous callback remains asynchronous; the blocking tool/test
readback gains an explicit unavailable-view error. No new wait is introduced.

## Native integration gate

`coupled_native::native_020_coupled_routes_feed_alpha_and_installed_queues`
installs actual state preparation, extraction, alpha sorting and draw queues.
Three authored persistent sprite emitters pass through `EffectCompiler` →
`EffectInstance` → `GpuEffectArtifact`; GPU particle and persistent storage start
empty. The fixture lowers descriptors and supplies host-route records and time;
it does not duplicate the simulation/event algorithms or seed completed results.

- A one-particle source dies, spawning 12 children; their deaths spawn 24
  grandchildren through two compiled event links. Two host bursts add 3 and 5
  particles at the supplied event position. Final population is 32.
- Requested/dropped/accepted link triples are exactly `[12, 0, 12]` and
  `[24, 0, 24]`; source event-overflow counts remain zero.
- Child output is `(tick 4, epoch 7, count 12)`. Grandchild outputs are
  `(2, 7, 3)` and `(7, 7, 29)`: tick 7 combines 24 linked births and 5 host births.
  External birth records remain output-only, preserving the existing no-feedback
  contract; the second link deliberately uses particle death, not those records.
- GPU live/free counts and indirect commands agree. Installed alpha sorting
  produces exact opposite-view permutations, including birth-ordinal/slot ties,
  and installed commands consume their real prepared bindings.
- Repeating tick 8 does not spawn or export twice. A jump to tick 120 converges
  through bounded preview frames without changing the population. `PlaybackOnly`
  retains no snapshots. Removing draw/effect retires submissions and state.

## Scope and remaining gates

This qualifies coupled **particle playback** and lowered event-route records,
not fluid-domain execution, joint stateful trail history, full host scheduling,
asynchronous host output delivery, replay/restoration, asynchronous shader readiness
or pipelined rendering. It observes records/bindings/submissions, not pixels or
performance. No shader changes, production waits/readbacks, CPU particle mirrors,
per-particle ECS entities or public API changes are introduced. B20-1 remains open.

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

- Candidate: **135 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 59 extraction/shared and **16 explicit serial native tests**.
- Shipping renderer lib: **151 passed**, three pre-existing ignored native tests,
  no filtering. Hardware-required shipping conformance: **36 passed** — 31 stateful,
  3 general GPU/CPU, 1 trail compaction, 1 per-view trail culling.
- Workspace all-target check and strict Clippy, candidate all-target strict
  Clippy, both formatting checks, PowerShell AST and whitespace checks pass.
- Coupled integration ran on NVIDIA RTX 4070 SUPER / Vulkan / driver 616.92.
  Other candidate gates also used AMD Radeon Graphics / Vulkan / driver 26.3.1.
  Native GPU processes ran serially, including shipping renderer tests.
- Cargo manifests and both lockfiles are unchanged. Sandbox configuration-access
  failures for formatting/Clippy were rerun with approved access; no lint was disabled.

Accepted immutable runner evidence:
`target/bevy-020-qualification/runs/75fa2a2b2e2d4ac29d8044bdd2703a7a/`.
The runner now hashes the shared coupled scheduler and actual executor, alongside
the fixture and existing inputs, and rejects source/executable changes or native
errors/panics. Source hashes stayed unchanged throughout the accepted run.

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `962ff601d152f5dc9262215172deca8a5b62d4b44d9364e22f774ffd692d8745` |
| `inputs-after.json` | `d66cdeb5bc37aafe2555d62e41c81488182ed9b547165c97bf0439b3a5f47126` |
| extraction executable | `f31c622e356e1282e8ce81ea2f42067ab21296d0865d6b0b2a894cd72d0347c0` |
| candidate lock | `23b7862f310867ef34697ab57f15cf043c229c793183d4c5bbe35166bf3bafdb` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
