# B20-1: host particle-light realization — 2026-10-11

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**; the isolated candidate uses
**0.20.0** / **30.0.1**. Root dependency files are unchanged. This follows the
[stage/volume installer slice](production-volume-installer-2026-10-11.md).

## Implementation

Include the actual host `particle_lights.rs`, `particle_light_gpu.rs`, representative
light adapter and lighting-quality policy in the isolated candidate. Call the real
`AestraParticleLightPlugin`, production light-selector installer, shared outer graph
and actual stage/volume installers. The fixture supplies the same narrow GPU input
preparation helpers used by earlier native gates; it is not the full host plugin.
No substitute selector, readback transport or cluster injector is installed.

Move the engine-specific extraction implementations and shader construction into
`particle_light_boundary_019.rs` and `_020.rs`. Shipping explicitly selects 0.19;
the candidate selects app-labeled `ExtractResource`/`ExtractComponent`, the required
`SyncComponent<RenderApp>`, and WESL. Only the clustered-light import changes; the
shared shader body and native Bevy clustered-light ABI stay unchanged. Use Bevy's
own encase re-export so buffer encoding follows the selected engine version.
Public host types, trait behavior, budgets and realization modes are preserved.

Route the actual selected-light transport through the already qualified mapped-view
boundary. wgpu 30 mapped-view failures now fail decoding closed rather than being
treated as bytes. Both versions release the mapped view before unmapping. No new
production wait, readback, replay requirement or GPU position upload is introduced.

The candidate also executes the real host policy, representative-light, pooled-light,
GPU-slot and transport unit tests: caps, source identity, stale packets, age/frame
expiry, replacement, layers, pool reuse and atomic quality-policy updates.
Its only manifest addition is direct `thiserror` for the included production policy;
the version was already in the candidate dependency graph.

## Required native gate

`host_lights_native::native_020_host_lights_realize_async_and_same_frame_and_retire`
uses a hardware Vulkan adapter, a real stateful `PlaybackOnly` owner and a 128×128
offscreen StandardMaterial receiver. No Aestra sprites, ambient light, bloom or
representative flashes contribute to the image. Tonemapping/dithering are disabled
and an unlit baseline must be black. It verifies:

- Portable async transport creates two shadowless proxies, applies 400-lumen /
  8-unit host clamps, and matches current paused GPU-selected positions after
  catch-up. Transport is bounded to two 112-byte in-flight staging buffers.
- Layer refresh and GPU motion reuse the original proxy entities. Actual receiver
  illumination moves across the image with the selected stars.
- A zero staging budget rejects transport, extinguishes the receiver, then recovers
  with the same proxy allocations when restored.
- Switching to SameFrameGpu removes portable proxies and reserves two zero-lumen
  main-world placeholders. Actual GPU injection illuminates the same receiver
  location without submitting additional selected-light readbacks.
- The injector consumes the exact current selection sequence, and receiver
  illumination follows subsequent GPU movement while slot entities remain stable.
- Non-default camera layers reject injection. Manifest-budget rejection produces
  a black receiver; restoring the budget restores illumination.
- Hiding or despawning the source retires illumination and hides the retained slot
  pool. Zero host capacity finally destroys the slots. The owner uses PlaybackOnly.

The native test's waits and selected-record/screenshot reads are instrumentation
only. Main-world transport statistics intentionally lag render submission, so the
no-further-readbacks assertion samples the shared mailbox after mode transition.
The runner requires this exact ignored test in its own serial process, rejects GPU
error logs, and hashes all host sources, shader and both version boundaries.

## Remaining boundaries

This gate uses a local RenderApp and one root stateful light owner. The real
stage/volume installers are registered together, but this is not a joint volume-
plus-particle-light pixel oracle. It does not certify pipelined host light delivery,
nested host-light realization, full monolithic preparation/transform sync, or the
complete AestraPlugin. Existing selector, volume, extraction and pipelined project
gates remain separate and do not imply combined integration.

Next: combine these installers with real pipelined root/project/live-binding
scheduling and shared outer-graph readiness, then finish editor API compatibility
and the coherent root dependency switch. Visual/performance parity remains required;
test durations are not performance measurements and no references are approved.

## Validation

Accepted source-frozen run:
`target/bevy-020-qualification/runs/b111e17c4157482c8c77516f0f8b69dc/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **218 checks** pass
(9 keyboard, 40 upstream focus, 4 cluster units, 4 shader, 3 storage,
126 extraction/shared, **32 explicitly executed native gates**). No preceding
gate is removed or weakened. All **437 input hashes** and all five native
executable hashes match after shipping unit/check/lint validation.

The new receiver gate passes on NVIDIA RTX 4070 SUPER / Vulkan, driver 616.92,
in **4.19 s** in the frozen run. Some lower-level gates use AMD Radeon integrated
graphics / Vulkan, driver 26.3.1. These are test durations, not performance claims.

Shipping host library: **63 passed**. Shipping renderer library: **157 passed**,
three pre-existing native tests remain ignored. The explicit shipping portable
selected-light receiver/lifecycle test and bounded same-frame GPU adapter/PBR
registration test also pass (**1 each**, executed serially). Workspace all-target check
and strict Clippy with `aestra-bench/gpu`, strict candidate Clippy in both default
and `async-qualification` configurations, both formatting checks, runner parsing
and whitespace validation pass. No lint is suppressed. Incremental hard-link
warnings fall back to copying.

Commands use the pinned toolchain and locked/offline dependencies:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --lib --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --test particle_light_realization --locked --offline -- --ignored --nocapture --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --test particle_light_latency bounded_gpu_adapter_registers_without_selected_light_readback --locked --offline -- --exact --ignored --nocapture --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --features async-qualification --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc fmt --all -- --check
cargo +1.98.1-x86_64-pc-windows-msvc fmt --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml -- --check
```

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `d1008cf6e7a4d6dd5ef468b67353c341e7f550dc8e915cdcd5d48ef0c57043a4` |
| `inputs-after.json` | `a862d8cbd5dea6c9769871ba018ac5a2e1f1d8e9c2ca4b7f98f330bf65d316da` |
| synchronous extraction executable | `65dfb79e14566def6cc12283b3ce1f1a05965c66c080988c467bae77fc6d6e49` |
| threaded extraction executable | `6abd0e8925345a2818f74281c852e80c17f29c5317ccdabc23783d60dfaf3d3d` |
| candidate lock | `fd2d39d93f6811f031a4678e0ff5ff89c3050538eda1a6395b76bce05fddef78` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

Root dependency files and unrelated workspace changes are preserved. Changes are
not committed by this slice.
