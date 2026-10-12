# B20-2 slice 16 — Catalog-backed semantic publication and rollback

The isolated Bevy 0.20 gate now executes production material/function publication
against the actual editor catalog and draft store. Shipping remains Bevy 0.19.1;
root manifests, lockfile and toolchain are unchanged.

## Implementation

- `project_content/model.rs` owns the existing `EditorProjectContent` resource,
  snapshot/version state, source queries, draft publication, project compilation
  and prepared-project cache. This is the production type, not a qualification
  surrogate. `project_content/mod.rs` retains startup defaults, plugin/watch/I/O
  composition and reexports; existing refresh consumers keep their access.
- `material_drafts.rs` is imported unchanged by the candidate, including its
  existing disk-baseline/save-receipt tests. Temporary projects use real indexes,
  parsed sources, draft overlays and compiler results.
- `history/publication.rs` owns existing detached material transaction execution,
  catalog replacement, project compilation, extraction/removal compensation,
  and cross-catalog function call-site validation/publication. Shipping history
  and function-editor adapters call this same code.
- An effect-context replacement installs the compiled project through the existing
  session callback. A standalone authoring document does not compile/install an
  unrelated effect. The single replacement command does not change its document's
  effect; using that context preserves the original session effect being compiled.
- Failed compile/preview installation restores the previous program. Failed
  extraction removes its newly created function; failed undo removal restores the
  material. Compensation remains best-effort with both errors retained if it also
  fails. No new promise of filesystem-wide atomicity is introduced.
- Session draft synchronization, status, actual preview installation, history
  routing/chronology and UI action execution remain in their existing host adapters.
  Successful publication is still required before recording or advancing history.
- The candidate adds only a direct `tempfile` test dependency already present in
  the lock graph. The runner freezes the new modules, actual catalog/draft sources,
  existing host tests and touched function-graph consumer.

## Regression coverage

Ten new shared tests cover:

- Real catalog resource insertion and standalone publication, with disk/preview
  preservation; effect-context compilation and one preview installation.
- Invalid authoring and stale draft rejection; real unresolved project dependency
  compilation failure and rollback; failed preview installation restoring an
  existing unsaved draft while retaining the actual redo entry for a successful retry.
- Rollback conflicts preserving newer work and reporting both errors.
- Extraction/removal round trips, failed extraction cleanup, cleanup failure,
  duplicate identity rejection and failed undo restoration/restore failure.
- Function edits validating indexed callers outside an active effect, with stale
  preparation and incompatible signature rejection and unchanged source bytes.

The candidate also now runs the five existing draft save/recovery-baseline tests.
Existing shipping catalog/watch, history, function-body/signature and compound
semantic/layout regressions remain intact. Installation and explicit restore-failure
callbacks inject faults at the host boundary; catalog resolution, drafts and project
compilation themselves are not mocked.

## Remaining scope

The complete session preview installer, focused history/action observer and ordered
host execution are not yet run by the 0.20 candidate. Project watch/I/O lifecycle,
remaining action handlers, full panel composition and native editor workflows also
remain open. This slice neither switches shipping dependencies nor replaces widgets
or approves new native visuals.

Next: qualify session preview installation and scoped action/history execution,
then complete watch/I/O, panel and native workflow acceptance.

## Validation

Accepted frozen headless run (2026-10-12):
`target/bevy-020-qualification/runs/2381149c9d92434bb1025a8e7440f3da/`.
It executes **562 checks**, including **148 graph checks** (one existing timing
ignore), with all **661 input hashes** unchanged. Counts are test executions,
including shared tests exercised in multiple gates, not unique tests. The graph
process takes 1.03 seconds after compilation.

Strict candidate all-target Clippy and both formatting checks pass. The runner
confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false; no new native editor acceptance is claimed.

Shipping regression: **1,340 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
workspace checking with `aestra-bench/gpu` pass. Formatting and whitespace checks
pass; root manifest/toolchain/lockfile diffs are empty. Existing Windows
incremental-cache hard-link fallback warnings do not fail these gates.
