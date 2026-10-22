# Material preview-worker orchestration — Bevy 0.20 qualification

Date: 2026-10-20. B20-2 slice 86. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

- `material_graph/node_previews.rs` now imports its explicit production dependencies
  and exposes the actual worker registrar. The native EditorMaterialGraphPlugin
  delegates registration; its previously unordered Update worker remains in Update.
- `material_graph/preview_pixels.rs` holds the unchanged deterministic material
  evaluator, pixel conversion, synthetic surface sampling and image construction.
  Workers, native graph/preset previews and native asset thumbnails share this code.
- The 0.20 candidate imports both production modules. It no longer suppresses
  dead-code warnings for preview state on the assumption that raster consumers
  are absent.
- Two running-job slots, snapshot/dependency-closure matching, cooperative scanline
  cancellation, per-project/program/target/instance keys, stale-but-valid cached
  images, asset publication and failure retry behavior are unchanged.
- Frozen hashing includes the evaluator and both new test files. There are no
  manifest, lockfile, pinned toolchain or CI workflow changes.

The existing three worker tests now execute in both suites. Three existing
pixel/basis tests move unchanged from the native root into a shared test module.
Remaining native material-root tests and assertions are preserved.

The new test harness retains the production system's actual Local<Jobs>. Pending
tasks are used only to make cancellation and slot occupancy deterministic; actual
production raster jobs cover completion, replacement and retry. Its small
test-only parameter adapter checks 0.20's fallible parameter validation while
accepting 0.19's direct tuple, without alternate worker implementations.

## Shared regressions

Nine new orchestration cases cover:

- Real registrar queues asynchronously, deduplicates views, publishes identical
  actual CPU pixels, and leaves authored effect/history/focus unchanged.
- Stable requests reuse assets without dirtying the preview-cache change tick.
- Instances and expression/output targets keep separate cache entries.
- Two occupied slots do not admit a third request; closing only one duplicate
  does not cancel its shared request; closing the final view frees a slot.
- Closing all views cancels outstanding work without publishing an image.
- A completed obsolete snapshot is discarded before publication; cached pixels
  remain visible until the edited snapshot completes.
- Project replacement cancels the old root even when program IDs match.
- Failed jobs do not publish/cache an image, and unchanged requests can retry.
- Dropping the system's local state signals cancellation for all outstanding jobs.

Two further pixel tests check cancellation between scanlines and the unchanged
difference between permissive graph fallback and strict asset rejection.
Existing shared tests check disconnected additions/deletions, immediate
cancellation, rebuild-time mounting, expected image size/content and mesh basis/
normal-map evaluation.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/e13fecf0cad342ac89ad3cb8cd5a75e5/`.
It executes **2,232 passing checks**, including **1,800 editor graph checks**
(110.37 seconds after compilation), with the existing ignored editor timing probe.
Counts are executions across gates, not unique tests. All **1,024 source input
hashes** are unchanged. Native execution is false.

The complete candidate graph suite also passes with default parallel execution:
1,800 passed, 1 existing ignored timing probe, no failures (14.65 seconds).
Both strict all-target Clippy gates, both formatting gates and whitespace
validation pass. Root manifests, lockfile, toolchain and CI workflow are unchanged.

Shipping passes **2,718 editor unit tests + 3 architecture tests**, with default
parallel execution, the same ten existing ignores and no failures.
Workspace GPU-feature all-target checking passes. Post-documentation input drift is zero.

## Remaining work

Continue with the material-preset preview pipeline, then remaining material-plugin
composition, application handlers and full editor workflows. This qualifies
headless CPU raster orchestration and image-asset publication, not native GPU
preview rendering, the complete MaterialGraphPlugin or editor acceptance.
Presets and asset-thumbnail orchestration still need their own candidate
integration even though they share the evaluator. Native picking, shader output,
rendered zoom/DPI and full project/document workflows remain acceptance work
before switching shipping dependencies. No visual references are approved.
