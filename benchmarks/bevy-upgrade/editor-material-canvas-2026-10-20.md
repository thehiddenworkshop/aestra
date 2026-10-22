# Material canvas, nodes, sockets and wires — Bevy 0.20 qualification

Date: 2026-10-20. B20-2 slice 80. Shipping remains Bevy 0.19.1;
the isolated candidate remains 0.20.0, with Rust 1.98.1. No dependency,
authored-format, widget replacement or rendering-style changes.

## Implementation

- `feathers/node_graph/composition.rs` shares the actual graph viewport,
  canvas/wire layers, node chrome, pin/disclosure/preview controls, preview
  surfaces and typed socket composition. Version-specific adapters select
  0.19's Button or 0.20's ui_widgets::Button for graph hit targets.
- `material_graph/composition.rs` shares the actual material canvas assembly,
  semantic expression nodes, synthetic outputs, inline/default controls and
  wires. The shipping workspace calls this builder; the candidate does not
  reproduce the canvas with its own UI implementation.
- `material_graph/ports.rs` shares existing typed labels, descriptions,
  evaluation-domain names and socket/wire colors.
- `material_graph/preview_surface.rs` shares the original preview request
  record and immediate mounting of a completed cached image. Raster evaluation,
  worker limits, cancellation and scheduling remain in the original worker
  module, outside this composition qualification.
- The native workspace keeps function/code-inspector branches, asset-drop
  registration and palette/context-menu assembly. Its material canvas block
  is factored into the shared builder; actual project/document/view geometry
  identity and scoped viewport metadata are mounted there.
- Frozen input inventory includes all new production modules and shared tests.
  Generic node-graph files already use recursive input hashing.

Source audit: all 25 extracted composition/port/cache-mount function bodies
are unchanged apart from the native Button alias and preview-module path.
Formatting wraps one signature with a trailing comma. The only changed remaining
material root function is workspace assembly, delegating its original canvas
block to the shared builder. The other 40 remaining material root functions,
all 15 remaining generic graph functions, original root tests and both native
graph plugin registrations are unchanged. Relocated records expand parent-only
visibility as required by their existing consumers.

## Shared regressions

16 shared cases mount real Feathers scenes and SVG assets, checking:

- Clipped viewport geometry and actual grid/wire/canvas child order, shared
  transforms and passive wire layers.
- Project/document/view identity, complete semantic geometry inventory and
  retained editing targets.
- Unseeded auto-framing, document-camera fallback and per-view camera priority.
- Bootstrap positions, semantic node scope and synthetic output exclusions from
  editable-node/modifier-drag markers.
- View-local selection, document-shared pin state and passive semantic wires
  with original endpoint identities/colors and insertion metadata.
- Typed socket metadata, real node anchors, 20px hit targets and passive 10px dots.
- Visible-only preview requests, real preview controls and bounded passive
  surfaces; completed cached images mounted in the same command batch, with
  project/program/instance/target isolation.
- Scalar/vector/color/boolean constant controls, snapshots, component values,
  numeric limits, toggle state and inline controls adjacent to the port label.
- Disabled/invalid warning styling and empty projections retaining outputs.

These are headless production composition tests using the same actual Feathers
fixture as the prior widget gates, not screenshots or approved visual references.
Looped value/cache/scope cases count as one test each.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/c021f74290eb4772ac3015366b3d9fe5/`.
It executes **2,105 passing checks**, including **1,683 editor checks**, with
one existing ignored editor timing probe. The editor suite takes 74.89 seconds
after compilation. Counts are executions across gates, not unique tests.
All 16 new shared cases pass in the full candidate gate.

All **1,003 input hashes** are unchanged. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false` and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
The shipping suite passes **2,642 editor unit + 3 architecture tests**, with the
same ten existing ignores; the new shared cases pass on both engines. Shipping
compilation takes 6m 27s and unit execution takes 32.82 seconds.

Both strict all-target Clippy gates, workspace all-target checking with
`aestra-bench/gpu`, both formatting gates and whitespace checks pass. Root
manifest/toolchain/lock diffs remain empty; post-documentation input drift is
zero. Existing Windows incremental-cache hard-link fallback warnings remain
non-fatal. No native GPU/render acceptance is claimed.

## Limits and next step

This slice qualifies material canvas composition, not full material workspace or
application acceptance. Generic pin/collapse action consumers, socket hover
visual synchronization, grid/wire material attachment, complete plugin scheduling,
native picking and rendered zoom/DPI remain separate gates. The actual preview
raster worker is not installed by these tests; immediate cache mounting does not
prove asynchronous raster readiness.

Function/code-inspector branches, native asset-drop and popup mounting are not
claimed by this canvas fixture. Existing geometry/navigation/action/history gates
continue to run, but their integration through the entire native graph plugin
is still required. No visual reference is approved; B20-2 stays open.

Next: generic node pin/collapse and socket visual consumers, then complete graph
plugin integration/scheduling and editor workflows. Preserve current geometry
and interaction behavior; upstream widget adoption remains a later milestone.
