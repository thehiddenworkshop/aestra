# B20-1: asynchronous readiness and pipelined scheduling — 2026-10-10

Implementation started October 10; final qualification completed October 11 CEST.

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The isolated candidate uses
Bevy **0.20.0** / wgpu **30.0.1**. This slice changes qualification fixtures and
their runner only, not shipping APIs, authored formats, runtime algorithms or art.

## Why a separate configuration

Bevy's asynchronous pipeline compilation requires `multi_threaded` on native
Windows; setting `synchronous_pipeline_compilation: false` without that feature
does not test asynchronous creation. Enabling that feature also installs
`PipelinedRenderingPlugin` in `DefaultPlugins`, which moves `RenderApp` to a render
thread during cleanup. Existing fixtures intentionally inspect that local subapp.

Add opt-in `async-qualification = ["bevy/multi_threaded"]` to the isolated candidate.
Keep the existing default configuration and all 22 native gates. The runner builds
a second executable, records its SHA-256 and explicitly executes two new ignored
tests in separate serial processes. Reject zero-test filters, native errors and
binary/source changes. No GPU tests contend with each other.

The candidate lock gains Bevy 0.20's state/state-macro packages through the threaded
feature graph. There is still only one Bevy version in the candidate; the root
manifest and lock are unchanged. The additional compilation variant has an initial
build cost, intentionally confined to qualification rather than shipping defaults.

## Delayed shader → asynchronous compilation → real persistent particles

`stateful_native::native_020_delayed_shader_recovers_stateful_simulation_and_draws`
shares the existing stateful integration harness, with asynchronous compilation
enabled and pipelined rendering explicitly disabled for direct test-only GPU
buffer inspection.

After processing the actual production initializer, clone its four canonical
descriptors (layout and entry points unchanged) with a reserved shader handle.
Withhold that asset for eight render frames while the requested simulation tick
is 30. Assert no dispatch, no consumed fixed ticks or spawn carry, retained state
allocation, zero spawn ordinals and no replay checkpoints. Supply the actual
generated stateful WGSL through `Assets<Shader>`; Bevy's shader extraction/cache
must recover automatically, with `CachedPipelineState::Creating` observed for
the replacement pipelines. There is no test-only compiler or ready flag.

At tick 30 compare actual GPU live particles against `StatefulSimulation`, then
retain the original integration suite: ticks 0/3/20/44/80/120, two capacities,
seed invalidation, allocation reuse, death/free-slot reuse, stop cutoff, repeated
presentation without duplicate integration, a five-particle burst exactly once,
two opposite-view alpha permutations, installed draw queues and teardown.
Use a 30-second convergence deadline instead of a frame-count race with worker
compilation. Correct the harness's budget assertion to measure backward-seek
reconstruction from reset tick zero rather than subtracting the old later tick.
All blocking GPU reads/waits remain test-only.

## Actual pipelined renderer → canonical nested host → stage outputs

`host_stage_native::native_020_pipelined_project_outputs_survive_restart_seek_and_teardown`
uses the real plugin with asynchronous compilation enabled. Capture the initialized
device before cleanup, then assert `RenderApp` is no longer in the main app and
`RenderAppChannels` exists. Test-only shared telemetry records graph thread ID and
context-stamped progress; convergence requires that progress, the current main
presentation identity and production receiver's accepted output tick agree. Assert
graph execution is on a different thread. Never manually update a render subapp.

Reuse the canonical clock, project reconciliation, live binding forwarding,
extraction, stage runtime preparation, `StageTimeline`, callback encoder and main
receiver. Two roots share a two-level project with positive source offsets and
inside/outside bound sources. Check:

- Distinct real GPU fluid forces and independent paused/advancing clocks.
- Restart with a large first live frame, followed by nested authored cue delivery
  exactly once with its two-level root path.
- Forward/backward seeks with no reconstructed cues or aggregate impacts; one
  later live tick resumes a single child-local impact.
