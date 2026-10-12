# B20-2 slice 5 — Shared color-picker compatibility

## Implementation and vertical mapping

The isolated Bevy 0.20 candidate now compiles the complete production
`feathers/color_picker.rs`, including its actual Feathers plane, color sliders,
numeric channels, RGB/HSL tabs, hex field, preview and Automatic button. The
hue/saturation scene moves into the shared scene wrappers. The picker calls the
numeric composition directly instead of depending on the surrounding UI shell.
Version-selected pointer aliases cover press, click, drag-start and cancel.
Shipping manifests and lockfile remain unchanged: shipping is still Bevy 0.19.1.

`ColorPlaneValue` and `ValueChange<Vec2>` hold **channel values**, not screen
coordinates. Inspection of the published sources shows that 0.19's HSL gradient
already increases saturation upward, but its pointer and thumb y increase
downward. Bevy 0.20 fixes the pointer and thumb to agree with the gradient.
Aestra continues passing actual saturation unchanged; adding an adapter inversion
would reintroduce the mismatch. Authored colors, HSL values and alpha retain their
meaning. Physical pointer-to-saturation behavior intentionally adopts the 0.20
upstream correction when the root graph switches. The 0.19 baseline is not silently
patched in this compatibility slice.

The picker also removes several real event defects exposed by full composition:

- The old unconditional Release observer could submit a duplicate final value
  before native DragEnd. A small per-plane pending/drag latch supplies only the
  missing click commit. Picking's Click-before-DragEnd order is covered; native
  Feathers still owns pointer mapping and drag updates/final submission.
- Cancel clears the latch without an extra commit. A fresh press recovers after
  a drag ended while disabled. Disabled planes and ancestors cannot change or
  finalize the color, and unrelated owners remain isolated.
- Programmatic hex-field display writes and selection-only notifications cannot
  feed back into a live color edit or clear Automatic mode. Focused typing still
  updates live, and Enter commits once. Focused edits are not overwritten by sync.
- Non-finite plane values are ignored. Non-ASCII hex input is rejected before
  byte slicing, avoiding a UTF-8 boundary panic.

## Required regression gate

The new required `editor-colors` gate uses real Feathers plugins, production
scenes and observers, not replacement widgets. Twelve new shared regressions
cover native corner mapping/outside clamping, native thumb positioning,
click/drag final-event ownership, cancel/recovery, disable/re-enable, owner
isolation, programmatic channel/slider/tab synchronization, Automatic reset,
invalid hex and real keyboard hex editing. The five existing picker tests and
46 numeric/text/SVG/localization dependency checks also execute: **63 checks**.
Those 46 dependency checks repeat the numeric gate deliberately; counts are
executed checks, not unique tests. Both gates reuse one fixture bootstrap.

The source-frozen runner hashes both pointer boundaries, shared picker regressions
and all fixture sources. Its shared scene-wrapper test also instantiates the
actual hue/saturation plane. The candidate's system-parameter lint allowance
matches the shipping editor's existing policy; no event or mapping assertion is
suppressed.

## Scope and next step

This is headless composition, native-observer and thumb-system qualification.
Tests provide deterministic geometry and dispatch real pointer events; they do
not exercise OS input or native hit testing. Shader pixels/gradient appearance,
DPI scaling, cursor behavior, multipointer gestures and full editor workflows
still need native acceptance. No new renderer-native evidence is claimed.

Next: remaining pointer consumers and custom code/search text consumers, then
full editor workflow acceptance and the coherent dependency switch. B20-2 stays
in progress; optional new-control adoption remains separate B20-4/B20-5 work.

## Validation

Accepted source-frozen **headless** run:
`target/bevy-020-qualification/runs/ed9095350b044bb5b91ad88f00372221/`.
Pinned MSVC Rust 1.98.1, locked/offline: **361 executed checks pass**
(9 keyboard, 8 theme, 18 editor controls, 46 numeric, 63 color/dependency,
40 upstream focus, 4 cluster units, 4 shader, 3 storage, 166 shared/candidate).
All **545 input hashes** remain unchanged. The color gate takes **8.45 seconds**.
Shipping lock SHA256 remains
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping: **1,207 editor unit and 3 architecture tests pass**, with the ten
existing ignores unchanged. The approved filesystem run permits the existing
temporary project/settings fixtures. Strict all-target workspace Clippy with
`aestra-bench/gpu`, strict all-target candidate Clippy with
`editor-qualification`, both formatting checks and `git diff --check` pass.
All-target workspace checking with the same GPU feature set also passes.
Existing incremental-cache hard-link warnings fall back to copying.

Renderer-native gates were not rerun for this UI-only slice; previous evidence
remains separately dated in [the earlier report](editor-controls-2026-10-11.md).
