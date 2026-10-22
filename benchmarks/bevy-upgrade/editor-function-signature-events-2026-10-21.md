# Function signature events and semantic placement — Bevy 0.20

Date: 2026-10-21. B20-2 slice 92. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

The production signature observer registrar and its text, numeric and activation
handlers now live in `material_function_editor/signature_input.rs`. Shipping
registration retains its original graph-then-signature order. The candidate uses
these same observers, composed Field/Action identities, session-backed publication
and global history dispatch; there are no qualification-only edit callbacks.

Structural actions use the actual `material_graph/semantic_placement.rs` Context,
including optional viewport memory, incremental placement, pinned/manual positions
and compound semantic/layout undo. The actual function-node size helper moves to
shared composition. The immutable graph-layout contract moves to
`feathers/graph_layout/model.rs`, retaining its canonical native export. Production
native and partial layout engines and their existing Elk reference tests are
compiled directly in the candidate. No placement budgets or algorithms change.

The production HistoryAction observer moves to `history/dispatch.rs` and retains
its canonical export and actual deletion-history callback. Existing routing and
publication services, not alternate fixture implementations, execute undo/redo.

Source comparison preserves all seven signature handler/helper bodies, the
signature registration order, function-node size helper, history observer body,
all eleven semantic-placement function bodies, all eight existing signature tests
and all eight existing semantic-placement test bodies. The layout contract is
unchanged apart from a narrow dead-code annotation on its existing reserved
node-state record; engine child paths are explicit for direct compilation.

Only the private qualification manifest/lockfile gains `elkrs = "=0.1.1"`, the
already-pinned shipping layout-test dependency. Its serde_json feature dependency
is reflected in that lockfile; existing package versions do not change. Shipping
manifests/lockfile, toolchain and CI are unchanged. Frozen hashing includes the
actual layout tree and new shared observer/test sources.

## Shared regressions

Ten new tests use real composed controls and production registered observers:

- Live versus final text/number edits, trimmed names, no-op commits and one undo
  entry per accepted edit, with real global undo/redo.
- Stable input/output IDs; empty names, malformed typed defaults and nonfinite
  values rejected without mutation or extra history.
- Bool, scalar, vector and color defaults; clearing a default before changing an
  unused input type, then removing that input and undoing the sequence.
- Add Input/Add Output through actual placement, preservation of a pinned manual
  position, and removal/restoration of new layout state through compound history.
- Required-port removal and incompatible signature changes checked against a real
  indexed saved caller, preserving the original function and history on rejection.
- Stale control ownership and untyped event sources ignored; actual keyboard live
  input followed by Enter commits once through the real text-input bridge.
- Structural editing with viewport memory absent retains validation and undo.

The rejected-output test initially had no caller and assumed local type validation
that the existing publication path does not perform. It now includes a saved
material caller, exercising actual compiler call-site checks rather than changing
production validation policy during an engine port. Saved function bytes remain
unchanged by draft edits.

The candidate additionally runs 52 existing checks: eight semantic-placement,
four layout-contract, 24 native-layout, three crossings, nine partial-layout and
four Elk reference tests. These are newly qualified executions, not new shipping
tests or a replacement layout engine.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/9f43f3441a184b4d81caade539b033a7/`.
It executes **2,352 passing checks**, including **1,920 editor graph checks**
(87.94 seconds after compilation), with the existing ignored editor timing probe.
Counts are executions across gates, not unique tests. All **1,069 input hashes**
are unchanged. Native execution is false.

The same candidate graph executable also passes with default parallel execution:
**1,920 passed**, one existing ignored timing probe, no failures (15.65 seconds).
Both strict all-target Clippy gates, both formatting gates and whitespace checks
pass. Workspace GPU-feature all-target checking passes.

Shipping passes **2,777 editor unit + 3 architecture tests**, with default parallel
execution, the same ten existing ignores and no failures (8.99 seconds for unit
tests). Post-documentation input drift is zero. Some Cargo invocations report
incremental-cache hard-link fallback to copying; there are no code lint failures.

## Remaining work

Qualify function-graph gesture/action consumers and their registrar, then remaining
material/plugin/application handlers and complete editor workflows. These tests
cover headless signature events, semantic publication and layout/history, not full
native pointer dispatch, rendered wires, DPI or complete editor acceptance. Native
acceptance is still required before switching shipping dependencies. No visual
reference is approved.
