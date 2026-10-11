# B20-1: production outer simulation graph — 2026-10-11

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The isolated candidate remains
Bevy **0.20.0** / wgpu **30.0.1**. Neither root dependency file changes.

## One graph implementation

Move the shipping `run_simulation` system, its local history stores and resource
parameters into `gpu/simulation_graph.rs`. Both shipping and the candidate now
compile that same system. Its analytic, independent-stateful, coupled, trail,
mesh-count-copy, diagnostics and timestamp branches are not fixture copies.

Move the existing particle world upload and domain/event coupling construction
to `gpu/stage_coupling.rs`, and share the current-frame `PresentedOwners` marker
through `gpu/presentation_readiness.rs`. The existing shipping installers still
schedule these systems at their original points. No authored format, public host
API, simulation algorithm, replay policy or production synchronization changes.

## Fix independent presentation readiness

The previous `run_stateful_dispatches` result conflated two different conditions:
an independent emitter presents successfully but has no shared lockstep work
count; a trail emitter waiting for lockstep pipelines cannot present. Both were
`None`, and the outer graph used `is_some()` as the particle-light readiness gate.
Independent stateful particles therefore never became eligible for selection.

Return explicit `presented` and optional `work` fields. Successful independent
presentation now publishes readiness while retaining **unknown**, not invented,
shared tick/checkpoint counters. Waiting trail dispatch remains unready. Coupled
dispatch retains its measured work. Apply the same distinction to the legacy
mixed branch's presentation marker. The candidate gate would stall at tick zero
without this fix even though the persistent allocation and bind group existed.

## Three new native gates

Retain every preceding native gate and its narrow driver as a comparison. Add:

- Production outer graph → independent persistent simulation → real alpha sort
  and installed draw queues. Retain the CPU position oracle, two opposite views,
  capacities 129/257, seed/allocation invalidation, tick-zero/forward/backward
  advancement, death reuse, stop cutoff, repeated time, one-shot burst and teardown.
  Enable hardware pass timestamps and require actual asynchronous timestamp samples.
- Production outer graph → canonical event coupling → three particle generations,
  two links, external bursts and two output rings → actual alpha/draw consumers.
  Require the expected 32 live particles, exact routed birth counts, paused
  deduplication, bounded catch-up, timestamped multi-tick work and retirement.
- Threaded shader cache with the **production outer graph**, retaining local
  render-world inspection. Withhold the real stateful shader dependency for eight
  frames at tick 30, assert no ticks/carry/ordinals are consumed and allocations
  survive, observe actual asynchronous creation, then rerun the full CPU/GPU and
  draw/profiling oracles after recovery.

`outer_graph_native.rs` wires production resources, initializers, preparation and
the system's normal graph ordering. Its observers only collect readiness and
timestamp evidence; they do not simulate particles. GPU reads/waits are test-only.
The shared fixture now packs the production empty physics header rather than
passing a zero-byte placeholder that the old narrow drivers never consumed.

The runner hashes the three new shared production modules, keeps separate
synchronous/threaded executable hashes, and executes all native gates serially.
Each new exact filter must report one passing test; missing filters are failures.

## Acceptance boundaries

This qualifies the full system's **independent and linked-stateful branches**,
including real timestamp transport and async shader readiness. It does not yet
execute its analytic/mixed or fluid-plus-joint-history branches through this outer
system, nor install the complete renderer/host plugin. Earlier gates still cover
those dispatch helpers, actual domains/trails, volume shaders, host outputs and
pipelined nested-host scheduling separately. The new readiness assertion observes
the particle-light eligibility marker, not a complete light-selection render.

Next: extend this shared graph integration to joint fluid/trail scheduling and
the production extension-stage/volume/light presentation installers, then combine
it with pipelined host scheduling. Required editor compatibility, the coherent
dependency switch and native visual/performance parity remain open. No rendering
references or fireworks art are changed or approved.

## Validation

The complete source-frozen run is accepted:
`target/bevy-020-qualification/runs/908024c68fa54c4ea1f10b873ba89bb3/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **183 checks** pass
(9 keyboard, 40 upstream focus, 4 cluster units, 4 shader, 3 storage,
96 extraction/shared, **27 explicit native gates**). All **424 input hashes**
remain unchanged; the runner verifies all five native executable hashes.

The three new gates run on NVIDIA RTX 4070 SUPER / Vulkan, driver 616.92:
independent outer graph **3.81 s**, coupled outer graph **3.50 s**, threaded
delayed outer graph **3.75 s**. These are test durations, not runtime performance
claims. Previous gates, including pipelined nested-host output delivery, still pass.

Shipping host library: **63 passed**. Shipping renderer library: **157 passed**,
three pre-existing native tests remain ignored. Workspace all-target check with
`aestra-bench/gpu`, strict workspace Clippy with the same feature set, strict
candidate Clippy in both default and `async-qualification` configurations, both
formatting checks, PowerShell AST parsing and whitespace validation pass.
No lint is suppressed. Incremental hard-link warnings fall back to copying;
approved retries resolve sandbox configuration-access failures.

Hardware-required shipping conformance: **40 passed** (31 stateful, 3 general,
4 domain, 1 trail compaction, 1 culling), with
`AESTRA_REQUIRE_GPU_CONFORMANCE=1` and its prior value restored afterward:

```powershell
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test gpu_conformance --test stateful_conformance --test trail_compaction_conformance --test trail_culling_conformance --test domain_spawn --locked --offline -- --test-threads=1
```

The final recheck matches every accepted source hash and all five executable
hashes after the additional shipping checks.

Commands use `cargo +1.98.1-x86_64-pc-windows-msvc` with `--locked --offline`:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --lib --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --features async-qualification --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc fmt --all -- --check
cargo +1.98.1-x86_64-pc-windows-msvc fmt --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml -- --check
```

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `3a50ca3fb4df5cb6cb2aaee724e6776d75671e150ed50959e153534ad0468582` |
| `inputs-after.json` | `9064983dc16e4a33d32214e8c10c42b4d9741bbd0e7ceeb2368c2ccf72c46b12` |
| synchronous extraction executable | `bf2b94375f549f0acbdcd7b0a8ffb125b6c30a8d44f02322d898d387a621a13a` |
| threaded extraction executable | `20c449c04491928117e253820120258fc7d288772d2bda899f4550398f412dfb` |
| candidate lock | `99de6e313138126ede8ed042b190d788ef2cc4274e900587d6196af2c01f0baf` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

Root dependency files and unrelated workspace changes are preserved. Changes are
not committed.
