# B20-2 slice 40: host-motion gestures and in-place key visuals

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now runs the
actual host-pose pointer controllers and key presentation system.

- `timeline/motion_gestures.rs` shares the real KeyControl identity, click and
  drag-start/move/end observers, displacement calculation and in-place visuals.
- Native `host_motion.rs` retains composition, control attachments, activation,
  numeric initialization and existing tests. Its install function and native
  TimelinePlugin registration/order are unchanged.
- The existing pointer version boundary exposes the actual pointer location:
  the 0.19 event field and 0.20 pointer accessor. This preserves the event's own
  window routing, including floating timelines, without translating behavior.
- The candidate uses actual session previews, host-motion action execution,
  timeline marker actions and global history routing. Two new files join the
  frozen input inventory.

Audits verify all **seven moved function/type contracts**, with only visibility,
formatting, equivalent upstream pointer aliases and the location accessor adapter
differing. No semantic behavior fix, authored-format, dependency, feature or
toolchain change is bundled.

## Regression coverage

Twenty-seven shared checks exercise actual upstream pointer events, production
observers, real ECS hierarchy propagation and semantic/history consumers:

- Primary selection, ignored secondary events, immutable time-zero anchor,
  stale effects/indices and missing tracks. Beginning a drag clears conflicting
  region selection without revising/rebuilding its target.
- Live preview changes the actual evaluated host pose without changing the
  canonical effect or history. Release commits one reversible edit. Retiming
  across other keys preserves sorted selection and consumes the trailing click
  once; a secondary click cannot consume that suppression token.
- No-op and late release, collisions and recovery, Escape preview restoration,
  document replacement and canonical clear during a drag. Stale releases cannot
  overwrite those tested canonical changes.
- Repeating interior/endpoint constraints, frame/seconds/Smart snapping and
  mixed marker/motion undo/redo ordering through the global router.
- Floating-window scale, nearest canvas ancestry through a wrapper, missing
  window fallback and missing canvas. UiScale is deliberately not divided out a
  second time: this preserves the existing host-motion coordinate policy.
- Live key positions keep the original target binding while the candidate key
  changes sorted index. Selected colors, stable entities, out-of-view/stale
  hiding and no redundant Node/BackgroundColor changes on an idle pass.

Fixtures supply computed geometry and targeted upstream pointer events. They do
not qualify native picking, layout, GPU output, real floating-window input or
complete TimelinePlugin/Curves composition.

## Preserved ownership limitation

Host-motion dragging uses the existing active drag/effect state and event target
ancestry. KeyDrag does not capture a pointer ID or target entity. This compatibility
slice does not add or claim cross-pointer/foreign-target ownership guarantees;
that behavior hardening needs separate implementation and regression coverage.
Previously documented automation cancellation/resize cleanup follow-ups remain
open as well.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/3837b73a533849dfa79ef9ce12209582/`.
It executes **1,141 checks**, including **722 graph/session/history/consumer checks**,
with all **812 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 22.75 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`,
and `native_executed: false`. Shipping passes **1,808 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting checks and whitespace validation. Root manifest/toolchain/lockfile
diffs remain empty. Existing Windows incremental-cache hard-link fallback warnings
are non-fatal.

## Remaining scope

Next: remaining timeline click/context-menu consumers, then complete timeline/
Curves panel/plugin composition. Remaining viewport/application handlers and full
native workflow acceptance stay open. B20-2 remains in progress; shipping switches
engine graphs only after B20-3.
