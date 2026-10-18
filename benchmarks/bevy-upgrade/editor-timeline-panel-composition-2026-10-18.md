# B20-2 slice 54: complete timeline panel composition

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now constructs
the complete production timeline tree, joining the previously qualified controls.

`timeline/panel_composition.rs` shares the original spawn_timeline constructor:
toolbar, heading and track list, ruler/cue/motion lanes, nested choreography rows,
scroll viewports/gutters, playhead/snap guide, drop preview and rejection overlay.
The native timeline module reexports the same API. Five existing panel markers
move unchanged into shared interaction state. The candidate's bounded host-motion
namespace reexports the actual header/lane functions used by the native host API;
it does not substitute composition or action behavior.

The constructor and all five marker contracts compare unchanged after formatting/
visibility normalization. Native TimelinePlugin wiring and the complete existing
root timeline test block remain unchanged. Obsolete parent imports are removed or
restricted to their existing test consumers. No dependency, authored format,
geometry policy, source-opening behavior or pointer-ownership hardening is added.

## Regression coverage

Twenty-two new shared tests construct the entire panel with real catalog assets,
module metadata, child constructors, attached pointer observers, action consumers,
transport plugin and global history routing. They cover:

- Body/header/canvas layout properties, exact paired grid rows/content height,
  complete toolbar/cue/motion/local/referenced track identities, usable empty
  panels, both locales and catalog identity, actual scrollbar targets and overlay
  stacking/non-pickability.
- Attached primary seeking/menu cleanup, secondary no-seek behavior and child
  control press isolation; real marker/event toolbar actions and the header Add
  action; transport frame stepping without semantic history.
- Actual payload drops on the constructed canvas and header viewport, globally
  reversible insertion, self-reference rejection through the constructed feedback
  overlay, Escape release guards, hover preview geometry/label/both spacers and
  leave cleanup. Source/project files stay byte-identical.
- A constructed effect control resolving its canvas through the real nested
  hierarchy, preview-only movement and one reversible release, paired scrolling,
  idle entity stability and complete child-control teardown.

The fixture contains three root tracks, one referenced emitter and three visible
automation lanes: seven paired grid rows. Geometry inputs are explicit, not the
result of a native window layout pass. Application UI rebuilds are not substituted
or inferred from the retained-control fixture.

These checks do not qualify native visual layout/rendering/picking, floating
windows, full TimelinePlugin schedule installation or Curves/Shell workflows.
Existing single-canvas and drop placement/overlay behavior is preserved. Prior
pointer ownership, stale-action and cancellation hardening follow-ups remain
separate from this compatibility port.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/2c30a4769be14c2f9dfac70978c73312/`.
It executes **1,517 checks**, including **1,096 graph/session/history/consumer
checks**, with all **871 input hashes** unchanged. Counts are executions across
gates, not unique tests. The new panel implementation and its shared tests join
the frozen inventory. The existing graph timing probe stays ignored; the graph
process takes 48.41 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`,
and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **2,126 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. Root dependency/toolchain diffs remain empty. Existing
Windows incremental-cache hard-link fallback warnings are non-fatal.

Next: qualify TimelinePlugin schedule integration, then remaining Curves action/
panel/plugin integration, viewport/application consumers and native workflow
acceptance. B20-2 remains open; shipping stays on 0.19.1 until the complete
compatibility gate.
