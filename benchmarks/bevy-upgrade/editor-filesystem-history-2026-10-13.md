# B20-2 slice 20: filesystem chronology and redo-branch synchronization

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated Bevy **0.20.0** candidate now
imports the complete production filesystem/document ordering bridge and its
session context selection, plus the existing history synchronization and
presentation-redo branch policy. No root dependency, authored format, deletion
policy or widget is changed.

- `history/asset_order.rs` declares its actual dependencies and is imported directly
  by the candidate. Its real `Context`/`EditOrder` types replace the previous
  candidate facade, so session history and filesystem ordering share one identity.
  Explicit source-relative module paths work in both consuming applications.
- `history/synchronization.rs` contains the existing effect-ledger synchronization,
  scoped presentation-redo clearing and document-order retirement. Shipping calls
  these same functions; no duplicate ordering algorithm or candidate ledger is added.
- The candidate exercises real `aestra-project` deletion plans, immutable recovery
  journals, restoration and redo checks against temporary projects. It also uses
  actual session-backed material/function publication and undo to populate histories.
- The frozen runner now includes the new shared tests and synchronization source.

## Regression coverage

Seventeen new shared regressions cover:

- Filesystem ordering remains inactive until a real deletion; the first deletion
  adopts existing document chronology, and later effect/material contexts interleave
  with it in the exact global step order.
- Real delete/restore/redo preserves exact bytes, empty directories and opaque host
  recovery data; redo produces a new journal. Wrong direction or stale revision
  cannot advance the recorded deletion step.
- External collisions and changed redo sources fail without overwriting disk or
  consuming the pending history entry. A blocked restore can be retried safely.
- Catalog revision refresh retains chronology, while project generation/root
  changes, reopening a document, serial regression and bounded-journal overflow
  retire ambiguous order. Recovery remains available after these resets and after
  the bounded 256-entry undo history evicts a deletion.
- Clearing one context or forgetting one restored journal preserves other contexts
  and recovery records.
- Presentation forks clear only the selected effect/program/function redo branch,
  preserving unrelated histories, source bytes, drafts and the installed preview.
- Recording deletion or syncing a new edit while filesystem chronology is active
  clears all semantic redo branches as before. Ordinary effect synchronization
  retains unrelated standalone redo; replacing the effect retires material history.

## Remaining scope

This qualifies chronology and redo coordination, not the complete GUI deletion
workflow or full presentation-aware action observer. Asynchronous deletion
preparation/application, closing/restoring open document views, compound graph
semantic/presentation validation and application, complete shortcut dispatch,
remaining application/panel composition and native editor workflows remain open.
No native dialog, rendering, responsiveness or whole-editor parity is claimed.

Next: qualify the compound presentation-aware undo/redo execution boundary, then
the remaining filesystem/UI action composition and shortcut dispatch. The root
engine switch still requires the full compatibility release gate.

## Validation

Targeted candidate graph/session/history gate: **265 passed**, one existing timing
probe ignored. Strict candidate all-target Clippy with
`editor-qualification,async-qualification` passes.

Accepted frozen headless run (2026-10-13):
`target/bevy-020-qualification/runs/98f4e3f7d1e64d0ea09a15a5519418ad/`.
It executes **684 checks**, including **265 graph/session/history checks**, with
all **693 input hashes** unchanged. Counts are test executions across gates, not
unique tests. The graph/session/history process takes 3.37 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false; no new native editor acceptance is claimed.

Shipping regression: **1,398 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
checking with `aestra-bench/gpu` pass. Both formatting checks and whitespace
validation pass. Root manifest, toolchain and lockfile diffs are empty.
Existing Windows incremental-cache hard-link fallback warnings do not fail the gates.
