# Function body actions and constant editing — Bevy 0.20

Date: 2026-10-21. B20-2 slice 93. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

Production body-action, constant text/number observers and their semantic helpers
move from `material_function_editor/graph.rs` to `graph/body_input.rs`. Native and
candidate controls use the same BodyAction/Constant identities and actual
publication, function-target activation, placement and global history services.
No pointer adapter, alternate edit callback or surrogate function is introduced.

The native graph registrar invokes the shared action registrar at the original
action position, retains socket-drop registration, then invokes the shared
constant registrar. All 16 original graph observers retain their order. Existing
socket-palette consumers retain the original node-creation helper through its
canonical parent import. Only now-test-only parent imports are narrowed.

Source comparison preserves all six moved handler/helper bodies and all 29
remaining graph handler/helper/test bodies. Registration order is checked after
expanding the two shared registrars. The action observer retains the shipping
argument-count lint policy through a narrow annotation; its dependencies and
behavior do not change. No manifests, lockfiles, toolchain, CI, placement budgets,
validation policy or visual references change. Frozen inputs include both new
source/test files.

## Shared regressions

Ten new tests construct real graph toolbars, node action menus and constant fields
with the production canvas composer, then register the actual shared observers:

- Node creation from an explicitly owned control activates its function from an
  unrelated active target without changing the effect; it places new nodes and
  preserves a pinned manual position, with one compound global undo/redo step.
- Input-node creation references the actual declared port and is globally undoable.
- Boolean selection preserves expression identity and repeated values are no-ops.
- Scalar text commits only final edits; bad syntax and nonfinite values leave the
  document/history unchanged. Repeating a value creates no extra undo entry.
- Vector/color numeric edits preserve type and other channels, reject nonfinite
  components, and round-trip actual global undo/redo.
- Node-menu deletion rejects a required output source atomically and removes an
  unused node with one undo step.
- Arrange, Locate and Back dispatch their actual payloads, including explicit
  project/function/view and selection seeds, without semantic edits. Arrangement
  execution remains separate from this request-routing check.
- Old controls cannot edit the new project after catalog/target replacement.
- Actual keyboard typing remains live until Enter submits one constant edit.
- Creation without viewport memory retains semantic validation and undo behavior.

Saved function bytes remain unchanged throughout these draft edits. Pointer
gestures and their propagation are not simulated by replacement callbacks.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/55266500d2e7457485ffc349764d4f0a/`.
It executes **2,362 passing checks**, including **1,930 editor graph checks**
(130.60 seconds after compilation), with the existing ignored editor timing probe.
Counts are executions across gates, not unique tests. All **1,071 input hashes**
are unchanged. Native execution is false.

The same candidate graph executable also passes with default parallel execution:
**1,930 passed**, one existing ignored timing probe, no failures (23.55 seconds).
Both strict all-target Clippy gates, both formatting gates and whitespace checks
pass. Workspace GPU-feature all-target checking passes.

Shipping passes **2,787 editor unit + 3 architecture tests**, with default parallel
execution, the same ten existing ignores and no failures (7.65 seconds for unit
tests). Post-documentation input drift is zero. Some Cargo invocations report
incremental-cache hard-link fallback to copying; there are no code lint failures.

## Remaining work

Qualify function-graph pointer gestures, context-menu consumers, wire updates and
the complete graph registrar, then remaining plugin/application handlers and full
editor workflows. This slice is headless action/value integration, not complete
native pointer, GPU wire, DPI or editor acceptance. Native acceptance remains a
prerequisite for changing shipping dependencies.
