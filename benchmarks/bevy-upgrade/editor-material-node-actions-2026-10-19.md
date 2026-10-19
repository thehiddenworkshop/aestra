# Material node selection/opening/modified drags — Bevy 0.20 qualification

Date: 2026-10-19. B20-2 slice 77. Shipping remains Bevy 0.19.1;
the isolated candidate remains 0.20.0, with Rust 1.98.1. No dependency,
authored-format, widget styling or interaction-policy changes.

## Implementation

- `apps/aestra-editor/src/material_graph/node_actions.rs` shares five original
  functions: node click selection, project-function-call opening, marquee
  selection, modified node drag dispatch and branch traversal.
- `node_controls.rs` shares the original node-scope and inline number/toggle
  identity records. The original crate-level node-scope path is re-exported.
- `asset_browser/actions/open_function.rs` shares the actual function-open
  document intent, re-exported from its existing browser path. This gate captures
  that event; it does not substitute a function-opening implementation.
- Click consumers use the existing native 0.19/0.20 event boundary. Test-only
  constructors set the count on the actual engine event. The completed-drag
  fixture uses the actual node-state transitions to exercise release suppression.
- Duplicate drag executes the actual clipboard/remapping, semantic publication,
  target activation and history path. Branch drag executes the actual compiler
  projection and placement-memory update and emits the actual presentation batch
  event. There is no candidate-only semantic implementation.

Source audit: all five extracted bodies and all **72 remaining top-level graph
functions** are unchanged apart from whitespace. Native graph plugin registration,
original root graph tests and browser action function bodies remain unchanged.
The only runtime API spelling changes are the native pointer-click type aliases;
record visibility is widened only to preserve parent consumers.

## Shared regressions

**22** shared cases cover:

- Descendant primary selection, inspector updates, revision count and consumption;
  both Shift/Control sides, toggle removal and Control precedence.
- Secondary/Space gates; socket and original inline number/toggle descendant
  exclusion; missing scope's default scope; action-free targets; one-shot
  release-click suppression after the real state transitions.
- Exact project-function open intent from a double-click descendant; rejected
  single/secondary/non-call clicks; retained view target with a different active
  session target; stale expressions/scopes and missing views. Real temporary
  material/function assets remain unchanged on disk.
- Marquee replace/add/toggle, unknown nodes, missing views, unchanged revision
  no-op and independent view selection. The original inspector policy follows
  the incoming marquee set, including toggle removal.
- Duplicate selection remapping, exact placement delta, unchanged source nodes,
  one semantic history operation and actual undo/redo; unselected seed behavior;
  nonfinite duplicate rejection without semantic publication.
- Upstream/downstream transitive placement, output-node inclusion, preservation
  of collapse state, avoiding a second seed delta after producer placement,
  missing-placement behavior and exact presentation-event before/origin payload.
  Stale modified-drag graph/node/view identities do not publish edits.

The test fixture keeps Properties active so the real panel-reveal helper does
not persist or modify the user's workspace settings.

## Preserved boundaries

Pointer consumers are delivered through target-scoped observers in these tests;
the unchanged global plugin composition is not installed. In particular, an
ignored descendant event can still be reconsidered at an ancestor by the original
global routing, as documented in slice 75. This is not a new bubbling guarantee.

Function opening is verified through the actual emitted document intent, not its
browser/document consumer. Branch dragging is verified through actual placement
and the actual emitted batch event, not the downstream presentation undo journal.
Duplicate undo/redo does execute the semantic history implementation.

Existing graph/node-key matching, scope fallback, branch placement/inspector
policy, missing-placement skips and pointer ownership/cancellation rules are
preserved. This slice does not introduce cross-view disambiguation, epochs or
new finite-coordinate guards for branch movement.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/f2fc7dfab496404a8f5e706eae336048/`.
It executes **2,041 passing checks**, including **1,619 editor graph/session/
history/consumer checks**, with one existing ignored editor timing probe. The
editor suite takes 69.85 seconds after compilation. Counts are executions across
gates, not unique tests. All 22 shared node-action cases pass in the full gate.

All **978 input hashes** are unchanged. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false` and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Both engines pass strict all-target Clippy; both formatting gates and Git
whitespace validation pass. Existing Windows incremental-cache hard-link
fallback warnings remain non-fatal.

The final-source shipping suite passes **2,578 editor unit + 3 architecture
tests**, with the same ten existing ignores. Workspace all-target checking with
`aestra-bench/gpu` passes. Root manifest/toolchain/lock diffs remain empty;
post-documentation input drift is zero.

## Limits and next step

This is headless actual-event/semantic/history evidence, not native picking,
rendered/DPI screenshots, full graph plugin composition or complete editor
workflow acceptance. No visual reference is approved. B20-2 remains open.

Next: remaining material toolbar/preview actions and inline numeric/toggle
consumers, followed by complete graph composition/plugin and application workflow
acceptance. Activate the qualified 0.20 boundaries only when the root dependency
graph switches coherently.
