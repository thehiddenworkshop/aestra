# B20-2 slice 21: compound semantic/presentation undo and redo

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated Bevy **0.20.0** candidate now
executes the actual compound semantic/layout history step, with real session,
catalog, preview visibility, graph memory and material/function history resources.
No root dependency, authored format or widget replacement is included.

- `history/compound.rs` contains the existing validation-first semantic/presentation
  execution block. The shipping action observer calls it after ownership/context
  routing and before finishing filesystem chronology. Failure still restores the
  wrapper's prior target; failed execution does not advance filesystem ordering.
- `material_graph/presentation/context.rs` contains actual project/session semantic
  resolution, snapshot capture/attachment and transaction validation/application.
  `presentation/state.rs` contains the existing optional ECS memory/preview borrows.
  The candidate uses these production adapters, not a substitute validator.
- `material_graph/graph_inventory.rs` contains the existing project-wide inventory
  policy: indexed sources, live drafts/tombstones and built-in effect materials.
  The placement lifecycle and presentation resolver now share this source.
- `material_graph/preview_state.rs` contains the actual visibility resource and
  raster-cache data. Shipping raster workers retain their scheduling/render logic
  and use the same data types. Only internal parent/crate visibility needed by
  existing consumers and the shared boundary changes; cache policy is unchanged.
- Shared graph-key helpers and a test-only plain transaction fixture constructor
  let regressions exercise real captured snapshots without duplicating history or
  presentation algorithms. The runner freezes all new sources and raster-worker
  consumers alongside the existing inputs.

## Regression coverage

Fourteen new shared regressions cover:

- Inserting/deleting nodes restores exact authored positions/collapse state in one
  semantic step, without creating a second layout-only undo item.
- Layout-only undo/redo restores position, collapse, pinning and preview visibility,
  without changing semantic revisions, compiled preview identity, another graph or
  the per-view pan/zoom camera.
- Missing graph memory, missing/mismatched preview state, manual placement/pinning,
  project-generation changes and explicit invalidation reject before mutation and
  retain the step for a valid retry. Canvas-less hosts still support transactions
  without preview deltas; semantic-only history requires no canvas resources.
- Deleting a previewed node prunes its visibility; compound undo restores both the
  node and visibility before the preceding history item is reached.
- A newer shared draft rejects stale compound history. Failed effect-scoped semantic
  undo preserves the draft, layout and installed preview; repairing the invalid
  effect allows retry.
- Real function and effect histories restore semantic changes and layout together.
  No-op function publication does not replace the previous compound attachment.
- External source comments survive draft-only undo/redo unchanged on disk. This
  retains the existing in-memory editing policy; it does not authorize saving over
  external edits or remove exact-byte persistence guards.

## Remaining scope

The complete focused-view/application action observer, asynchronous filesystem/UI
deletion composition, graph-origin ownership and full panel composition, complete
shortcut dispatch and native editor workflow acceptance remain open. Arrangement,
pin/edit recording and raster-worker scheduling remain shipping-owned consumers
of the shared model; this slice does not claim their whole GUI composition on 0.20.
No native rendering, dialog, responsiveness or whole-editor parity is claimed.

Next: qualify the remaining application history/action integration and filesystem/UI
composition, followed by complete shortcut and native workflow acceptance. The root
engine switch still requires the full compatibility release gate.

## Validation

Targeted candidate graph/session/history gate: **279 passed**, one existing timing
probe ignored. Strict candidate all-target Clippy with
`editor-qualification,async-qualification` passes.

Accepted frozen headless run (2026-10-13):
`target/bevy-020-qualification/runs/f1f24e20409843489859809f023c6484/`.
It executes **698 checks**, including **279 graph/session/history checks**, with
all **700 input hashes** unchanged. Counts are test executions across gates, not
unique tests. The graph/session/history process takes 2.88 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false; no new native editor acceptance is claimed.

Shipping regression: **1,412 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
checking with `aestra-bench/gpu` pass. Both formatting checks and whitespace
validation pass. Root manifest, toolchain and lockfile diffs are empty.
Existing Windows incremental-cache hard-link fallback warnings do not fail the gates.
