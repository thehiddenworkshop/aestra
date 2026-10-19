# Material toolbar, preview and inline controls — Bevy 0.20 qualification

Date: 2026-10-19. B20-2 slice 78. Shipping remains Bevy 0.19.1;
the isolated candidate remains 0.20.0, with Rust 1.98.1. No dependency,
authored-format, widget replacement or interaction-policy changes.

## Implementation

- `apps/aestra-editor/src/material_graph/controls.rs` shares eleven original
  functions: toolbar dispatch, preview dispatch/click consumption, numeric focus,
  numeric synchronization/constraints, number/toggle changes, semantic commit and
  scalar/vector component access/update.
- The existing version-specific control query accepts an additional filter,
  retaining the original node-preview-toggle classification. Native 0.19
  Interaction and 0.20 Pressed APIs preserve Activate-required dispatch and
  consume/reset behavior. Existing menu consumers keep their default filter.
- The native click boundary also exposes the actual press event for numeric
  focus. No synthetic pointer-event forwarding is introduced.
- `presentation/preview.rs` shares the original preview transaction and context
  recorder. Preview visibility now exercises actual snapshot capture, redo
  forking, chronology and undo/redo on both engines, not a test history adapter.
  Existing presentation callers retain their original paths.
- `asset_browser/actions/locate.rs` and `feathers/node_graph/preview_toggle.rs`
  share the actual locate intent and node-toggle marker, with existing native
  paths re-exported. Asset-browser reveal consumption remains a separate gate.
- The legacy text-clip removal stays behind the version boundary: 0.19 removes
  TextScroll exactly as before; 0.20 has no such upstream component to remove.
  The input and viewport retain their normal clipping. This does not establish
  rendered zoom/DPI parity.

Source audit: the two extracted presentation bodies and all **61 remaining
top-level graph functions** are unchanged apart from whitespace. The eleven
extracted control bodies differ only in native click/press names, pressed-query
iteration/consumption and the legacy TextScroll API boundary. Original graph
plugin registration, root graph tests and browser action bodies remain unchanged.

## Shared regressions

**25** shared cases cover real engine events, ECS state, temporary material
assets, the semantic planner/publication path and presentation/semantic history:

- Pointer pressed state alone cannot dispatch toolbar or preview actions.
  Activate consumes once and idle updates do not repeat it.
- Add Node uses inverse-scale viewport center and the requested view's retained
  editing target. Two scopes of one program stay separate; missing views reject.
- Arrange emits the actual document/view/target/seeds request without rebuilding;
  missing views report unavailable. Locate emits the actual asset identity once.
  Effect Context returns to the effect and closes palette/menu state.
- Individual and all-preview visibility use actual presentation undo/redo without
  changing material semantics or saved assets. Toggle-all preserves other programs.
  Missing views and missing node-toggle markers preserve the original gates.
  Preview click consumption retains its exact-target/primary behavior.
- Final scalar/vector and bool edits use actual target activation, semantic
  replacement/publication and undo/redo. Non-final, unchanged, nonfinite,
  unsupported component/value and unmarked events do not commit. Missing
  expressions and mismatched retained program identity cannot publish history.
- Component helpers retain scalar/vector/sRGB-color/bool type and bounds policy.
  Other vector components remain intact.
- Primary numeric descendant presses focus the actual editable-text entity and
  consume the event; secondary/no-editable cases retain focus and bubble.
- Added numeric controls emit one typed app-owned update, constrain parent/text
  widths and clipping, center text, and remove the actual legacy clip on 0.19.
  Idle updates do not repeat synchronization; unsupported values remain silent.

The fixture explicitly retains authored constant nodes, so planner cleanup does
not prune the otherwise unconnected test expressions. It captures numeric update
payloads; complete numeric widget composition is covered by the earlier widget
gate, not installed again here.

## Preserved boundaries

This is compatibility work, not interaction hardening. Final-only numeric commit,
control-snapshot values, exact-target preview click handling, six-level focus
search, view lookup and stale-action policies remain unchanged. No new gesture
coalescing, cancellation, cross-field freshness or pointer ownership rules are
introduced. The global pointer reconsideration limitation documented in slice 75
remains separate; these pointer tests use target-scoped observers.

The test schedule orders actual toolbar, preview and numeric synchronization
consumers. It does not install the complete editor graph plugin or native UI
PostLayout/picking/render schedule. Arrange and locate are verified through the
actual outgoing intents, not their complete asynchronous layout/browser consumers.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/21cbf271a1054851a982b48162cb8593/`.
It executes **2,066 passing checks**, including **1,644 editor graph/session/
history/consumer checks**, with one existing ignored editor timing probe. The
editor suite takes 71.02 seconds after compilation. Counts are executions across
gates, not unique tests. All 25 shared control cases pass in the full gate.

All **983 input hashes** are unchanged. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false` and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Both engines pass strict all-target Clippy; both formatting gates and Git
whitespace validation pass. Existing Windows incremental-cache hard-link fallback
warnings remain non-fatal.

The final-source shipping suite passes **2,603 editor unit + 3 architecture
tests**, with the same ten existing ignores. Workspace all-target checking with
`aestra-bench/gpu` passes. Root manifest/toolchain/lock diffs remain empty;
post-documentation input drift is zero.

## Limits and next step

No native screenshot, graph-zoom/DPI reference, GPU rendering or full application
acceptance is approved by this headless gate. B20-2 remains open; shipping stays
on 0.19.1 until the root dependency graph switches coherently.

Next: remaining node hover styling and graph workspace/header/node composition,
then full graph plugin/scheduling and complete editor workflow acceptance. Keep
the existing controls and geometry during the compatibility port; upstream widget
adoption remains a later milestone.
