# Graph pin/collapse and socket visuals — Bevy 0.20 qualification

Date: 2026-10-20. B20-2 slice 81. Shipping remains Bevy 0.19.1;
the isolated candidate remains 0.20.0, with Rust 1.98.1. No dependency,
authored-format, geometry or rendering-style changes.

## Implementation

- `feathers/node_graph/node_controls.rs` shares nine actual production functions:
  node restoration, live shared-memory synchronization, pin/collapse activation
  queues and consumers, icon/body application and socket visual synchronization.
- Pin/collapse queues use the existing shared production Feathers activation
  classifier. Controls still require both a classified action and an Activate;
  a pressed pointer state alone cannot author an edit.
- `node_control_input_019.rs` retains the original changed-Interaction query
  and consumption policy. `node_control_input_020.rs` uses native Pressed and
  Hovered components, with explicit removal tracking for socket release/leave.
  Pressed takes precedence over hover; hover false/removal restores idle colors.
  Candidate code does not use deprecated Interaction.
- Existing memory, node/action/icon records, composed Feathers controls, SVG
  loader, edit events, theme colors and body visibility are used directly.
  No candidate-only pin/collapse or socket styling implementation is introduced.
- Existing recursive frozen-input hashing includes all four new graph files.

Source audit: restoration, memory synchronization and both body/icon application
functions are unchanged apart from visibility/formatting. Pin/collapse handlers
retain their exact node/memory/icon/label/edit-payload bodies after native input
consumption. Only activation/query/consumption and socket state representation
cross the version boundary. All six remaining generic graph functions, original
root tests and native graph plugin registration are unchanged.

## Shared regressions

15 shared cases mount actual graph nodes, pin/disclosure controls and typed sockets
using real Feathers scenes and SVG assets:

- Activate-required pinning, consumed pressed/pending state, bootstrap-anchor
  materialization, exact pin-edit identity and one-shot behavior.
- Pin/unpin round trips retaining authored bases and temporary display offsets.
- Collapse/expand body visibility, actual SVG swaps, accessible labels and exact
  presentation-edit payloads, without committing temporary displacement.
- Same-frame duplicate Activate coalescing, unclassified controls and retained
  controls whose node has disappeared.
- Material/function node restoration across rebuilds and silent split-view
  synchronization, isolated from other documents.
- Idle node style/icon change ticks.
- Exact socket hover/press/release/leave colors, press precedence, marker removal,
  idle paint/geometry stability and unrelated child/entity isolation.
- Missing socket children and despawned socket entities.

Node actions run through the real headless app Update schedule. For controlled
socket state injection, tests run the retained Update schedule directly: the
headless PreUpdate picking pass has no pointer and would clear injected Hovered.
This retains actual system change ticks/removal readers and actual controls,
without claiming native pointer delivery or replacing the consumer with a test
implementation. The first targeted run exposed that fixture mismatch; the
corrected controlled-state tests pass.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/56b66893be594eb89e9cb551988e5a92/`.
It executes **2,120 passing checks**, including **1,698 editor checks**, with
one existing ignored editor timing probe. The editor suite takes 81.27 seconds
after compilation. Counts are executions across gates, not unique tests.
All 15 new shared cases pass in the full candidate gate.

All **1,007 input hashes** are unchanged. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false` and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
The shipping suite passes **2,657 editor unit + 3 architecture tests**, with the
same ten existing ignores. All 15 new shared cases pass on both engines.
Shipping compilation takes 6m 05s and unit execution takes 34.07 seconds.

Both strict all-target Clippy gates, workspace all-target checking with
`aestra-bench/gpu`, both formatting gates and whitespace checks pass. Root
manifest/toolchain/lock diffs remain empty; post-documentation input drift is
zero. Existing Windows incremental-cache hard-link fallback warnings remain
non-fatal. No native GPU/render acceptance is claimed.

## Limits and next step

This is headless composed-control, event-payload and native state-transition
qualification. It is not OS pointer delivery, native shader/render acceptance,
rendered zoom/DPI or complete graph/application plugin scheduling. The shared
presentation-history consumers remain covered by their existing gates; this
fixture captures actual edit events rather than installing the complete history
pipeline. Existing label refresh, memory revision and activation-coalescing
policies are preserved rather than redesigned.

Next: remaining viewport/grid/selection presentation and full graph plugin
integration/scheduling, then application handlers and complete editor workflows.
Keep B20-2 open and shipping on 0.19.1 until the full compatibility gates pass.
No visual reference is approved.
