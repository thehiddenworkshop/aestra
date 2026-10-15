# B20-2 slice 32: timeline marker/event document actions

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate imports the
production timeline action/snap contracts and six session-backed marker/event
branches through `timeline/document_actions.rs`. The native observer delegates to
these same branches, retaining its timeline-state cleanup in the original order.
Duration, framing, snap execution and emitter-region split/join stay native-owned.

The six session-edit bodies are unchanged apart from passing an optional localizer
reference at the boundary. Adding clears emitter selection without clearing nested
inspection; selecting clears both, even for stale IDs; deletion leaves native
selection cleanup unchanged. Three shipping-only tests execute the actual native
observer with real TimelineState to protect that wrapper behavior.

The shared helper reports whether it owns an action, not whether semantic validation
accepted an edit. Native-owned actions return false without changing the session.
No independent production observer or additional registration is introduced. No
behavior fix, dependency, feature, toolchain or authored-format change is bundled.
The runner adds the timeline root, action contract, helper and both test files to
its frozen input inventory.

## Regression coverage

Fourteen shared checks use actual EditorSession, catalog, material/function history,
localization and production global history Routing. They cover:

- Marker/event creation at the playhead, existing labels/default payloads, fresh
  identities, selection, dirty state and one undo entry per accepted edit.
- English/French marker labels, non-semantic selection and stale-ID rejection.
- Exact identity/payload restoration on deletion undo/redo and atomic rejection of
  stale deletions without partial document/history mutations.
- Mixed marker/event global undo/redo ordering and redo invalidation after a fork.
- The unchanged snap contract and no-op handling of native-owned action variants.

The shared fixture dispatches events through a small observer adapter calling the
production helper. It does not substitute TimelineState or claim the full native
timeline observer, keyboard routing, plugin scheduling or panel composition on
0.20. An unexpected filesystem-history callback fails the fixture. Existing native
timeline regressions remain intact; the three wrapper regressions add coverage
without replacing them.

## Validation

Accepted frozen headless run (2026-10-14):
`target/bevy-020-qualification/runs/b312b79c08d04071b9b179ec60aa480a/`.
It executes **934 checks**, including **515 graph/session/history/consumer checks**,
with all **772 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 12.85 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false. The final shipping snapshot passes **1,611 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace all-target checking with
`aestra-bench/gpu`, both formatting checks and whitespace validation. Root manifest,
toolchain and lockfile diffs remain empty. Existing Windows incremental-cache
hard-link fallback warnings do not fail these checks. All 772 frozen inputs still
match after validation and documentation updates.

## Remaining scope

Next: actual timeline state/keyboard/choreography dependencies and their complete
consumers. TimelineState still depends on drag, automation, host-motion and UI
types; this narrow document branch deliberately does not introduce a replacement
state just to compile the keyboard fixture. Remaining viewport/application action
handling, graph menu/palette composition and complete editor workflows stay open.
No new native-window/GPU acceptance is claimed; the engine switch still requires
B20-3.
