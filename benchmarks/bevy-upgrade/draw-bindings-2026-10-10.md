# B20-1: production draw commands and bindings — 2026-10-10

Shipping remains Bevy **0.19.1**. The independent qualification graph remains
locked to **0.20.0**. This slice exercises actual custom render commands with real
Bevy view/prepass bindings and mesh allocations; it does not switch the engine.

## Implementation

- Move the actual five 2D/3D command tuples and their components into
  `gpu/draw_commands.rs`. Share the unchanged mesh/effect/material/depth preparation
  systems in `gpu/draw_preparation.rs`. Shipping installation, queue policy and
  schedule ordering remain in `gpu/render.rs`.
- Share producer-owned sort, trail-compaction/culling and bounded submission records
  in `gpu/draw_resources.rs`. The shipping compute producers and draw consumers use
  the same resources. Preserve dispatched gates, buffer ownership, indirect offsets,
  fallback precedence and the 2,048-command diagnostic ceiling.
- Isolate the prepass accessor change: shipping calls `depth_view()`, while the
  candidate calls 0.20's `depth_only_view()`. Only the coherent engine switch will
  select the candidate adapter in production.
- Move the existing portable sampler translator beside the material layout
  translator. No material, particle or authored ABI change; no new runtime wait,
  particle mirror, per-particle entity or replay requirement.
- Extend the independent fixture with actual production wireframe preparation and
  its existing regressions, a bounded submission-frame contract and a seventh
  serial native test. The runner hashes all new shared sources and their producer
  callsites and rejects changed inputs/executables or native error logs.

## Native command gate

`draw_native::native_020_commands_submit_with_real_mesh_view_and_prepass_bindings`
creates real headless Bevy cameras, render targets, GPU images, extracted views,
depth prepasses, view bind groups, RenderMesh assets and MeshAllocator slices.
Production preparation systems create Aestra's effect, material and scene-depth
groups. Real registered DrawFunctions execute all five production command tuples
through TrackedRenderPass; validation scopes cover encoder submission as well as
resource creation. Recorded submissions prove commands did not silently skip.

The matrix submits **18 draws**: two 2D variants, plus eight variants for each of
the 1x/4x MSAA 3D views. Cases are legacy sprites, generated semantic sprites,
depth fade, indexed/nonindexed deformed triangle meshes, deformed mesh wireframes,
authored point-lit fireworks smoke with its uniform slot, and a fixture-textured
variant of that smoke with real texture/sampler/uniform bindings. The final variant
adds a sampled mask in the fixture only; it does not change the authored smoke asset.
Uniform values and a white texture are fixture inputs, not visual references.

3D descriptors use Bevy's **actual ViewKeyCache**, matching the production queue.
An initial fixture using an incomplete hand-built key failed native validation:
its expected view layout omitted bindings present in the depth-prepass/tonemapping
view group. Using the real key resolves that setup error and guards against
mistaking descriptor-only validation for working view-binding integration.

Additional routing checks seed producer work records without running their compute
passes. They verify undispatched-sort and missing-sorted-group skips, actual sorted
group preparation, view-cull > compact > direct fallback by recorded BufferId,
missing-effect-group and unprepared-mesh skips. Mesh preparation reuses unchanged
buffers and removes stale geometry; prepass loss removes the stale depth group
without affecting the other view. Native waits are bounded and test-only.

This is a command/binding/submission gate, **not** a complete compute-to-render
schedule, pixel comparison, smoke-quality assessment or performance benchmark.
Actual sorting/compaction/culling dispatch and full renderer integration remain
separate gates. Shaders still compose through real ShaderCache/WESL libraries,
then flat WGSL enters PipelineCache with the production explicit descriptors.

## Validation

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, locked offline dependencies, Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --all-targets --locked --offline --target-dir target/bevy-020-qualification -- -D warnings
# AESTRA_REQUIRE_GPU_CONFORMANCE=1; previous environment value restored afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --test gpu_conformance --locked --offline -- --test-threads=1 --nocapture
```

- Candidate: **120 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 53 extraction/shared-module checks and 7 explicitly executed
  native tests. Five existing wireframe regressions and one bounded submission
  contract account for the six additional headless checks; composition helper
  coverage remains shared with the shader target.
- Shipping renderer lib: **150 passed**, three pre-existing native tests ignored,
  no filtered tests. Moving the production modules preserves existing regression
  coverage and the public API; the engine remains 0.19.1.
- Hardware-required GPU/CPU conformance: **3 passed**, including all seven bundled
  showcases, with no missing-device skip accepted.
- Workspace all-target check/strict Clippy, independent all-target strict Clippy,
  both formatting checks, PowerShell AST parsing and whitespace validation pass.
  Configuration-read sandbox denials were retried with approved access. Existing
  incremental hard-link warnings use Cargo's copy fallback.

Accepted run: `target/bevy-020-qualification/runs/8190742da6bf46399c7c34a2ed33ce97/`.
All native processes were serial, without overlap with shipping GPU tests. Cluster,
pipeline and command gates used NVIDIA RTX 4070 SUPER / Vulkan / 616.92; shader,
storage and publication gates used AMD Radeon Graphics / Vulkan / 26.3.1. Source
and executable hashes stayed unchanged during qualification. The shipping and
independent lockfiles are unchanged from the preceding slice.

| Artifact | SHA-256 |
| --- | --- |
| Accepted summary | `ed37474a60aa0cc99af423b372a5703aa09513d01d36a9607160f2f3fa2762f9` |
| Accepted input manifest | `3a511eb5a6ef432574e465eca028b909a0ee7fdbce3f86cb918567fb83c4e461` |
| Extraction / pipeline / command executable | `e500869eac689f181cd2812acdc35b4a41a96448df8232e191d82a11e0eb932d` |
| Independent lockfile | `23b7862f310867ef34697ab57f15cf043c229c793183d4c5bbe35166bf3bafdb` |
| Unchanged shipping lockfile | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |

## Remaining

Full queue/schedule integration, B20-2 editor compatibility, the coherent
Bevy/wgpu/glam graph switch and selection of prepared adapters, renderer/cache
identity review, and native visual/performance parity remain required. This slice
does not claim B20-1 or the complete upgrade finished.
