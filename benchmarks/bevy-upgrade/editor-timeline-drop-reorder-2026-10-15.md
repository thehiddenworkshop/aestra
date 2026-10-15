# B20-2 slice 43: timeline asset placement and track reorder

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now executes
actual timeline asset-drop and emitter/effect-clip reorder controllers.

- `timeline/drop_execution.rs` shares source ancestry resolution, drag preview,
  enter/leave feedback, canvas/header drop observers, placement and mixed-order
  insertion, the compound semantic transaction, rejection display and expiry.
- `timeline/reorder.rs` shares drag start/end, source/target ancestry lookup,
  reorder-index policy and emitter/effect-clip order transactions. Native cursor
  types come through the existing qualified version boundary.
- Six actual feedback/header/reorder identities and the feedback timer default
  move into interaction_state.rs. Native UI keeps the same identities and
  controller attachment names; TimelinePlugin registration/order is unchanged.
- The real timeline payload source/preview adapter is shared from
  timeline/asset_drop/source.rs. Native asset_drop.rs retains its integration
  tests and reexports the adapter under the same names.
- The candidate now compiles the complete production AssetPayload implementation
  and AuthoringDropGuard, not just the identity model. Shared nearest-ancestor,
  snapshot-active and cancellation helpers move to asset_drop/transport.rs.
  Explicit imports/model paths and a fully qualified cancellation call preserve
  native namespace resolution. Actual catalog/source-cycle checks and document
  protection/I/O guards are retained, not replaced by stubs.
- Pointer boundaries select equivalent upstream DragDrop/DragEnter/DragLeave
  event names and provide actual upstream test constructors.

Audits verify **36 moved function/type/impl/alias contracts**: 22 controller/helper
functions, six identities, one timer default, four source-adapter contracts and
three shared transport helpers. Only visibility, formatting, namespace resolution
and equivalent upstream event aliases differ. The entire native root timeline
test module compares unchanged, as does TimelinePlugin registration/order.
Nine files join the frozen inventory, including native wrappers/integration tests.
No dependency, feature, toolchain or authored-format change is bundled.

## Regression coverage

Twenty-five shared checks use actual upstream pointer events with production
snapshot, catalog, session and global history consumers:

- Canvas and header placement from nested source labels, playhead/source-duration
  clamping, relative mixed-track insertion and one compound reversible edit.
  Placement transfers subsequent Undo to the effect without changing the open
  standalone material target. Successful source bytes remain unchanged.
- Secondary/unrecognized drops do not edit. Missing/ambiguous canvas positions,
  Escape release, document modal/pending proposal, stale snapshots, duplicate
  source identities, self-reference, cyclic graphs and incompatible files reject
  without changing the owner document or discarding its proposal.
- Source preview and enter/leave are non-semantic. Escape or removed source
  clears active hover preview/insertion. Valid entry clears old rejection;
  rejection survives leave, carries localized title/reason and expires through
  the real timer. Drag end clears the existing preview/source state.
- Emitter and effect-clip reorder across mixed tracks, before/after placement,
  descendant source and destination lookup, cursor/override transitions and
  exact global undo/redo. Payloads remain unchanged when only order is edited.
- Adjacent/self no-op moves, stale source/target IDs, missing ancestry and
  unrecognized sources do not create edits. Existing secondary reorder behavior
  and index-boundary policy are recorded explicitly.

Fixtures provide real Window components, ECS hierarchy and targeted events.
They do not qualify native pointer generation/picking, floating-window input,
full timeline/browser composition or rendered feedback. The production I/O guard
is compiled and retained; these new tests do not claim a new asynchronous busy-I/O
race proof. Existing native asset-drop integration tests remain intact.

## Preserved limitations

- Reorder start/drop handlers do not currently reject secondary buttons. Unlike
  asset placement, this policy remains permissive; the port does not silently
  change it. Primary-only hardening is a separate behavior change.
- Existing reorder and drag-preview state does not capture pointer/target/window
  ownership. No new foreign-pointer, overlapping-gesture or stale-release safety
  guarantee is claimed.
- Canvas placement still requires a single matching canvas and rejects ambiguous
  canvases. Per-window target routing needs separate implementation/acceptance.

Previously recorded automation cancellation/resize cleanup, host-motion ownership,
stale-header/reorder-click and color-preview hardening remain separate follow-ups.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/78322f7fe5da4d3dbb91ff09b25a322e/`.
It executes **1,224 checks**, including **805 graph/session/history/consumer checks**,
with all **830 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 18.19 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`,
and `native_executed: false`. Shipping passes **1,891 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting checks and whitespace validation. Root manifest/toolchain/lockfile
diffs remain empty. Existing Windows incremental-cache hard-link fallback warnings
are non-fatal.

## Remaining scope

Next: remaining timeline presentation systems, including drop-gap/reorder hints,
geometry and scrolling, then full timeline/Curves panel/plugin integration.
Remaining viewport/application handlers and complete native workflow acceptance
stay open. B20-2 remains in progress; shipping switches engine graphs after B20-3.
