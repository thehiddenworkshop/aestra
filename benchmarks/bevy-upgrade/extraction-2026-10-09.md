# B20-1 draw and optional-resource extraction — 2026-10-09

Status: shipping 0.19 refactor/cleanup with a separately compiled real 0.20 adapter.
**Not** a completed renderer/editor engine upgrade.

## Implementation

- `gpu/draw_instance.rs` contains the actual production `GpuDrawInstance` and its
  complete semantic-material binding, shared wireframe geometry and sprite-cull
  bounds. Existing wireframe/culling modules reexport the moved types internally.
  Extraction still clones handles/Arcs, updates the world-space mesh center and
  sprite-cull matrix, and leaves the main-world component unchanged.
- `gpu/world_sdf.rs` contains the actual host `AestraWorldSdf` resource. Its public
  paths, revision/clear behavior and Arc-backed packed field data are unchanged.
- `gpu/extraction.rs` installs the current 0.19 traits/plugins. The separately
  compiled `gpu/extraction_020.rs` uses `bevy::extract` and explicit `RenderApp`
  parameters on `SyncComponent`, `ExtractComponent`, `ExtractResource` and plugins.
  The root still selects only the 0.19 adapter.
- `gpu/extraction_cleanup.rs` removes the optional render-world SDF when absent
  from the main world. Upstream `ExtractResourcePlugin` does not propagate resource
  removal; the stale copy previously prevented consumers from seeing absence.
  Existing particle/stage preparation already accepts an optional SDF and produces
  an absent field/header when appropriate. Required config resources are untouched.
- `gpu/extraction_tests.rs` is one shared contract source used in both graphs.
  The 0.20 fixture imports the real production types, policies and adapter by path.
  It has no stand-in draw components, world fields or mesh-input definitions.
- The migration runner includes these tests and hashes both adapters, the payload,
  cleanup/tests and actual module call sites along with its other inputs.

Explicit `None` for a hidden draw is retained deliberately: merely filtering out
hidden entities can leave the previous extracted component in the render world.
Resource absence cleanup and resource presence extraction are disjoint; deferred
commands apply at the real render schedule's extraction-command boundary.

## Contract scope

Six identical tests execute each version's actual `ExtractPlugin`, entity syncing,
extraction schedule and deferred render-command application, without a GPU/window:

- Every draw handle, order/range, owner, material, indirect/trail field and render
  mode arrives unchanged. Material program/uniforms and geometry keep Arc identity.
  Render preparation sees the extracted draw; main payload is not mutated.
- Visible → hidden removes the render draw; visible again restores it using current
  transform and bounds, including the sampled sprite-cull matrix.
- Removing/reinserting the component cleans/replaces old optional data without
  changing the other owner's draw.
- Despawn followed by bounded batch recycling tests the same entity index with a
  different generation before the next extraction. Old render entities disappear;
  replacement data survives and final teardown leaves zero draws. The test does
  not assume that the allocator immediately reuses a freed ID.
- SDF updates preserve revision and Arc word identity; `clear()` publishes the
  existing absent eight-word header with a new revision.
- Removing the host resource removes the render copy. Reinsertion and loss of only
  the render copy both recover on subsequent extraction.

The candidate additionally checks the actual component's safe removal with no
render app (the upstream 0.20 fix) and runs the two actual mesh-input/layout tests.
No production GPU waits, readback, CPU particle mirror, replay dependency, authored
schema, shader ABI or host API change is introduced.

## Execution evidence

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, Windows, locked offline dependencies:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib gpu:: --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --test extraction --locked --offline --target-dir target/bevy-020-qualification -- -D warnings
# With AESTRA_REQUIRE_GPU_CONFORMANCE=1, restored after testing:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test gpu_conformance --locked --offline -- --test-threads=1 --nocapture
```

- Candidate runner: **73 passed**: the previous 64 keyboard/upstream-focus/cluster/
  shader/storage checks plus nine extraction/mesh-input checks. All four ignored
  native fixtures explicitly execute sequentially after the headless tests.
- Shipping GPU renderer modules: **112 passed**, three pre-existing native tests
  ignored, 27 unrelated lib tests filtered out. The six shared extraction contracts
  pass in the shipping 0.19 graph as well as the independent 0.20 graph.
- Shipping hardware-required GPU/CPU conformance: **3 passed**, including all seven
  bundled showcase effects; no missing-GPU skip accepted.
- Workspace all-target check/strict Clippy, candidate extraction strict Clippy,
  both formatting checks, PowerShell AST parsing and `git diff --check` pass.
  Sandbox toolchain/configuration-read denials were retried with approved access.
  Sandbox hard-link warnings used the existing copy fallback.

Accepted run: `target/bevy-020-qualification/runs/6dfb2ae6022644a6b7d3fa32d4b14684/`.
The runner confirmed one Bevy version (0.20.0), both local patches selected,
unchanged recorded inputs/executables, and no native ERROR/panic output. No
dependencies or shipping lockfile changed in this slice.

| Artifact | SHA-256 |
| --- | --- |
| Accepted summary | `04f9924053e937cb09d0e6eed59525837bb7ecf924d7be039ea9b79c3f9b1892` |
| Accepted input manifest | `e321393d6c813202e46efd928a185b1eec5af72d30c02f851ab8df2c9c43240a` |
| Extraction executable | `9d8072c8745a87a46b3847912abb1d67ba096b7d178b6f27f18c4f04509618e7` |
| Independent qualification lockfile | `06b75595dc9f7365be47f23df2ec6bda4dd65a07c43a0a978f7180e3ba4b70a6` |
| Unchanged shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

Native cluster checks used NVIDIA RTX 4070 SUPER/Vulkan, driver 616.92; shader/
storage checks used AMD Radeon Graphics/Vulkan, driver 26.3.1. The new extraction
tests are deliberately GPU-independent; native buffer/shader checks do not turn
them into a visual or frame-time benchmark.

## Remaining migration

This slice qualifies draw/SDF ECS lifecycles, not all Aestra extraction. Effect
buffers, extension-stage snapshots, particle-light resources, custom draw commands,
pipeline layouts/view APIs and editor compatibility still need the coherent 0.20
port. Headless schedules do not establish pipelined-thread behavior, full renderer
integration, visual parity or performance. Do not select the candidate adapter in
the shipping graph or remove legacy copies until those gates pass.
