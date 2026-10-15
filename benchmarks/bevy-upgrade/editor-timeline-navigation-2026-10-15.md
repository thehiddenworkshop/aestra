# B20-2 slice 36: timeline navigation and cancellation controllers

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate executes the
actual production wheel/pan, popup dismissal/focus restoration and host-motion
cancellation systems.

- `timeline/navigation.rs` shares the complete navigation system, popup dismissal
  and deferred focus restoration, plus the existing wheel/cursor/dismiss helpers.
- `timeline/interaction_state.rs` now owns the real pane/header/popup component
  identities used by both native composition and the shared controllers. Automation
  menu identities move here too; there are no substitute UI markers.
- `timeline/motion_execution.rs` shares the actual drag/numeric cancellation system.
  Native host-motion installation retains its ordering relative to timeline visuals.
- `feathers/context_menu/dismissal.rs` shares the existing dismissal policy. Native
  context-menu callers keep the same reexport and their previous regression tests.
- Tiny `timeline/cursor_019.rs` / `cursor_020.rs` boundaries handle the upstream
  OverrideCursor namespace move from Feathers to picking. No cursor behavior changes.

Audits compare all **18 moved system/helper/type contracts**. Only visibility,
formatting and the path to the same shared timeline-duration helper differ.
TimelinePlugin registrations/order and host-motion installation remain unchanged.
Candidate system/query lint allowances match the existing shipping crate policy;
strict Clippy is still required. Five added files join the frozen input inventory.
No authored-format, dependency, feature or toolchain change is bundled.

## Regression coverage

Twenty-four new shared checks run the real systems and motion observers:

- Line/pixel track scroll units, content clamping, missing/nonoverflowing panes,
  ignoring header extent and draining wheel input outside the canvas.
- Horizontal/Shift pan, Control zoom precedence and pointer anchoring, missing
  pointer position, middle-button pan scaling/start/release ownership, and motion
  endpoints extending view bounds without changing effect playback or authored data.
- Popup hover protection, outside primary dismissal, right-click/idle preservation,
  Escape cleanup, one UI revision per combined dismissal and emitter/clip focus.
  Deferred focus waits for the actual header/resource and preserves emitter priority.
- Escape clears all four timeline drag payloads, snap guide and cursor override.
- Live host drags persist until cancellation; Escape or stale effect/track identity
  restores the committed preview without producing an edit/history entry.
- Numeric previews survive idle; Escape, stale selection/effect/track, inactive
  ownership or removed controls cancel safely. Late final values cannot commit from
  cancelled fields, and removed fields retire their cancelled identity.

The fixture provides explicit computed geometry, hover/button state and real mouse
messages; it does not run native picking/layout. Numeric previews start through the
actual ValueChange<f32> observer. Drag payloads are explicit test state: these checks
do not qualify pointer drag initiation, movement or final commit. Existing native
tests remain intact. No production behavior fix was needed.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/834878a0af6b46e4856bcc2b41e94fbc/`.
It executes **1,036 checks**, including **617 graph/session/history/consumer checks**,
with all **800 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 14.27 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`,
and `native_executed: false`. Shipping passes **1,703 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting checks and whitespace validation. Root manifest/toolchain/lockfile diffs
remain empty. Existing Windows incremental-cache hard-link fallback warnings are
non-fatal.

## Remaining scope

Next: actual timeline pointer seeking and drag gestures (regions, clips, markers,
events, automation and host-motion keys), including begin/move/final commit/cancel
and event propagation on the 0.20 pointer API. Then complete timeline/Curves
panel/plugin composition. This slice does not qualify the full TimelinePlugin,
native-window rendering or end-to-end document-opening workflows. Remaining
viewport/application and graph palette/menu consumers stay open. B20-2 is still in
progress; the shipping engine graph switches only after B20-3.
