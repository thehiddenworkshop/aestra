# Semantic drag-on-wire insertion — Bevy 0.20

Date: 2026-10-21. B20-2 slice 99. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

The candidate now compiles the complete production `material_graph/insertion.rs`
adapter. Explicit canonical imports and child-module paths let both engine roots
use that same source. The function-wire helper is compiled through the actual
adapter instead of a standalone qualification module. The candidate's canonical
material-graph facade exposes the actual adapter.

Source comparison preserves all six production registration, command, planning,
preparation, probe and drop bodies. All five original test bodies and their 30
assertions are unchanged; explicit imports and a test-only fixture visibility
change allow their reuse. The candidate no longer needs the widget-insertion
dead-code allowance: production semantic observers consume its event types and
refresh system. Native root imports used only by tests are test-gated or removed
where obsolete. There are no production behavior, dependency, toolchain, CI,
VFX-budget, history-policy or visual-reference changes.

## Shared regressions

Five new shared cases extend the original five semantic/placement regressions.
They use actual saved program/function sources, catalog/session publication, the
production registrar and observers, viewport identity markers, placement memory
and global undo/redo. There are no replacement semantic or history callbacks.

- Registered probes validate unfocused program/function sources without activating
  them. The registered drop routes to that source, changes semantics and placement,
  supports compound undo/redo and preserves the active effect.
- Wrong project, view, asset identity and protected-spacing failures revoke valid
  feedback without changing the source or placement. Leaving the candidate restores
  the normal move status.
- A retired or retargeted view after valid feedback is revalidated at drop; the
  source is unchanged and the original placement and transient offset are restored.
- A missing optional function editor rejects the drop after bootstrap spacing has
  been preserved, then rolls that spacing back without leaving seeded placement.
  The fixture restores that resource before exercising global history, whose normal
  routing system requires it.
- Default-literal inputs may be replaced. Duplicate compatible choices are counted
  once, but the existing sixteen-choice raw-input bound is still enforced without
  mutating the source document.

The original cases retain ambiguous, stale, connected, incompatible, existing-branch
and cyclic rejection, plus program/function compound undo/redo with stored and
bootstrap neighbors and restoration after a stale drop.

## Validation

The default-parallel candidate graph executable passes **2,013 tests**, with the
same one existing ignored timing probe and no failures (28.38 seconds). All ten
insertion cases pass directly. Strict candidate all-target Clippy passes.

Accepted frozen run:
`target/bevy-020-qualification/runs/71c3b1b02a574d43bf5090bec75e2ea9/`.
It executes **2,445 passing checks**, including **2,013 editor graph checks**
(138.85 seconds for the serial graph gate). Counts are executions across gates,
not unique tests. All **1,092 input hashes** are unchanged; native execution is
false. Post-documentation input drift is zero.

Shipping passes **2,836 editor unit + 3 architecture tests**, with default parallel
execution, the same ten existing ignores and no failures (7.79 seconds for units).
Both strict all-target Clippy gates, both formatting gates, whitespace checks and
workspace GPU-feature all-target checking pass. Some Cargo invocations report the
existing incremental-cache hard-link fallback to copying; there are no code lint
failures. Root manifests, shipping lockfile, toolchain and CI are unchanged.

## Remaining work

Complete material-plugin registration and remaining application handlers, then full
native editor workflow acceptance. This slice qualifies the semantic adapter's
actual ECS registration, event consumption, placement and history. The widget's
measured hit-testing and drag transport remain covered by their existing gates;
these new adapter fixtures supply candidates and viewport identity metadata rather
than claiming rendered picking or a full OS pointer-to-editor workflow. GPU/native
rendering is not executed or newly approved. Shipping stays on 0.19.1.
