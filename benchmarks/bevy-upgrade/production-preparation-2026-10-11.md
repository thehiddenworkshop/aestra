# Bevy 0.20 production particle and material preparation

## Implementation

`gpu/preparation.rs` now owns the existing initial particle allocation, live
emitter/renderer/material input updates, fallback textures, shader cache and
trail-statistics readback setup. Shipping and the isolated candidate compile the
same systems and preparation installer. Initial preparation retains its existing
`AestraRenderSet::Prepare` ordering; live inputs still run after initial preparation.
The root module reexports existing public components without changing their API.

Project-root texture/mesh loading and its cache move intact into `project_assets.rs`,
including the existing root-switch/labelled-mesh regression. Host-motion draw
identity lives with draw records. Bounds cross the Bevy/engine-neutral glam
boundary as scalar vectors/matrix columns; the existing bounds algorithm and
encase storage ABI remain unchanged. No root dependency or lockfile changes are
needed for this slice.

A narrow shader boundary preserves shipping WGSL composition and selects the
qualified WESL composition/asset constructor for 0.20. Both single-sampled and
multisampled generated material assets pass through that boundary. Existing
shader fingerprints/cache behavior remain shared. Timing components now have
explicit imports so the candidate includes the actual observers and regressions.

## Qualification

The combined threaded native gate no longer allocates particle buffers, draw
records, statistics or light-pool ranges with a fixture allocator. It waits for
real device capability publication and assigns the fixture's backend status;
production preparation performs allocation and live updates. Existing nested
host-clock, output, light-mode, receiver-pixel, profiling, restart/seek, placement,
buffer-reuse and teardown assertions remain required.

Five new regressions run from the same source on shipping and candidate engines:

- Allocation happens once; live render mode, layers and sampling updates retain
  particle/global/renderer/render-parameter handles and child counts.
- Pending/CPU owners and empty effects do not allocate particles; unsupported
  capacity produces explicit CPU fallback rather than an invalid allocation.
- The readback backend owns one correctly parented readback and no native draws.
- Persistent trails allocate the actual history/scratch/indirect resources and
  context-owned counters/readback, retaining those handles across updates.
- Material value edits change uniforms without recompiling shaders or growing
  buffer assets; removing the binding clears the semantic draw binding.

A separate 0.20 regression compiles actual shader assets returned by production
preparation through Bevy's real ShaderCache and published shader libraries. It
covers unlit, scene-depth and authored lit-smoke programs, both sampling variants,
and lighting enabled/disabled. It checks WESL source metadata, not just standalone
generated strings. Existing mesh/ribbon bounds, sampling, project asset and
timing regressions also run in the candidate.

The first full run exposed a pre-existing asynchronous host-stage probe that
mistook a sendable system's worker thread for the render schedule caller. Bevy
can execute that work on the host thread while it waits for rendering. The probe
now records the caller with an exclusive render-graph system ordered after the
simulation. The actual render-app/channel checks, separate-thread assertion and
all output/seek/lifecycle assertions remain intact; production scheduling is not
changed. The unsuccessful run is retained at
`target/bevy-020-qualification/runs/1dbca711f29d4956920338800b013848/`.

## Limits and next step

The combined native gate still uses analytic particles with dense fluid stages.
It is not a new combined stateful/fluid lockstep or volume-light scattering oracle;
the independent stateful/coupled/history gates remain required. The candidate
still assembles installers explicitly rather than invoking the complete shipping
renderer/host plugins. Main-world host-motion draw-child synchronization,
bounds/visibility/wireframe scheduling and full-plugin bootstrap remain to qualify.

Next: remove that draw-child/visibility and plugin-bootstrap boundary, including
combined coupled readiness. Editor API compatibility, the coherent root
Bevy/wgpu/glam switch and native visual/performance parity remain open. Shipping
stays on Bevy **0.19.1**; this slice does not approve changed visual references.

## Validation

Accepted source-frozen run:
`target/bevy-020-qualification/runs/3f311dc3a00d476e8bcc61a172e80230/`.
Pinned MSVC Rust **1.98.1**, locked/offline dependencies: **249 checks** pass
(9 keyboard, 40 upstream focus, 4 cluster units, 4 shader, 3 storage,
156 shared/candidate checks and **33 explicit native gates**).
All **454 source hashes and five executable hashes** remain unchanged during the
run. The combined pipelined gate using production preparation passes in **5.02
seconds**; the corrected separate-render-thread host-stage gate passes in **4.48
seconds**. These run on NVIDIA RTX 4070 SUPER / Vulkan 616.92; low-level native
device/shader/storage/timestamp gates also use AMD integrated Vulkan 26.3.1.

Shipping renderer library: **163 pass**, three existing ignored tests unchanged.
Host library: **63 pass**. Engine-neutral ribbon bounds: **2 pass**. Strict
workspace Clippy/all-target check with `aestra-bench/gpu`, strict candidate Clippy
in both configurations, both formatting checks and whitespace validation pass.
GPU suites run serially. Existing incremental-cache hard-link warnings fall back
to copying; sandbox-denied Clippy/format checks pass when rerun with approved
access. No lint suppression or assertion removal is added.

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-gpu --lib ribbon_bounds --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --features async-qualification --locked --offline -- -D warnings
```

SHA-256 evidence:

| Artifact | SHA-256 |
| --- | --- |
| Synchronous extraction executable | `e81f26143dc05bde617328ed044793a179fba1159d938c375bea9c4104134925` |
| Asynchronous extraction executable | `594f0b1645ff8c06ad16d363c49e24dca004d14b45434daa0d81e5aa3ab4dd15` |
| Candidate lockfile | `fd2d39d93f6811f031a4678e0ff5ff89c3050538eda1a6395b76bce05fddef78` |
| Shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
