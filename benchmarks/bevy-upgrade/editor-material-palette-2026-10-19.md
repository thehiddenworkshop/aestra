# B20-2 slice 73: material Add Node palette consumers

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** editor candidate now
executes the actual material palette lifecycle, typed option selection and real
Feathers popup/search/scroll composition.

`material_graph/palette.rs` shares ten existing functions: keyboard opening,
dismissal, live search, added-search focus, catalog resolution, semantic option
filtering, projection connection lookup, popup construction, category grouping
and option construction. Nine original palette/action/option and menu-marker
records move with it, gaining only parent-module visibility. The native graph
imports the shared consumers without changing plugin registration.

`material_graph/connections.rs` shares the original named-input and graph-edge
mapping helpers. The existing crate-level material_graph::input_target path is
retained. The palette candidate uses these actual helpers, existing semantic
planner, actual session/catalog/target types, actual search-field module, context
menu composition and scroll wrapper; it does not substitute a rendered palette
or invent candidate-only palette records.

Source audits confirm all twelve extracted function bodies, all **105 remaining
top-level graph function bodies**, the material graph plugin implementation and
original root test body are unchanged. All nine record definitions are unchanged
after parent visibility normalization. No root dependency/toolchain, authored
format, focus/Tab/dismissal, semantic, selection, playback or save-policy changes.
The original keyboard-open consumer still uses ButtonInput; shortcut ownership
and event propagation are not hardened during this extraction.

Three new production/test files enter the frozen inventory. Existing search/text/
popup/scroll dependencies were already inventoried. The candidate's palette ECS
query allowance matches the shipping type-complexity policy; no global lint or
test gate is weakened.

## Regression coverage and limits

**21 shared palette tests** execute on both engines: one original category-order
regression mirrored into the candidate, and twenty new cases. Coverage includes:

- Hover/normalized-cursor requirements for Tab, coordinate unprojection, exact
  document target/view scope, existing-open retention and context/search reset
  without semantic edits. Escape clears each actual open-state variant once.
- Hover protection for palette, node menu, connection menu and submenu surfaces;
  left click-away, right-click retention and Escape precedence.
- Search trimming/case normalization, row/category visibility, no-match and clear
  recovery, unrelated-value isolation, retained entities and added-only focus.
- Actual compiler catalog order and MaterialToolPlanner acceptance. Output-origin
  options consume their exact source; input-origin options preserve their target
  and reuse the connected source only when appropriate. Stale program/source
  identities produce no options. Named/unknown ports and output edge mappings
  use the original semantic helper.
- A restored standalone material resolves its own catalog before focus changes,
  without switching or dirtying the effect target.
- Actual Feathers popup/menu-item/search/scroll components, navigation blocking,
  width and every action payload field. Both locales provide accessible search
  labels and empty/no-match messages. Real KeyboardInput edits the search through
  the native text bridge; real clear activation emits one container value event
  and restores the same retained option entities.

The actual search-field module's six existing tests also execute in the editor
candidate gate. They were already covered by a separate widget gate; execution
totals are not unique-test counts.

This is headless consumer/semantic-planner and real Feathers composition/input
evidence, not native visual/DPI acceptance or complete material graph plugin/
application integration. Pointer opening on canvas/node/pins, palette/context
semantic action dispatch, connection gestures and full graph workspace acceptance
remain separate. No new control replacement or visual reference is approved.

## Validation

The focused candidate palette gate passes all **21 tests**. Accepted frozen run
(2026-10-19):
`target/bevy-020-qualification/runs/e27a8b865aee402dbe10f0ba8d45f3b6/`.
The complete qualification executes **1,948 checks**, including **1,526 editor
graph/session/history/consumer checks** with the existing timing probe ignored,
in 71.43 seconds for that suite after compilation. Counts are executions across
gates, not unique tests.

All **952 input hashes** are unchanged. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false`, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping passes **2,490 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting gates and
Git whitespace validation. Root dependency/toolchain inputs are unchanged and
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings remain non-fatal.

Next: material palette/context-menu semantic action dispatch and pointer-opening
handlers, then remaining application action/workflow acceptance. B20-2 remains
open; shipping stays on 0.19.1 until the full compatibility gate passes.
