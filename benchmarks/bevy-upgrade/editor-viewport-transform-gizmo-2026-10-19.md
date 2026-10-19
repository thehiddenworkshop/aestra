# B20-2 slice 69: transform gizmos and host-motion viewport consumers

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** editor candidate now
executes the actual transform-gizmo consumers and complete viewport host-motion
module with the real session, timeline, module registry, modal protection,
camera projection, gizmo state and semantic command/history implementations.

`viewport/transform_gizmo.rs` separates 20 existing functions from native scene/
plugin setup: settings, material setup, focus/proxy synchronization, transactional
drag handling, precision/snapping, target/command selection, conversion, visual
feedback, cursor-following readout and screen-space marker sizing.
`viewport/gizmo_state.rs` holds the original shape/TRS records. Existing crate
re-export paths for the interaction resource and transform conversion remain intact.
The candidate imports the actual host-motion picking/drawing module, not substitute
hover state or command implementations; its timeline facade re-exports the
already-qualified production active/busy/pose operations.

Source audits confirm:

- All 20 extracted function bodies are unchanged. The remaining viewport-root
  function bodies and test helpers, native plugin implementation
  and existing root test body are unchanged.
- All five host-motion function bodies and its six existing regressions are
  unchanged. Only explicit imports and a new test-module registration are added.
- All eleven relevant record definitions retain their original fields/layouts
  after parent-module visibility normalization. No serialization, runtime API,
  root dependency/toolchain, semantic transaction or playback policy is changed.
- The candidate uses shipping's existing explicit-ECS-signature lint policy for
  these systems; strict all-target linting is not globally relaxed.

## Regression coverage and limits

**36 shared tests** comprise 27 transform-gizmo tests (13 mirrored shipping
regressions and 14 new cases) plus nine new host-motion consumer tests.
The six original host-motion tests also execute unchanged in the candidate,
for **42 slice-specific candidate cases**. The broader focused viewport gate
passes **193 tests**.

Coverage includes:

- Original settings/hit scale, absolute and precise snapping, modifier reanchoring,
  TRS conversion, active-modifier readouts and world-rotation/axis-scale feedback.
- Emitter and effect-clip drag preview/commit/undo, closed-seam host-pose TRS edits,
  one undo step per drag, no-op/multiple-proxy behavior, and fluid module position
  handles that retain live-edit classification and reject host-driven inputs.
- Escape/late release, changed selection or document revision, effect locks and
  pending reviews cancel without committing a stale transform. Modal protection,
  hovered host markers and shape handles take focus before standard TRS.
- Actual live value-label text, cursor clamping/visibility, tagged visual-root
  transforms and child global-transform propagation.
- Actual Camera projection and viewport clipping for pose hover/click, selected
  marker centers left available to the TRS gizmo, missing/invalid camera/window/
  cursor inputs, navigation buttons and hovered toolbar/canvas ownership.
- Clicking a pose changes selection, not authored state, undo history or the
  playback clock. The test expectation preserves that existing selection-only
  behavior; no seek was added to make a fixture pass.
- Bounded dense-track sampling, closed-seam picking, screen-space sizing and
  fallback, plus real GizmoAsset line/strip output from the production path draw
  system. Inactive motion retires its current gizmo handle.

These are headless ECS, command/history, projection and CPU gizmo-asset checks,
not native GPU/visual/picking acceptance or installation of the entire 0.20
ViewportPlugin. Shape-specific dragging/drawing, remaining preview composition/
setup and full application scheduling/workflows stay separate. No visual
reference or performance change is approved.

## Validation

Accepted frozen run (2026-10-19):
`target/bevy-020-qualification/runs/380181504e4c41f4926ea7bad5647224/`.
The complete qualification executes **1,860 checks**, including **1,438 editor
graph/session/history/consumer checks** with the existing timing probe ignored,
in 39.71 seconds for that suite after compilation.

All **940 input hashes** are unchanged. Counts are executions across gates, not
unique tests. Four new production/test paths and the existing host-motion module
are the five newly inventoried source paths. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false`, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping passes **2,408 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
Git whitespace validation. Root dependencies/toolchain remain unchanged;
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings remain non-fatal.

Next: remaining viewport shape-gizmo interactions/visualization, then preview
composition/full-plugin integration and graph/application workflows.
B20-2 remains open; shipping stays on 0.19.1 until the full compatibility gate.
