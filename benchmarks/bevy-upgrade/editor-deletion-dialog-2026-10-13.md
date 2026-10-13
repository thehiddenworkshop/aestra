# B20-2 slice 24: complete delete/restore dialog and browser publication

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated Bevy **0.20.0** candidate now
executes the complete production delete/restore coordinator and Feathers dialog,
including actual browser state and bookmark reconciliation. This extends the
previous worker-only gate; browser callbacks are no longer replaced by recorders
in these new regressions.

- `asset_browser/deletion/panel.rs` contains the existing action observers, modal
  state, publication callbacks, dialog spawning, synchronization, focus return,
  localized labels and actual Feathers button composition. The shipping module
  preserves its existing Open/register/spawn/history-step entry points via reexports.
  Native browser integration and existing tests keep using those entry points.
- The actual browser snapshot model is imported directly. The bounded bookmark
  identity/collection model moves to `bookmarks/model.rs`; native successful-open
  observers and preference persistence keep their existing behavior and ownership.
  No candidate browser model or replacement recovery policy is introduced.
- Real scrollbar spawning and overflow visibility move to `feathers/scroll/bars.rs`.
  Both engines use the same scene wrappers and visibility system. Native persisted
  vertical/XY scroll wrappers and their callers retain their interfaces and policy.
- The candidate uses the actual version-selected Aestra theme with real Feathers
  plugins, text measurement, UI layout and an explicit headless window/camera.
  Tests initialize no native window event loop or GPU backend.
- Internal visibility needed by the shared composition changes without exposing a
  public library API. State/data shapes, journal encoding, authored formats, deletion
  guards, callback order, button styling and layout are unchanged.
- The frozen runner includes the dialog/tests, browser state, bookmark consumer/model
  and scrollbar consumer/model. Root manifest, toolchain and lock remain unchanged;
  no additional fixture dependency is needed in this slice.

## Regression coverage

Fifteen new shared regressions cover:

- Empty recovery loading and real initial deletion followed by manual restore,
  including selection/inspection cleanup, dormant favorites/recent items, locate
  behavior, filter clearing, modal closure and localized status publication.
- Document changes during preparation reject disk writes; explicit retry safely
  reloads. Other modals, busy I/O, stale generations, malformed activation and
  missing recovery indices cannot apply an unintended operation.
- Stale manual-restore preflight rejects until refreshed. Real recovery journals
  paginate and clamp correctly at both ends.
- Real Feathers buttons, accessible localized Close labels, long-error scrolling
  and visible overflow scrollbars at 800x500/1x English and 900x600/1.5x French.
  The blocker contains Retry/Close, never a force-delete action.
- Actual Escape keyboard messages and Close activation restore prior focus or clear
  a removed target. Busy controls remain disabled until completion; idle controls
  preserve entity identity and focus rather than rebuilding every frame.
- Project switching closes recovery without mutating either project.
  The real history router/worker uses real browser publication callbacks to revive
  favorites on undo and retire selection on redo.
- Initial deletion and bin restoration recover an open unsaved material while
  preserving the saved source bytes rather than saving its draft over disk.

## Remaining scope

History menu/button availability, complete shortcut dispatch, other application
action handlers, the complete asset-browser plugin (including preference/open
observers), full editor panel composition and interactive native workflow
acceptance remain open. This is a real headless dialog/layout gate, not native
window/input/rendering acceptance or a claim of whole-editor parity.

Next: history controls/availability and complete keyboard shortcuts, then remaining
application/panel composition and native workflows. The root engine switch still
requires the full compatibility release gate.

## Validation

Candidate graph/session/history gate: **325 passed**, one existing timing probe
ignored. Shipping regression: **1,458 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict shipping and candidate all-target
Clippy, workspace all-target checking with `aestra-bench/gpu`, both formatting
checks and whitespace validation pass.

Accepted final frozen headless run (2026-10-13):
`target/bevy-020-qualification/runs/3cca0c57ab504e03ade84aea83261d25/`.
It executes **744 checks**, including **325 graph/session/history checks**, with
all **715 input hashes** unchanged. Counts are executions across gates, not unique
tests. The frozen graph/session/history process takes 6.30 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false. Root manifest/toolchain/lockfile diffs remain empty.
Existing Windows incremental-cache hard-link fallback warnings do not fail the gates.
