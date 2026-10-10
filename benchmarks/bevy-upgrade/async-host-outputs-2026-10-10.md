# B20-1: asynchronous particle outputs reach the host — 2026-10-10

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The isolated candidate uses
Bevy **0.20.0** / wgpu **30.0.1**. No manifests, locks or shaders change in this slice.

## Shared production modules

- `presented_effect.rs`: move the existing presentation payload, material-binding
  implementation, render-mode enum and instance-owned resource test out of
  `lib.rs`. Existing public exports are retained. Fields consumed by other renderer
  modules are crate-visible; no new public fields or runtime behavior are added.
- `gpu/particle_output_readback.rs`: move the actual asynchronous counter observer,
  arrival readback state and event-link statistics out of `gpu.rs`. The shipping
  preparation path still creates the same child readback and installs the same
  observer. Counter accounting and event delivery algorithms are unchanged.
- `gpu/output_context.rs`: own the existing `AestraOutputEvent` message beside its
  routing context. Preserve the `extension_stages` and public `gpu` re-exports.
- The candidate imports these actual sources, the actual material implementation
  and route high-water marks. Their existing unit tests run under Bevy 0.20 too.

## Native gate

The new test in `coupled_trails_native.rs` compiles a real smoke domain with
Secondary Emission and Spawn From Domain. Ten short-lived persistent trail heads
are born exclusively from the solver; ordinary emitter emission is zero.
Short-lived heads use a sufficiently short authored trail sample interval.

The fixture advances live ticks past the output ring's wrap before adding
`Readback::buffer` to the observed child. Bevy submits/maps its own requests and
triggers the production observer's `ReadbackComplete`. A normal main-world message
reader collects `AestraOutputEvent`. Repeated `app.update()` calls converge on a
bounded count of actual observer completions, not a synchronous GPU drain.
Only after delivery does a small test-only read inspect the GPU ring as a payload
oracle. The test never manually triggers completions or writes ring records.

Contracts:

- All retained nonempty ring records arrive once in tick order across slot wrap;
  each message has its actual local position and accepted cohort count.
- Nested root entity, two-clip path and host playback epoch are preserved.
  Source effect/seed, inherited root time and historical authored motion are
  sampled at the event tick, not the later delivery time. ECS placement contributes
  the actual world-space location.
- Repeated real readbacks while paused emit no duplicates. Restarting the host
  rejects actual reads of the old-epoch ring even before GPU reconstruction; the
  new epoch then emits one fresh cohort.
- Seeking suppresses reconstruction outputs; genuinely live ticks after the
  suppression boundary resume delivery in the current routed epoch.
- Missing GPU inputs reject late completions; effect removal retires its child
  readback and produces no more host messages. Actual trail/view queue bindings
  remain valid and native validation must finish without GPU errors.
- Playback-only remains the default fixture policy with no particle/domain/trail
  snapshots, checked by the shared production-scheduler harness.

## Explicit boundaries

Main-world clock synchronization and lowered input preparation are still fixture
controlled. This is actual asynchronous **particle-output** delivery, not full
`aestra-bevy` host/player scheduling or the extension-stage output mailbox path.
The existing 32-tick ring retains a bounded history: records overwritten before
the first read are not promised, and arbitrary host stalls are not lossless.
Out-of-order/stale high-water behavior has shared unit coverage; the native test
does not force the driver's completion order. Async shader readiness, pipelined
rendering, visual parity and performance remain separate gates.

No production wait/readback, CPU particle mirror, per-particle entity or mandatory
replay is introduced. The final validation-scope wait and payload oracle reads
are native-test-only. The runner adds a nineteenth explicit serial gate, hashes
all newly shared sources and rejects input/executable changes during a run.

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

- Candidate: **155 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 76 extraction/shared and **19 explicit serial native tests**.
- Shipping renderer lib: **151 passed**, three pre-existing ignored native tests,
  no filtering. Hardware-required conformance: **40 passed** — 31 stateful,
  3 general GPU/CPU, 1 trail compaction, 1 per-view culling and 4 domain-spawn/
  accepted-birth-output tests.
- Workspace all-target check and strict Clippy, candidate all-target strict Clippy,
  both formatting checks, PowerShell AST and whitespace checks pass. Configuration
  access failures were rerun with approved access; no lint is disabled.
- Actual asynchronous output delivery ran on NVIDIA RTX 4070 SUPER / Vulkan /
  driver 616.92. Some other gates used AMD Radeon Graphics / Vulkan / driver 26.3.1.
  Native GPU processes ran serially, including shipping regressions.
- No manifests, locks or shaders changed in this slice. The shipping root
  manifest/lock remain unchanged across the preceding candidate slices too.

Accepted source-frozen run:
`target/bevy-020-qualification/runs/e3bad99d235b4b1f8043c6e1755e1678/`.
Source hashes and executable identities stayed unchanged throughout the run.
This supersedes earlier slice evidence for the current shared fixture contents.

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `0d4de6c980e956f0c2d9b96bea3cd5fc9ba9bbb0fa269dfda294ab7149d43ed7` |
| `inputs-after.json` | `82edcf6a55bbe62749ba1cc52bd3296ff0920378ba6f71310f1af0e82461bde5` |
| extraction executable | `afb8d6de9bf75e765da964c09d2c41dfed30822c0b55e7166fcdf153a0e7582b` |
| candidate lock | `47eb58384c394fc9853d918cfdea384881cfed04fcf688c69502e3bdbd46e2ce` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
