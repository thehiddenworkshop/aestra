# B20-1 effect/domain/light extraction metadata — 2026-10-09

Status: fifth compatibility slice, with shipping Bevy 0.19.1 and a separately
compiled Bevy 0.20.0 adapter. The full engine/editor upgrade is not complete.

## Implementation

- `gpu/effect_inputs.rs` contains the actual complete `GpuEffectBuffers`,
  `StatefulDispatch`, `StatefulAppearance`, `HostEventHistory` and `TickSchedule`
  payloads. Their preparation, fingerprinting and runtime methods remain in the
  shipping renderer. `gpu/trail_context.rs` shares the real checkpoint identity
  independently from allocation/copy code; existing trail consumers use it.
- `gpu/stage_inputs.rs` contains the actual `ExtractedStages`, `FieldViewTarget`
  and `VolumeFieldTarget` snapshots. Stage execution, coupled ticking, field
  copying and volume presentation remain in their existing modules.
- `gpu/particle_light_inputs.rs` contains the actual selection inputs, settings,
  mode and stable source/artifact identity. `gpu/particle_light_transport.rs`
  contains the actual readback settings/frame. Public paths are retained through
  reexports; selection kernels, manifests, asynchronous transport and mailbox
  validation are unchanged.
- `gpu/clone_extraction.rs` shares the clone-only implementation while the two
  `gpu/extraction*.rs` adapters explicitly select their engine's trait paths.
  The candidate uses `bevy::extract` and `RenderApp` labels. The root still selects
  only the shipping 0.19 adapter. No mixed Bevy types cross between graphs.
- Effect/stage extraction registration is centralized in the adapter. Light
  registration remains at the light installer; opt-in readback registration
  remains at the readback plugin. Neither transport nor replay becomes mandatory.
- The independent fixture imports all these production types verbatim. The runner
  hashes their sources, tests and actual call sites alongside previous evidence.

No authored schema, shader/storage ABI, host API, CPU particle mirror, per-particle
ECS entities or production device wait is added. Handles and immutable Arcs retain
the same clone policy; no GPU state is copied into the main-world snapshot.

## Shared contracts

Four additional tests execute both versions' real extraction/render schedules:

- Complete effect/dispatch field preservation, shared checkpoint/input/physics
  allocations and tick schedules, independent owners, PlaybackOnly/ReplayEnabled,
  history epoch, simulation time, seek quality, placement and revision updates.
- Complete stage metadata/field targets, sparse-brick layout and table identity,
  compiled artifact ownership, host words, optional SDF, coupled flag and context
  updates. Removing targets/SDF from a snapshot clears previous optional values.
- Removing effect buffers independently, then stage/light components, reinserting
  all three and despawning the owner removes stale render components without
  touching another owner. Source identity retains main-world root/owner entities.
- Light artifact/clip-path changes are not the same occurrence; empty inputs clear
  previous sources. Selection and transport resources copy budget/mode updates,
  explicit zero-light disabling and frame values across u64 wrap.

Required renderer configuration resources are not treated like the optional world
SDF: production consumers require their presence. Disable through settings, rather
than silently removing render copies. These tests check transport metadata, not
completion/ordering of a real GPU readback.

The field targets and light plans use explicit fixture metadata with an actual
compiled basic effect; they do not claim to execute a real fluid solve or particle
light kernel under 0.20. Existing production behavior is separately regression
tested in the shipping graph. Headless schedules also do not establish pipelined
thread behavior, visual parity or frame-time/allocation improvements.

## Executed validation

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib gpu:: --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --test extraction --locked --offline --target-dir target/bevy-020-qualification -- -D warnings
# AESTRA_REQUIRE_GPU_CONFORMANCE=1; previous environment value restored afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test gpu_conformance --locked --offline -- --test-threads=1 --nocapture
```

- Candidate runner: **77 passed** (9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 13 extraction/mesh, 4 explicitly executed native checks).
  Native checks ran serially, without overlap with shipping GPU tests.
- Shipping renderer GPU modules: **116 passed**, three pre-existing native tests
  ignored, 27 unrelated lib tests filtered. New shared contracts pass on 0.19 too.
- Hardware-required GPU/CPU conformance: **3 passed**, including all seven bundled
  showcase effects; no missing-device skip accepted.
- All-target workspace check/strict Clippy, candidate extraction strict Clippy,
  both formatting checks, PowerShell AST parsing and whitespace validation pass.
  Toolchain configuration-read sandbox denials were retried with approved access;
  hard-link cache warnings used the existing copy fallback.

Accepted final run: `target/bevy-020-qualification/runs/bfde2b589b4348d5a28d8ea1b29895e8/`.
Inputs/native executables remained unchanged during the run; one Bevy version and
the expected local patches were selected. Native cluster tests used NVIDIA RTX
4070 SUPER/Vulkan (616.92), shader/storage tests AMD Radeon Graphics/Vulkan (26.3.1).

| Artifact | SHA-256 |
| --- | --- |
| Accepted summary | `04f9924053e937cb09d0e6eed59525837bb7ecf924d7be039ea9b79c3f9b1892` |
| Accepted input manifest | `f9b3c6737cba12b2b8778503952651572670a213c2badd1ca167936c7b50960f` |
| Extraction executable | `8cce759fc021cc1091037d1a53f294dabba65ed4b9d771d41b0711aaf65db57c` |
| Independent lockfile | `06b75595dc9f7365be47f23df2ec6bda4dd65a07c43a0a978f7180e3ba4b70a6` |
| Unchanged shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

## Remaining

Renderer settings/custom extraction systems, custom render commands/pipelines/view
APIs, editor compatibility and the coherent root Bevy/wgpu/glam switch still need
their ports and native integration/parity gates. Select the candidate adapters only
with that coherent switch; do not retire shipping copies or approve changed visual
references based on these metadata tests.
