# B20-1: actual trail producers to installed queues — 2026-10-10

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The independent candidate is
locked to Bevy **0.20.0** / wgpu **30.0.1**. This slice is producer integration
qualification, not the coherent workspace engine switch.

## Implementation

The candidate imports production trail compaction and per-view culling directly,
including installation, embedded shader loading, PipelineCache readiness checks,
preparation, buffer allocation/rebinding, compute passes and retirement.
Explicit imports remove ambient dependencies on the shipping renderer module.
Shared simulation/compaction/culling sets retain same-frame ordering before
render-graph drawing, inside Bevy's graph diagnostic window.

The existing bounded asynchronous timestamp transport is extracted from host
profile reporting into a shared module. Preparation timing uses the real
extracted effect owner, context token and time. Three in-flight slots, 256-owner
budget, latest-frame mailbox sequencing, timestamp conversion, separate
resolve/copy command buffers and nonblocking backpressure are retained.
Public timing/profile APIs remain re-exported at their existing paths.

Narrow 0.19/0.20 adapters handle Bevy's GPU resource-wrapper removal, RenderDevice
construction in tests and wgpu 30's fallible mapped view. Failure yields an empty
diagnostic snapshot and releases the reservation instead of retaining stale
timings. Array-based conversions cross Bevy's glam 0.33 and the neutral GPU
crate's glam 0.32 without changing the uniform/storage ABI.

No production allocation policy, trail algorithm, shader, buffer usage flags,
playback/replay choice, CPU particle mirror or GPU wait is added.

## Native contracts

`trail_native::native_020_trail_producers_feed_installed_queues_and_fail_open`
installs the actual producers alongside actual extraction, render preparation,
queues and registered commands. Normal `App::update` runs ShaderBuffer/Image
asset preparation, embedded WESL loading and all compute stages before drawing.
Two real image-target 3D cameras deliberately receive different GPU decisions;
CPU visibility stays conservative so GPU culling is actually exercised.

The contract checks:

- Exact indirect words and stable compacted candidate indices at **70/1,025
  owners**, including the extra prefix-page stage beyond 1,024 owners.
- Exact buffer identities consumed by both installed draw commands, not merely
  prepared records. Same-capacity source replacement rebinds reused output;
  owner-count growth replaces it.
- Camera movement refreshes cull uniforms. A stale history epoch fails open,
  current known-empty bounds reject, and expired segments rebuild to zero.
- Three controlled dispatch fallbacks after a successful frame: compaction only,
  culling with the full-range source, and direct full-range drawing. Preparation
  resets dispatched flags so old output cannot masquerade as current work.
- Removed cameras, hidden draws and final destruction retire producer entries.
- Real asynchronous producer timestamps preserve owner/token/time metadata;
  timestamp-capable Vulkan hardware is required for this gate.

A second explicit native contract calls the shared timestamp regression with
hardware required: bounded in-flight reservations, allocation backpressure,
resolve/copy completion, work metadata and slot recycling. Shipping retains its
existing default timestamp test; candidate native tests stay opt-in and serial.

History records are **seeded test inputs**, not actual simulation. Dispatch gates
exercise routing/reset behavior, **not asynchronous shader compilation or the
pipeline-not-ready branch itself**. Pipeline compilation is synchronous and
rendering is headless/nonpipelined. Staging, readbacks and bounded waits are test
observers only. This is not pixel, transparency-quality, allocation/performance
parity or the full simulation-to-render acceptance gate.

## Validation

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
# With AESTRA_REQUIRE_GPU_CONFORMANCE=1, restoring its previous value afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test trail_compaction_conformance --test trail_culling_conformance --test gpu_conformance --locked --offline -- --test-threads=1 --nocapture
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
```

- Candidate: **131 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 59 extraction/shared tests and **12 explicit native tests**.
- Shipping renderer lib: **151 passed**, three pre-existing ignored native tests,
  no filtering. Hardware-required shipping conformance: **5 passed** across
  GPU/CPU showcase parity (all seven showcases), trail compaction and trail culling.
  These GPU processes also ran serially after the candidate suite.
- Workspace all-target check and strict Clippy, candidate all-target strict
  Clippy, both formatting checks, PowerShell AST parsing and whitespace checks
  pass. Sandbox configuration-read denials were retried with approved access;
  existing incremental hard-link warnings use Cargo's copy fallback.

Accepted final run:
`target/bevy-020-qualification/runs/f4e835d174c048b1acfed20d7aeac7c8/`.
Source and executable hashes remained unchanged. Native processes ran serially.
The installed trail gate used NVIDIA RTX 4070 SUPER / Vulkan / 616.92; the
standalone timestamp gate used AMD Radeon Graphics / Vulkan / 26.3.1.
Both lockfiles remain unchanged.

| Evidence | SHA-256 |
| --- | --- |
| Final summary | `bf0f89a3ccdcbcb52ed0f66c08f32a68f47b863058474e7f8b3530f6a925a95a` |
| Input inventory after run | `2b1f34b5831196746fb1ccbc19279294e069803a3023f00c393cf7411006118f` |
| Extraction/native executable | `8aa7456b8f88f85d40ac3759bb7dfa3bf0eb70d70c4e8fb5fc5224dd89137577` |
| Candidate lockfile | `23b7862f310867ef34697ab57f15cf043c229c793183d4c5bbe35166bf3bafdb` |
| Shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

The earlier successful run `918831cd300246f0ae0f46fa46d57d0a` preceded the final
opt-in timestamp-test arrangement. The final run above supersedes it.

Next: qualify actual simulation feeding the installed producers and queues,
then pipelined rendering/asynchronous readiness, alongside required editor API
migration. Full visual/performance gates and the coherent engine switch remain.
