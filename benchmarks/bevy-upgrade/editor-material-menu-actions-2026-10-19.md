# Material palette and context-menu semantic dispatch — Bevy 0.20 qualification

Date: 2026-10-19. B20-2 slice 74. Shipping remains Bevy 0.19.1; the
isolated candidate remains 0.20.0, with Rust 1.98.1. No dependency/toolchain,
authored-format, playback, visual-style or new-control adoption changes.

## Implementation

- `apps/aestra-editor/src/material_graph/menu_actions.rs` shares the actual
  Activate queue, target lookup, arrange seed selection, Add Node consumer and
  context consumer. Three original action records have parent visibility.
  Existing preview/toolbar consumers retain these exact records and registration.
- `control_input_019.rs` keeps the original Changed<Interaction> query, explicit
  PendingFeathersActivation gate and Interaction::None reset. The isolated
  `control_input_020.rs` translates Changed<Pressed> and marker removal to that
  same consumer contract. A pointer press alone still cannot edit.
- `arrange/request.rs` contains the actual ArrangeScope and ArrangeGraph event,
  retaining their native paths. Asynchronous arrangement execution stays in
  `arrange.rs`; this gate observes the actual dispatched request, not a fake
  layout worker.
- Actual Feathers placement executes in the candidate, with explicit imports.
  Original node chrome dimensions and material row/height calculations are
  shared source, not candidate copies. The five existing bounded placement
  algorithm regressions run on both engines, plus one new seed/named-placement
  case. The original measured native UI-layout regression remains intact in
  the shipping node-graph test suite; only its registration/imports move and
  the measured flag gains parent visibility for that test.

Source audit: queue/target/seeds and both metric helper bodies are unchanged.
Both semantic consumer bodies after the version-specific gate/reset are unchanged.
All remaining original top-level graph function bodies, original root graph tests
and native material graph plugin registration are unchanged.

## Regression coverage

18 new shared consumer tests execute actual catalog/session mutation, compiler/
planner, clipboard, scoped selection, presentation memory and compound history:

- Duplicate Activate events coalesce to one creation; no later repeated edit.
  Created nodes are placed and selected, inspector identity is updated, existing
  positions survive, and undo/redo restores the semantic document.
- Invalid positions, stale programs, planner failure, unavailable mounted views
  and stale scopes preserve the original failure/close/retry policies.
- Explicit output wiring, retained standalone target reactivation without
  switching the effect, and another view's independent selection.
- Copy, cut and paste through the real fragment; duplicate/delete, extraction
  and exact connection reset; undo and original saved material preservation.
- Empty clipboard and queue ownership: unrelated/unmarked controls are ignored,
  while existing toolbar/preview records remain owned by the same queue.
- All four arrangement scopes dispatch exact document/view/target/seed payloads,
  remove only popup ancestry and leave graph entities/UI revision intact.
- Actual placement Context resolves only the matching mounted document, computes
  its graph-space center, preserves bootstrap positions and retains the explicit
  unavailable-measurement notice.

These tests do not introduce availability, stale-menu or disabled-state policy
changes. They preserve the original consumers' behavior at this compatibility
boundary.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/6dda8b3b396c4feabb0cbc699c65f852/`.
It executes **1,972 passing checks**, including **1,550 editor graph/session/
history/consumer checks** and one existing ignored timing probe; that editor
suite takes 60.52 seconds after compilation. Counts are executions across gates,
not unique tests. The focused dispatch suite passes all **18** new cases.

All **961 input hashes** remain unchanged, including after documentation edits.
The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false` and
unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **2,509 editor unit + 3 architecture tests**, with the same ten
existing ignores. Both engines pass strict all-target Clippy. Workspace all-target
checking with `aestra-bench/gpu`, both formatting gates and Git whitespace
validation pass. Root manifest/toolchain/lock diffs remain empty. Existing Windows
incremental-cache hard-link fallback warnings are non-fatal.

## Limits and next step

This is headless input/semantic/history and request-dispatch evidence. It does
not establish native pointer picking, rendered menus, measured 0.20 UI layout,
asynchronous arrangement completion, GPU behavior, full graph plugin/application,
visual/DPI or performance acceptance. The native measured-layout regression
continues to run on shipping only.

Next: material canvas/node/pin pointer opening and remaining graph gestures,
then complete material-graph/application workflow integration. B20-2 remains open.
