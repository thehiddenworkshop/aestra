# Bevy 0.20 production clock and transform synchronization

## Implementation

`gpu/transform_sync.rs` now owns the existing production main-world synchronization
system and its schedule installer. Shipping 0.19 and the isolated 0.20 candidate
call that same installer: after transform propagation, before visibility checking,
with light collection ordered after the shared synchronization set. The existing
host-motion draw, mesh/ribbon bounds and wireframe ordering stays intact.

This moves code, rather than creating a second synchronization implementation.
It preserves simulation clocks, seed/history/statistics tokens, seek quality,
history policy, emission cutoffs, input-event history, physics/world inputs,
homing/attachment resolution, output suppression and trail checkpoint context.
Matrices cross the Bevy/engine-neutral glam boundary as scalar columns; encase
still encodes the unchanged storage ABI. No GPU wait or new readback is added.

The particle statistics component now uses explicit imports. Its geometry sample
payload lives with shared draw records rather than depending on the complete
readback transport. Shipping geometry collection and public statistics behavior
are unchanged. Candidate queues reuse the same selected storage adapter instead
of instantiating another private copy.

## Qualification

The required combined pipelined gate no longer refreshes particle clocks, epochs,
seeds or render globals itself. Its allocator runs only for unprepared owners and
attaches the actual statistics component; the production PostUpdate installer
owns subsequent uploads. The existing clock/binding/stage/light/pixel/profiling/
teardown assertions remain. Additional checks move both paused root placements,
await exact world-position changes in the actual async light proxies, restore
their original positions, and verify particle/global asset handles survive
placement, forward playback, restart and forward/backward seeks.

New non-GPU candidate tests exercise the actual installer with parent transform
propagation and an authored host-motion track. They drain ShaderBuffer CPU data
using 0.20's real RenderAsset API, run multiple host updates before extraction,
and verify exact upload lengths, scalar clock/seed/epoch/token/cutoff lanes,
simulation/render matrices, policy/quality/output suppression and asset reuse.
Rotation normalization retains the existing zero-quaternion fallback. A shared
statistics regression checks geometry-profile invalidation after context changes.

## Limits and next step

The combined native gate still uses fixture-owned **initial analytic allocation**.
It does not yet run monolithic `prepare_gpu_effects`, dynamic emitter/material
preparation, host-motion draw-child synchronization or the complete renderer/host
plugin. Compiling the shared stateful synchronization code is not a new native
homing/attachment/physics oracle. Existing independent stateful/fluid gates remain
required; no nested coupled or joint volume-light scattering claim is added.

Next: share and qualify initial/dynamic particle and material preparation, then
complete full-plugin integration, editor compatibility and the coherent dependency
switch. Shipping remains Bevy 0.19.1; neither lockfile nor approved visuals changes
in this slice.

## Validation

Accepted source-frozen run:
`target/bevy-020-qualification/runs/b96fedce02e545a7b503f25732fcd1ce/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **228 checks** pass
(9 keyboard, 40 upstream focus, 4 cluster units, 4 shader, 3 storage,
135 shared/candidate units and **33 explicit native gates**).
**442 source hashes and five executable hashes** remain unchanged. The strengthened
combined pipelined gate passes in **5.68 seconds** on NVIDIA RTX 4070 SUPER / Vulkan
616.92; low-level native device/shader/storage/timestamp gates also use AMD Radeon
integrated Vulkan 26.3.1.

Shipping renderer library regressions: **158 pass**, three existing ignored tests
unchanged; host library: **63 pass**. Strict workspace Clippy and all-target check with `aestra-bench/gpu`,
strict candidate Clippy in synchronous/asynchronous configurations, both formatting
checks and whitespace validation pass. Native suites run serially. Existing
incremental-cache hard-link warnings fall back to copying; they are not lint failures.

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --features async-qualification --locked --offline -- -D warnings
```

SHA-256 evidence:

| Artifact | SHA-256 |
| --- | --- |
| Synchronous extraction executable | `ae12f7046526cdde83d0c15ec4f7580eabd6dc84e6c2be988047932f83a36e17` |
| Asynchronous extraction executable | `b54ace8bd77e276805198494779d1d65228ac1f7ab5caf95cf7716239bba6230` |
| Candidate lockfile | `fd2d39d93f6811f031a4678e0ff5ff89c3050538eda1a6395b76bce05fddef78` |
| Shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
