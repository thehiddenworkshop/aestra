# B20-1: production extension-stage/volume installers — 2026-10-11

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The isolated candidate uses
**0.20.0** / **30.0.1**. Root dependency files are unchanged. This follows the
[production light-selector slice](production-light-installer-2026-10-11.md).

## Implementation

Include the actual production `extension_stages.rs` and `volume.rs` in the
candidate, calling both real installers. This exercises normal main-world volume
creation, stage-input synchronization, extraction, runtime preparation, extension
dispatch, field-to-volume copy, PBR material preparation and camera rendering.
No pre-filled density textures or substitute stage dispatcher is installed.

Replace parent wildcard imports with explicit dependencies. Order extension
dispatch after the existing `SimulateEffects` set rather than importing the
monolithic renderer function. Move the unchanged public `AestraRenderSet` to a
small shared module and keep its original root re-export. No public API changes.
The outer-graph fixture has a second installation entry point so it does not
register field-follow initialization/world preparation twice when the real stage
installer already owns them. Existing native gates keep their original wiring.

Split the volume shader creation boundary into `volume_shader_019.rs` (shipping
WGSL/import composition) and `volume_shader_020.rs` (candidate WESL/import
composition). Both use the previously qualified shared composition bodies.
`volume.rs` delegates composition and `Shader` construction through this boundary;
the root renderer continues selecting 0.19. No solver/shader-body changes,
shipping polling, extra readbacks or altered playback policy are introduced.

The existing portable Naga volume-body test explicitly retains the 0.19 source
format it strips for its standalone parse. Candidate shader composition, real
material compilation and native rendering independently validate the 0.20 path.
The 0.20 dialect is still test-only until the coherent root dependency switch.

## Required native gate

`volume_native::native_020_production_volume_installer_copies_live_fields_draws_and_retires`
requires a hardware Vulkan adapter and uses a 64×64 offscreen 3D camera with a
depth prepass. The authored smoke effect has **no particle emitters**, ensuring
volume-only progress does not accidentally depend on particle counts. It checks:

- Actual stage progress reaches ticks 30 and 45, using normally extracted
  presentation time and `PlaybackOnly` policy, with zero domain checkpoint bytes.
- The generated dense RGBA16Float texture matches the live scalar density buffer
  cell by cell within half-float tolerance, contains nonzero density, remains
  identical when paused, and changes on forward playback.
- Actual volume material pixels are nonblack against a black background. A
  same-layout ambient edit changes rendered pixels while retaining the material,
  volume entity and density image.
- Changing grid resolution from 16³ to 24³ replaces old geometry/texture targets
  and the new image again matches the actual live field.
- A real stage timestamp callback reaches `GpuStageTiming` through main-world
  delivery. The bounded latest-batch mailbox can discard older samples when
  paused frames supersede them: test-only device completion observes a dedicated
  advancing frame before submitting a paused one. Production transport is unchanged.
- Automatic debug slices are suppressed while a volume is drawn. Removing the
  3D camera retires the volume and exposes a real nonempty debug-slice texture;
  restoring a camera replaces volume targets and retires the automatic slice.
- Despawning the owner removes its child volume entities and render-world stage
  timelines. A device validation error scope stays empty.

All copies, mapped reads and waits are test-only. The test adds COPY_SRC permission
to its observed images; production image usage flags are unchanged. The native
runner requires this exact ignored filter, serially in its own process, in
addition to every prior gate. Source hashing includes the new fixture, shared
scheduling module and both shader-boundary files.

## Boundaries still open

This is dense, independent, volume-only integration. Bricked/sparse volume
textures/table copies, coupled particles plus volume presentation, multiple
overlapping volumes, transformed/nested volumes, opaque occlusion/camera-inside
pixel oracles, complete plugin integration and combined pipelined scheduling
remain open. Existing descriptor tests cover additional shader variants but do
not substitute for those integrated native cases.

Host particle-light realization (portable async and same-frame GPU cluster
admission), editor compatibility, coherent root dependency switch and visual/
performance parity are still required. No visual references are approved and
test durations are not performance measurements.

## Validation

Accepted source-frozen run:
`target/bevy-020-qualification/runs/377fccda63854fe8b2cd4ee60a7c2f24/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **196 checks** pass
(9 keyboard, 40 upstream focus, 4 cluster units, 4 shader, 3 storage,
105 extraction/shared, **31 explicitly executed native gates**). Ordinary tests
still ignore opt-in native filters; the runner executes each required gate
separately. No preceding gate is removed or weakened. All **429 input hashes**
and all five native executable hashes still match after shipping regression checks.

The new volume gate runs on NVIDIA RTX 4070 SUPER / Vulkan, driver 616.92, in
**4.50 s**. Some lower-level native gates use AMD Radeon integrated graphics /
Vulkan, driver 26.3.1. These are test durations, not performance claims.

Shipping renderer library: **157 passed**, three pre-existing native tests remain
ignored. Shipping host library: **63 passed**. Shipping staged-fluid conformance:
**3 passed**, with `AESTRA_REQUIRE_GPU_CONFORMANCE=1` and its prior value restored.
Workspace all-target check and strict Clippy with `aestra-bench/gpu`, strict
candidate Clippy in both default and `async-qualification` configurations,
both formatting checks, PowerShell AST parsing and whitespace validation pass.
No lint is suppressed. Incremental hard-link warnings fall back to copying.

Commands use the pinned toolchain and locked/offline dependencies:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --lib --locked --offline
# AESTRA_REQUIRE_GPU_CONFORMANCE=1 for the following test; restore its previous value afterward.
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test staged_fluid_conformance --locked --offline -- --test-threads=1 --nocapture
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --features async-qualification --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc fmt --all -- --check
cargo +1.98.1-x86_64-pc-windows-msvc fmt --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml -- --check
```

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `b369d3dbbbaa342ef7cfb4325f4ef52b0f30ddc212b1c09865878780c122d98d` |
| `inputs-after.json` | `91b57d693839989c54cd0c0322f939f94aa13d536a2c71b0850d380beb7079af` |
| synchronous extraction executable | `ed45f50e5553bc5a6988f7065b211b5d9b7c0ce6fa61f451deb21593baf6eacf` |
| threaded extraction executable | `582abe0b7dd07c754c6ff87570b4bb0eb9cde704b0330858144bb4b4191cc10e` |
| candidate lock | `99de6e313138126ede8ed042b190d788ef2cc4274e900587d6196af2c01f0baf` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

Root dependency files and unrelated workspace changes are preserved. Changes are
not committed.
