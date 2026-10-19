# B20-2 slice 70: shape-gizmo interaction and visualization

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** editor candidate now
executes the actual shape-gizmo module with the real session, localization,
module-parameter projection, menu state, Camera, shared gizmo state and semantic
command/history implementations.

`viewport/shape_gizmo.rs` separates 13 existing functions: target selection,
lock gating, handle placement and screen-space picking, scalar drag projection,
shape updates, cursor/readout behavior, transactional interaction, shape/handle
drawing, ring tessellation and overlay configuration. It retains the original
SelectedShapeModule and RingPlane definitions, with only parent-module visibility
adjusted. The candidate facade re-exports the actual parameter projection helper,
not a substitute module implementation. The previous candidate dead-code
allowance for shape state is no longer needed.

Source audits confirm all 13 extracted function bodies, all ten remaining
viewport-root functions (including test helpers), the native plugin implementation
and existing root test body are unchanged. One longer function signature is
reformatted; it does not change its body. Both extracted record definitions are
unchanged after parent visibility normalization.

No shape, selection, preview, lock, transaction, cancellation or playback policy
is changed to pass the fixtures. No root dependency/toolchain or authored-format
changes. Native control replacement and new active-drag ownership/cancellation
policies are not part of this compatibility slice.

## Regression coverage and limits

**22 shared tests** run on both engines: three original shipping regressions
mirrored into the candidate and nineteen new cases. Coverage includes:

- Original per-shape handles for all eight supported shapes, dimensional
  independence, minimum size clamping, no-op unrelated handles and resize cursors.
- Actual projected hit-radius boundaries, degenerate axes and failed projections.
  Rotated/nonuniform player transforms preserve cylinder depth scaling and the
  cone-radius anchor during scalar projection.
- Real press/hold/release interaction creates a live preview without modifying
  the authored effect/revision/dirty state, then commits one reversible undo step.
  No-motion release has no semantic history. Escape restores the preview and a
  late release cannot commit the cancelled adjustment.
- Effect/emitter/module locks prevent new drags. Menus, tab context, canvas
  ownership, pending review and disabled modules retain their original gates.
  Missing proxies clear handle state and hide the readout.
- Both locales receive the actual radius/depth/XYZ-extent messages, formatted
  scalar values, visibility and canvas-clamped readout coordinates.
- Real GizmoAsset buffers verify original line counts and finite geometry for
  point, circle, ring, sphere, hemisphere, box, cylinder and cone; ring seams and
  nonuniform transforms; hemisphere bounds; overlay layer and line width.
- Full scene drawing uses the live dragged shape and highlights the hovered
  handle. Shape outlines use line lists while Bevy's spherical handles use
  line strips; the test inspects both actual buffers. Deselecting the module
  retires the current shape-gizmo handle.

This is headless consumer/projection/command-history and CPU gizmo-asset evidence,
not native rendering, complete picking delivery, every-DPI visual acceptance,
performance approval or installation of the complete 0.20 viewport plugin.
Existing active-drag ownership policies are preserved, not hardened here.
Remaining preview setup/composition/status/stand-in drawing and full plugin/
application workflows stay separate. No visual reference is approved.

## Validation

The focused candidate shape gate passes all **22 tests**.
Accepted frozen run (2026-10-19):
`target/bevy-020-qualification/runs/87849ce3a7af48c8ad11d1a71a53e332/`.
The full qualification executes **1,882 checks**, including **1,460 editor
graph/session/history/consumer checks** with the existing timing probe ignored,
in 52.58 seconds for that suite after compilation.

All **942 input hashes** are unchanged. Counts are executions across gates, not
unique tests. The shared shape module and test file are the two new inventory
paths. The runner confirms Bevy **0.20.0**, Rust **1.98.1**,
`native_executed: false`, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping passes **2,430 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting gates and
Git whitespace validation. Root dependency/toolchain inputs are unchanged and
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings remain non-fatal.

Next: remaining preview scene/composition consumers and complete viewport-plugin
integration, then graph/application workflows. B20-2 remains open; shipping
stays on 0.19.1 until the full compatibility gate.
