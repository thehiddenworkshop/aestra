# B20-1: production outer fluid/history integration — 2026-10-11

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**; the isolated candidate uses
**0.20.0** / **30.0.1**. Root dependency files are unchanged. This builds on the
[shared outer graph](outer-simulation-graph-2026-10-11.md), not another dispatcher.

## Two additional native gates

Retain all preceding gates and the narrow fluid dispatchers as comparisons.
`coupled_trails_native.rs` adds two exact-filter gates using the shipping
`simulation_graph::run_simulation`, `prepare_stage_runtimes`, coupling construction,
world upload, persistent preparation, trail observer, timestamp transport and
installed trail compaction/culling/draw queues:

- `native_020_outer_graph_fluid_and_joint_trails_restore_and_present` runs the real
  linked fluid solver and field-follow particles with capacities **70 / 1025**
  (inline/paged history). Check nonzero density/vertical velocity, actual head
  acceleration, skipped-tick trail observations, pause identity, retained tails
  after head death, expiry, seed/source replacement and teardown. Switch from
  playback-only into optional replay, then compare exact particle/history/domain
  buffers after backward restore. Require a real timestamp window reporting
  **ten ticks and ten observations** for tick **60 → 70**, not reconstruction
  from zero. Verify particle and domain checkpoint availability at tick 60.
- `native_020_outer_graph_domain_births_reach_trails_and_output_rings` reads real
  solver emission records, accepts exactly **10 / 16** requested births, and
  compares their actual positions/half-inherited velocity. Check no rejected
  births are exported, one-shot/paused output-ring identity, slot reuse, trail
  consumers and exact optional replay/restore without duplicate outputs.

The test supplies actual `ExtractedStages` using `from_presented`, then updates
the requested clock/context alongside the existing fixture-owned particle inputs.
The **production preparer** allocates, invalidates and retires domain timelines;
the test does not allocate a substitute outer-mode `StageExecutor`. Link the real
fluid extension before preparation, as a host must do. Set deterministic catch-up
through main-world `AestraCatchupPacing { paced: false }`, extracted normally; the
quality budget is still bounded at 24 ticks. No shipping default changes.

## Required-domain readiness fix

The negative gate initially fails: removing extracted stages leaves the outer
graph's event/trail fallback with an empty domain list, yet particles consume
ticks and publish presentation readiness. A prepared owner with a missing
timeline or field/emission resource has the same risk because optional per-tick
lookups silently skip the dependency.

Validate required field-follow and spawn-from-domain bindings at the shared
`run_stateful_dispatches` entry, before clearing live counters or advancing any
state. Require the referenced stage and its field/emission buffer. If unavailable,
return `presented: false, work: None`. Event-only/link-only effects without domain
requirements remain valid. No new host API, mandatory replay, polling, CPU
readback or production synchronization is introduced.

Native negative checks cover four frames without extracted stages, four with a
`None` timeline, and four with an unavailable emission resource. Verify no tick
consumption or readiness; the birth case also preserves spawn ordinals and its
output ring. Restore the dependency and recheck exact state/draw convergence.

## Evidence semantics and boundaries

Asynchronous production timestamps intentionally skip samples when their bounded
three-slot pool is busy and retain only the latest completed snapshot. Test-only
device waits before/after the specifically required restore window prevent that
window from being dropped. No production timer behavior changes. This is not a
performance measurement. The original native gates still run in separate serial
processes; the two new exact filters must each execute one passing test.

Playback-only asserts empty particle/domain snapshots and zero reported history
capture bytes before opting into replay. The outer history is system-local:
exact restored buffer comparison plus the measured ten-tick/ten-observation
window proves joint restore without exposing private history state for testing.
Old narrow gates retain their direct history-store assertions.

This qualifies production outer **joint fluid/history and domain births**. The
full extension-stage/volume/light presentation installers, outer analytic/mixed
combinations, combined pipelined host integration and complete plugin remain open.
Earlier shader/descriptor/host gates are complementary, not substitutes for that
integration. Editor compatibility, coherent dependency switch and native
visual/performance parity also remain required. No visual references are approved.

## Validation

The source-frozen run is accepted:
`target/bevy-020-qualification/runs/7eaf8aab86b64e2b897f00e66636df80/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **185 checks** pass
(9 keyboard, 40 upstream focus, 4 cluster units, 4 shader, 3 storage,
96 extraction/shared, **29 explicit native gates**). All preceding gates remain
required. All **424 input hashes** and all five native executable hashes match
after the shipping regression checks.

The two new gates run on NVIDIA RTX 4070 SUPER / Vulkan, driver 616.92:
outer joint fluid/trails **5.13 s**, outer domain births **5.31 s**. These are test
durations, not performance claims. No visual references are changed or approved.

Shipping host library: **63 passed**. Shipping renderer library: **157 passed**,
with three pre-existing native tests still ignored. Hardware-required shipping
conformance: **40 passed** (31 stateful, 3 general, 4 domain, 1 trail compaction,
1 culling), with `AESTRA_REQUIRE_GPU_CONFORMANCE=1` and its prior value restored.
Workspace all-target check and strict Clippy with `aestra-bench/gpu`, strict
candidate Clippy in default and `async-qualification` configurations, both
formatting checks, PowerShell AST parsing and whitespace validation pass.
No lint is suppressed. Incremental hard-link warnings fall back to copying.

Commands use the pinned toolchain and locked/offline dependencies:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --lib --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test gpu_conformance --test stateful_conformance --test trail_compaction_conformance --test trail_culling_conformance --test domain_spawn --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --features async-qualification --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc fmt --all -- --check
cargo +1.98.1-x86_64-pc-windows-msvc fmt --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml -- --check
```

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `e510a87a19d8a0237f3248e5086ae08291dda254f92e8cc2db898fdab002315c` |
| `inputs-after.json` | `0bd23e181ee36c314efa48dd83286c4c335d3cba6c4e997e07f7538cc626155a` |
| synchronous extraction executable | `49c5a85192d6d3df100fd7ea9ef11812b84d09513f029f7d9bcc851e6afe28ed` |
| threaded extraction executable | `c5a81fa6edbca8a3498cabd9c584d7fd2502664fe04152d7558bc52fb45deb91` |
| candidate lock | `99de6e313138126ede8ed042b190d788ef2cc4274e900587d6196af2c01f0baf` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

Root dependency files and unrelated workspace changes are preserved. Changes are
not committed.
