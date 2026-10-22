# Viewport camera, grid and selection presentation — Bevy 0.20 qualification

Date: 2026-10-20. B20-2 slice 82. Shipping remains Bevy 0.19.1;
the isolated candidate remains 0.20.0, with Rust 1.98.1. No dependency,
authored-format, geometry, shader or rendering-style changes.

## Implementation

- `feathers/node_graph/viewport_presentation.rs` shares the six actual production
  functions for camera remembrance/restoration, grid asset attachment/uniform
  synchronization and selection-outline sizing/synchronization.
- The shipping graph plugin retains its original registration and scheduling.
  Both engines compile the same functions without a version-specific behavior
  adapter or candidate-only camera/grid/outline implementation.
- Existing GraphViewportMemory, viewport/grid/node records, GraphGridMaterial,
  Assets and MaterialNode types are used directly. The real composed hierarchy,
  Feathers scenes and SVG loader are installed in the composition regressions.
- Candidate composition no longer needs its previous dead-code allowance for
  unqualified grid/material attachment.
- Recursive frozen-input hashing includes both new graph files.

Source audit: all six extracted function bodies are unchanged apart from
visibility/formatting. The generic root now contains no remaining top-level
presentation functions; its native plugin registration and original root tests
are unchanged. Theme visibility is retained for existing child consumers.

## Shared regressions

16 shared cases cover:

- Added viewports do not overwrite a remembered camera before restoration.
- Uncached views preserve a seed or the original automatic-frame request.
- Split-view cameras remain keyed separately from document node placement.
- Restoration remains Added-only rather than resetting an already live camera.
- Later camera changes are remembered, while idle memory is not rewritten.
- Rebuilt viewports restore the last camera without accumulating offsets.
- Real grid assets are distinct per grid, allocated once, and reused thereafter.
- Existing handles and spacing survive attachment; unrelated nodes are skipped.
- Post-layout uniforms follow each grid's own camera and preserve grid spacing.
- Missing owners and removed assets are skipped without automatic replacements.
- Restored cameras drive the actual canvas transform and grid in the same frame.
- Outline sizing preserves the existing minimum visible edge and small-zoom clamp.
- Selection resolves the nearest viewport ancestor, including nested views sharing
  a document key.
- Selection changes do not mutate authored node/socket geometry.
- Orphan nodes and absent outlines are safe.
- Idle outline updates do not rewrite the component.

Composed cases run actual headless Feathers/SVG and UI schedules. Isolated
Added-only and orphan cases exercise the same production systems over their
actual component records. The existing navigation transform function is installed,
not replaced by a test transform implementation.

## Preserved policy

Camera restoration still applies only when a viewport is added; it does not
continuously pull external memory into a live view. Duplicate camera keys retain
their existing memory semantics. Grid uniforms retain their existing per-frame
writes; this slice introduces no idle-grid optimization. Existing grid handles
with removed assets are not silently replaced.

Selection keeps its existing color/width policy: deselection hides the outline
without resetting its width or changing node/socket layout. The existing minimum
zoom guard and selected-border width are unchanged; no new validation or style
policy is added.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/0d82aeb8c3bb46f895e37242eadc1f51/`.
It executes **2,136 passing checks**, including **1,714 editor checks**, with
one existing ignored editor timing probe. The editor suite takes 82.43 seconds
after compilation. Counts are executions across gates, not unique tests.
All 16 new shared cases pass in the full candidate gate.

All **1,009 input hashes** are unchanged. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false` and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
The shipping suite passes **2,673 editor unit + 3 architecture tests**, with the
same ten existing ignores. All 16 new shared cases pass on both engines.
Shipping compilation takes 6m 08s and unit execution takes 32.56 seconds.

Both strict all-target Clippy gates, workspace all-target checking with
`aestra-bench/gpu`, both formatting gates and whitespace checks pass. Root
manifest/toolchain/lock diffs remain empty; post-documentation input drift is
zero. Existing Windows incremental-cache hard-link fallback warnings remain
non-fatal. No native GPU/render acceptance is claimed.

## Limits and next step

This is headless production camera/asset/uniform/component qualification, not
native GPU shader compilation, bind-group uploads, rendered zoom/DPI or screenshot
acceptance. Grid tests use actual registered material assets, not the complete
UiMaterial rendering plugin. The tested schedule matches the relevant production
ordering, but does not install all graph geometry/history/overlay/pointer systems
or qualify the complete graph plugin.

Next: full graph plugin integration/scheduling, then application handlers and
complete editor workflows. Keep B20-2 open and shipping on 0.19.1 until all
compatibility gates pass. No visual reference is approved.
