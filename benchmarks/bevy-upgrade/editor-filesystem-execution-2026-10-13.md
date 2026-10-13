# B20-2 slice 23: guarded filesystem history execution

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated Bevy **0.20.0** candidate now
executes the actual serialized filesystem-history worker, including real recovery
journals, closed-document state, draft/reference checks and catalog publication.
It no longer stops at observing a deletion-dispatch request for this gate.

- `asset_browser/deletion/history.rs` contains the existing undo/redo worker.
  The shipping `history_step` entry point delegates without changing its contract.
  Busy I/O, ordering, modal and document/catalog guards remain checked before
  queueing and again before publication; failed/cancelled work retains its entry.
- The existing `ClosedDocuments` implementation is imported directly with explicit
  dependencies. Recovery encoding, exact material/function draft baselines, target
  restoration and untitled/current-effect source-path behavior are unchanged.
- `deletion/reference_checks.rs` contains the actual active/proposed effect checks
  and complete draft inventory. The inspector, initial delete flow and history
  worker use this shared policy rather than candidate reference-check substitutes.
- Actual publication refreshes the prepared catalog, calls the browser reconciliation
  adapter, publishes through serialized project I/O and then calls the dialog/status
  adapter. Shipping keeps these adapters' existing implementation and ordering.
  Candidate tests observe only those UI callbacks; disk operations, catalog refresh,
  draft/session changes and history advancement execute production code.
- One regression wires the actual application router through deferred commands to
  the real worker, interleaving document undo/redo with filesystem restore/deletion.
  The router module is crate-visible for this composition; a test-only plain data
  snapshot exposes journal/revision observations without widening the history model.
- The candidate adds `ron = "0.12"` (locked **0.12.2**) as a development dependency for the real recovery
  encoding. This was already in its transitive lock graph; only the fixture package's
  dependency list changes. Root manifests, toolchain, lock and authored formats
  are unchanged. The runner freezes the worker, checks, recovery source and native
  browser/inspector consumers.

## Regression coverage

Seventeen new shared regressions cover:

- Repeated real restore/redo restores exact bytes and empty folders, publishes the
  correct catalog and creates fresh redo journals.
- Preparation alone leaves disk, ordering and publication untouched; busy remains
  authoritative until completion is applied and prevents duplicate work.
- Stale revisions, wrong directions and every modal branch reject before queueing.
  Completion rechecks document edits, target, catalog, ordering and modal state
  before any disk mutation.
- Occupied restore destinations and externally edited restored files are never
  overwritten/deleted; failure retains history for a safe retry.
- Live/proposed effect references, saved surviving owners and unsaved program
  dependencies block deletion using actual preflight and complete draft inventory.
- Program/function drafts and exact disk baselines restore with their editing target.
  New conflicting drafts, edits to restored drafts, incomplete inventories and
  malformed recovery records block without losing work.
- A deleted active effect remains in memory as untitled until guarded restore
  rebinds its original source path. Unrelated drafts survive both history directions.
- The real application router executes document undo, filesystem undo, filesystem
  redo and document redo in order using the actual worker.

## Remaining scope

Initial Delete/Restore modal controls, full browser reconciliation/bookmark/UI
composition, history-button/menu availability, complete shortcut dispatch, other
application handlers and native editor workflow acceptance remain open.
Test setup creates the initial journal with the real deletion plan and recovery
inventory; it does not claim qualification of the initial delete modal on 0.20.
No new native rendering, dialog-layout or whole-editor GUI acceptance is claimed.

Next: qualify the remaining delete/restore modal and browser publication adapters,
then history controls/availability and complete keyboard shortcuts. The root engine
switch still requires the full compatibility release gate.

## Validation

Candidate graph/session/history gate: **310 passed**, one existing timing probe
ignored. Strict candidate all-target Clippy with
`editor-qualification,async-qualification` and both formatting checks pass.

Accepted frozen headless run (2026-10-13):
`target/bevy-020-qualification/runs/89f3cb83f04e48bcb86a616a7240cccd/`.
It executes **729 checks**, including **310 graph/session/history checks**, with
all **708 input hashes** unchanged. Counts are executions across gates, not unique
tests. The frozen graph/session/history process takes 6.20 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false; no new native editor acceptance is claimed.

Shipping regression: **1,443 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
checking with `aestra-bench/gpu` pass. Both formatting checks and whitespace
validation pass. Root manifest/toolchain/lockfile diffs remain empty.
Existing Windows incremental-cache hard-link fallback warnings do not fail the gates.
