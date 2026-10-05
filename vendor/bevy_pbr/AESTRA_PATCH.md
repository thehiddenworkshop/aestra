# Bevy 0.19.1 GPU cluster buffer reuse patch

Published `bevy_pbr` 0.19.1, retaining its MIT/Apache-2.0 licenses and embedded
shader/LUT assets. Original crates.io checksum:
`244ae7d618b51a59c913c36b0564a295cd85ea91a5e777b542bb7bdb846a23d6`.
Cargo uses this directory through the root `[patch.crates-io]`; no Cargo registry
files are edited. `UPSTREAM_SHA256.json` records the imported published files;
only `src/cluster/gpu.rs` is intentionally changed.

## Reason and localized change

Native `prepare_clusters_for_gpu_clustering` creates new `ViewClusterBindings`
and `ViewGpuClusteringBuffers` on every view/frame. F7E4B3B2A's repeated matrix
observed replacements on every measured observation, even in selection-only
controls with constant sizes. This is not Aestra light-slot despawning.

- Reuse the existing per-view components before reserve/upload; initialize only
  missing components. Keep index, offsets, Z-slice, scratchpad and metadata GPU
  buffers at high-water capacity. No old-handle clone cache is introduced.
- Clear logical public contents/counts every frame, reserve the current grid and
  current adaptive index capacity, and refresh all native metadata as before.
  Reset scratchpad **logical length** before adding the current grid, preventing
  unbounded length growth; the unchanged allocation shader zeroes scratchpad
  counts before populate. Z-slice storage grows only when its capacity requires it.
- Convert old uniform bindings to storage if GPU preparation resumes after CPU
  clustering. Keep existing native adaptive growth and asynchronous staging reuse;
  this does not disable resize warnings or raise native overflow limits.
- Drop owning containers and referring native bind groups for inactive views
  without extracted cluster configuration. Removed render entities drop normally;
  the existing readback map is pruned. wgpu retains resources needed by submitted
  commands; exact fence retirement is not claimed.

No shaders, public renderer APIs, native light-selection policy or Aestra runtime
algorithms are changed. The patch applies to all workspace hosts using this pinned
Bevy PBR, not only the viewer; general Aestra particle-light layer support remains
restricted separately.

Cargo patches are not transitive: consumers outside this workspace must apply an
equivalent patch in their own root manifest or use an upstream version with this
fix. Publishing an Aestra crate alone does not ship this Bevy dependency override.

## Regressions and removal

`cargo test --locked -p bevy_pbr --lib aestra_reuse_tests` checks repeated content/
count reset, growth/shrink and uniform-to-storage initialization without a GPU.
The ignored `cluster_buffer_reuse` Aestra Bevy integration test runs native GPU
binding identity windows, grid growth/shrink, independent views, inactive/
reactivated/removed cameras, CPU/GPU mode switching and final light retirement.
Run it explicitly and alone; it is not an ordinary CI timing test.

F7 allocation matrices use the existing no-churn/workload/retirement gates without
relaxing acceptance. Full private allocation/in-flight and deliberate overflow
qualification are still separate from public-list reuse.

On upgrading Bevy, check whether upstream preserves per-view containers and
equivalent reset/growth/lifetime behavior. Run the native regression and repeated
allocation matrix against the replacement, then remove this patch and directory
together when equivalent behavior passes. Do not silently repoint the patch at
a different version or maintain edits in the user's registry cache.
