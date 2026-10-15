# B20-2 slice 49: ruler and choreography-event lane composition

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now builds
the production ruler ticks, marker controls, choreography-event lane/controls and
effect-drop spacer from `timeline/cue_composition.rs`.

Three existing constructors move unchanged apart from visibility/formatting and
equivalent import resolution. Native exports retain their existing call sites;
the complete root timeline test module and TimelinePlugin wiring compare
unchanged. Existing selection, pointer drag, presentation and history consumers
are used directly; no substitute cue or action controller is introduced.

Two new production files join the frozen input inventory. Dependency versions,
features, toolchain and authored formats stay unchanged. Toolbar and remaining
track construction/full timeline/Curves integration are still open.

## Regression coverage

Eighteen new shared integration tests build the actual controls and exercise
their attached observers with explicit canvas geometry:

- The retained 32-tick ruler, label children, initial hidden geometry, native
  cue/action/cursor identities and real localized labels/tooltips in both catalogs.
- Selected marker/event fills, text and border geometry; the actual event lane
  parentage/offset and hidden insertion-spacer layout contract.
- Production visual updates position cues and reuse their identities. Zoomed
  ticks reuse entities and format their labels; offscreen cues hide/reappear.
  Missing authored cue IDs hide without accepting stale drag starts; an empty
  cue document keeps the ruler and event lane without dummy controls.
- Primary clicks use the real selection actions and stop propagation. Secondary
  clicks and injected Activate do not select native controls. Attached press
  stoppers prevent the canvas from seeking through either cue type.
- Attached marker/event drag observers preview without semantic edits or parent
  seeking, commit one globally undoable step, retain event payloads and restore
  selection/cursor state. Movement uses the real window scale at two DPIs and
  clamps to effect bounds. No-motion releases select without creating history.

The fixture supplies one retained canvas and explicit geometry; presentation is
run through the actual systems. This is not native layout, picking, rendering,
floating-window routing or full-plugin acceptance. No stronger pointer ownership,
button gating, stale-click or cancellation policy is claimed. Previous hardening
follow-ups remain unchanged; no authored behavior or optional controls are replaced.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/5145fe475b12420f8beb57f477705dfc/`.
It executes **1,405 checks**, including **985 graph/session/history/consumer
checks**, with all **848 input hashes** unchanged. Counts are executions across
gates, not unique tests. One existing graph timing probe stays ignored; the graph
process takes 37.63 seconds after compilation while the shipping build runs.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`,
and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Strict all-target Clippy passes on both engines, alongside workspace all-target
checking with `aestra-bench/gpu`, both formatting checks and whitespace validation.
Shipping passes **2,016 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Root dependency/toolchain diffs remain empty. Existing Windows
incremental-cache hard-link fallback warnings are non-fatal.

Next: remaining track and toolbar construction, full timeline/Curves panel/plugin
integration, then remaining viewport/application consumers and native workflow
acceptance. B20-2 remains open; shipping stays on 0.19.1 until the complete
compatibility release gate passes.
