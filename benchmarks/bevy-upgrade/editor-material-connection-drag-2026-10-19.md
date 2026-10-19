# Material connection drag/snap/finish — Bevy 0.20 qualification

Date: 2026-10-19. B20-2 slice 76. Shipping remains Bevy 0.19.1;
the isolated candidate remains 0.20.0, with Rust 1.98.1. No dependency,
authored-format, rendering style or interaction policy changes.

## Implementation

- `apps/aestra-editor/src/material_graph/connection_drag.rs` shares twelve actual
  consumers/helpers: begin/update/finish drag, endpoint mapping, socket click
  consumption, connection publication/current-source lookup, wire material
  attachment/update, owning layer lookup and uniform/control-point updates.
- `connection_state.rs` shares the original gesture and ghost records.
  `feathers/node_graph/wire_layer.rs` shares the original owning-view record.
  Parent visibility is the only record change.
- Native event adapters `drag_019.rs` and `drag_020.rs` expose original drag
  types and real pointer locations, without synthetic event translation.
  Socket click delivery uses the already qualified click boundary.
- Snapping executes the actual wire update, socket cache and MaterialToolPlanner
  against the retained editing target's document. There is no candidate-only
  planner, snap algorithm, material or history implementation.
- The original `material_graph::update_wire_material` path remains re-exported
  for function graphs and existing insertion regressions.

Source audit: all twelve extracted bodies are unchanged except native event type/
location spelling and formatting. All **77 remaining top-level graph functions**,
native material graph plugin registration and original root graph tests remain
unchanged. The snap radius remains 38; existing detach, ghost cleanup, selection,
insertion-color and idle-write policies are preserved.

## Shared regressions

**22** shared cases exercise actual engine events, ECS/resources, UI material
assets, catalog/session, semantic planner/publication and history:

- Start identifies the containing viewport and creates a real non-pickable ghost
  with its own GraphWireMaterial/parent. Secondary/Space gestures and missing
  containing views retain their gates; absence of a wire layer retains the
  original non-rendering gesture behavior.
- Motion keeps inverse-scale conversion and ignores non-socket targets.
  Primary socket clicks consume; secondary clicks bubble.
- Forward and reverse drags snap and publish the same semantic connection.
  Real ghost uniforms retain output-to-input direction and snap color.
  Undo/redo restores the original/replacement program; saved material stays intact.
- The original radius boundary, nearest-valid choice with a closer invalid
  socket, foreign programs, missing expressions and cycles.
- Unsnapped ends open typed source/input palettes in the origin view. Changed
  editing targets between snap and completion reject publication, and a removed
  origin view prevents publication.
- Unchanged connections remain no-ops without history; secondary finish does not
  terminate the active primary gesture.
- Actual passive wire assets attach once and retain handle identity, selection
  highlight and input-detach suppression. Cached socket offsets do not mix later
  UI transforms into their endpoints.
- Two views of one program keep independent passive projections, ghost ownership,
  cursor projection and unsnapped palette coordinates.
- The retained uniform-update API is exercised with real AssetPlugin tracking:
  changed uniforms emit Modified, while identical idle uniforms emit none.

The tests supply computed UI sizes/transforms and run the actual attach/deferred/
update systems in an ordered headless test schedule. This is not the full native
PostLayout/transform/render schedule.

## Preserved boundaries

This slice does not add pointer-ID ownership/cancel cleanup, epoch-based snap
freshness checks, cross-view socket filtering or scoped ghost retirement.
In particular, the original finish consumer still iterates all ghosts, and the
original snap scan retains its program/interactive/planner filters. These are
compatibility boundaries, not newly hardened guarantees. The earlier documented
menu bubbling limitation remains separate.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/596b0560e487487f82d4791d5cb073ef/`.
It executes **2,019 passing checks**, including **1,597 editor graph/session/
history/consumer checks**, with one existing ignored timing probe. The editor
suite takes 65.08 seconds after compilation. Counts are executions across gates,
not unique tests. All 22 shared connection cases pass in the full gate.

All **973 input hashes** are unchanged. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false` and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
The final-source shipping rerun passes **2,556 editor unit + 3 architecture tests**,
with the same ten existing ignores. Both engines pass strict all-target Clippy.
Workspace all-target checking with `aestra-bench/gpu`, both formatting gates and
Git whitespace validation pass. Root manifest/toolchain/lock diffs remain empty;
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings remain non-fatal.

## Limits and next step

This is real-event/semantic/history and main-world UI-asset evidence, not native
picking, rendered ghost/wire shaders, DPI screenshots, GPU extraction or complete
material graph/application acceptance. No new visual reference is approved.
Root dependency/toolchain/lock inputs remain unchanged; B20-2 remains open.

Next: remaining material graph node selection/function-call opening and modified
drag consumers, then toolbar/numeric controls, full graph composition/plugin and
complete application workflow acceptance.
