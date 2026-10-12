# B20-2 slice 15 — Semantic snapshot resolution and function history

The isolated Bevy 0.20 graph gate imports the production semantic snapshot resolver,
persisted graph identities and function-history resource/transitions. Shipping
remains Bevy 0.19.1; no root dependency or authored-format change is included.

## Implementation

- `material_graph/keys.rs` owns existing material/function document keys, material
  expression keys and the material output identity. Shipping reexports preserve
  consumers and exact persisted strings, including root-scoped function layouts.
- `material_graph/presentation/semantic.rs` selects the matching document, checks
  source readiness, computes the existing semantic stamp and projects canvas keys.
  Shipping `presentation.rs` supplies the real catalog inventory and availability
  callbacks. Indexed programs/draft overlay/tombstones still come from the original
  `layout_lifecycle::programs`; function readiness still checks the cached project
  source. Source-free built-ins retain their existing exception. The candidate
  does not invent a catalog or session implementation.
- `material_function_editor/history.rs` owns the actual `FunctionEditor` resource,
  root/function-scoped stacks and reports. No-op/read-only guards and edit/undo/redo
  transitions run through this shared code. Publication callbacks must succeed
  before stacks move; failed edits retain their diagnostic, while failed history
  steps preserve both stacks and the previous report. The 256-entry bound and
  local/global redo clearing policy are unchanged.
- Function body execution, cross-catalog call-site validation, conflict detection,
  draft publication, session status and document chronology remain in the shipping
  host adapter. They use the shared resource instead of a second history model.
- The runner freezes the new sources, host adapters and referenced function asset.
  It also now hashes the actual `aestra-authoring` path dependency, alongside the
  already-frozen core/compiler/project/runtime crates.

## Shared regression coverage

Twelve new tests execute on both engine versions:

- Exact LIFO function payload/report round trips through the real Bevy resource;
  empty history, failure-safe undo/redo and edit retry; no-op/read-only rejection;
  root/asset isolation, local forks, clearing and the 256-entry bound.
- Graph document identity versus viewport identity, missing inventory/source
  rejection and the built-in exception; real compiler canvas projection parity for
  inline versus explicitly retained constants; function root/identity/readiness
  and read-only body guards; stable stamps and invalidation for semantic edits
  that do not change canvas keys.

Existing shipping tests continue to exercise real session/catalog publication,
signature/body changes, disk preservation and compound semantic/layout chronology.
Callbacks in the shared tests cover the transition contract, not successful native
catalog execution in the candidate.

## Remaining scope

Full host execution/publication on 0.20, filesystem deletion ordering, remaining
application action handlers, panel composition and native editor workflow acceptance
remain open. Catalog inventory/draft overlay lifecycle is still host-qualified;
this slice is not a full catalog port. No optional control replacement or new visual
approval is included.

Next: qualify the host semantic execution/publication boundary and its rollback,
then complete action/panel/native workflow acceptance.

## Validation

Accepted frozen headless run (2026-10-12):
`target/bevy-020-qualification/runs/c8295378041140dd839c0247cd884ad8/`.
It executes **547 checks**, including **133 graph checks** (one existing timing
ignore), with all **652 input hashes** unchanged. Counts are test executions,
including shared tests exercised in multiple gates, not unique tests. The graph
process takes 0.61 seconds after compilation.

Strict candidate all-target Clippy and both formatting checks pass. The runner
confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false; no new native editor acceptance is claimed.

Shipping regression: **1,330 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
workspace checking with `aestra-bench/gpu` pass. Formatting and whitespace checks
pass; root manifest/toolchain/lockfile diffs are empty. Existing Windows
incremental-cache hard-link fallback warnings do not fail the checks.
