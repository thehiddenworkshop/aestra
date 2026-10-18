# B20-2 slice 62: camera viewport lifecycle

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now
executes the actual camera viewport synchronization, activation helper and gizmo
overlay camera configuration from `viewport/lifecycle.rs`.

All three function bodies are shared unchanged. Native ViewportPlugin startup
and PostUpdate registration/order, all 63 remaining viewport root functions and
the complete existing viewport root test block compare unchanged in source audits.
The existing preview-camera/canvas markers are reused; no replacement camera,
geometry model or input algorithm is introduced. The candidate's query lint
policy matches the existing shipping editor policy. Root engine/dependency/
toolchain inputs and authored formats remain unchanged.

The consumer continues to assign physical viewports to the preview and exact
layer-15 3D gizmo cameras, deactivate them for invalid geometry, and wait one
valid update before activation. The readiness Local and original clamp/conversion
rules are retained, not redesigned as unrelated interaction hardening.

## Regression coverage and limits

Sixteen new shared headless tests execute the actual systems on both engines:

- First valid update assigns both camera viewports while inactive; the next
  valid update activates them. Ordinary valid resize/movement does not rearm.
- Fractional conversion, negative origins, edge overflow, oversized and wholly
  outside canvases preserve the original physical clamping rules, including
  the existing last-pixel viewport behavior.
- Zero, too-small, negative and nonfinite sizes deactivate both cameras;
  exactly 16 physical pixels is valid. Missing or ambiguous canvases deactivate
  and require a fresh valid update before reactivation.
- Each zero-surface-dimension case and repeated invalid/valid transitions reset
  readiness. Restored physical bounds reach both cameras before activation.
- Mixed active/inactive unrelated cameras retain their state and viewports
  during both activation and deactivation: other layers, mixed layer masks,
  layer-15 2D cameras and cameras without explicit layers remain isolated.
- Actual PostStartup overlay configuration retains color-buffer composition and
  order, while the PostUpdate lifecycle owns activation. Synchronization does not
  overwrite camera clear color, order or output blending.
- Explicit physical geometry is not multiplied again across nine display/UI scale
  combinations. Secondary-window dimensions do not replace primary-window bounds.

Fixtures use actual ComputedNode, UiGlobalTransform, Window resolution, Camera,
RenderLayers and the production Local readiness state. They provide explicit
geometry rather than running native layout or an OS surface. No files/settings
are written by these fixtures.

This qualifies the headless camera consumer contract, not rendered minimize/
restore acceptance, actual DPI/layout measurement, GPU cluster assignment,
render-world extraction, gizmo drawing or full viewport/application scheduling.
In particular, the one-update readiness test is not proof of native render-stage
ordering or a GPU crash fix. Missing/ambiguous primary-window or preview-camera
ownership, floating-window lifecycle and camera replacement remain outside this
bounded gate. Native workflow acceptance remains required before the engine swap.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/7cfeff72468f47a4bc77a05ffe4cf217/`.
The editor graph/session/history/consumer suite passes **1,239 tests**, with its
existing timing probe ignored, in 38.74 seconds after compilation. All sixteen
new lifecycle tests also pass in isolation.

The complete run executes **1,660 checks**, with all **905 input hashes**
unchanged. Counts are executions across gates, not unique tests. The lifecycle
module and its tests are the two additional inventory paths. The runner confirms
Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`, and unchanged shipping
lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping passes **2,250 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks
and Git whitespace validation. Root dependency/toolchain inputs are unchanged;
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings are non-fatal.

Next: photographic preview/overlay composition consumers, followed by remaining
grid/display, runtime preview and gizmo consumers before native editor workflows.
B20-2 remains open; shipping stays on 0.19.1 until the complete compatibility gate.
