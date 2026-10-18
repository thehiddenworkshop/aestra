# B20-2 slice 57: Curves activation, keyboard and action routing

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate executes
the actual Curves activation observer, keyboard handler and complete action
dispatcher, including property, gradient and host-transform actions.

- `curves/controls.rs` shares the production activation/dispatch behavior. Its
  semantic action match is unchanged. The input boundary retains the original
  Interaction query on shipping; the candidate maps native Pressed/Hovered marker
  changes and removals to the same phases. Pending activation consumption,
  legacy-button styling and Feathers-owned styling retain their existing policy.
- `curves/keyboard.rs` shares the original Insert/Delete handler and graph marker,
  including optional-input, hovered-graph and focused-editable-text guards.
- `curves/host_editing.rs` shares fifteen unchanged host-curve operations and
  projection/preview/cancellation helpers. The native host module reexports them;
  its constructor, installation function and existing tests remain unchanged.
  Candidate pose selection uses the same actual timeline motion consumers.
- `docking/model.rs` shares the complete original persisted workspace model,
  including tool/editor tab identities, defaults, serialization, model operations,
  load/save and config paths. Native docking reexports the same API. Internal
  visibility is widened only across the extraction boundary; no model behavior
  or serialized field changes. `shell/reveal.rs` shares the unchanged actual
  panel reveal/save function, not a replacement docking callback.

Audits compare the complete model, fifteen host helpers, keyboard handler and
semantic action match after visibility/format/trailing-comma normalization. Native
Curves/Docking plugin installation, host installation/construction, reveal behavior
and all four existing Curves/host/docking/shell root test blocks remain unchanged.
Root dependencies, engine features, toolchain and authored formats are unchanged.

## Regression coverage and limits

Twenty-three new shared headless tests install the real observer/consumers, actual
module metadata and sessions, complete workspace model, host tracks, and production
global history routing. They cover:

- One edit per real activation, repeated activations, pending/pressed cleanup,
  non-command Feathers presses and unrelated/non-Feathers activation targets.
- Actual native-control hover/press/release styling without repeating edits;
  Feathers styling ownership; property opening and vector-channel selection.
- Scalar/vector interpolation with untouched channels preserved and duplicate
  interpolation as a no-op; property delete/time/value commands; gradient RGBA
  dispatch and interpolation no-op; stale property metadata.
- Host opening/channel clamping, scalar and quaternion interpolation, key-at-
  playhead insertion without duplicate time keys, anchor deletion protection,
  non-anchor deletion and stale-effect guards, with global undo/redo.
- Real keyboard Insert/Delete consumers with explicit cursor positions, missing
  or unhovered graphs, absent key resources and focused EditableText guards.
- The actual model restoring a closed Curves tab without duplication, RON
  round-tripping and preserving already-floating Curves panels.

Action fixtures deliberately start with Curves already active. The actual reveal
function runs its no-change path, so these tests do not save the user's workspace
layout. Separate model-only tests exercise closed/floating transitions without
persistence. Source effect files remain byte-identical; accepted edits are checked
through global undo/redo for one semantic history entry.

This is not native picking/layout/rendering or a complete Curves panel/plugin
acceptance gate. Closed-panel reveal persistence, native floating-window lifecycle,
host graph drag observers and complete composition remain separate. Existing
stale-action/pointer-ownership/cancellation hardening is not silently changed by
this compatibility port.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/a7c83b35621b43399b3f16d51a695d90/`.
It executes **1,571 checks**, including **1,150 editor graph/session/history and
consumer checks**, with all **884 input hashes** unchanged. Counts are executions
across gates, not unique tests. Eight source/test paths join the inventory. The
graph suite takes 46.45 seconds after compilation, with its existing timing probe
ignored. The twenty-three new tests also pass in isolation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`,
and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Strict candidate all-target Clippy, both formatting checks and Git whitespace
validation pass.

Shipping passes **2,178 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict shipping workspace all-target Clippy and workspace
all-target checking with `aestra-bench/gpu` pass. Root dependency/toolchain diffs
remain empty and post-documentation input drift is zero. Existing Windows
incremental-cache hard-link fallback warnings are non-fatal.

Next: complete Curves panel composition and host graph drag observers, then full
Curves plugin integration and native editor workflow acceptance. B20-2 remains
open; shipping stays on 0.19.1 until the full compatibility release gate.
