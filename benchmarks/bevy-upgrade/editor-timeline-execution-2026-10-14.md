# B20-2 slice 34: host-motion and automation action execution

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now executes
the production host-motion action observer, numeric edit observer and automation
action handler with real session/history/curve-selection state.

- `timeline/motion_execution.rs` shares inspect/select/add/delete/clear/repeat/
  close-loop actions, pose sampling/validation/retiming, selected-pose access and
  numeric live-preview/final-commit handling. It uses the actual action and numeric
  control identities. Native buttons, pointer drag controllers and inspector
  composition continue calling the same shared implementation.
- `timeline/automation_execution.rs` shares the complete automation action branch,
  semantic command construction and stored-curve sampling. Native choreography
  execution delegates to the same handler in its original position, and native
  pointer commits keep using the same command builder.
- `curves/state.rs` shares the full production curve-selection resource, complex
  selection and host-curve transient selection/drag payloads. Native curve-panel
  consumers retain their existing paths; no simplified fixture resource replaces it.
- The runner freezes these modules/tests and the native curve callers as well as
  the already-frozen timeline callers. Candidate composition uses actual source
  imports, with no dependency, feature, toolchain or authored-format change.

Source audits compare all **23 moved contracts/functions** plus native motion
installation/activation/button dispatch. Only visibility, formatting and the internal
curve-state type path differ. Production observer registration and ordering are
unchanged. Existing native timeline, curves and motion regressions are retained.
No behavior fix or UI redesign is bundled with this compatibility slice.

## Regression coverage

Twenty-three new shared regressions cover actual execution, not just event capture:

- Real keyboard Insert/Delete reaches host-motion and automation handlers, creates
  or removes semantic data, updates selection and participates in production history.
- Pose creation/anchor sampling, duplicate-time no-ops, nearest-key inspection,
  stale selection/owner rejection, protected first-anchor deletion and single-key
  track removal.
- Rejected open-seam repeat, reversible close-loop/repeat/clear actions and endpoint
  position/rotation/scale edits preserving the loop seam.
- Numeric live preview leaves authored data/history unchanged, followed by one
  final undoable commit. No-op/invalid final values restore the committed preview;
  Escape suppresses a late release; stale controls/effects/selections do not edit.
- Numeric retiming rejects collisions and accepts valid changes without changing
  existing pose identities/selection policy.
- Scalar and gradient insertion, duplicate-time selection, minimum-key protection,
  missing/stale IDs, nonfinite key bits and non-semantic visibility/selection.
- Playhead insertion accounts for the selected region's source offset. Parameter
  bindings edit the parameter rather than the module bank, and a vector source-bank
  edit changes only the requested channel and curve-panel selection.
- Mixed motion/automation global undo/redo order and redo forks use actual Routing,
  ledger, session, catalog and material/function history. Unexpected filesystem
  history fails the fixture. Unowned choreography actions explicitly decline.

The automation fixture observer adapts actual event resources to the unchanged
production handler, including recursively queued add/select actions. It is not a
replacement implementation of choreography semantics. The motion tests register
the actual production observers and use real typed ValueChange events. Keyboard
integration uses actual InputPlugin, focus/dispatch plugins and EditorKeyboardPlugin.
Full native choreography observer scheduling and rendered numeric composition are
not implied by those fixtures.

## Validation

Accepted frozen headless run (2026-10-14):
`target/bevy-020-qualification/runs/c98a5fd9bbe846c79838c6c0c3804db4/`.
It executes **986 checks**, including **567 graph/session/history/consumer checks**,
with all **791 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 12.56 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false. Shipping passes **1,653 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting checks and whitespace validation. Root manifest/toolchain/lockfile diffs
remain empty. All 791 frozen inputs still match after validation and documentation
updates. Existing Windows incremental-cache hard-link fallback warnings do not fail
these checks.

## Remaining scope

Next: the remaining top-level timeline/choreography observer, local emitter/region
and referenced-effect actions, then complete panel/plugin composition. Host-motion
pointer/numeric cancellation controllers and full Curves panel consumers still need
their own 0.20 composition acceptance; this slice does not claim their rendered UI
or complete native workflow parity. Remaining viewport/application and graph menu/
palette consumers remain open. No new native-window/GPU acceptance is claimed;
shipping still waits for B20-3 before switching the engine graph.
