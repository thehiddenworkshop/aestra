# Function socket and selection pointer events — Bevy 0.20

Date: 2026-10-21. B20-2 slice 94. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

The actual function socket start/motion/end/drop lifecycle and preview state move
to `material_function_editor/graph/connection.rs`. Node/canvas/marquee selection
and nested-call navigation move to `graph/selection_input.rs`. Native registration
uses these shared subregistrars in the original positions: all 16 graph observers
retain their order after expanding the body-action and pointer subregistrars.

The existing material-graph native drag/click API boundaries are reused within the
crate, with only native DragDrop aliases and test-event constructors added. No
gesture lifecycle is translated or synthesized by a replacement callback. Pointer
position reads use the same existing native location accessor on both engines.

Socket release also reaches the actual `graph/socket_palette.rs`, compiled directly
in the candidate with explicit imports. Its 12 production function bodies are
unchanged. All seven socket handler/helper bodies are preserved apart from native
location access; all four selection/navigation bodies are preserved apart from
the picked-target correction below. All 18 remaining graph handler/helper/test
bodies are unchanged. Preview state, snapping radius, validation, placement,
publication and history policy are retained.

The isolated fixture matches the inherited shipping argument/query-complexity
lint policy on these existing observer modules. Shipping lint configuration is
unchanged. No manifests, lockfiles, toolchain, CI, budgets or visual references
change. Frozen hashing includes the actual shared handlers, palette and tests.

## Picked-target regression and fix

A new propagation case failed before the correction, including when tested with
the actual composed constant field's editable child. A control click was initially
excluded by the control/socket guard, but bubbling changed the current event target.
The global observers then reinterpreted an ancestor as a node or canvas click,
changing or clearing selection. This is not assumed to be a 0.20-only regression.

The three shared click handlers now resolve from `original_event_target()`, so
ancestor lookup and control guards retain the originally picked descendant during
bubbling. Node clicks still select once and stop propagation; canvas clicks still
clear only their own view. The same correction applies to shipping and candidate.

## Shared regressions

Ten new native-event cases use the real composed function canvas and actual
registered socket/selection handlers:

- Descendant socket resolution, preview cursor updates and foreign-view filtering.
- Release-time snapping without an intermediate layout pass at three DPI/zoom
  combinations, one semantic commit and real global undo/redo.
- Drop on socket descendants followed by DragEnd cannot create a duplicate edit.
- Escape cancellation, unrelated targets and non-primary starts preserve work.
- Cross-view and other-owner drops cannot rewire the function.
- Child clicks, Control toggle and Shift extension preserve scoped node selection.
- Socket/constant descendants, Space and secondary clicks do not select nodes.
- Real marquee keys and canvas clicks preserve selection in another view.
- Node propagation is consumed once and actual constant-field clicks cannot switch
  or clear an existing selection during bubbling.
- Empty-canvas palette entry creates no semantic edit before a choice.

Eight existing palette tests additionally run on the candidate, retaining all
35 assertions and test identities. They cover transactional choices, missing
endpoints, named argument choices, foreign views, filtered Enter, explicit restored
targets, coordinates/cleanup and compound undo/redo with stale-menu rejection.
Their UI fixture now registers the actual focused palette/socket/selection paths
on both engines; the existing native full-registrar graph tests remain unchanged.
Only native pointer construction and fixture setup/imports are adapted.

## Validation

The complete candidate graph executable passes with default parallel execution:
**1,948 passed**, one existing ignored timing probe, no failures (17.59 seconds).
Shipping passes **2,797 editor unit + 3 architecture tests**, with default parallel
execution, the same ten existing ignores and no failures (7.58 seconds for units).
Both strict all-target Clippy gates, both formatting gates, whitespace and workspace
GPU-feature all-target checking pass.

Accepted frozen run:
`target/bevy-020-qualification/runs/f5da2e9a652948449cb8665e49fdb2b8/`.
It executes **2,380 passing checks**, including **1,948 editor graph checks**
(116.13 seconds after compilation). Counts are executions across gates, not unique
tests. All **1,076 input hashes** are unchanged; native execution is false.
Post-documentation input drift is zero. Some Cargo invocations report incremental
cache hard-link fallback to copying; there are no code lint failures.

## Remaining work

Qualify function context-menu consumers, modified-node drags, wire updates and the
complete graph registrar, then remaining plugin/application handlers and complete
editor workflows. These are native event dispatch and real headless UI layout
checks, not OS picking, GPU wire rendering or full native editor acceptance. Native
acceptance remains required before changing shipping dependencies.
