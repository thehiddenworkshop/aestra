# B20-2 slice 41: timeline clicks and context-menu intent

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now executes
the actual timeline click/source-opening and pointer/keyboard popup controllers.

- `timeline/clicks.rs` shares marker/event/region/effect-clip selection, source
  double-click routing, referenced-emitter click timing, header action dispatch,
  emitter-group collapse and pointer/keyboard context-menu opening.
- Five actual action/clip/reorder/referenced-header component identities join
  `timeline/interaction_state.rs`. Native timeline, regions and referenced-effect
  construction retains the same controller attachment names and type identities.
- `feathers/context_menu/intent.rs` shares the existing keyboard request and
  node-relative coordinate helpers. Native popup composition is unchanged.
- Pointer boundaries expose equivalent upstream click location and Button marker
  paths. The 0.20 Button marker uses `ui_widgets::Button`, not its deprecated alias.
- The candidate uses actual session, catalog/source resolver, CurvesState,
  complete timeline/choreography execution and global history. Source-opening
  document intents are observed, not replaced with stub source resolution.

Audits verify **25 moved function/type/constant contracts**: 17 timeline handlers/
helpers, five identities, the referenced double-click interval and two popup
helpers. Only visibility, formatting, equivalent upstream event/marker aliases
and the actual pointer-location accessor adapter differ. Native TimelinePlugin
registration/order is unchanged. Three new files join the frozen input inventory.
No behavior fix, authored-format, dependency, feature or toolchain change is
bundled.

## Regression coverage

Thirty-two shared checks use actual upstream pointer events and production
controllers/consumers:

- Marker and choreography-event selection, region selection, missing controls
  and ignored secondary clicks. Effect-clip single/double clicks reach actual
  selection/source-opening execution; trimming and unclassified/stale controls
  cannot open a source.
- Referenced-emitter selection reaches the real catalog resolver and inspected
  child state. Header first-click arming, explicit/repeated double clicks,
  timeout, changing rows and non-emitter fallback preserve the existing policy.
  Actual source/emitter document intents are checked; source bytes stay unchanged.
- Emitter and effect-clip popup opening walks real ChildOf hierarchies. Matching
  group/region selection is preserved, outside targets select singly, conflicting
  popovers/child selection close as appropriate, CurvesState clears and UI
  revision advances without a semantic edit. Missing region headers use the
  existing zero-position fallback.
- ContextMenu and either Shift+F10, active-descendant/ancestor focus routing,
  unrelated/missing focus, absent InputFocus, existing-menu guards and clamped
  keyboard anchors. Supplied geometry checks the current translation/size policy.
- Actual native action-control configuration removes standard button handling,
  marks native ownership and installs the real click observer. The resulting
  semantic action is a single global undoable edit; secondary/missing actions
  are ignored. Modifier selection and action-control propagation are preserved.

The fixture supplies a real Window component because upstream PointerTraversal
queries Window while traversing parents. It supplies computed UI geometry and
targeted events; this is not native picking/layout/window-input acceptance.
Full popup/panel composition and filesystem document-opening execution are not
claimed. Existing native regression tests remain intact.

## Preserved limitations

- Header popup controllers keep the existing stale-header menu identity while
  guarding semantic selection. They do not newly reject all missing menu targets.
- The group-collapse observer tests the current propagated target for action/
  reorder markers. Native action controls stop click propagation themselves, but
  an unhandled reorder-handle click can arrive as a header click and collapse a
  group. A regression records that existing behavior; using original-target
  ancestry or a dedicated stopper is a separate hardening change.
- Popup coordinates retain the existing translation/size calculation; rotated,
  scaled and native floating-window geometry still require native acceptance.

Previously documented automation cancellation/resize cleanup and host-motion
pointer ownership hardening remain separate follow-ups.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/1aac3a3d8cca45a88ddeaa9337a55172/`.
It executes **1,173 checks**, including **754 graph/session/history/consumer checks**,
with all **815 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 14.73 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`,
and `native_executed: false`. Shipping passes **1,840 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting checks and whitespace validation. Root manifest/toolchain/lockfile
diffs remain empty. Existing Windows incremental-cache hard-link fallback warnings
are non-fatal.

## Remaining scope

Next: remaining timeline action dispatch and track-color consumers, then drop/
reorder/presentation and full timeline/Curves panel/plugin composition. Remaining
viewport/application handlers and complete native workflow acceptance stay open.
B20-2 remains in progress; shipping switches engine graphs only after B20-3.
