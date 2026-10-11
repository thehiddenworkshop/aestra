# B20-1: production particle-light installer — 2026-10-11

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**; the isolated candidate uses
**0.20.0** / **30.0.1**. Root dependency files are unchanged. This builds on the
[outer fluid/history integration](outer-fluid-history-2026-10-11.md).

## Actual installer, not another selector

Include the shipping `particle_lights.rs` directly in the candidate harness and
call its actual `install`. This registers embedded selection shaders, main-world
collection, real component/resource extraction, render-world preparation and
graph selection. The production outer simulation graph supplies both analytic and
independent stateful particles in the same app. No selected records are seeded.

Replace the module's parent wildcard imports with explicit dependencies. Schedule
selection after the existing `SimulateEffects` set. Add an internal
`SyncGpuRenderTransforms` set to the existing transform synchronization system;
collection observes that set instead of importing the monolithic function. The
shipping ordering is preserved: propagation → transform sync → collection, then
preparation → simulation → selection → rendering. No public API changes.

The old `Mat4::IDENTITY` passed to the portable light planner used Bevy's glam
type and does not compile across the isolated 0.20/0.19 math boundary. Use the
planner's inferred identity default instead. Runtime matrices and shader layouts
are unchanged; the planner still requests the live GPU render-globals transform.

## Native lifecycle gate

`native_020_production_light_installer_selects_mixed_playback_and_retires_stale_sources`
runs capacities **129 / 257**, global cap **5** and authored per-output cap **3**.
It checks:

- Disabled-by-default behavior, real shader preparation and actual light records
  from both analytic and independent stateful presentation.
- Matching live source ordinals, actual world-space positions under rotated,
  nonuniform transforms, constant lumens/range, sorted source manifests, exact
  paused records, counters and zero padding.
- Withhold the real stateful pipeline resource for four frames. Its old particles
  remain allocated, but only the current analytic owner may produce lights.
  Restore the resource and require exact paused selection recovery.
- Forward-time/transform refresh while reusing global selection storage; nested
  root/clip-path metadata and root epoch rebinding without retaining the old
  manifest identity.
- Replace an artifact in place, preserving authored IDs/entity/pool size but
  changing allocation/seed. Only the replacement identity can be selected.
- Explicit rejection under a one-byte memory budget, budget recovery, disable
  and re-enable with a smaller global cap, pool-component removal/reinsertion,
  CPU-backend switching, despawn, replacement owner and final teardown.
- Playback-only persistent states retain no snapshots. Real simulation timestamps
  complete through the existing bounded transport.

The fixture supplies compiled particle inputs and render-globals buffers using
the existing qualification helpers; it does **not** qualify the complete main-world
`prepare_gpu_effects`/transform-sync installer. Actual light collection/extraction/
preparation/selection and actual outer simulation run normally. All buffer reads
and GPU waits are test-only. This is correctness evidence, not a performance test.

The gate is an additional required exact filter in `run-020-patches.ps1 -Native`,
executed serially in its own process with hardware required. All prior gates remain.

## Remaining integration

Production extension-stage/volume presentation installers, light realization
(portable async and same-frame GPU cluster admission), complete plugin integration,
combined pipelined scheduling and mixed emitters within one effect remain open.
This test mixes separate analytic/stateful owners, not different emitter classes
inside one owner. Editor compatibility, coherent dependency switch and native
visual/performance parity are also still required. No visual reference is approved.

## Validation

The source-frozen run is accepted:
`target/bevy-020-qualification/runs/b77c5762b01344adb972d52344ab94e4/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **186 checks** pass
(9 keyboard, 40 upstream focus, 4 cluster units, 4 shader, 3 storage,
96 extraction/shared, **30 explicit native gates**). All prior gates remain
required. All **425 input hashes** and all five native executable hashes still
match after shipping regression checks.

The new gate runs on NVIDIA RTX 4070 SUPER / Vulkan, driver 616.92, in **2.56 s**.
Some lower-level native gates use AMD Radeon integrated graphics / Vulkan,
driver 26.3.1. These durations are not performance claims. No visual references
are changed or approved.

Shipping host library: **63 passed**. Shipping renderer library: **157 passed**,
with three pre-existing native tests still ignored. Shipping light conformance:
**2 passed**, with its profiling probe still ignored, on AMD Radeon integrated
graphics / Vulkan with `AESTRA_REQUIRE_GPU_CONFORMANCE=1` and the previous value
restored. Both shipping full-plugin native light playback tests also pass,
including 512 event-born star candidates capped to 48 lights. Those shipping
tests do not establish complete-plugin compatibility on the candidate.

Workspace all-target check and strict Clippy with `aestra-bench/gpu`, strict
candidate Clippy in default and `async-qualification` configurations, both
formatting checks, PowerShell AST parsing and whitespace validation pass.
No lint is suppressed. Incremental hard-link warnings fall back to copying.

Commands use the pinned toolchain and locked/offline dependencies:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --lib --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test particle_light_conformance --locked --offline -- --test-threads=1 --nocapture
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test particle_light_playback --locked --offline -- --ignored --test-threads=1 --nocapture
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --features async-qualification --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc fmt --all -- --check
cargo +1.98.1-x86_64-pc-windows-msvc fmt --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml -- --check
```

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `d12ab52a43904620f14abf96dd968b8992c51528bd9037b537975a5386e2aca0` |
| `inputs-after.json` | `e1121e7212ffa81ad041e15b6b63f3dbe7a4cb5f4e06834cc8642ea4ad5ab8fc` |
| synchronous extraction executable | `cb6c7f112e2570db7f7367e1ab8eda02bb60dd6f334f6cf80b773ac541d84996` |
| threaded extraction executable | `52347d920039e8c36b5faf01889d1c73e7f926010d9d4f16ec531941b8cfa800` |
| candidate lock | `99de6e313138126ede8ed042b190d788ef2cc4274e900587d6196af2c01f0baf` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

Root dependency files and unrelated workspace changes are preserved. Changes are
not committed.
