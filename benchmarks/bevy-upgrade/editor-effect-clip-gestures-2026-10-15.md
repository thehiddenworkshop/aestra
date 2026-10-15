# B20-2 slice 38: source-aware effect-clip pointer gestures

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate executes the
actual effect-clip drag start/move/end observers, snapping, source-aware trim
preview and final timing command.

- `timeline/clip_gestures.rs` shares all seven production observers/helpers,
  including catalog-backed source duration/looping lookup and the existing
  conservative fallback for unavailable sources.
- The actual `TimelineEffectClipInteraction` identity moves into
  `timeline/interaction_state.rs`; native UI construction keeps the same type.
- Native callers and existing helper tests use imported implementations. The
  candidate uses the already qualified cursor/pointer namespace boundaries and
  real session, project catalog, complete choreography action observer and global
  history router. No substitute resources or dispatchers are introduced.

Audits verify all **eight moved observer/helper/type contracts**. Only visibility,
formatting and equivalent upstream pointer type aliases differ. Native
TimelinePlugin registration/order remains unchanged during this slice; native
control attachment names remain intact. Two added source/test files join the
frozen inventory. No behavior fix, authored-format, dependency, feature or
toolchain change is bundled.

## Regression coverage

Twenty-three new shared checks exercise actual upstream pointer events and
production handlers:

- Real published catalog duration and Once/LoopRestart/LoopContinuous policy,
  unavailable-source fallback, stale/unrelated starts and replacing an active
  emitter-region drag with a clip gesture.
- Preview-only movement, source-window preservation, parent bounds, window
  scaling, largest-canvas width and current view span. Child drag motion stops
  before reaching the parent canvas's real playhead seeker.
- Start trimming advances source offset while preserving the parent end.
  One-shot expansion stops at source zero; looping expansion wraps negative
  source offsets. End expansion respects one-shot source extent or permits
  looping until the parent end. Both ends retain at least one frame.
- One semantic commit per changed release, final selection/cursor cleanup and
  exact undo/redo. Unchanged releases remain non-semantic for all three kinds.
- Escape and late release, mismatched clip/kind motion/release, motion without
  an active gesture and deleting a clip mid-gesture without resurrection.
- Frame/seconds/Smart snapping, choosing a closer end-boundary guide and clearing
  stale guides when snapping is disabled.
- Marker-anchored clip movement updates its offset without moving the marker.
  Clip releases and other timeline actions share the real global history order.

Referenced source bytes are checked throughout editing/history scenarios; missing
sources remain missing. Fixtures supply explicit computed geometry and targeted
upstream events with real hierarchy propagation. They do not qualify native
picking/layout/window input, click/context-menu controllers, automation or
host-motion gestures, or complete TimelinePlugin/panel composition. Existing
native tests are retained.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/ed90922c9caa41b885d6ea4a5e198fac/`.
It executes **1,087 checks**, including **668 graph/session/history/consumer checks**,
with all **808 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 17.88 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`,
and `native_executed: false`. Shipping passes **1,754 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting checks and whitespace validation. Root manifest/toolchain/lockfile
diffs remain empty. Existing Windows incremental-cache hard-link fallback warnings
are non-fatal.

## Remaining scope

Next: automation-key/lane and host-motion pointer gestures, remaining timeline
click/context-menu consumers, then full timeline/Curves panel/plugin composition.
Remaining viewport/application handlers and native workflow acceptance stay open.
No new native-window/GPU acceptance is claimed. B20-2 remains in progress; the
shipping engine graph switches only after B20-3.
