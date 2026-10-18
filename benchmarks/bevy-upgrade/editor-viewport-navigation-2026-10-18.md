# B20-2 slice 60: production viewport camera navigation

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now
executes the actual preview camera navigation consumer and controller.

- `viewport/navigation.rs` shares `navigate_preview_camera`, the complete
  `PreviewCameraController` and navigation state, default pitch, camera transform
  helper and existing preview-camera/emitter-proxy markers.
- Native viewport consumers import those same types and functions; the existing
  crate-level camera/controller/proxy APIs remain reexported. Members used by the
  original parent module gain parent visibility, not a new external API.
- `patches-020/editor_viewport.rs` imports that production module and retains the
  actual shortcut identities. No substitute camera algorithm, input context or
  synthetic semantic controller is introduced. Existing upstream mouse message,
  button state, time, transform and query APIs compile directly on both engines.
- The frozen runner tracks the module, shared tests and candidate wrapper as
  three additional inputs.

Source audits confirm the original navigation body, complete controller/state,
camera transform helper, all 70 remaining root functions, native ViewportPlugin
registration and full existing viewport root test block are unchanged after
visibility/format/trailing-comma normalization where needed. There is no change
to shipping behavior, engine dependencies, toolchain or authored formats.

## Regression coverage and limits

Eighteen shared headless tests execute the production consumer on both engines.
Ten mirror existing native regressions without changing their assertions; the
original native tests remain intact. Eight additional tests exercise edge cases.
These are additional gate executions, not eighteen previously unknown behaviors.

- MMB pan, RMB orbit direction/radius/pitch clamp, and WASD flight without RMB.
- Hover ownership, Assets blocking scroll/pan start, press-inside capture,
  continued orbit outside the canvas, release and simultaneous orbit/pan priority.
- Diagonal normalization, real-time frame-rate independence, either Shift boost,
  stalled-frame distance bounds and all Control/Alt/Super modifier sides.
- Escape, focus loss, text editing and menus; cancellation clears queued motion
  and wheel input and does not resume a drag without a fresh press.
- Line/pixel wheel equivalence, multiple-message accumulation and zoom clamps.
- Frame requests wait while input is blocked, use the single emitter proxy or
  fall back to the origin for missing/ambiguous proxies, and clear once applied.
- A temporarily ambiguous canvas defers navigation until its single owner returns.
  Idle updates do not mark the camera Transform changed; unrelated transforms
  remain untouched when navigation resumes.

Fixtures supply actual Bevy mouse messages, ButtonInput resources, real time,
RelativeCursorPosition and transforms to the real consumer. This is not native
OS input delivery, rendered camera acceptance, UI layout/hit testing, surface
minimize/restore or floating-window qualification. It does not install the full
ViewportPlugin or qualify action execution, viewport synchronization, gizmo
rendering, preview runtime scheduling or the complete editor application.
Single-owner ambiguity and existing blocking policies are preserved, not hardened
as an unrelated behavioral change. No upstream orbit-controller replacement is
part of this compatibility slice.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/7736a8763db94076869d9fa854adf757/`.
The editor graph/session/history/consumer suite passes **1,189 tests**, with its
existing timing probe ignored, in 52.29 seconds after compilation. All eighteen
navigation tests also pass in isolation.

The complete run executes **1,610 checks**, with all **895 input hashes**
unchanged. Counts are executions across gates, not unique tests. The runner
confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`, and unchanged
shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **2,217 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
both formatting checks and Git whitespace validation. Root dependency/toolchain
inputs remain unchanged. Existing Windows incremental-cache hard-link fallback
warnings are non-fatal. Workspace all-target checking with `aestra-bench/gpu`
passes. Post-documentation input drift is zero.

Next: viewport action activation/execution and camera viewport lifecycle, then
remaining graph palette/menu and application consumers before native workflow
acceptance. B20-2 remains open; shipping stays on 0.19.1 until the complete
compatibility release gate.
