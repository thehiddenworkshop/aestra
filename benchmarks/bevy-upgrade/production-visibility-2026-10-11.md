# Bevy 0.20 production draw-child and visibility wiring

## Implementation

`gpu/visibility_updates.rs` now contains the existing main-world installer for
draw-child host-motion transforms, exact global matrices, mesh/ribbon bounds,
wireframe geometry and replay/pixel-floor culling policy. Shipping and the
isolated candidate call the same installer, including production clock/transform
synchronization. The systems and their ordering are moved intact: local poses
precede transform propagation, exact global poses precede bounds/wireframe and
render-global uploads, and bounds/culling policy precede visibility checks.

Existing visibility regressions move to `gpu/visibility_tests.rs` and run on
both engine graphs. The only math bridge uses engine-neutral scalar arrays for
ribbon bounds; no storage ABI, bounds algorithm or culling behavior changes.
The qualification runner hashes both new shared files.

## Qualification

The combined real pipelined native gate now installs production visibility wiring
instead of only transform uploads. Read-only render-thread instrumentation observes
actual extracted `GpuDrawInstance` records, keyed by their main-world child IDs.
It does not manufacture draw records or force `ViewVisibility`.

The strengthened gate verifies:

- Production-created nested draw children survive paused root-placement edits,
  and their actual extracted sorting centers match propagated bounds/transforms.
- Hiding one root retires its extracted draws without removing the other root's
  draws or deleting main-world children.
- Showing it restores the same draw-child IDs and centers; subsequent playback,
  restart and seeks retain the original particle/global buffers.
- Existing host-clock, binding, stage readiness/output delivery, both light modes,
  receiver-pixel, profiling and teardown checks remain required.

The shared unit regressions retain all existing assertions: sampling overrides
are opt-in and reversible across renderer/blend kinds, while draw globals and GPU
matrix uploads agree under nested host tracks, clip offsets, nonuniform/negative
scales, shear and repeated/backward seeks. Ribbon culling uses the same transform.

## Limits and next step

This removes the main-world draw-child/visibility fixture boundary, not the complete
plugin-bootstrap boundary. The candidate still assembles installers explicitly.
Its combined gate uses analytic particles and dense stages, not a new coupled
stateful/fluid readiness or volume-light scattering oracle. Independent coupled,
history, queue and draw-command gates remain necessary.

Next: qualify the complete renderer/host plugin bootstrap and combined coupled
readiness. Required editor API changes, the coherent root Bevy/wgpu/glam switch
and final native visual/performance parity remain open. Shipping stays on Bevy
**0.19.1**. This slice neither approves changed references nor adopts new widgets.

## Validation

Accepted source-frozen run:
`target/bevy-020-qualification/runs/a4ab3ba6b2064626afc893d87e910c58/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **251 checks** pass
(9 keyboard, 40 upstream focus, 4 cluster units, 4 shader, 3 storage,
158 shared/candidate checks and **33 explicit native gates**).
All **456 input hashes and five executable hashes** remain unchanged during the
run. The strengthened combined pipelined gate passes in **5.21 seconds** on NVIDIA
RTX 4070 SUPER / Vulkan 616.92. Low-level native device/shader/storage/timestamp
gates also use AMD integrated Vulkan 26.3.1. These are correctness checks, not a
new performance or image-parity baseline.

Shipping renderer library: **163 pass**, three existing ignored tests unchanged.
Host library: **63 pass**. The shared visibility regressions execute on both engines.

Strict workspace Clippy/all-target check with `aestra-bench/gpu`, strict candidate
Clippy in both configurations, both formatting checks and whitespace validation
pass. Sandbox-denied candidate Clippy and formatting checks pass when rerun with
approved access. Existing incremental-cache hard-link warnings fall back to copying.
No lint suppression, assertion removal or new ignored test is added. GPU suites
run serially.

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
| Synchronous extraction executable | `60763eb6fc2f99f4f32a4ead4f5d7512e7badf6324ba7efcc533b05925908cc9` |
| Asynchronous extraction executable | `6a05e5a305d8ba32bf4e3aff1272b67e0f984096f146437245b6b69916c434fb` |
| Shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
