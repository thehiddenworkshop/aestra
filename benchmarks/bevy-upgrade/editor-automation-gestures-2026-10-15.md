# B20-2 slice 39: automation gestures and live presentation

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now executes
actual automation key/lane pointer controllers and live presentation systems.

- `timeline/automation_gestures.rs` shares selection, double-click insertion, key
  and lane drag observers, source-window projection, snap policy, key/graph/raster
  previews and the existing lane-height method.
- The three real key/lane/resize-handle component identities move into
  `timeline/interaction_state.rs`. Native `automation.rs` keeps metadata/lane
  discovery and reexports the shared controllers under the same names.
- Existing pointer version boundaries add equivalent upstream PointerClick names
  and test constructors. Production event behavior is not translated.
- The candidate uses the actual automation-curve widget data/raster components,
  session, catalog, complete choreography observer, CurvesState and global history
  router. The native keyboard retains the same shared insertion-helper import.

Audits verify all **19 moved function/impl/type contracts**. Only visibility,
formatting and equivalent upstream pointer aliases differ. Native TimelinePlugin
registration/order and control attachment names remain intact. Two new files
join the frozen source inventory. No semantic behavior fix, authored-format,
dependency, feature or toolchain change is bundled.

## Regression coverage

Twenty-seven new shared checks use actual upstream pointer events, production
observers and real semantic/history/presentation consumers:

- Primary selection reaches timeline and CurvesState without an edit. Scalar and
  gradient double-click insertion reaches the complete action observer; primary
  button, click count, known cursor and propagation policies are preserved.
- Key time/value preview is non-semantic, source-window aware, scaled by window,
  largest canvas, view span and lane height, and bounded between neighboring keys.
  Child pointer propagation prevents parent playhead seeking.
- Changed release creates exactly one reversible edit and updates selection.
  Unchanged release does not invent an edit or implicit selection. Gradient
  dragging changes time without rewriting color; vector dragging edits only the
  selected channel. Bound parameters and stored source banks retain their real
  command routing without rewriting unrelated or legacy module payloads.
- Selected-region offsets, stale lane/key/value data, mismatched key motion and
  release, frame/seconds/Smart snapping and mixed global history ordering.
- Actual raster data and key nodes receive preview changes without editing the
  document; gradient key vertical position stays unchanged. Missing/out-of-view
  graph/key nodes hide, and graph geometry keeps the existing source/view policy.
- Lane resize uses saved/default height and window scale, updates matching nodes,
  clamps limits, stops propagation and remains non-semantic on release.
  Mismatched resize motion is ignored.

Fixtures provide computed geometry and targeted upstream events with real ECS
hierarchy propagation. Raster-component data is checked, not native GPU output.
Native picking/layout/window input, full TimelinePlugin/Curves composition and
remaining host-motion/context-menu consumers are not qualified here. Existing
native regression tests remain intact.

## Inherited limitations, not fixed by this compatibility slice

- The existing timeline Escape navigation path clears region/clip/marker/event
  drags, but not automation-key or lane-resize transient state. No new automation
  Escape-cancellation guarantee is claimed.
- Lane-resize release takes the pending resize before checking lane identity, so a
  mismatched release can retire it. This original cleanup policy is unchanged;
  mismatched-release hardening is separate from the tested motion ownership guard.

These are behavior-hardening follow-ups, not new 0.20 adaptations.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/4ba32ec2703e4a5cbc05f3645998b9ca/`.
It executes **1,114 checks**, including **695 graph/session/history/consumer checks**,
with all **810 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 14.34 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`,
and `native_executed: false`. Shipping passes **1,781 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting checks and whitespace validation. Root manifest/toolchain/lockfile
diffs remain empty. Existing Windows incremental-cache hard-link fallback warnings
are non-fatal.

## Remaining scope

Next: host-motion pointer gestures and remaining timeline click/context-menu
consumers, then complete timeline/Curves panel/plugin composition. Track automation
cancellation/resize-release hardening separately. Remaining viewport/application
handlers and complete native workflow acceptance stay open. B20-2 remains in
progress; shipping switches engine graphs only after B20-3.
