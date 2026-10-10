# B20-1: Bevy-dependent shader composition — 2026-10-09

This is the second B20-1 slice after the portable WESL/Naga compiler upgrade.
It prepares and qualifies real Bevy 0.20 shader composition. The shipping workspace
still runs Bevy 0.19.1; the full renderer/editor upgrade is **not complete**.

## Implementation

- Extract production material/volume composition into
  `bevy/aestra-bevy-render/src/gpu/shader_composition.rs`. Existing call sites
  explicitly select the 0.19 dialect. Ray setup, scene-light helpers and plugin
  march functions are shared, not independently reimplemented for the probe.
- Add a test-only 0.20 dialect: WESL `import` / `@if`, real
  `bevy_pbr::render::*` and `bevy_pbr::prepass::utils` module paths, and
  `constants::MATERIAL_BIND_GROUP` instead of Naga Oil's text placeholder.
  No engine-neutral shader imports Bevy.
- Preserve invocation-private grid/world transforms, opaque-depth clipping,
  bounded point-light traversal (including inactive slots), finite attenuation,
  and opt-in/neutral-2D lighting. No new readback, light pool, particle ECS,
  synchronization or replay work is introduced.
- Add `patches-020/shader_composition.rs`, importing that same composition module.
  Resolve libraries from the independent locked Cargo graph, register their
  actual sources with Bevy 0.20's `Shader::from_wesl` / `ShaderCache`, and validate
  the composed output with Naga 30. Use actual MeshRenderPlugin light constants.
  No stand-in View, cluster or UI definitions and no preprocessing Bevy sources
  into approximate WGSL.
- Cover smoke/fire and liquid marchers across storage/uniform and no-depth /
  single-sample-depth / multisampled-depth variants (12); generated lit smoke with
  lighting enabled/disabled and storage/uniform clusters (4); graph grid/wire
  fragment bodies using the real 0.20 UiVertexOutput (2).
  Assert volume bindings, depth binding presence, entry points, one view binding
  and absent clustered lights in the neutral branch.
- The graph adapter changes only the import to
  `bevy_ui_render::ui_vertex_output::UiVertexOutput`. The editor's active embedded
  assets are deliberately still 0.19-compatible. Graph geometry, zoom/DPI
  calculations and derivative antialiasing are unchanged.
- Extend `run-020-patches.ps1` with headless shader checks and explicit native
  module/pipeline validation, serially after both cluster tests. Hash external
  production shader inputs and engine-neutral path-dependency sources as well
  as the fixture/vendor files. Preserve failure logs and require unchanged inputs
  and native executable hashes before producing an accepted summary.

## Executed checks

Pinned Rust `1.98.1-x86_64-pc-windows-msvc`, Windows. No dependency downloads
were necessary; independent-lockfile additions resolved offline.

| Check | Result |
| --- | --- |
| `run-020-patches.ps1 -Native` | 60 passed: 9 keyboard, 40 upstream focus, 4 cluster units, 4 shader composition, 2 native cluster, 1 native shader test |
| Native shader test | All 18 modules and render pipelines validate on AMD Radeon(TM) Graphics, Vulkan, AMD driver 26.3.1 |
| Native cluster tests, rerun sequentially | RTX 4070 SUPER, Vulkan, NVIDIA 616.92; reuse/growth/reset/retirement and private generations/overflow recovery pass |
| `cargo test -p aestra-bevy-render --lib gpu:: --locked --offline -- --test-threads=1` | 104 passed; 3 existing opt-in native tests ignored, 27 unrelated tests filtered |
| Hardware-required `aestra-bevy-render --test gpu_conformance` | 3 passed, no ignored; GPU/CPU agreement for all seven bundled showcase effects |
| Workspace `check --all-targets --features aestra-bench/gpu --locked --offline` | Passed |
| Workspace strict Clippy, same target/features | Passed with `-D warnings` |
| Isolated shader fixture strict Clippy | Passed with `-D warnings` |
| Root and isolated workspace formatting; PowerShell AST; `git diff --check` | Passed |

A one-off differential test compared the pre-refactor composers against the new
0.19 path: full smoke/fire and liquid volume sources and lit-material sources with
composition on/off are byte-identical. The temporary historical-source copy was
removed after this check; it is not an additional maintained shader implementation.
The persistent shared-body regression runs in both qualification and renderer tests.

Formatter/Clippy initially encountered the known sandbox configuration access
denial and passed when rerun with approval. Cargo's sandbox incremental-cache
hard-link warnings fell back to copying; no lint was suppressed.

## Retained evidence

Accepted runner directory:
`target/bevy-020-qualification/runs/636951c664714be68177749a996fef7f/`.
Logs, metadata and before/after source hashes are retained there locally.

- Summary SHA-256:
  `49176f5c7c68194f99138a7ec51dd6cc7d7c3975e5855191427209feccd02874`.
- Shader executable SHA-256:
  `e60ca0b0c7707893018ee23d8f5703b573cbd32043ce7b479e7e0df8b2a7bdf7`.
- Cluster executable SHA-256:
  `15d62e69e4cb1493083765c28e4d50f60bf3d63d677e376f9638c6ea382c1ba0`.
- Independent lockfile SHA-256:
  `0ae7af0e950ff778f0ddb66955d47014c4b26b84fe45e309e10440498484d96f`.
- Shipping Cargo.lock remains unchanged from the portable compiler slice:
  `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

The qualification graph has one Bevy engine (0.20.0), wgpu/Naga 30.0.1 and WESL
0.6.0. Engine-neutral Aestra path dependencies temporarily retain glam 0.32.1
alongside Bevy's 0.33.12. Only shader text crosses this test boundary; no mismatched
math/ECS/render types are passed to Bevy. The root graph still uses its original
0.19 patches; both 0.20 candidates remain confined to this independent workspace.

## Limits and next action

The native test creates real wgpu 30 shader modules and render pipelines with
inferred layouts. Volumes and lit sprites use their real vertex entry points;
UI fragments use a fixture vertex emitting the real imported UiVertexOutput.
This checks shader-stage linkage and resource compatibility, **not** the real
Bevy Material/UiMaterial CPU-side layouts, texture uploads, assets/hot reload,
editor picking/clipping, rendered pixels, HDR/MSAA image parity or frame time.
It creates no window or image capture. Missing native hardware fails the explicit
test rather than counting a skip as qualification.

Next: port extraction, typed shader-buffer and custom rendering APIs together
with B20-2's required editor changes; switch the engine/wgpu/glam graph coherently,
activate `Shader::from_wesl` and the prepared imports, and retire the temporary
dialect bridge. Review renderer/cache identity at that switch. Do not invalidate
the unchanged shipping 0.19 shader output just for this refactor.
Then qualify actual Bevy layouts and native visual/performance parity before
retiring the 0.19 vendor copies or marking B20-1/B20-2 complete.
