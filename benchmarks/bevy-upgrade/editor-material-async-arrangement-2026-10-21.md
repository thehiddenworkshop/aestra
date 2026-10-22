# Asynchronous graph arrangement and layout adapter — Bevy 0.20

Date: 2026-10-21. B20-2 slice 98. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

The candidate now compiles the complete production `material_graph/arrange.rs`
coordinator and `layout_adapter.rs`, not only the request definition. Explicit
imports and child-module paths let both engine roots use the same source. The
candidate's canonical arrangement facade exposes the actual coordinator and its
request types, avoiding a second, incompatible request definition.

The original task dispatch, bounded single-job state, preparation, full/partial
solver dispatch, polling, stale-result checks, semantic-key resolution and deferred
installation bodies are unchanged. Source comparison preserves all nine production
coordinator/helper bodies, the existing restored-tab test body and all five adapter
method bodies. The three original adapter tests also run on the candidate.

An obsolete adapter dead-code allowance is removed: the real coordinator exercises
its production entry points. Root imports used only by native tests are test-gated.
A test-only function-canvas setup re-export allows the arrangement integration
fixture to reuse the existing production canvas helper without exposing its private
implementation module. No production API, dependencies, toolchain, CI, VFX budgets,
history policy or visual references change.

## Shared regressions

Eight new cases run on both engine graphs. They compose actual program/function
canvases, obtain stable geometry from the production collector, register the real
coordinator, dispatch real AsyncComputeTaskPool work and poll through its registered
Update system. There are no fabricated geometry snapshots or substitute solver,
history or result-installation callbacks. Successful cases also require the real
installer's UI revision change, so silent rejection cannot count as success.

- Full program and function layouts run from unfocused targets, install finite
  positions, create one global undo step and redo exactly without semantic changes.
- Selection, upstream and downstream requests use the actual partial solver and
  keep every node outside their selected region fixed.
- Full arrangement with a pinned anchor uses partial work while preserving the
  anchor, collapse state and per-view camera.
- A second request cannot replace an in-flight job; after completion the registrar
  accepts another request, and an identical arrangement creates no history entry.
- Wrong project/view/target, missing measurements, empty selection and unknown
  seeds fail before spawning without placement or chronology changes.
- Manual movement or pinning after dispatch rejects the worker result at polling.
- A newer semantic draft is rejected by the actual deferred result installer.
- Retired or remeasured views reject captured geometry. These cases run the real
  PostUpdate collector before polling, testing the registry-readiness contract.

Fixtures deliberately materialize explicit starting placement from a measured
canvas; bootstrap-only geometry is not treated as saved placement. The initial
function fixture failed measurement until its parent was configured with the same
vertical toolbar/canvas layout as `workspace_host.rs`. That fixture correction does
not change production composition or relax the geometry gate. The 30-second task
watchdog prevents a hung test; it is not a performance acceptance threshold.

## Validation

The complete candidate graph executable passes with default parallel execution:
**2,003 passed**, one existing ignored timing probe, no failures (32.64 seconds).
Both strict all-target Clippy gates, both formatting gates and whitespace checks pass.

Accepted frozen run:
`target/bevy-020-qualification/runs/366284df16994a91a04b7cb3b8bff1f0/`.
It executes **2,435 passing checks**, including **2,003 editor graph checks**
(172.58 seconds after compilation). Counts are executions across gates, not unique
tests. All **1,090 input hashes** are unchanged; native execution is false.
Post-documentation input drift is zero.

Shipping passes **2,831 editor unit + 3 architecture tests**, with default parallel
execution, the same ten existing ignores and no failures (7.70 seconds for units).
Workspace GPU-feature all-target checking passes. Some Cargo invocations report
incremental-cache hard-link fallback to copying; there are no code lint failures.

## Remaining work

Qualify semantic drag-on-wire insertion, then complete material-plugin registration
and remaining application handlers. This slice qualifies real CPU task execution,
measured geometry, arrangement and history, not GPU rendering, OS picking or full
native editor acceptance. No visual reference is approved. Shipping stays on 0.19.1.
