# Material-preset preview pipeline — Bevy 0.20 qualification

Date: 2026-10-20. B20-2 slice 87. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

- `material_graph/preset_previews.rs` shares the production preview composition,
  cache/status types, Update raster system, status presentation, recipe fingerprint,
  preset program resolution and standard material seed.
- The native EditorMaterialGraphPlugin delegates the actual registrar. Existing
  spawn/resolution/seed entry points remain available to native asset and property
  consumers. The Update system retains its previously unordered scheduling.
- The candidate imports this real implementation, the existing shared CPU evaluator
  and actual tooltip component. No replacement renderer, catalog or status widget
  is introduced.
- Recipe-only cache keys, project publication-version observation, built-in catalog
  fallback, ready/incompatible/failed presentation and cache pruning are unchanged.
  Preset raster generation remains synchronous as before; this compatibility slice
  does not change its performance model.
- Frozen hashing includes the module, both test files and all bundled project
  preset recipes resolved by the shared catalog test.

A source audit confirms all seven extracted production function bodies are
unchanged. Three existing tests move to the shared module, preserving their
assertions: the full bundled preset preview check, published-version refresh
without resource change ticks, and recipe-versus-metadata fingerprints. The
bundled fixture locator is adjusted to find the same repository assets from both
manifest roots; it does not substitute or reduce the registered preset catalog.
Remaining native material-root tests are unchanged.

## Shared regressions

Eleven new cases exercise:

- Actual clipped preview dimensions, minimum sizes, border/radius and noninteractive
  child-label composition.
- The complete registrar deduplicates image assets across views without changing
  the effect or semantic history.
- Idle catalogs leave cache, image and label change ticks untouched.
- Late-added requests mount cached pixels without rewriting existing views.
- Metadata publication reuses images; changed recipes replace them.
- Removal clears old pixels, displays actionable failure, prunes cache, and recovers
  after the preset is republished.
- Same-ID presets in another project revalidate their changed recipe.
- Malformed project files become unavailable without poisoning built-in requests.
- A conflicting project catalog really triggers the built-in fallback.
- Ready, failed and incompatible transitions remove stale images/tooltips/labels;
  missing status-label entities are handled safely.
- The resolved program renders the same pixels as preset preview construction;
  unknown presets are rejected/incompatible.

The tests distinguish malformed-file indexing from catalog construction errors:
malformed entries have no preset identity and are excluded by the real project
index, whereas a project descriptor colliding with a built-in identity makes
catalog construction fail. Neither path is simulated by replacing the catalog.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/7242b1c103744e2a9537b756be43b3d9/`.
It executes **2,246 passing checks**, including **1,814 editor graph checks**
(109.86 seconds after compilation), with the existing ignored editor timing probe.
Counts are executions across gates, not unique tests. All **1,035 source input
hashes** are unchanged. Native execution is false.

The complete candidate graph suite also passes with default parallel execution:
1,814 passed, 1 existing ignored timing probe, no failures (18.56 seconds).
Both strict all-target Clippy gates, both formatting gates and whitespace checks
pass. Root manifests, lockfile, toolchain and CI workflow are unchanged.

Shipping passes **2,729 editor unit tests + 3 architecture tests**, with default
parallel execution, the same ten existing ignores and no failures.
Workspace GPU-feature all-target checking passes. Post-documentation input drift is zero.

## Remaining work

Continue with saved-material asset-preview validation, then remaining material
host/plugin composition, application handlers and complete editor workflows.
This qualifies headless preset CPU rasterization and image/status component
publication, not browser GPU thumbnail orchestration or full native editor
acceptance. Native picking, shader output, rendered zoom/DPI and complete
project/document workflows remain acceptance work before switching shipping
dependencies. No visual references are approved.
