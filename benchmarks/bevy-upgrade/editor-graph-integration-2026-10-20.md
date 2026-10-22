# Generic graph registration and scheduling — Bevy 0.20 qualification

Date: 2026-10-20. B20-2 slice 83. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

- `feathers/node_graph/integration.rs` owns the exact production resource,
  observer and Update/PostUpdate registration, including the shared embedded-control
  filter and GraphWidgetSync set. The shipping plugin calls this registrar.
- The original ordered chain is preserved, including geometry collection after
  PostLayout, frame preparation after collection, deferred command boundaries,
  and grid uniforms after Layout. A source audit confirms the existing registration
  pipeline and original generic-root test bodies are unchanged.
- Shader embedding remains in the native root to preserve the embedded asset
  namespace. Material registration is shared; shader execution is not claimed.
- The candidate now imports the complete production geometry, framing, overlay
  and drag-assistance modules, replacing its partial geometry/drag-assistance
  facades. The obsolete `editor_graph_geometry.rs` fixture wrapper is removed.
- Explicit child paths let the same native module trees compile in the isolated
  candidate. Drag-assistance tests construct native pointer events through the
  existing 0.19/0.20 boundary. All existing assertions are retained.
- The catalog-scope regression uses an explicit empty catalog rather than the
  shipping-only Default implementation that scans the bundled sample project.

## Integration finding and fix

The real composition exposed a bubbling bug: rejecting an embedded socket/control
at the original target did not stop a later global observer invocation at its
parent node from starting a drag. Node press, node drag-start and marquee
classification now use `original_event_target()` on both engines, preserving
the control's ancestry through propagation. This does not stop socket-specific
observers or replace native pointer delivery.

## Shared regressions

Ten new cases exercise the complete registrar:

- Resources and actual material plugins initialize; the deferred grid attachment,
  canvas camera transform and post-layout uniforms agree in the same frame.
- Host rebuild commands ordered before GraphWidgetSync flush before the graph
  consumers and a same-frame host readback.
- Pin and collapse Activate events route once through their real native controls.
- Remembered cameras restore before seeding, while split-view cameras stay separate.
- Embedded socket drags cannot move a node; a valid node drag commits once.
- Active-drag teardown and rebuild do not retain node drag state or guide entities.
- A frame command is scoped to its own view and remains pending without measurements.
- Real Bevy UI layout and stable geometry settle the existing measured-frame contract.
- The complete schedule stages measured framing in PostUpdate and applies it to
  canvas and grid together on the next Update, without generating semantic edits.
- All six embedded control classes, and children of each, reject node press/drag
  and marquee selection even while native pointer events bubble.

The candidate additionally executes 24 existing geometry/framing/overlay/snapping/
drag-guide cases previously outside its graph gate. Tests use actual Feathers,
SVG assets, material assets and production consumers; no substitute graph model,
layout solver, command consumer or manual per-test registration chain is introduced.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/58265dd208e84a6bba9a8805de9ef347/`.
It executes **2,185 passing checks**, including **1,753 editor graph checks**
(85.59 seconds after compilation), with the existing one ignored editor timing
probe. Counts are executions across gates, not unique tests. All **1,011 input
hashes** are unchanged; post-documentation source drift is zero. Native execution
is false.

The complete candidate graph suite also passes with default parallel execution:
1,753 passed, 1 existing ignored probe, no failures (6.87 seconds after compilation).

Shipping passes **2,688 editor unit tests + 3 architecture tests** with default
parallel execution, the same ten existing ignores and no failures. Both strict
all-target Clippy gates, workspace GPU-feature all-target checking, both formatting
gates and whitespace validation pass. The root manifests, lockfile, toolchain and
CI workflow are unchanged.

## Remaining work

This qualifies the generic graph registrar and headless schedule integration,
not the complete MaterialGraphPlugin, application command handlers, native
picking/shader rendering, zoom/DPI visual acceptance or full editor workflows.
Continue with material graph host/plugin composition and application handlers,
then native/workspace workflow acceptance before switching shipping dependencies.
No visual references are approved and no native GPU tests are claimed.
