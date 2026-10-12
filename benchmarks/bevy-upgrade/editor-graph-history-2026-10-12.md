# B20-2 slice 14 — Presentation transactions and document edit ordering

The isolated Bevy 0.20 graph gate now imports the production graph presentation
transaction and per-document edit-order model. Shipping remains Bevy 0.19.1;
root manifests, lockfile and toolchain are unchanged.

## Implementation

- `material_graph/presentation/transaction.rs` owns the existing `Snapshot` and
  `Transaction` data, invalidation, changed-node selection, validation and apply
  policy. Shipping `presentation.rs` reexports the types and delegates to this
  same code rather than using a second candidate implementation.
- Host semantic lookup stays authoritative: validation receives the current
  root/generation and a lazy callback resolving the current graph stamp. It keeps
  the existing short-circuit order and error messages. Catalog/session snapshot
  capture and semantic publication are not substituted in the candidate.
- Preview validation/apply uses the real visibility set and the extracted
  production `MaterialGraphPreviewTarget` identity. The full preview resource,
  raster cache and rendering remain host-owned; no fake preview resource is added.
- `history/asset_order/model.rs` owns the existing `Context`, `EditOrder`, edit
  journal, semantic/layout classification, attachment and undo/redo chronology.
  Host context selection and the filesystem deletion ordering bridge stay in
  `asset_order.rs`. Reexports preserve existing consumers and the 256-entry bound.
- The candidate imports these actual modules and real `GraphViewportMemory`.
  The runner freezes the new sources and existing host adapters/regression tests.

## Shared regression coverage

Thirteen new shared tests execute on both engine versions:

- Bounded edit/journal storage, latest matching-context undo/redo, root/asset
  scope separation, context-local redo invalidation, context clearing, monotonic
  serials, empty steps and cloned-history independence.
- Position/collapse/pin round trips, created-node removal/restoration, untouched
  other graphs/camera state, and clearing only the target graph's display offsets.
- Root/generation/semantic-stamp invalidation, missing semantics, graph-scoped
  invalidation, manual placement/pin conflicts and non-finite delta rejection.
- Preview availability, stale visibility rejection and per-program round trips.
- A semantic edit and attached layout remain one ordered item through undo/redo;
  layout-only entries retain their classification, wrong-context attachment is
  ignored, and invalidation reaches both applied and redo entries.

These tests exercise production transaction/order logic. They do not simulate
catalog publication or claim that the candidate runs full semantic undo. Existing
shipping presentation/history tests continue to cover the real session/catalog
path, including compound edits, failed validation and focused document ownership.

## Remaining scope

Full host history integration remains open: semantic snapshot/catalog resolution,
successful and failed publication, function history, filesystem deletion ordering,
availability, and the complete action execution observer. Full panel composition
and native editor workflow acceptance are also still required. This slice does
not switch root dependencies, replace controls or approve new native visuals.

Next: qualify production semantic snapshot resolution and function history, then
complete the host execution/publication boundary and panel/native workflows.

## Validation

Accepted frozen headless run (2026-10-12):
`target/bevy-020-qualification/runs/30b85e5347cc4adb821d304193c41932/`.
It executes **535 checks**, including **121 graph checks** (one existing timing
ignore), with all **628 input hashes** unchanged. Counts are test executions,
including shared tests run by multiple gates, not unique tests. The graph test
process takes 1.28 seconds after compilation.

Strict candidate all-target Clippy and both formatting checks pass. The runner
confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false; no new native editor acceptance is claimed.

Shipping regression: **1,318 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged, including the original compound semantic/layout,
failed undo, placement restoration, function history and project-switch tests.

Strict workspace all-target Clippy and all-target workspace checking with
`aestra-bench/gpu` pass. Formatting and whitespace checks pass; root manifests,
toolchain and lockfile have no diff. Existing Windows incremental-cache hard-link
fallback warnings do not fail the gates.
