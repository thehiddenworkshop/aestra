# B20-2 slice 13 — Activation dispatch and document-target routing

The Bevy 0.20 candidate imports production Feathers activation, history action
identity/event dispatch, and view-to-material target routing. Shipping remains
Bevy 0.19.1; root manifests, lockfile and toolchain are not switched.

## Implementation

- `feathers/action_activation.rs` owns the existing action/native/pending markers
  and typed activation queue. Editor action, graph menu and history observers
  call it with their original domain filters. Old marker paths are reexported.
- The shared event dispatcher preserves pending-activation guards, disabled
  rejection, activation consumption and event payloads. Shipping history buttons
  call it and retain legacy colors, menu dismissal and session revision updates.
  Event execution remains deferred until those resource updates are visible.
- `history/action.rs` contains the actual `HistoryAction` event/component and
  its observer. The candidate does not invent a history action or session/catalog.
- Version adapters clear `Interaction` to `None` on 0.19 and remove native
  `Pressed` on 0.20. The 0.20 dispatcher does not use deprecated `Interaction`.
- `material_target.rs` contains the unchanged project-scoped target model.
  `editor_target.rs` moves existing resolution/reconciliation without changing
  routing policy. Shipping callers retain reexported paths; candidate tests use
  the actual document/view registries.
- The text composition gate also imports shared activation and the real history
  action. Its unrelated host classifications remain bounded fixture dependencies,
  not full-editor acceptance. The frozen runner includes all new source inputs.

## Regression coverage

Fourteen shared tests run under both engine versions:

- Classified/domain-filtered activation, bare pressed-state rejection,
  pending/pressed consumption, no redispatch after consumption, disabled
  rejection, real undo/redo payloads, reactivation and legacy controls.
- Program/function identities and kinds, focused-view routing, mismatched or
  incomplete active-context rejection, closed views/documents, split focus,
  asset opening/refocusing, inline-context clearing, WESL routing/focus
  preservation and current-project root scoping.

A shipping-only test runs the actual history button handler and execution
observer against real editor resources: undo, menu dismissal, pending/pressed
cleanup, untouched Feathers styling, no repeated edit and redo. Existing host
graph-menu activation and focused-document history tests remain enabled.

## Remaining scope

This qualifies activation/event dispatch and target routing, not the complete
0.20 application dispatcher. Menu/focus resources, semantic palette/toolbar
handlers, full history availability/execution, catalog publication/rollback,
function/ordered history and semantic/layout transaction attachment still need
candidate host integration. Full panel composition and native editor workflows
remain open. No optional widget replacement or root dependency switch is included.

Next: production history/semantic host integration with actual resources and
ordered semantic/layout transactions, followed by full panel/native acceptance.

## Validation

Accepted frozen headless run (2026-10-12):
`target/bevy-020-qualification/runs/35c11e44bda643fc957ef0b5f73433fc/`.
All **620 input hashes** remain unchanged. The suite executes **522 checks**,
including **108 graph checks** (one existing timing ignore) and **38 text checks**.
Counts are executions, including shared tests run by multiple gates, not unique
tests. Graph tests take 0.48 seconds after compilation.

Strict candidate all-target Clippy passes. The runner confirms Bevy **0.20.0**,
Rust **1.98.1** and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false; this slice claims no new native editor evidence.

Shipping validation: **1,305 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy, all-target
workspace checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation pass. Root manifest/toolchain/lockfile diffs are empty. Existing
Windows incremental-cache hard-link fallback warnings do not fail the gates.
