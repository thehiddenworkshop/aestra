# B20-2 slice 9 — Marquee and shared node ownership

The isolated Bevy 0.20 graph now imports the actual production marquee
observers, node presentation state, node-target resolver and fresh-press guard.
Shipping remains Bevy 0.19.1; root manifests and lockfile are unchanged.
This is a bounded interaction slice, not complete node dragging or editor acceptance.

## Implementation

- `feathers/node_graph/node.rs` owns the existing node component, presentation
  events, drag release-click guard and ancestor resolver. Existing material and
  function panels retain their public names through reexports. Drag transaction,
  snapping, insertion and undo call sites still use this same production state.
- `feathers/node_graph/marquee.rs` owns the real begin/move/end observers and
  selection event. The generic control filter is a host routing policy, not a
  replacement node or document type. Shipping instantiates the unchanged filter
  for sockets, collapse actions, numeric fields, checkboxes, text and buttons.
- Narrow pointer boundaries select real generic 0.19 events or flattened 0.20
  events. They expose the original pointer ID/location and do not translate
  behavior, simulate picking or manufacture production events.
- Marquee coordinates now convert logical pointer pixels and physical UI
  transforms to UI units exactly once before undoing graph pan/zoom. The final
  release position is used even without a preceding move event.
- A gesture retains its pointer ID, render target and viewport. Foreign pointer,
  window and button events cannot steal or finish it. Escape, pointer cancel,
  removed viewport and removed overlay clear it without publishing selection.
  Non-finite input, singular transforms and invalid node measurements are rejected.

## Required regression gate

The existing required, source-frozen `editor-graph` gate now executes **47 checks**,
with its one existing manual timing probe ignored. Fourteen new shared tests use
real pointer events and ECS observers on both engine versions:

- All nine 1x/1.5x/2x UI/display-scale combinations at overview, normal and close
  zoom; actual overlay dimensions and non-pickable policy.
- Final release without movement, reverse rectangles, inclusive edge hits,
  same-view ancestry and captured ownership when the release target changes.
- Replace/Add/Toggle precedence captured at gesture start; Space and secondary
  buttons; nested node/control routing; foreign pointer/window/button rejection.
- Escape, pointer cancellation, viewport/overlay teardown and invalid geometry.
- Fresh primary press reset and single-use suppression of a post-drag click.

The fixture supplies a test control filter to exercise the generic routing
contract. A separate shipping regression checks all six actual filter branches
and their descendants. The full graph subtree, both pointer boundaries and
the shared tests are hashed by the existing runner; no runner exemption is added.

## Remaining scope

Full node drag/modified-drag transactions still depend on measured geometry,
document/view authority, snapping and insertion controllers. Those production
paths remain covered by shipping tests but are not yet imported into the
candidate. Their dependencies are not replaced with fixture stubs. Application
action dispatch and full native editor workflows also remain open.

The headless gate dispatches actual upstream events; it does not prove native
hit-testing, camera viewport offsets, OS pointer capture or graph visual parity.
Complete those workflow checks before switching the root graph to 0.20.

## Validation

Accepted headless source-frozen run:
`target/bevy-020-qualification/runs/2267e1a86d5e4c7dbc47c3e1538d4653/`.
Pinned MSVC Rust 1.98.1, locked/offline: **455 executed checks pass**, including
**47 graph checks** (one existing timing ignore). All **592 input hashes** remain
unchanged. The graph gate runs in **0.46 seconds** after compilation. Dependency
checks intentionally repeat across gates; these are executions, not unique tests.

Shipping validation: **1,255 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace/candidate all-target Clippy,
all-target workspace checking with `aestra-bench/gpu`, both formatting checks
and whitespace validation pass. Existing incremental hard-link warnings fall
back to copying. Shipping lock SHA256 remains
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
No new renderer-native or full-editor visual evidence is claimed.
