# B20-2 slice 48: emitter headers and their popovers

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now builds
actual production emitter headers and their context, color and automation
visibility popovers. No header, popup or value controller is substituted.

- `timeline/emitter_composition.rs` shares the existing eight constructors/helpers.
  It retains current/stale multi-selection policy, enabled/solo/diagnostic styling,
  localized status/accessibility, native inline action classification, reorder
  icons/observers and source-name geometry.
- The context menu retains five real action payloads, including region-sensitive
  duplicate/delete and the actual reusable-effect AssetAction identity. Host
  extraction/external filesystem execution is not simulated or claimed qualified.
- Color composition uses the complete production picker, numeric widget, scrub,
  arrow and text dependencies through a candidate import facade. Existing 0.20
  pointer/scene adapters are reused; no new pointer behavior is introduced.
- Automation visibility uses actual registry-discovered lanes, localized show/hide
  labels, production SVGs and action handlers. Empty-lane headers keep their
  spacer, not a dummy action control.
- Three existing header presentation markers move unchanged into shared
  interaction state. Native TimelinePlugin wiring and its complete existing root
  tests are unchanged. Eight extracted function bodies compare unchanged apart
  from visibility/formatting and equivalent dependency namespaces.

The real plain-button scene is imported through its production scene boundary;
shipping aliases and color/numeric/text implementations remain unchanged.
Two production files and one candidate import facade join the frozen inventory.
Root dependency versions, features, toolchain and authored formats are unchanged.

## Regression coverage

Twenty new shared tests exercise real controls and action/history consumers:

- Native list/header/action identity, grid geometry, current versus stale
  multi-selection, disabled/solo/diagnostic states and both localized catalogs.
- Inline mute performs one undoable semantic edit; solo remains preview-only.
  Color/automation buttons open the actual transient state; injected Feathers
  activation does not double-dispatch native inline controls. Secondary clicks
  resolve the actual header selection and explicit local context position.
- Exact context-menu action identities and region-sensitive payloads; real menu
  duplicate/mute dispatch and globally reversible history without replaying a
  consumed pending activation.
- Complete color-picker/popover identities and placement configuration. Real
  alpha changes preview the swatch without editing the document, then final
  changes commit through history. The actual Automatic button restores `None`
  and roundtrips undo/redo; no alternate color implementation is installed.
- Automation popup lane identities/counts come from the registry. Show-all and
  single-hide use the production consumers without semantic edits; constant-source
  emitters keep the empty spacer. Actual list selection and attached reorder
  controls preserve selection/history across a mixed emitter/effect-clip order.

Forty-nine existing color/numeric/text regressions also execute in the integrated graph gate,
in addition to their dedicated widget gates; these are not new shipping tests.
Counts below are executions across gates, not unique tests.

Fixtures use explicit node geometry and capture only bounded UI/action behavior.
They do not qualify native picking/layout/window-clamping/rendering, floating
windows, host extraction, deletion confirmation workflows or full timeline/Curves
plugin composition. Existing stale-action, pointer-ownership and automatic-color
preview-restoration policies remain unchanged; this compatibility slice does not
claim to harden them or replace controls.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/dec34841bef043cd8664d69601e23eb1/`.
It executes **1,387 checks**, including **967 graph/session/history/consumer
checks**, with all **846 input hashes** unchanged. One existing graph timing
probe stays ignored; the graph process takes 31.68 seconds after compilation
while the shipping build runs concurrently.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`,
and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **1,998 editor unit + 3 architecture tests**; ten existing ignores
remain unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks
and whitespace validation. Root dependency/toolchain diffs remain empty.
Existing Windows incremental-cache hard-link fallback warnings are non-fatal.

Next: remaining ruler/event-lane, track and toolbar construction, then full
timeline/Curves panel/plugin integration, remaining viewport/application
consumers and native workflow acceptance. B20-2 remains open and shipping stays
on 0.19.1 until the complete compatibility release gate passes.
