# B20-2 slice 22: application history routing

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated Bevy **0.20.0** candidate now
executes the actual application undo/redo routing through real ECS system
parameters: ownership, session/catalog, document/view registries, filesystem
chronology, semantic histories and compound presentation state.

- `history/routing.rs` contains the existing application routing block and its
  actual resource/query borrows. The shipping `execute_history_action` observer
  delegates to this source; its event contract and existing callers are unchanged.
- Ownership runs before global-order synchronization. Busy/unapplied I/O and
  every protection-modal branch block the action; an active code buffer remains
  authoritative even when its undo/redo stack is empty.
- Global document/filesystem chronology takes precedence over view focus.
  Without global ordering, material history follows the real focused view/document
  registry while explicit effect focus continues to select effect history.
- Compound validation/execution remains shared. Failed steps restore the previous
  target and history-focus flag; only successful steps finish document chronology.
  A new global edit still retires redo across histories before routing.
- The native deletion worker remains behind a narrow deferred callback. Shipping
  supplies the unchanged `asset_browser::deletion::history_step`, which rechecks
  I/O, modal, revision and filesystem guards. No I/O is moved into observer borrows.
  Candidate tests capture this dispatch rather than substitute a recovery worker.
- The qualification runner freezes both new sources. Root dependency manifests,
  toolchain, authored formats, widgets and filesystem policy are unchanged.

## Regression coverage

Fourteen new shared regressions exercise the same router on both engine versions:

- Effect undo/redo without optional order, focus, code-editor or canvas resources.
- Real focused material/function history instead of the compatibility session target;
  effect focus preserves effect history even while a graph view is still focused.
- Stale views, closed documents and incomplete focus cannot select another history.
- Failed semantic or missing-canvas compound undo restores the previous target;
  repairing the conflict or canvas allows undo/redo with exact semantic/layout state.
- Active/empty code history precedes global filesystem actions; stale entities
  release ownership. Every modal branch and unapplied I/O prevent dispatch.
- Global document history overrides focus, then dispatches deletion with the exact
  journal, direction and current revision. Dispatch alone never pops the entry or
  changes filesystem bytes; mutable callback borrows confirm deferred execution.
- Empty global history does not fall through to retained local stacks.
  Project switches retire stale global deletions, and new edits invalidate old
  program redo before executing the new global step.
- Restored filesystem entries dispatch redo with the current journal/revision,
  leaving restored bytes unchanged until the real recovery worker completes.

## Remaining scope

This qualifies the application history router, not the complete native deletion
worker/UI lifecycle. Candidate deletion tests intentionally capture the callback;
the shipping regression suite retains the real worker tests.

Filesystem/UI action composition, history-button/menu availability composition,
complete keyboard-shortcut dispatch, other application action handlers, full
panels and native workflow acceptance remain open. No whole-editor GUI parity,
native rendering, dialog or responsiveness acceptance is claimed.

Next: qualify the guarded filesystem/UI history adapter and remaining action/
shortcut composition. The root engine switch still requires the full release gate.

## Validation

Candidate graph/session/history gate: **293 passed**, one existing timing probe
ignored. Strict candidate all-target Clippy with
`editor-qualification,async-qualification` and both formatting checks pass.

Accepted frozen headless run (2026-10-13):
`target/bevy-020-qualification/runs/0127e261d5f44a8f9272d9a6fde08e1b/`.
It executes **712 checks**, including **293 graph/session/history checks**, with
all **702 input hashes** unchanged. Counts are test executions across gates, not
unique tests. The frozen graph/session/history process takes 3.47 seconds after
compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false; no new native editor acceptance is claimed.

Shipping regression: **1,426 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
checking with `aestra-bench/gpu` pass. Both formatting checks and whitespace
validation pass. Root manifest, toolchain and lockfile diffs are empty.
Existing Windows incremental-cache hard-link fallback warnings do not fail the gates.
