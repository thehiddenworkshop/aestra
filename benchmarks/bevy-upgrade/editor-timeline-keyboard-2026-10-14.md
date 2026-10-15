# B20-2 slice 33: production timeline state and keyboard consumer

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now imports
the actual TimelineState and complete choreography keyboard consumer, rather than
a reduced fixture state. Shipping keeps the same system in its original chained
TimelineSet::Input registration; no observer or shortcut registration is added.

- `timeline/interaction_state.rs` shares actual canvas/set identities, drag payloads,
  automation lane/key selection and nested effect-clip paths/inspection.
- `timeline/motion_state.rs` shares the actual host-motion state, action identity,
  active-owner predicate and keyboard handler. Native pose editing/UI execution
  continues using these types and functions.
- `timeline/state.rs` retains the full production resource, navigation snapshots,
  framing/panning/zooming and emitter selection. Existing region-selection methods
  and normalized track ordering move here unchanged from their native modules.
- `timeline/automation_data.rs` shares actual curve/gradient/vector-channel lookup,
  graph-data projection and pointer-to-key action creation. Its lookup calls
  `active_parameter_value` directly, the exact method previously wrapped by the
  one-line `module_parameter` helper; parameter-binding precedence is unchanged.
- `timeline/keyboard.rs` shares the complete production keyboard system unchanged.
  `actions.rs` supplies the real choreography action contract. Existing timeline
  tests continue using the same consumer through a test-only re-export.
- `asset_drop/payload/model.rs` shares the actual immutable asset identity needed
  by TimelineState. Capture, validation and release implementations stay native;
  no substitute payload or filesystem authority is introduced.

The candidate also imports the unchanged production automation-curve widget for
value projection, including its six existing tests. No renderer/widget replacement,
behavior fix, authored-format change, dependency, feature or toolchain change is
bundled. Body audits verify unchanged keyboard, host-motion ownership, state methods,
region selection, normalized ordering and automation projection/lookup bodies apart
from visibility/imports and the equivalent one-line lookup adaptation above.

The runner freezes the newly imported source modules and their native callers.
Candidate-only lint annotations mirror the shipping policy for explicit Bevy system
arguments/query complexity; strict Clippy remains enabled and shipping policy is
unchanged.

## Regression coverage

Nineteen new shared regressions run the real keyboard consumer with actual
InputPlugin, InputFocusPlugin, InputDispatchPlugin, EditorKeyboardPlugin, session,
TimelineState, canvas, graph identities and action events. They cover:

- Ordered same-frame Ctrl+Enter/Ctrl+D, logical letters, both Control keys,
  event-time modifier release, held keys and repeat suppression.
- Delete precedence: selected automation key, selected regions, marker/event,
  local effect clip and emitter; nested inspection protects referenced content.
- Real marker/event deletion and session undo. Choreography and pose events are
  observed for exact routing, not executed by replacement semantic handlers.
- Host-motion Insert/Delete ownership and suppression of emitter creation/duplicate
  while motion owns selection; a stale owner no longer captures the chord.
- Canvas presence versus hover, multiple-canvas deduplication, teardown, actual
  material viewport hover, editable/code/menu/browser ancestor focus, palettes,
  document/asset modals, window focus and fresh-chord recovery.
- Hovered automation insertion through actual curve/gradient projection, missing
  cursor positions, invalid/unhovered lanes, active property-source values,
  effect-parameter binding priority and valid/stale vector-channel indices.
- Full navigation restoration retaining view/context/automation heights while
  clearing transient selection/motion, plus normalized stale/duplicate track order.

Four existing TimelineState tests now execute on 0.20 too, as do six existing
automation-curve tests, without removing or duplicating shipping tests. These plus
the nineteen new tests add **29 candidate executions**. The previous fourteen
marker/event tests remain required, now under the actual timeline module context.

The fixture uses synthetic keyboard messages and explicit hover geometry. It
qualifies the real keyboard consumer/state, not native OS input, complete timeline
plugin scheduling, choreography/pose action execution or full panel composition.
The automation widget's existing raster update checks do not establish visual parity.

## Validation

Accepted frozen headless run (2026-10-14):
`target/bevy-020-qualification/runs/a033704e726f4008b0f3406fccd770fc/`.
It executes **963 checks**, including **544 graph/session/history/consumer checks**,
with all **785 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 10.75 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false. Shipping passes **1,630 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting gates and whitespace validation. Root manifest/toolchain/lockfile diffs
remain empty. Existing Windows incremental-cache hard-link fallback warnings do
not fail these checks.

## Remaining scope

Next: remaining timeline/choreography action execution, including host-motion and
automation editing, then timeline panel/plugin composition. Keyboard dispatch is
now covered, but observing its action payload is not semantic execution acceptance.
Remaining viewport/application action handling, graph palette/menu composition and
complete native editor workflows remain open. No new native-window/GPU acceptance
is claimed; shipping still waits for B20-3 before switching the engine graph.
