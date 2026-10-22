# Complete graph workspace host and function signatures — Bevy 0.20

Date: 2026-10-20. B20-2 slice 91. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

The entire production `spawn_material_graph_workspace` host now lives in
`material_graph/workspace_host.rs` and is compiled by both engines. The shipping
entry point remains a canonical re-export with the same signature. Its actual
material/effect canvas, function graph canvas and function-inspection scroll/header
branches are composed together; none is substituted in the candidate.

Function signature construction and formatting move to
`material_function_editor/signature.rs`. Its actual Field/Action/Port identities
are shared through `signature_model.rs`, retaining the native observer consumers.
Custom-WESL functions still create no editable signature controls. Required input
defaults, scalar scrubbing, type choices and add/remove actions are preserved.

Source audits preserve all four signature construction/formatting bodies, all
eight native signature handler/helper bodies, original signature tests and the
remaining material-root tests. The host and inspection bodies are unchanged except
for the explicit-target routing correction below. No manifests, lockfiles,
toolchain or CI settings change. Frozen hashing includes all five new source/test
files; both production parent modules were already hashed.

## Explicit-target regression and fix

Two new real-host cases failed before the correction: an unfocused code-function
tab and an unavailable/wrong-project tab displayed the active graph function's
signature controls and graph description. Branch selection already resolved the
explicit target, but fallback text and controls called `session.graph_function`.

Both fallback reads now call `graph_function_for(target, catalog)`, and the
inspection formatter receives that same explicit target. An unavailable source
shows its own error and never inherits another function's controls/source. A code
tab stays read-only and shows its own signature/source even before focus. With no
override, the tool continues following the active session target. This fixes
composition routing, not semantic editing or document selection policy.

## Shared regressions

Five complete-host cases cover:

- Unfocused code inspection against a different active editable function, including
  read-only fields, scroll/drop/canvas ownership and preservation of active target,
  effect and semantic edit serial.
- Missing and wrong-project targets display their actual resolution error with no
  stale active-function controls or canvas.
- Function and material tabs route to their actual canvases under a code-function
  active target without changing that target.
- The unscoped tool follows code/graph target switches and retains status and source.
- English/French empty states and localized Back action composition.

Four signature cases cover real field owners and input/output IDs, required versus
saved typed defaults, no texture-default editor, the exact scalar scrub value and
bounds/step, typed add/remove/type action payloads for every port, real displayed
saved default text and fully read-only custom-WESL signatures.

These exercise actual Feathers controls and production action components. Semantic
observer dispatch remains a separate qualification boundary; no edit callbacks are
replaced or invented for these tests.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/6f9f1fe7bd2d499fa6ddf50f87edbb01/`.
It executes **2,290 passing checks**, including **1,858 editor graph checks**
(118.80 seconds after compilation), with the existing ignored editor timing probe.
Counts are executions across gates, not unique tests. All **1,055 input hashes**
are unchanged. Native execution is false.

The complete candidate graph suite also passes with default parallel execution:
**1,858 passed**, one existing ignored timing probe, no failures (16.74 seconds).
Both strict all-target Clippy gates, both formatting gates and whitespace checks
pass. Root manifests, lockfile, toolchain and CI workflow are unchanged.

Shipping passes **2,767 editor unit + 3 architecture tests**, with default parallel
execution, the same ten existing ignores and no failures. Workspace GPU-feature
all-target checking passes. Post-documentation input drift is zero.

## Remaining work

Qualify function signature and graph input/action registration, then the remaining
material/plugin/application handlers and complete editor workflows. This slice is
complete workspace composition, not native pointer dispatch, GPU wire rendering,
DPI or full editor acceptance. Native acceptance is still required before changing
shipping dependencies. No visual reference is approved.
