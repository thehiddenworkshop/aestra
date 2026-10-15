# B20-2 slice 47: outer effect-clip headers and popup composition

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate builds the
production outer root effect-clip headers, reorder handles and their five-item
context menus. Outer emitter headers and color/automation popovers remain next.

- `timeline/effect_clip_composition.rs` shares the existing constructors, retaining
  selected styling, native list identity, disclosure/mute/solo click routing,
  localized labels, source/count text, SVG reorder icons and attached production
  source-open, context-menu and reorder observers.
- `feathers/context_menu/composition.rs` shares the complete production pointer
  popup constructors and interaction observers. The native wrapper preserves its
  API and complete existing tests. Candidate graph composition uses actual
  Feathers scenes, pointer adapters and graph navigation-blocker identity; the
  existing standalone text-widget gate continues to exercise submenu behavior.
- `asset_actions/action.rs` shares the real AssetAction enum, including the
  explode action identity. Its native reexport and host execution stay intact.
  The candidate verifies the popup's exact explode payload, not filesystem
  execution or successful source extraction. Other asset-action consumers remain
  host-qualified; the narrow candidate dead-code allowance says so explicitly.
- Four new files join the frozen input inventory. Dependency versions, features,
  toolchain and authored formats are unchanged.

Audits compare **15 function contracts**, the AssetAction enum, native timeline
and asset plugin wiring, and complete existing root timeline/context-menu test
modules. Function bodies are unchanged except formatting, visibility and
equivalent namespace resolution. Wider crate-private submenu visibility permits
the existing wrapper tests and reexports without changing behavior.

## Regression coverage

Twenty new shared tests build the actual production controls with real source
files/catalogs and the real action/history consumers:

- Header grid geometry, selected fill/marker, native list/control classification,
  localized accessibility, source name/count, inverse preview labels and styles.
- Inline disclosure/mute/solo clicks update only preview/expansion state and stop
  propagation. Injected Feathers activation does not double-dispatch these native
  click controls; secondary clicks open the correct header context instead.
- Real popup anchor/placement configuration, exact five action identities,
  action classification, click isolation and single pending-activation consumption.
- Source menu activation and header double-click emit the real DocumentAction
  intent; list selection uses the actual header action without semantic edits.
- Menu deletion clears clip preview/inspection state and roundtrips real history.
  Attached reorder handles/drop headers perform one globally reversible mixed
  emitter/effect-clip order edit, preserving cursor and SVG geometry contracts.

Existing removal policy is deliberately preserved: the authored choreography
order vector retains removed IDs, while the production normalized projection
filters them out. The test checks both facts, rather than silently modifying
authoring behavior during an engine compatibility port. Existing stale-menu and
pointer-ownership policies also remain unchanged.

These are scene/observer tests with explicit geometry, not native picking,
layout/clamping/rendering or floating-window acceptance. Source-opening intents
are captured, not executed externally; the explode host operation is not mocked
or claimed qualified. Full timeline/Curves plugin and workflow acceptance remain
open. No optional replacement widgets are introduced.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/ca3d3fdfdb7249e1b8d920f036f4f4f9/`.
It executes **1,318 checks**, including **898 graph/session/history/consumer
checks**, with all **843 input hashes** unchanged. Counts are executions across
gates, not unique tests. One existing graph timing probe stays ignored; the graph
process takes 17.35 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`,
and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **1,978 editor unit + 3 architecture tests**; ten existing ignores
remain unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. Root dependency/toolchain diffs remain empty. Existing
Windows incremental-cache hard-link fallback warnings are non-fatal.

Next: outer emitter headers and actual color/automation/context popovers, remaining
track/toolbar construction and full timeline/Curves integration, then remaining
viewport/application consumers and native workflow acceptance. B20-2 stays open;
these headless results do not authorize switching the shipping engine graph.