- Paused frames do not duplicate outputs. Expiring one root's clips preserves the
  other root's outputs. Root teardown leaves no stage owners or stray messages.
- Bevy's own executor-pumping shutdown drains the render channel before the final
  device validation scope is checked. Playback-only retains no snapshots.

## Acceptance boundaries

These are **two complementary gates**, not a combined particle/stage/volume
renderer or complete `AestraPlugin` on 0.20. The stateful test uses a narrow
independent-particle graph driver and synchronous test readback. The threaded
host test retains the four-tick stage driver; its raw-wgpu stage executor does
not itself use Bevy's asynchronous pipeline cache. It qualifies real render-thread
transport and host/output scheduling, not all outer-graph readiness branches.
Test-only telemetry uses a mutex; it is not production synchronization or a
performance measurement. No rendering references are approved.

Full-plugin/outer-graph integration (including coupled readiness, profiling,
volume/lighting presentation and input providers), editor API migration, coherent
shipping graph switch and native visual/performance parity remain open. Stage
impacts retain the legacy child-local contract; choreography is the root/path
routing oracle. Prior held-callback and ordering regressions remain in the suite.

## Validation

Use pinned MSVC Rust 1.98.1, locked/offline dependencies and serial native GPU
processes. The full runner passes **180 checks**: 9 keyboard, 40 upstream focus,
4 cluster units, 4 shader, 3 storage, 96 extraction/shared, and **24 explicit native
gates**. Both new gates use NVIDIA RTX 4070 SUPER / Vulkan, driver 616.92; delayed
readiness passes in **3.23 seconds**, pipelined host outputs in **4.37 seconds**.
Other gates also exercise AMD Radeon Graphics / Vulkan, driver 26.3.1.

The first full run passed the original 22 native gates, then failed to create a
Windows log filename containing the Rust test namespace separator. Replace `::`
with `-` in the two new log names and rerun everything into a fresh directory.
Only the second, complete, source-frozen run is accepted:
`target/bevy-020-qualification/runs/2ef0ebf07ba74bfd97f7c5c3d08c62f6/`.

Executed checks:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --lib --locked --offline
# AESTRA_REQUIRE_GPU_CONFORMANCE=1; restore the previous value afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --test gpu_conformance --test stateful_conformance --test trail_compaction_conformance --test trail_culling_conformance --test domain_spawn --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --features async-qualification --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc fmt --all -- --check
cargo +1.98.1-x86_64-pc-windows-msvc fmt --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml -- --check
```

Shipping host library: **63 passed**. Workspace all-target check, all three strict
Clippy configurations, both formatting checks, PowerShell AST and whitespace
validation pass. Sandbox configuration-access failures required approved
formatting/Clippy retries; no lint was suppressed. Incremental hard-link warnings
fell back to copying. No shipping code or dependencies change in this slice;
unrelated workspace changes are preserved. Changes are not committed.

Shipping renderer library: **157 passed**, three pre-existing native tests remain
ignored. Hardware-required conformance: **40 passed** (31 stateful, 3 general,
4 domain, 1 trail compaction, 1 culling), with the prior environment flag restored.
Final source/executable hash recheck matches the accepted evidence.

The accepted run verifies unchanged source inputs and all five native executable
hashes, including both extraction configurations:

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `6235e04af46c7b6d75de60e1514096bf42e7cf9178d67a751d26eebf308ac0c4` |
| `inputs-after.json` | `08c8613a46c39263120224376856bb9d1d62ea3fb083b2f5c26daf24b1f6f44f` |
| synchronous extraction executable | `5e51850606d247f2e43c6358f8e54a78ddb65f6f07245c203144d407cb426043` |
| threaded extraction executable | `ff39ff064537905724261e10a5fd543b6522fd9eadca6a7e5b7e9d99ebba43ea` |
| candidate lock | `99de6e313138126ede8ed042b190d788ef2cc4274e900587d6196af2c01f0baf` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
