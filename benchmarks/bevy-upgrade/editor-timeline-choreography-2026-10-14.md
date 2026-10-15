# B20-2 slice 35: complete timeline/choreography action observers

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now runs the
actual complete timeline and choreography observers, not fixture adapters around
individual action branches.

- `timeline/execution.rs` shares both production observers, emitter targeting and
  deletion preparation. The native plugin retains the same observer registrations;
  existing native timeline tests continue using the imported observers.
- `timeline/region_execution.rs` shares region split/join/duplicate/delete semantics,
  merge planning and selection normalization. Native pointer controllers remain
  separate; their existing merge/selection tests are retained.
- `timeline/referenced_execution.rs` shares actual catalog-backed nested path
  resolution, inspection, expansion/collapse and source-opening intents. Native
  callers retain the same exported resolver path.
- The candidate uses the existing real DocumentAction contract, session, full
  TimelineState, CurvesState, catalog and history Routing. No replacement resource,
  event contract, layout implementation or semantic dispatcher is introduced.

Audits compare all **13 moved handler/helper contracts**. Bodies are unchanged
apart from visibility/formatting, routing FrameAll to the same shared motion helper,
and removing an unused WorkspaceLayout parameter from region deletion and its
observer caller. That parameter was never read; removing it avoids a false layout
dependency in this action-only gate. Native plugin registration/order is unchanged.
No behavior fix, authored-format, dependency, feature or toolchain change is bundled.
The four new source/test files join the runner's frozen input inventory.

## Regression coverage

Twenty-six new shared checks register the complete production observers. They cover:

- Fresh emitter creation/duplication, enabled edits, stale/non-emitter targeting,
  minimum-emitter protection and actual deletion preview/confirm/cancel behavior.
- Control/Shift emitter and region selection, curve/nested-inspection cleanup,
  preview-only solo/mute exclusion and context-menu/color-picker revision policy.
- Region split/join, boundary and cross-emitter rejection, duplicate identities and
  source offsets, survivor selection after deletion and confirmation when deleting
  all regions. Undo/redo restores exact semantic state.
- Complete timeline marker dispatch, stale deletion, duration edits and non-semantic
  snap/framing changes. Full choreography delegation executes automation exactly once.
- Real nested catalog resolution, referenced clip/emitter inspection, ancestor
  collapse, clip deletion cleanup and stale paths/emitters. Source files stay unchanged.
- Exact OpenSource/OpenSourceEmitter document intents for local/nested sources.
  These are observed intents, not end-to-end native document-opening acceptance.
- Real keyboard creation/duplication/deletion reaches the production observers and
  confirmation transaction. Mixed timeline/emitter edits use actual global history
  ordering, undo/redo and redo forks; unexpected filesystem history fails the fixture.

Tests use synthetic keyboard messages and explicit selection/hover state. They do
not qualify complete timeline plugin scheduling, popup/pointer controllers, panel
composition or native rendering. Existing native tests and previous narrow gates
remain intact; no validation is removed.

## Validation

Accepted frozen headless run (2026-10-14):
`target/bevy-020-qualification/runs/88e681acd32e47ad9943cf4551d0efc8/`.
It executes **1,012 checks**, including **593 graph/session/history/consumer checks**,
with all **795 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 16.59 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false. Shipping passes **1,679 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting checks and whitespace validation. Root manifest/toolchain/lockfile diffs
remain empty. All 795 frozen inputs still match after validation/documentation.
Existing Windows incremental-cache hard-link fallback warnings do not fail checks.

## Remaining scope

Next: timeline pointer/navigation, popup and numeric cancellation controllers,
then full timeline/Curves panel and plugin composition. Source-opening intents still
need complete application-handler/workflow acceptance. Remaining viewport/application
and graph menu/palette consumers stay open. No new native-window/GPU acceptance is
claimed; shipping still waits for B20-3 before switching the engine graph.
