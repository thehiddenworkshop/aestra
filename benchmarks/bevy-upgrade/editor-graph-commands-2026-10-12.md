# B20-2 slice 12 — Semantic preparation and material history boundary

The isolated Bevy 0.20 graph gate now imports the actual editor material-command
preparation and material-program history storage/stack transitions. Shipping
remains Bevy 0.19.1; root manifests, toolchain and lockfile are unchanged.

## Implementation

- `material_graph/command_preparation.rs` extracts the editor's existing detached
  material tool planning and validated execution. The returned `PreparedEdit`
  contains the real `MaterialToolPlan` and before/after programs. The shipping
  `apply_material_tool_command` calls it, then retains the existing host
  replacement/extraction publication paths. One redundant lookup of the same
  current material is removed; the authoring document is the common snapshot.
- `history/material_program.rs` owns the production `MaterialProgramEditHistory`
  resource, entries, per-project/program map, 256-entry bound and stack operations.
  `history.rs` reexports the type and retains target routing, catalog operations,
  session synchronization, operation ordering, extraction asset rollback and
  history activation/availability. The original host tests remain enabled.
- Successful replacement/extraction publication calls `record_applied`; failed
  publication records no entry. Undo/redo use `step_applied` with the original
  host publication callbacks. A failed callback restores its original stack
  entry; successful callbacks keep the existing session ordering updates.
- The candidate imports these production modules by path. It does not replace
  the session/catalog with mock host types or switch the editor to the separate
  authoring crate's `MaterialCommandHistory` implementation.
- The frozen runner includes both new modules/tests and their host call sites.

## Shared regression coverage

Twelve new tests run from the same source under both Bevy versions:

- Detached creation, real typed connect/disconnect, duplicate/delete, invalid
  source/type/cycle rejection and missing program/deleted-node rejection.
- Extraction's function payload and combined transaction, including exact
  inverse restoration through the real command executor.
- Empty-stack callback suppression, LIFO payload/label retention, failed
  undo/redo retry, redo invalidation, the existing 256-entry limit and root/asset
  scope isolation/clear behavior.
- The real Bevy history resource round-tripping a validated program replacement.

The tests invoke real preparation, planner, executor and history operations.
They do not simulate filesystem failures or claim to exercise full host event
dispatch in the candidate. Existing shipping tests cover graph edits recompiling
the project preview, standalone editing, failed standalone history, focused
document routing, ordered history, extraction assets and semantic/layout history.

## Remaining scope

This is the semantic preparation/history-storage boundary, not completion of all
graph dispatch. Full pointer/keyboard/menu dispatch, session/catalog publication
and rollback, the effect/material ordering ledger, function editor history and
semantic/layout transaction attachment still need candidate host integration.
The full production panel and native editor workflow acceptance remain open.
No optional widget replacement or root Bevy dependency switch is included.

Next: qualify the production activation/dispatch boundary with document-target
routing, then extend the candidate to full host semantic/layout history and
native graph workflows. Keep actual host types; do not claim a mock application
as full-editor acceptance.

## Validation

Accepted source-frozen headless run (2026-10-12):
`target/bevy-020-qualification/runs/c04bb6b5081d4e50a8dda1bc72e009f0/`.
It executes **502 checks**, including **94 graph checks** (one existing graph
timing ignore). All **613 input hashes** remain unchanged; the graph test
process takes 0.70 seconds after compilation. Totals count test executions,
including shared tests run by multiple gates, not unique tests.

Strict candidate all-target Clippy and both formatting checks pass. The runner
confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false; no new native editor evidence is claimed.

Shipping validation: **1,290 editor unit + 3 architecture tests pass**, with
ten existing ignores unchanged. Strict workspace all-target Clippy and workspace
all-target checking with `aestra-bench/gpu` pass; both formatting checks and
whitespace validation pass. Root manifest/toolchain/lockfile diffs are empty.
Windows emits the existing incremental-cache hard-link fallback warnings during
compilation; these do not fail the gates.
