# B20-2 slice 37: timeline seeking and region/marker/event gestures

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now runs
actual production seeking, control propagation stoppers and emitter-region,
marker and choreography-event drag start/move/end observers.

- `timeline/gestures.rs` shares seeking, control propagation and marker/event
  pointer observers. Their final selection intents reach the complete production
  timeline action observer.
- `timeline/region_gestures.rs` shares region drag observers, grouped preview,
  snapping and final semantic transactions. Native `regions.rs` retains the same
  imported names and its context-menu/dismissal controllers and existing tests.
- `timeline/gesture_math.rs` shares the existing tick spacing, marker/event snap
  policy, distance conversion and drag cursor helpers. Other native consumers
  continue calling the same implementations through imports.
- The real marker/event/region-control component identities move into
  `timeline/interaction_state.rs`; no substitute control contracts are introduced.
- `timeline/pointer_019.rs` / `pointer_020.rs` select equivalent upstream event
  names. Only test constructors differ between generic 0.19 pointer payloads and
  0.20 event structs. Cursor imports use the already qualified namespace boundary.

Audits compare all **28 moved observer/helper/type contracts**: bodies are unchanged
apart from visibility, formatting and equivalent pointer type aliases. Native
TimelinePlugin registration/order remains unchanged during this slice. Existing
native UI observer attachment names stay intact. There is no behavior fix,
dependency, feature, toolchain or authored-format change. Six added source/test
files join the runner's frozen input inventory.

## Regression coverage

Twenty-eight new shared checks dispatch real upstream pointer events into the
production observers with the actual session, catalog and global history router:

- Primary seek within the current view, cursor clamping, unknown/unrelated targets,
  secondary-button exclusion and cursor-based rather than distance-based seeking.
  Primary seek retains existing combined popup cleanup/revision behavior.
- Real ChildOf propagation: control press/drag stoppers and drag-motion observers
  keep child interactions from reaching the parent canvas's playhead seeker.
- Region movement, start/end trimming and source offsets, minimum-frame duration,
  effect bounds, window scaling and largest-canvas width. Movement previews do not
  edit the document; release creates exactly one reversible history entry.
- Selected region groups move together, retain relative timing, clamp together and
  preserve multi-selection. Unchanged releases are non-semantic; stale starts and
  mismatched motion are ignored. Escape prevents a late release from committing.
- Marker/event drag previews, scaling/bounds, frame/seconds/Smart snapping and
  guides, final semantic selection, exact undo/redo, unchanged release, stale
  targets, mismatched motion/release and Escape/late-release behavior.
- Moving a marker updates an anchored event in the same command. Moving an anchored
  event preserves its marker and updates its offset; both round-trip exactly.

The fixture supplies explicit computed geometry and targeted pointer events.
It exercises real ECS observers and hierarchy propagation, not native picking,
layout or window input. It does not qualify marker/event click observers, region
context menus, effect-clip, automation or host-motion pointer controllers, or the
full TimelinePlugin/panel composition. Existing tests are retained.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/3d19431cf7474c368330ab533af49e0d/`.
It executes **1,064 checks**, including **645 graph/session/history/consumer checks**,
with all **806 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 20.13 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`,
and `native_executed: false`. Shipping passes **1,731 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting checks and whitespace validation. Root manifest/toolchain/lockfile
diffs remain empty. Existing Windows incremental-cache hard-link fallback warnings
are non-fatal.

## Remaining scope

Next: remaining effect-clip, automation and host-motion pointer gestures and
click/context-menu consumers. Then full timeline/Curves panel/plugin composition,
remaining viewport/application handlers and complete editor workflow acceptance.
No new native-window/GPU acceptance is claimed. B20-2 remains in progress; shipping
switches to the candidate engine graph only after B20-3.
