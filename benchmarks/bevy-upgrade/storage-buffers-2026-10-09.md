# B20-1 storage-buffer boundary — 2026-10-09

Status: implemented in the shipping 0.19 renderer, with a separately compiled and
native-qualified 0.20 adapter. **Not** a completed engine/editor upgrade.

## Implementation

- `bevy/aestra-bevy-render/src/gpu/storage_encoding.rs` preserves the current
  encase 0.12.1 WGSL storage layout. Vec3 padding, matrices, record arrays and raw
  word arrays are encoded, never cast from Rust struct memory. Hardcoded offset/
  size assertions cover simulation globals (112 bytes, matrix at 32, cutoffs at
  96/100), render globals/parameters (80 bytes) and particles (48 bytes).
- `gpu/storage_buffers.rs` is the current Bevy 0.19 adapter. All eleven constructors,
  four updates, indirect usage and CPU-byte reads in `gpu.rs` use this boundary.
  Regression tests compare the actual emitter/renderer/particle artifact and all
  other uploaded record kinds with the original `ShaderBuffer::from` bytes and
  metadata. Constructor ownership avoids the original extra Vec clone; no measured
  performance improvement is claimed. The existing checkpoint copy is unchanged.
- `gpu/storage_buffers_020.rs` is the migration candidate. Bevy 0.20 accepts owned
  `Vec<T: NoUninit>` data, not encase ShaderType structs directly. This adapter feeds
  owned encoded **bytes** to the real API and clears/reinitializes data on updates.
  It reads CPU bytes only as `u8`, with no stronger CPU alignment assumption.
- `patches-020/storage_buffers.rs` imports the actual encoder/candidate source and
  uses the real Aestra compiler/runtime/GPU types. The independent graph contains
  one Bevy version (0.20.0); differing engine-neutral/Bevy glam types never cross as
  ECS components or raw Rust memory.
- `run-020-patches.ps1` includes the headless fixture and a fourth serial native
  test, hashing the production call sites, adapters and encoder as well as the
  existing inputs. The shipping Cargo.lock was not changed by this slice.

No shader ABI, authored format, cache identity, playback policy, production GPU
wait, per-particle entity or new CPU particle mirror was introduced.

## Checked behavior

Headless tests verify every uploaded record kind, WGSL offsets, replacement rather
than concatenation on repeated updates, asset extraction moving CPU bytes while
retaining the GPU size, and reinitialization after extraction, including grow/shrink.

The hardware test calls Bevy's actual `GpuShaderBuffer::take_gpu_data` and
`prepare_asset`, with `RenderDevice`, `RenderQueue`, real `Assets<ShaderBuffer>` and
`RenderChangedShaderBuffers`. It verifies:

- Exact native upload/readback bytes for emitter, renderer, particle and all three
  globals/parameter record kinds, as well as indirect command words.
- Same-size upload reuses buffer identity without reporting changed bindings.
- Growing, shrinking, relabeling or changing usage replaces identity and reports
  the asset ID for binding invalidation. Indirect usage survives updates.
- Extracted initialized data cannot be uploaded again without a host update.
- A GPU-owned uninitialized asset grows with `copy_on_resize`, preserves its GPU
  prefix, zero-initializes the tail and does not acquire a CPU particle mirror.

Readback and bounded blocking polls exist only in the explicit native fixture.
Production ownership/extraction remains nonblocking.

## Execution evidence

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, Windows, offline locked dependencies.

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib gpu:: --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --test storage_buffers --locked --offline --target-dir target/bevy-020-qualification -- -D warnings
# With AESTRA_REQUIRE_GPU_CONFORMANCE=1 (restored after the test):
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test gpu_conformance --locked --offline -- --test-threads=1 --nocapture
```

- Candidate runner: **64 passed** (9 keyboard + 40 upstream input focus + 4 cluster
  units + 4 shader composition + 3 storage units + 2 native cluster + 1 native
  shader + 1 native storage). Ignored native fixtures were explicitly executed.
- Shipping renderer GPU modules: **106 passed**, three existing native tests
  ignored, 27 unrelated lib tests filtered out. Includes the new storage parity/
  offset tests, same-frame transform upload and existing telemetry/native readbacks.
- Workspace all-target check and strict Clippy pass; candidate storage Clippy passes.
  Sandbox configuration-read denials were retried with approved access. Local
  sandbox hard-link warnings are environmental; compilation used copy fallback.
- Shipping hardware-required GPU/CPU conformance: **3 passed**, including seven
  bundled showcase effects; no skip was accepted. Root and candidate formatting,
  runner PowerShell AST parsing and `git diff --check` also pass.

Accepted run: `target/bevy-020-qualification/runs/27fec300f3bf448ba43ed7fa3aa2b315/`.
The runner verified a single engine, both candidate patches selected, all recorded
inputs unchanged, no native ERROR/panic output and unchanged native executables.

| Artifact | SHA-256 |
| --- | --- |
| Accepted summary | `04f9924053e937cb09d0e6eed59525837bb7ecf924d7be039ea9b79c3f9b1892` |
| Storage executable | `8222627ffcf95aa6cf64ef1f452be34d068d1d11c3c77e1d5bf5ec4ed82365ae` |
| Independent qualification lockfile | `06b75595dc9f7365be47f23df2ec6bda4dd65a07c43a0a978f7180e3ba4b70a6` |
| Unchanged shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

Storage/shader native device: AMD Radeon(TM) Graphics, Vulkan, driver 26.3.1.
Cluster native device: NVIDIA RTX 4070 SUPER, Vulkan, driver 616.92. This validates
buffer operations, not comparative performance on those devices.

## Remaining migration

Select the candidate adapter only when the root Bevy/wgpu/glam graph switches
coherently. App-labeled ECS extraction derives/manual implementations, component
removal/visibility cleanup, WgpuWrapper removal, fallible mapped ranges, custom
render phases/material layouts and required editor API changes remain to be ported.
The tested asset extraction methods do **not** establish the full ExtractSchedule
ordering/lifetime contract. Changed-buffer IDs are verified signals, not proof
that all Aestra custom bind groups refresh correctly on 0.20. Preserve that explicit
integration gate, native visual/performance checks and full B20-3 acceptance.
