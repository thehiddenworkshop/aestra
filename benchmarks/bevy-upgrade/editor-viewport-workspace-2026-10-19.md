# B20-2 slice 71: preview scene, workspace and status consumers

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** editor candidate
now executes the actual remaining preview scene and workspace consumers.

`viewport/scene.rs` shares startup camera/grid/player/proxy construction and
stand-in gizmo drawing. `viewport/workspace.rs` shares panel/canvas construction,
the seven Feathers toolbar controls, SVG children, drag readouts and runtime
status synchronization. The original viewport-pane scene is shared through
`feathers/scenes/viewport.rs`, without exposing it to unrelated widget gates.
The existing crate-level `spawn_preview` path is retained.

Source audits confirm all eight extracted function bodies, the two remaining
root test helpers, the native plugin implementation and original root tests are
unchanged. The pane scene body and tool-icon enum are unchanged; the status-label
marker only gains parent-module visibility. Import changes retain the native
keyboard consumer's PreviewCanvas path. Root dependencies, toolchain, authored
formats and preview/playback policy are unchanged.

The candidate facade imports these actual modules and re-exports the actual pane
scene. Five new production/test paths enter the frozen source inventory. No
additional lint exemption is needed.

## Regression coverage and limits

**24 new shared tests** run on both engines: ten scene/stand-in cases and
fourteen workspace/status cases. They cover:

- One camera, grid, presented player and transform proxy after startup; actual
  mesh/material payload, render layer, camera clear/order/initial viewport and
  gizmo-camera marker. Session time/seed and compiled artifact identity reach
  the player; the proxy uses the authored pose. Empty effects retain valid
  startup state, and later updates do not duplicate assets or entities.
- Still, Orbit and FlyBy stand-ins use actual playhead poses. CPU GizmoAsset
  buffers contain spheres and moving velocity arrows, with finite geometry.
  Missing/orphan assignments draw nothing; removal retires the current handle.
  Drawing leaves authored state, history, seed and clock unchanged.
- Actual pane/canvas layout, clipping, seven classified native Feathers buttons,
  accessible labels/tooltips, both locales, initial maximize icon selection,
  SVG asset paths/size/tint/nonpicking and hidden drag readouts.
- Real Activate events reach mode/display/frame/maximize consumers. Retained
  controls/icons/readouts update without document edits or duplicate dispatch.
- All four actual runtime backends, live count, error precedence and recovery,
  unowned status filtering and the original idle change gate. Removing runtime
  status alone does not wake the original system; tests preserve this policy
  until session/project changes rather than altering it during the port.

This is headless scene/payload/composition/consumer and CPU gizmo-asset evidence.
It does not establish full ViewportPlugin installation/scheduling, native
rendering/picking, icon asset-load completion in these composition fixtures,
every-DPI visual acceptance or performance approval. SVG rasterization/loading
has a separate existing gate. No visual reference is approved.

## Validation

The focused candidate viewport gate passes **239 tests**, including all 24 new
cases. Accepted frozen run (2026-10-19):
`target/bevy-020-qualification/runs/ed9c0b3630174823bbffaf54d5bae76e/`.
The full qualification executes **1,906 checks**, including **1,484 editor
graph/session/history/consumer checks** with the existing timing probe ignored,
in 48.07 seconds for that suite after compilation. Counts are executions across
gates, not unique tests.

All **947 input hashes** are unchanged. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false`, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping passes **2,454 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting gates and
Git whitespace validation. Root dependency/toolchain inputs are unchanged and
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings remain non-fatal.

Next: complete viewport-plugin integration, then graph palette/menu and
application workflows. B20-2 remains open; shipping stays on 0.19.1 until the
full compatibility gate.
