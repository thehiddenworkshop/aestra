# Material canvas/node/pin pointer opening — Bevy 0.20 qualification

Date: 2026-10-19. B20-2 slice 75. Shipping remains Bevy 0.19.1; the isolated
candidate remains 0.20.0 with Rust 1.98.1. No dependency, authored format,
interaction policy, playback or visual-reference changes.

## Implementation

- `apps/aestra-editor/src/material_graph/menu_opening.rs` shares four actual
  consumers: canvas Add Node/wire-context opening, connected pin menus, node
  menus and primary canvas/wire selection.
- `click_019.rs` and `click_020.rs` expose native click events and their actual
  pointer position. Target identity, hierarchy traversal and propagation remain
  in the original consumers; there is no synthetic event translation.
- `material_graph/sockets.rs` shares five original socket/wire records and their
  side mapping, measured/explicit/compact socket position collection,
  viewport-local coordinates and actual Bézier distance forwarding.
  The original crate-level viewport_local_position/distance_to_graph_wire paths
  remain re-exported for existing consumers.
- `feathers/node_graph/socket.rs` shares the original GraphSocketSide and
  compact-header proxy helpers. Expanded socket anchors still use measured
  transforms; collapsed nodes use logical header edges rather than stale hidden
  socket transforms. Existing native proxy tests stay unchanged.

Source audit confirms all four consumer bodies are unchanged except native click
type/location spelling and formatting. All six extracted geometry helper bodies,
the side mapping, **89 remaining top-level graph functions**, original root graph
tests and native material graph plugin registration are unchanged. Record changes
are parent visibility only. Candidate modules import actual production source.

## Shared regression coverage

**23** new menu/geometry consumer cases use real engine click events:

- Canvas opening preserves program, editing target, owning view, graph key,
  popup coordinates and pan/zoom conversion, including two views of one program.
- Node descendants select an unselected node but preserve an existing multi-
  selection and another view's selection. Inspector identity and competing menu/
  query reset follow the original behavior.
- Connected input pins select the exact connection; output pins collect all
  matching connections while ignoring another program. Detached/mismatched
  targets and unconnected pins retain their single-delivery guards.
- Actual Bézier hit-testing distinguishes wire context from Add Node at the
  original nine-pixel threshold. Primary clicks select a wire or clear only the
  owning selection, and repeated unchanged selection does not dirty UI revision.
- Isolated marker/node/menu descendants, middle clicks and propagation behavior.
- Explicit anchors, measured fallback, missing anchors, program/kind identity
  lookup, transform and inverse-scale coordinate math.
- Two global-delivery parity cases retain the original ancestor fallback behavior.

Two further shared compact-proxy tests exercise actual node collapsed state,
logical inverse-scale dimensions and invalid/unmeasured size rejection. They
supplement, not replace, the original native tests.

No test edits semantic effect content or advances its edit serial. These are
consumer/event tests, not full material graph plugin or rendered scene tests.

## Propagation limitation retained explicitly

Single-delivery guard tests attach the actual consumers to the target entity so
their return/consume decisions are tested without repeated ancestor invocations.
Separate tests register them globally, like the native plugin. With no other UI
blockers, an unconnected pin click may subsequently open its ancestor node menu;
a marker-only popup may subsequently open the ancestor canvas palette. This
behavior occurs on both engine graphs and is not silently fixed in this
compatibility port.

A marker-only fixture is not the actual popup's event-blocking composition.
Whether a complete native scene needs additional blockers remains a full-plugin/
native acceptance and interaction-hardening follow-up, not a regression claim
from this isolated gate.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/56f44ac36ab24d13b6c3510f84c8410f/`.
It executes **1,997 passing checks**, including **1,575 editor graph/session/
history/consumer checks** and one existing ignored timing probe. The editor suite
takes 58.33 seconds after compilation. Counts are executions across gates, not
unique tests; all 23 new menu/geometry cases and both compact-proxy cases pass.

All **967 input hashes** are unchanged. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false` and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **2,534 editor unit + 3 architecture tests**, with the same ten
existing ignores; these include the shared single-delivery and global bubbling
parity cases. Both engines pass strict all-target Clippy. Workspace all-target
checking with `aestra-bench/gpu`, both formatting gates and Git whitespace
validation pass. Root manifest/toolchain/lock diffs remain empty, and
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings remain non-fatal.

## Limits and next step

Headless real-event/consumer/geometry evidence does not establish native picking,
rendered menu position/DPI, popup blocker integration, complete material graph
plugin scheduling, GPU behavior or complete application workflows. Root dependency/
toolchain/lock inputs remain unchanged; B20-2 remains open.

Next: connection drag/snap/finish lifecycle, then remaining graph controls and
complete graph/application integration.
