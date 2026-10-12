# B20-2 slice 18: project watch and background I/O lifecycle

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated candidate qualifies the same
production project I/O, watcher and lifecycle schedule on Bevy **0.20.0**.
No root dependency, authored format or widget changes are included.

- `project_content/services.rs` holds the existing serialized I/O polling followed
  by idle-gated filesystem refresh, in the same `ProjectContentSet::Input` set.
  The shipping plugin installs it after catalog initialization. Native texture-root
  initialization/synchronization remains in the shipping wrapper with its existing
  persistence/render ordering; it is not replaced by a candidate renderer mock.
- `project_content/io.rs` and `refresh.rs` now declare their dependencies explicitly.
  The candidate directly imports these production sources, actual catalog/session,
  drafts, compiler and Fluent localization. Existing refresh tests are shared too.
- The graph/session gate now enables `editor-qualification,async-qualification`,
  so the actual Bevy I/O pool runs multithreaded as in the shipping editor. Other
  headless/native renderer gates retain their separate feature configurations.
  This feature combination has an additional first-build cost; task behavior is
  not substituted with a synchronous candidate implementation.
- The frozen runner includes the service wrapper and new shared tests in its input
  hashes. No filesystem action handler is rewritten by this slice.

## Regression coverage

Thirteen new shared regressions cover:

- A real worker disk write, result draining, and busy state retained until deferred
  application; busy/stale enqueue rejection calls modal cleanup without running
  the rejected worker, and the queue remains usable afterward.
- Distinct project/document guards, catalog revisions, material targets, locks,
  pending proposals, draft changes, and reopening the same identity/path. Material
  reload accepts preview-only recompilation but rejects authored state changes.
- Prepared catalog publication preserves newer live drafts, leaves disk unchanged,
  and resets the watch baseline to the published revision.
- Actual task-pool/update scheduling discovers generic files without invalidating
  preview/clock revisions, reloads clean sources, preserves dirty documents, and
  applies explicit I/O completions before watching the published catalog.
- An old-root scan cannot publish after switching projects. The new root remains
  watchable without replacing the session. Watching stays gated even when a worker
  has finished but its completion has not been applied.
- Malformed external sources preserve the presentable instance; repairing the file
  and requesting refresh recovers the document.

The candidate also executes eleven existing refresh tests and eight localization
tests. Existing deterministic refresh regressions exercise stale worker results,
captured-byte application, coherent cached queries and exact-byte draft conflicts.

## Remaining scope

The complete focus/modal/code-editor/filesystem/presentation-aware history observer,
remaining application action handlers, full panel composition and native editor
workflow acceptance remain open. Headless project lifecycle qualification does not
establish native GUI, GPU, texture-root, responsiveness or whole-editor parity.

Next: qualify the complete history/action integration without replacing current
controls, then continue application/panel and native workflow acceptance. The root
engine switch still requires the full compatibility release gate.

## Validation

Targeted candidate graph/session/lifecycle gate: **231 passed**, one existing timing
probe ignored. Strict candidate all-target Clippy with the editor/asynchronous
feature combination passes. Strict workspace all-target Clippy with
`aestra-bench/gpu` passes.

Accepted frozen headless run (2026-10-12):
`target/bevy-020-qualification/runs/23f4798368f34f3bb62de6436bf6018f/`.
It executes **645 checks**, including **231 graph/session/lifecycle checks**, with
all **680 input hashes** unchanged. Counts are test executions across the gates,
not unique tests. The graph/session/lifecycle process takes 2.38 seconds after
compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false; no new native editor acceptance is claimed.

Shipping regression: **1,365 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
workspace checking with `aestra-bench/gpu` pass. Both formatting checks and
whitespace validation pass; root manifest/toolchain/lockfile diffs are empty.
Existing Windows incremental-cache hard-link fallback warnings do not fail the gates.
