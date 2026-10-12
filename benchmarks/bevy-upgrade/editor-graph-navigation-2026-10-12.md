# B20-2 slice 8 — Graph navigation, materials and bounded solvers

The isolated Bevy 0.20 graph now imports production viewport navigation,
coordinate transforms, wire/grid UI materials and bounded snapping/resize
solvers. Shipping remains Bevy 0.19.1; root manifests and lockfile are unchanged.
This is the first graph slice, not a complete node-graph or editor port.

## Implementation

- `feathers/node_graph/navigation.rs` owns the existing viewport/canvas state,
  pan capture, cursor-centered wheel zoom, frame requests and staged canvas
  transform updates. The shipping plugin still schedules these same systems in
  the same order. Material/function callers retain their existing public names
  through reexports; document ownership and semantic history are not moved.
- Narrow cursor exports select Feathers cursor types on 0.19 and picking cursor
  types on 0.20. Both expose the real upstream resource/component types.
- `feathers/node_graph/materials.rs` retains the production `UiMaterial` and
  `AsBindGroup` implementations, defaults and shipping embedded shader paths.
  Grid and snapping share one spacing constant; no copied candidate materials
  or synthetic graph components replace production types.
- The isolated editor feature set now enables `bevy_ui_render`, allowing the
  real UI material implementations to compile through the Bevy facade. The
  candidate lockfile gains that dependency edge only; no package versions change.
- Pan motion now converts logical window pixels to UI units exactly once,
  without applying display scale or graph zoom again. Foreign-window wheel
  messages no longer zoom the primary graph. Removing a captured viewport now
  clears its pan state and cursor override instead of retaining a dead owner.

## Required regression gate

The source-frozen runner requires `editor-graph`. It hashes the full graph source
subtree and the shared resize profiling source, alongside the existing widget,
shader and fixture inputs. All earlier editor gates rerun with the expanded
feature set; their earlier passing results are not assumed to remain valid.

The gate runs **33 checks**, with one existing manual timing probe ignored:

- Ten new shared navigation checks use real ECS schedules, input resources,
  window messages and native cursor types. They cover all nine 1x/1.5x/2x
  UI/display-scale combinations, overview/normal/close graph zoom, MMB and
  Space+LMB capture, final release motion outside the viewport, wheel unit
  equivalence, one-time consumption, foreign windows, overlay blocking,
  hover-gated frame requests, vanished owners and staged canvas ownership.
- Two new shared material contracts check actual asset handles, independent
  grid camera uniforms, wire defaults and unchanged embedded shader paths.
- Six existing soft-snapping checks retain screen-space thresholds and
  deterministic bounded behavior.
- Fifteen resize/insertion checks include the existing bounded-operation probe
  and two new insertion regressions: anchored drops, neighbor displacement,
  stale atomic application, frozen conflicts and exhausted budgets.

## Remaining scope

Node press/drag/modified-drag and marquee pointer observers, measured geometry
collection, document/view routing, overlay controllers and their full-plugin
integration still need candidate qualification. Existing shipping coverage for
those paths remains enabled. This fixture does not stub them and does not claim
they compile on 0.20 yet. Real UI material trait compilation and existing shader
composition gates are not native graph-render or visual acceptance.

Next: qualify node/marquee pointer observers together with their production
geometry/ownership boundary, then application action dispatch and full editor
workflows before switching the root dependency graph coherently to Bevy 0.20.

## Source-frozen validation

Accepted **headless** run:
`target/bevy-020-qualification/runs/d185bd54ea54453baaccdd77b6f3a1d6/`.
Pinned MSVC Rust 1.98.1, locked/offline: **441 executed checks pass**
(9 keyboard, 8 theme, 18 controls, 46 numeric, 63 color/dependency,
32 custom text/menu/dependency, 15 tooltips, 33 graph, 40 upstream focus,
4 cluster units, 4 shader, 3 storage, 166 shared/candidate). Dependency checks
deliberately repeat across gates; these are executions, not unique tests.
All **587 input hashes** remain unchanged. The graph gate takes **0.36 seconds**
after compilation; its existing manual timing probe remains ignored.

Shipping lock SHA256 remains
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Candidate strict all-target Clippy, both formatting checks and whitespace
validation pass. Existing incremental-cache hard-link warnings fall back to
copying. No new renderer-native or full-editor visual evidence is claimed.

Shipping validation: **1,240 editor unit + 3 architecture tests pass**, with
ten existing ignores unchanged. Strict all-target workspace Clippy with
`aestra-bench/gpu` passes using the pinned toolchain, locked and offline.
All-target workspace checking with the same GPU feature set also passes.
