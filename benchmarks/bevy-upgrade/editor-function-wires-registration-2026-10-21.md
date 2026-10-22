# Function wires and complete registration — Bevy 0.20

Date: 2026-10-21. B20-2 slice 96. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

The original function wire attachment and post-layout update systems move into
`material_function_editor/graph/wires.rs`. Their bodies are unchanged, including
optional asset handling, measured socket anchors, per-viewport endpoint lookup,
insertion coloring, snap-radius feedback, ghost direction and stale-origin cleanup.
Wire attachment remains in Update; uniform updates remain after UiSystems::PostLayout.

The complete production graph registrar moves to `graph/registration.rs`; the
function editor root registrar moves to `material_function_editor/registration.rs`.
The candidate compiles and calls both actual registrars, not a selected subset of
their observers. Graph registration still precedes signature registration, and all
16 graph observers retain their order. Update dismissal and clipboard keyboard
registration remain unchanged. Shipping entry points are re-exports.

The existing saved-canvas fixture accepts a registration function so its original
focused tests retain exactly their observers while the new integration tests use
the complete production root. No edit/history/presentation callback is substituted.
The new fixture removes inherited editor resources before registration to verify
the production root recreates its owned resources. Wire assets are real Assets
and MaterialNode handles; layout uses the real UI layout systems without a GPU.

Source comparison preserves both wire-system bodies byte-for-byte after newline
normalization, the graph registrar after expanding the wire subregistrar, and all
six original graph test bodies. No dependencies, root
manifests, lockfiles, toolchain, CI, budgets, validation policy or visual references
change in this slice. Frozen hashing includes the four new shared source/test files.

## Shared regressions

Eight new cases run on both engine graphs:

- Complete registrar resource ownership without the widget plugin.
- Missing optional wire assets do not panic; adding the asset store attaches one
  stable handle per wire/ghost without repeated allocations on later updates.
- Saved-camera rebuilds project real socket anchors at two pans, two zooms and
  display scales 1.0, 1.25 and 2.0, preserving exact graph projection and logical
  line width/inverse display scale.
- Two composed views of the same function keep independent endpoint projection;
  only the initiating view displays its ghost wire.
- Native drag motion snaps at logical distances 17/19 around the existing radius
  of 18, updates socket feedback and hides the ghost on release. Release uses the
  latest pointer, commits one actual connection and round-trips through real undo.
- Dragging from an input reverses ghost endpoint direction; despawning the origin
  clears the connection preview and hides the ghost on the registered layout pass.
- Signature text changes, constant numeric changes and body creation remain
  distinct with all observers registered, including real global undo/redo and
  preservation of the current effect.
- Actual node clicks select, the registered clipboard keyboard system duplicates,
  undo restores the function, and the registered context dismissal handles Escape.

Initial fixture failures clarified existing contracts, without production changes:
UI rounds socket/node pixels independently (the visual comparison allows 1.5
physical pixels; graph projection retains a 0.1 logical-unit tolerance); release
over a compatible socket commits even before the next layout update; saved camera
memory seeds composition rather than mutating an already-mounted live camera.
The tests explicitly exercise rebuilds and real release/undo behavior accordingly.

## Validation

The complete candidate graph executable passes with default parallel execution:
**1,967 passed**, one existing ignored timing probe, no failures (30.03 seconds).
Both strict all-target Clippy gates, both formatting gates and whitespace checks pass.

Accepted frozen run:
`target/bevy-020-qualification/runs/4f3020180e1141b28522d4fa80fcc620/`.
It executes **2,399 passing checks**, including **1,967 editor graph checks**
(116.76 seconds after compilation). Counts are executions across gates, not unique
tests. All **1,085 input hashes** are unchanged; native execution is false.
Post-documentation input drift is zero.

Shipping passes **2,815 editor unit + 3 architecture tests**, with default parallel
execution, the same ten existing ignores and no failures (7.38 seconds for units).
Workspace GPU-feature all-target checking passes. Some Cargo invocations report
incremental-cache hard-link fallback to copying; there are no code lint failures.

## Remaining work

Continue with remaining material-plugin registration/application handlers and
complete editor workflows. These tests qualify actual CPU layout, native event
transport and wire uniform data, not OS picking, GPU wire appearance or complete
native editor acceptance. Native acceptance remains required before switching
shipping dependencies. No visual reference is approved.
