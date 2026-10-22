# Material graph host lifecycle and wire scheduling — Bevy 0.20 qualification

Date: 2026-10-20. B20-2 slice 84. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

- `material_graph/host_integration.rs` shares the actual host setup for material
  gesture, palette, scoped selection and function-menu resources; connection
  drag/click observers; document-transient lifecycle; and post-layout wire updates.
- The native EditorMaterialGraphPlugin delegates this part of its registration.
  Its remaining authoring, preview, layout-persistence and panel setup is unchanged.
- The unchanged EditorSet identity is shared from `shell/sets.rs`; both engines
  use the actual GraphWidgetSync identity from the generic graph registrar.
- Wire material attachment, ApplyDeferred and update retain their original chain
  after both transform propagation and UI PostLayout.
- The existing 22 connection regressions now install this production registrar,
  rather than their old manually selected observers/Update wire chain. An initial
  update primes the real document-identity observer before test pointer input.
- Frozen-input hashing includes the host module, tests and editor-stage file.

Source audit confirms both extracted cleanup bodies, the original material-root
tests, every existing connection test/assertion, and EditorSet's variants are
unchanged. The existing optional-resource behavior of explicit cleanup is retained.

## Ordering finding and fix

The old cleanup system was constrained only before UiRebuild. That did not require
it to observe a target change made during the same frame's MainUpdate; rebuild
could therefore see stale transient graph state. Cleanup now runs after
EditorSet::MainUpdate and before EditorSet::UiRebuild. GraphWidgetSync remains
after UiRebuild.

The regression first observes a stable document, leaves live controls open, then
changes the target inside the actual MainUpdate set and checks cleared state in
UiRebuild and subsequent graph synchronization. This is an ordering requirement,
not a replacement of application handlers or a new document identity model.

## Shared regressions

Ten new tests cover:

- Initial observation clears transient controls, not the authored effect/history.
- Stable identity preserves live controls and idle palette/selection/menu ticks.
- Preview/status/playback/UI-revision changes do not close live graph controls.
- Program/function identities and project-root changes clear both selection scopes.
- Replacing an effect clears transients even with the same material target.
- Explicit cleanup is safe when resources are absent or partially removed.
- Explicit cleanup preserves authored placement and session chronology.
- Same-frame target changes are observed before rebuild and GraphWidgetSync.
- Queued project cleanup flushes before rebuild without creating a semantic edit.
- Wire assets attach after both transform and PostLayout stages, observe values
  written by those stages, flush before final readback, and are reused next frame.

The existing connection tests continue exercising real native pointer events,
semantic planner/publication/undo, snap validation, scoped view projection,
selection/detachment styling, material reuse and socket-offset caching through
the shared production registration.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/9705ad38617a4fe1925a04270c9c5eb1/`.
It executes **2,195 passing checks**, including **1,763 editor graph checks**
(61.48 seconds after compilation), with the existing ignored editor timing probe.
Counts are executions across gates, not unique tests. All **1,014 source input
hashes** are unchanged. Native execution is false.

The complete candidate graph suite also passes with default parallel execution:
1,763 passed, 1 existing ignored probe, no failures (12.59 seconds after compilation).
Shipping passes **2,698 editor unit tests + 3 architecture tests**, with default
parallel execution, the same ten existing ignores and no failures. Both strict
all-target Clippy gates, both formatting gates and whitespace validation pass.
Root manifests, lockfile, toolchain and CI workflow are unchanged.
Workspace GPU-feature all-target checking also passes. Post-documentation source
drift is zero.

## Remaining work

This qualifies the material host lifecycle, connection registrar and wire schedule,
not the entire EditorMaterialGraphPlugin or application/editor workflow. Continue
with project-bound layout persistence and preview-worker orchestration, then
remaining panel/application integration. Real native picking, shaders, rendered
zoom/DPI and full project/document workflows still need native acceptance before
switching shipping dependencies. No visual references are approved.
