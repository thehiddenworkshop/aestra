# B20-2 slice 19: focus, modal and code-editor history ownership

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated Bevy **0.20.0** candidate now
executes the actual early history ownership gate, pointer/keyboard focus observers,
code-buffer undo/redo model and document protection state. No root dependency or
authored format is changed, and no widget is replaced.

- `history/ownership.rs` holds the existing early action-routing policy. The
  shipping action observer delegates to it before touching document chronology,
  filesystem or presentation history. Menu availability uses the same modal/I/O
  gate. An active code buffer remains authoritative even when its own stack is
  empty; a removed/stale code entity falls through to document routing.
- `history/focus.rs` contains the existing ancestor-based pointer/keyboard ownership
  policy. Neutral scopes preserve the prior editing context; program/function
  targets activate material history, while inline effect targets do not. Panel-to-
  scope mapping remains in the shipping wrapper.
- `history/focus_pointer_019.rs` and `focus_pointer_020.rs` isolate the changed
  pointer-event names/construction. They do not translate behavior or synthesize
  replacement events; both gates exercise native Bevy pointer dispatch.
- `feathers/code_editor/buffer.rs` contains the existing text/selection state,
  bounded/coalesced history and active-code-editor resource. The production widget
  imports this same model and retains its keyboard, clipboard, rendering, active
  editor tracking and change-event behavior. Its existing widget gate still runs.
- `persistence/state.rs` contains the existing document action enum and protection
  resource. Native dialogs, recovery orchestration and persistence execution stay
  in their existing shipping modules. Only previously parent-private internals
  become visible to that parent to preserve existing consumers.
- The runner freezes the shared sources, version boundaries and tests. The graph
  gate retains `editor-qualification,async-qualification` for real background I/O.

## Regression coverage

Sixteen new shared regressions cover:

- Exact Unicode text/caret/anchor restoration; character-based line mapping;
  independent cloned histories; the 256-entry history bound; coalescing and redo
  invalidation after a new edit.
- Real pointer/keyboard focus traversal, closest neutral scope precedence, absent/
  cleared/stale focus, program/function versus effect targets, and unchanged
  authored data/UI revisions during focus navigation.
- Optional-resource fallback, real active-buffer undo/redo without touching effect
  history, no fallthrough from an empty code stack, removed/recycled active entities,
  and independent histories when switching active buffers.
- Every protection branch (including recovery, dirty-close, deletion, creation,
  transaction and project launcher) blocks both history routing and the shared
  availability gate. Clearing protection restores routing.
- A completed but unapplied real I/O worker continues blocking code/document history
  and availability; applying its completion releases the gate.

One existing keyboard-focus regression moved alongside the shared observer and now
also runs on 0.20. Five new buffer tests run in both the graph and real code-widget
gates. Counts below therefore represent test executions, not unique tests.

## Remaining scope

This slice qualifies early ownership, not the entire application history observer.
Filesystem deletion ordering/application, compound semantic/presentation history,
complete keyboard-shortcut integration, remaining application action handlers,
full panel composition and native editor workflow acceptance remain open. It does
not establish native dialog, rendering, responsiveness or whole-editor parity.

Next: continue the filesystem/presentation-aware history/action integration and
complete shortcut dispatch, then remaining panel/native workflow acceptance. The
root engine switch still requires the full compatibility release gate.

## Validation

Targeted candidate graph/session/history gate: **248 passed**, one existing timing
probe ignored. Strict candidate all-target Clippy with the editor/asynchronous
feature combination passes.

Accepted final frozen headless run (2026-10-13):
`target/bevy-020-qualification/runs/eca5211cfb1e42e3bf59df9bb5a1386a/`.
It executes **667 checks**, including **248 graph/session/history checks** and
**43 code/search/context-menu widget checks**, with all **690 input hashes**
unchanged. The graph/session/history process takes 2.66 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false; no new native editor acceptance is claimed.

Strict workspace all-target Clippy and all-target checking with `aestra-bench/gpu`
pass. Both formatting checks and whitespace validation pass. Root manifest,
toolchain and lockfile diffs are empty.

Shipping regression: **1,381 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Existing Windows incremental-cache hard-link fallback
warnings do not fail the gates.
