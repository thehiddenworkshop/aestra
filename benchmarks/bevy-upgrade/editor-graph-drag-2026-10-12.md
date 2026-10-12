# B20-2 slice 10 — Node drag transactions and geometry dependencies

The isolated Bevy 0.20 graph now imports the production node drag observers,
placement memory, geometry registry, snapping controller and wire-insertion
controller. Shipping remains Bevy 0.19.1. This qualifies the shared transaction
path, not the complete host editor or native graph workflow.

## Implementation

- `feathers/node_graph/drag.rs` contains the existing begin/move/end observers.
  The host still supplies the original embedded-control routing filter. Narrow
  pointer aliases select actual upstream 0.19 or 0.20 events without substituting
  graph behavior. Modifier precedence and delta scaling are preserved.
- `feathers/node_graph/memory.rs` contains the real placement/view memory,
  including authored bases, temporary offsets, pins and revision tokens.
- `editor_identity.rs` contains the existing document, asset, WESL-source and
  editor-view identities. Document/docking/source modules reexport the same
  types; serialized representations and ID generation are unchanged.
- `geometry/model.rs` contains the real stable observation registry, snapshot
  ownership, generation checks and change coalescing. `geometry.rs` still
  registers the host UI collector and framing schedule. The collector is not
  replaced with a fixture implementation.
- `drag_context.rs` separates captured geometry and snapping from toolbar
  composition. Wire insertion imports the same real geometry and memory types;
  its bounded spacing adapter is also compiled and tested by the candidate.
- The candidate adds the local `aestra-authoring` package and a direct serde
  dependency for those production types. No external package version changes;
  root manifests and lockfile remain unchanged.
- The source-frozen runner includes the new identity module and its host
  reexport modules, in addition to the complete graph subtree. No input-hash
  exemption or weaker lint/test policy is introduced.

## Shared regression coverage

Eleven new tests execute real pointer events and ECS observers on both versions:

- Single-node placement at three scale factors and three zooms, deferred
  history events, release guards, cursor cleanup and duplicate release.
- Selected groups as one transaction, with same-document nodes in another
  viewport excluded.
- Duplicate/upstream/downstream modifier precedence captured at start;
  duplication restores the original node and synthetic outputs retain normal
  movement. No-op drags do not publish edits.
- Nested controls, Space and secondary-button routing; retargeted release;
  non-finite movement does not corrupt placement memory.
- Alignment from actual stable registry snapshots, invalidated targets,
  accumulated raw grid motion and Alt bypass.
- Authored undo-before positions for temporarily displaced nodes, without
  touching unrelated document placement.
- Drag-on-wire candidate/probe/drop routing with real ports and spacing plans;
  changed project/view identity rejects the insertion at release.

Five existing registry tests and three existing spacing tests also run in the
candidate. Existing shipping layout, snapping, insertion and host tests remain
enabled. Tests feed explicit measured observations through the real registry;
this is a controlled input boundary, not proof of native UI collection.

## Remaining scope

The host geometry collector still depends on document-manager authority,
focused docking views, socket/preview UI measurements and full panel composition.
Those paths remain shipping-only. Semantic action dispatch, actual rewiring,
document undo/redo and complete native editor workflows are not qualified by
observing a presentation/drop event here.

Injected delta tests deliberately preserve the shipping scale contract. They
do not prove OS pointer/DPI units, camera viewport offsets, native hit-testing or
pointer capture. Unlike marquee ownership, legacy node dragging does not yet
capture pointer ID/render target or implement a complete cancel/teardown path.
These limitations remain explicit acceptance work, not silently claimed parity.

Next: qualify the host geometry collection/document-view authority boundary,
then action dispatch and end-to-end native graph editing before switching the
root engine graph. Optional replacement widgets remain out of scope.

## Validation

Accepted source-frozen headless run:
`target/bevy-020-qualification/runs/b88cd5a1af664e65a76e4863a4d22a08/`.
Pinned MSVC Rust 1.98.1, locked/offline: **474 executed checks pass**, including
**66 graph checks** (one existing manual timing ignore). All **602 input hashes**
remain unchanged. The graph gate runs in **0.39 seconds** after compilation.
Repeated dependency checks are executions, not unique tests. No native tests
were requested or run for this slice.

Shipping validation: **1,266 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace/candidate all-target Clippy,
all-target workspace checking with `aestra-bench/gpu`, both formatting checks
and whitespace validation pass. Incremental hard-link warnings fall back to
copying. Shipping lock SHA256 remains
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
