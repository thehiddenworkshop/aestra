# B20-2 slice 17 — Real session preview and scoped semantic history

The isolated Bevy 0.20 candidate now imports the actual `EditorSession`, including
its preview installer, command history, playback driver, checkpoints, save/reopen,
recovery and interaction candidates. Shipping remains Bevy 0.19.1; the root engine
manifest, lockfile and Rust toolchain are unchanged.

## Implementation

- `session.rs` is imported directly, not reproduced by a qualification surrogate.
  The candidate also uses the existing `test_support.rs` and `preview_mocks.rs`.
  Existing session tests run on both engines, alongside five new installer tests.
- `material_document/context.rs` contains the existing standalone/effect graph
  authoring context, source identity validation and project-root guards. Existing
  material-document tests and the host I/O guard remain in their original module.
- `history/material_program_execution.rs` contains existing session-backed material
  replacement/extraction, draft synchronization, scoped stack selection and undo/redo.
  `material_function_editor/execution.rs` contains actual function body preparation,
  call-site-validated publication, diagnostics and root-scoped edit/step execution.
- `history/document_execution.rs` contains the existing effect/material ledger and
  semantic action-domain executor. `history/asset_order/session_context.rs` contains
  actual session context selection. Shipping calls these same modules; the complete
  application observer still owns focus routing, modal/I/O guards, filesystem history
  and compound presentation transactions. No second history algorithm is introduced.
- The candidate adds the actual linked example extension used by existing migration
  tests. Their expected requirement now reads the registered extension version,
  instead of assuming it equals the consuming executable's package version. No
  installed extension version, source file or shipping dependency is changed.
- The frozen runner now includes these sources, their fixtures, and all example
  extension source inputs. Candidate lock changes add only the local path package.

## Regression coverage

Twelve new shared regressions cover:

- Exact compiled instance installation with preserved effect, source, selection,
  undo stack, playhead, seed, playback and dirty state; stale samples/checkpoints
  are invalidated. Ordinary effect undo still retains shared material compilation.
- Solo recompile and unmodified authored flags; interaction candidate precedence
  over an applicable pending candidate; neither candidate is committed.
- Failed solo recompilation preserves the previous presentable instance, samples,
  checkpoint store, clock and revision; successful installation recovers stale preview.
- Actual effect/material action chronology and compiled material undo/redo; standalone
  material actions do not touch an unrelated invalid effect or its preview/playhead.
- Same program identity across two project roots has independent stacks; wrong-root
  publication fails without recording history. Failed undo remains retryable without
  losing either history stack or the currently installed preview.
- Function actions retain effect/program history; replacing the effect resets obsolete
  global material history. Function and material changes remain draft-only until saved.
- Real planner-produced function extraction rewires the material, compiles/publishes
  both assets, saves them, then undoes/redoes them together without overwriting disk.
  Assertions use canonical normalized source form after publication/persistence.

The candidate additionally executes 34 existing session tests, three existing semantic
fixture tests, and two existing preview-mock tests. Catalog, compiler, drafts, session,
playback, ledger and semantic execution are real production types throughout.

## Remaining scope

Project watch/background-I/O lifecycle, the complete focus/modal/code-editor/filesystem
and presentation-aware action observer, remaining application handlers, full panel
composition and native editor workflow acceptance remain open. This headless slice
does not establish native rendering, responsiveness or end-to-end GUI parity.

Next: qualify project watch/I/O lifecycle and the remaining application action integration,
then full panel/native workflow acceptance. Do not switch the root dependency graph or
replace controls until the compatibility release gate is satisfied.

## Validation

Targeted candidate graph/session gate: **199 passed**, one existing timing probe
ignored. Strict candidate all-target Clippy and both formatting checks pass.

Accepted frozen headless run (2026-10-12):
`target/bevy-020-qualification/runs/83df6f96f0b54d449c5edbd896750ff5/`.
It executes **613 checks**, including **199 graph/session checks**, with all **675
input hashes** unchanged. Counts are test executions across the gates, not unique
tests. The graph/session process takes 1.26 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false; no new native editor acceptance is claimed.

Shipping regression: **1,352 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
workspace checking with `aestra-bench/gpu` pass. Both formatting checks and
whitespace validation pass; root manifest/toolchain/lockfile diffs are empty.
Existing Windows incremental-cache hard-link fallback warnings do not fail the gates.
