# B20-2 slice 44: timeline presentation and scrolling

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now executes
the actual timeline geometry, scroll and drop/reorder presentation systems.

- `timeline/presentation.rs` shares emitter-region, marker, event, effect-clip and
  referenced-child geometry; trim handle visibility; ruler/playhead/snap guide and
  transport label updates; horizontal/vertical scroll synchronization; emitter
  reveal/focus; hover insertion, drop gaps/preview and reorder borders.
- `timeline/projections.rs` shares the real catalog-backed nested track expansion,
  ancestor interval clipping and row metadata. Visible automation lane validation,
  counting and custom-height aggregation also move here. No simplified candidate
  projection or copied controller replaces production behavior.
- Sixteen actual presentation identities move to `interaction_state.rs`. The
  four nested-track metadata types live with projections. Native UI construction
  retains those identities, attachment names and the existing plugin order.
- Native imports/reexports are adjusted without changing action routing or authored
  data. Three new files join the runner's frozen source inventory.

Audits verify **53 moved contracts**: 26 presentation functions, four nested-track
projection functions, three automation layout helpers and twenty component/data
types. Only visibility, formatting and explicit import resolution differ. The
entire native root timeline test module and TimelinePlugin registration/order
compare unchanged. Dependency, feature, toolchain and authored-format inputs stay
unchanged.

## Regression coverage

Twenty-seven shared regressions execute production systems with actual ECS types,
session/catalog data and explicitly supplied computed geometry:

- Clipped bars, live region/effect-clip drag previews, referenced-child source
  trimming, stale identities and offscreen trim boundaries.
- Marker/event previews, playhead and snap-guide visibility, tick spacing using
  the largest canvas, text precision and idle transport label behavior.
- User scrollbar input versus programmatic view changes, full-view gutter reset,
  ambiguous horizontal viewport rejection, synchronized panes, track-input
  precedence, unmeasured-track fallback and content-shrink clamping.
- Delayed emitter reveal waits for the row/list, updates ActiveDescendant and
  keyboard focus, then consumes its pending request.
- Header-before-track insertion priority, retaining insertion while hovering the
  gap, clearing it on leave/cancel, grid-row restoration, preview placement/label/
  scroll clamp and ambiguous canvas hiding.
- Reorder target/source borders, cross-kind reorder hints, neutral drop-row
  borders and asset-preview suppression of reorder decoration.
- Real nested catalog expansion, clipped ancestor timing and preserved path/depth/
  source metadata; expanded rows and visible automation custom heights contribute
  to insertion offsets/content size. Stale, hidden and invalid-channel lanes do not.

Fixture updates assert that presentation does not modify the semantic document.
These checks do **not** run native layout, picking, scrolling widgets or rendering.
They are not full panel/plugin or floating-window workflow acceptance.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/56abe47c189f4f129cc81d606fdb4277/`.
It executes **1,251 checks**, including **832 graph/session/history/consumer checks**,
with all **833 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph process
takes 19.81 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false` and
unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **1,918 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. Root manifest/toolchain/lockfile diffs remain empty.
Existing Windows incremental-cache hard-link fallback warnings are non-fatal.

## Remaining scope

Canvas preview/placement and horizontal scrolling still require a single matching
canvas/viewport; multiple canvases are not routed by floating window. Ruler spacing
still uses the largest canvas. Existing pointer ownership, secondary reorder,
automation cancellation and stale-header/color-preview hardening remain separate.
This compatibility slice does not silently change these policies.

Next: timeline/Curves panel and plugin composition, beginning with remaining
lane/track construction, then the remaining viewport/application handlers and
complete native workflows. B20-2 stays in progress; shipping changes engine graphs
only after B20-3 acceptance.
