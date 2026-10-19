# Material header, node appearance and bootstrap layout — Bevy 0.20 qualification

Date: 2026-10-19. B20-2 slice 79. Shipping remains Bevy 0.19.1;
the isolated candidate remains 0.20.0, with Rust 1.98.1. No dependency,
authored-format, widget replacement or rendering-style changes.

## Implementation

- `apps/aestra-editor/src/material_graph/header.rs` shares the actual material
  header, toolbar-button forwarding, preview-target enumeration and per-view
  camera-key helper.
- `workspace_layout.rs` shares the original bootstrap layout, depth traversal,
  layout record and constants. Existing constant/depth paths remain available to
  asset-drop, function graph and original root tests.
- `appearance.rs` shares the original node background policy. Native adapters
  retain 0.19's Interaction/Button change filter and use 0.20's Button,
  Hovered/Pressed transitions and marker removals. Hover, press and selection
  colors remain exact; this is not a visual redesign.
- `feathers/node_graph/toolbar.rs` shares actual graph toolbar chrome, summary,
  icon/frame button composition, frame-action identity and frame dispatch. Frame
  activation uses the existing shared production queue; native pressed-state
  queries/consumption are isolated in version-specific files.
- `drag_assist/controls.rs` shares the actual snapping control identities,
  composition, Activate observer and selected/variant/localized-tooltip sync.
  There is no candidate-only snapping or frame implementation.
- Read-only test accessors expose actual frame requests/action identity. They do
  not replace navigation, framing, the geometry registry or pointer dispatch.

Source audit: the four header helpers, two layout functions, three snapping
functions and four toolbar composition bodies are unchanged apart from whitespace.
Only node interaction representation and frame queue/query/consumption use the
native API boundaries. All **55 remaining top-level material graph functions**
(including the generic inline-default helper), original root tests and both
native graph plugin registrations are unchanged. Frame-record/layout visibility
only expands to preserve existing parent consumers after extraction.

## Shared regressions

**23** new shared cases execute actual production code:

- Seven material-header cases compose real Feathers controls and the SVG icon
  plugin. They check fixed header/button dimensions, theme background, action
  identity and actual child order, frame keys, scope/program/arrangement identity,
  standalone back/summary, absent projections, preview visibility labels in both
  locales, actual Activate classification and independent split/tool camera keys.
- Six shared-toolbar cases cover Activate-required frame dispatch, consumption
  with no matching view, unclassified action rejection, real snapping defaults
  and independent toggles, locale refresh without idle tooltip writes and passive
  summary styling.
- Six node-appearance cases check exact selected/unselected colors through hover,
  press, release and hover removal, scope/program isolation, missing-scope
  fallback, unclassified entities and idle change ticks without selection edits.
- Four bootstrap cases check deterministic layout/output placement, preview
  spacing/canvas bounds, empty projections and fan-in/missing-input/cyclic depth
  traversal.

Composed controls use the same production headless Feathers setup as the earlier
numeric/text gates. The SVG loader is installed rather than substituting icon
entities. Child order, not ECS table iteration order, defines header action order.

## Preserved boundaries

Frame requests retain the original key-match behavior; all mounted views with an
identical key receive the request. Distinct per-view keys are tested, but no new
view identity/epoch or cancellation policy is introduced. Tests inspect the actual
request field; camera framing remains covered by the earlier geometry/navigation
gates, not reimplemented here.

Node styling remains driven by interaction transitions rather than refreshing on
every selection change. Snapping keeps its existing session-only state and
Activate routing. Bootstrap layout retains the original ordering, dimensions and
cycle behavior. Existing summary wording and localization boundaries are preserved.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/9e9036f293c64dadb53c45324d0acb14/`.
It executes **2,089 passing checks**, including **1,667 editor graph/session/
history/composition checks**, with one existing ignored editor timing probe. The
editor suite takes 68.95 seconds after compilation. Counts are executions across
gates, not unique tests. All 23 new shared cases pass in the full gate.

All **996 input hashes** are unchanged. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false` and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Both engines pass strict all-target Clippy; both formatting gates and Git
whitespace validation pass. Existing Windows incremental-cache hard-link fallback
warnings remain non-fatal.

The final-source shipping suite passes **2,626 editor unit + 3 architecture
tests**, with the same ten existing ignores. Workspace all-target checking with
`aestra-bench/gpu` passes. Root manifest/toolchain/lock diffs remain empty;
post-documentation input drift is zero.

## Limits and next step

This qualifies the actual composed header and its shared controls, not the entire
material workspace. The node/socket/wire canvas builders and function-inspector
branches are not installed by this gate. Native picking, rendered text/icons,
graph zoom/DPI, complete graph plugin scheduling and full editor workflow
acceptance remain separate. No visual reference is approved; B20-2 stays open.

Next: material canvas/node/socket/wire composition, then complete graph plugin
integration/scheduling and editor workflows. Keep the existing controls and
geometry during the compatibility port; upstream widget adoption remains later.
