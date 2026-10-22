# Material presentation edits and arrangement installation — Bevy 0.20

Date: 2026-10-21. B20-2 slice 97. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

The original node/pin presentation observers and atomic arrangement-result installer
move to `material_graph/presentation/edits.rs`. The three function bodies are
unchanged. They retain actual snapshot validation, graph ownership, context routing,
placement updates, transient-offset cleanup, redo branching and global chronology.
No semantic editing, publication or history callback is substituted.

A shared presentation registrar registers node, batch and pin observers in the
original order. The shipping material plugin invokes it at exactly the original
point, after GraphClipboard initialization and before the remaining node observers.
Source comparison preserves the complete plugin build implementation after
expanding this subregistrar. The asynchronous layout solver and other plugin
registrars are not part of this slice.

All 16 existing presentation-history cases now also run on the candidate. They use
explicit production imports, and their fixture calls the real three-observer
registrar. Two pressed-button fixtures use the existing engine-specific test input
boundary (Interaction on 0.19, Pressed on 0.20), preserving intentional pending
activation. The other 15 test bodies are unchanged apart from explicit module paths;
all 65 existing assertions are retained. No deprecation/lint allowance is added.

The shipping presentation entry points remain re-exports. The old batch-observer
convenience re-export is test-only now that production registration directly uses
the actual observer. Shipping root dependencies, lockfile, toolchain, CI, VFX
budgets, validation policy and visual references are unchanged. Frozen hashing
includes the new source/tests and the existing history regression file.

## Shared regressions

Eight new cases run on both engine graphs, using the real registered consumers,
saved project catalogs and global undo/redo:

- Node move, pin and batch/collapse changes form three exact history steps for
  programs and functions. A real nested graph-origin marker routes edits from an
  unfocused graph to its owner. Undo/redo preserves camera state, semantic revisions,
  effect data and saved bytes.
- Unknown graph/node identities and no-op node, pin and batch events cannot replace
  a valid redo branch.
- Missing/extra arrangement keys and NaN/infinite positions reject atomically,
  preserving base positions, temporary offsets, placement revision and chronology.
- A changed material draft invalidates a dispatched arrangement even when manual
  placement has not changed.
- Pinning since dispatch changes the placement token and rejects the old result
  without removing the pin or mutating other placement.
- An explicitly owned, unfocused function arrangement preserves pins/collapse,
  clears temporary displacement, keeps its camera and the material graph intact,
  and round-trips through actual history.
- An exact no-op arrangement does not fork the existing redo branch.
- Switching project roots rejects an old result even if the new project contains
  the same program identity.

The existing cases additionally cover compound semantic/placement history,
preview visibility restoration, inline-constant exclusion, failed creation and
semantic undo, stale source reload/removal, document discard and independent
function/material chronology.

## Validation

The complete candidate graph executable passes with default parallel execution:
**1,991 passed**, one existing ignored timing probe, no failures (21.84 seconds).
Both strict all-target Clippy gates, both formatting gates and whitespace checks pass.

Accepted frozen run:
`target/bevy-020-qualification/runs/9e296daa11cf47edae5ad82def40d235/`.
It executes **2,423 passing checks**, including **1,991 editor graph checks**
(154.78 seconds after compilation). Counts are executions across gates, not unique
tests. All **1,087 input hashes** are unchanged; native execution is false.
Post-documentation input drift is zero.

After the interrupted session, the shipping suite was rerun on the unchanged final
sources: **2,823 editor unit + 3 architecture tests** pass with default parallel
execution, the same ten existing ignores and no failures (8.72 seconds for units).
Workspace GPU-feature all-target checking passes. Some Cargo invocations report
incremental-cache hard-link fallback to copying; there are no code lint failures.

## Remaining work

Qualify the asynchronous arrangement coordinator/layout adapter and semantic
drag-on-wire insertion adapter, then the complete material-plugin registration and
remaining application handlers. These are CPU ECS event/history and result-installation
tests, not asynchronous solver completion, OS picking, rendered UI or full native
editor acceptance. No visual reference is approved. Shipping stays on 0.19.1.
