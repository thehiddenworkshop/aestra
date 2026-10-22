# Function context menus and modified drags — Bevy 0.20

Date: 2026-10-21. B20-2 slice 95. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

Production function context-menu opening/hit testing, action dispatch and dismissal
move to `material_function_editor/graph/context_input.rs`. Duplication and modified
node-drag consumers move to `graph/modified_input.rs`. The candidate uses the same
composed menu action identities, clipboard, function activation, publication,
placement and global history services. Native click transport reuses the existing
engine boundary; only event types and native pointer-location access differ.

The actual presentation batch observer and its context-resolution/recording helpers
move to `material_graph/presentation/batch_input.rs`. Native node/pin recording
still uses the same helper, and the native batch entry point remains a re-export.
Tests register this production observer so branch movement reaches real
presentation history rather than a request-only stand-in.

Source comparison preserves all seven context handler/helper bodies apart from
native location reads, all three modified-drag helper/handler bodies, all four
presentation batch/context bodies apart from an explicit module path, and the
eight remaining graph wire/helper/test bodies. All 16 graph observers retain their
order after expanding shared subregistrars. Dismissal remains in Update; wire
updates retain their PostLayout scheduling. The existing arrangement-context test
body is unchanged, with only explicit imports and its test module parent adapted.
Remaining native arrangement, pin and node presentation bodies are unchanged.

The candidate retains the existing shipping observer argument/query lint policy.
No manifests, lockfiles, toolchain, CI, budgets, validation policy or visual
references change. Frozen hashing includes the actual shared sources and tests.

## Shared regressions

Ten new cases use real graph composition, context menu items, registered production
consumers and global undo/redo:

- Group duplication activates an explicitly owned function from an unrelated active
  target, preserves the effect and undoes semantic/layout changes in one step.
- Actual node-menu deletion removes an unused expression and its placement in one
  undo step; deleting a required output source preserves body, placement and history.
- Context Copy/Paste uses the real shared clipboard and creates one undoable edit.
- Connected-pin menus identify required outputs and reject removing their source.
- The real Arrange submenu retains selected seeds and explicit project/view scope
  without focusing an unrelated active target or making a semantic edit.
- Hover retention and Escape dismissal use actual menu surfaces without edits.
- Modified duplication uses the drag delta, retains the original node and records
  one compound global edit with undo/redo.
- Upstream/downstream moves include the correct branch/output, leave unrelated
  nodes alone, and round-trip through actual presentation-only history.
- An unknown modified-drag target cannot move or edit the graph.

The modified-drag cases deliberately use explicit placement snapshots from the
composed nodes. They test the existing positioned-graph contract, not a new policy
for manufacturing placements of bootstrap-only nodes. Initial failures from assuming
that bootstrap geometry was already saved placement were corrected in the fixture;
production drag policy is unchanged. Saved function bytes remain unchanged.

The existing arrangement-context case additionally runs on the candidate. It covers
selection/upstream/downstream requests, explicit unfocused targets, submenu hover,
focus/measurement preservation and actual menu-parent cleanup, with its original
assertions intact.

## Validation

The complete candidate graph executable passes with default parallel execution:
**1,959 passed**, one existing ignored timing probe, no failures (29.38 seconds).
Both strict all-target Clippy gates, both formatting gates and whitespace checks pass.

Accepted frozen run:
`target/bevy-020-qualification/runs/11bbeac66118466185553a7608427108/`.
It executes **2,391 passing checks**, including **1,959 editor graph checks**
(141.33 seconds after compilation). Counts are executions across gates, not unique
tests. All **1,081 input hashes** are unchanged; native execution is false.

Shipping passes **2,807 editor unit + 3 architecture tests**, with default parallel
execution, the same ten existing ignores and no failures (6.85 seconds for units).
Workspace GPU-feature all-target checking passes. Post-documentation input drift
is zero. Some Cargo invocations report incremental-cache hard-link fallback to
copying; there are no code lint failures.

## Remaining work

Qualify function wire updates and the complete function graph registrar, then
remaining plugin/application handlers and full editor workflows. These tests are
headless menu/event integration and positioned-graph history, not OS picking,
GPU wire rendering or full native editor acceptance. Native acceptance remains
required before changing shipping dependencies. No visual reference is approved.
