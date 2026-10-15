# B20-2 slice 42: timeline action dispatch and track-color controls

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now executes
the actual timeline activation queue/dispatch, list-selection routing, control
classification audit and track-color preview/commit observer.

- `timeline/action_controls.rs` shares seven production observers/systems and
  both original control-audit filters. Native TimelinePlugin observer/system
  registration, order and control attachment names stay unchanged.
- `control_input_019.rs` retains the original Changed<Interaction> query and
  pressed/pending guard. `control_input_020.rs` selects native Changed<Pressed>
  controls; explicit activation inserts PendingFeathersActivation plus Pressed,
  and dispatch removes both. These adapters only report action-relevant pressed
  transitions; they do not implement hover/presentation or general button logic.
- Three real picker/swatch/region-clip identities join interaction_state.rs.
  The two actual keyboard-list markers move into feathers/list_row/state.rs;
  native widget composition reexports them without changing identity.
- The candidate uses actual MenuState, session, CurvesState, catalog/referenced
  resolver, complete timeline/choreography execution and global history routing.

Audits verify **14 moved function/type contracts**. Activation/dispatch contracts
are compared with the explicit query/queue/clear adapter substitutions; selection,
color editing, audit policy, identities and TimelinePlugin order are preserved.
This is a required Interaction-to-Pressed compatibility adaptation, not a new
action policy. Six files join the frozen inventory, including the existing native
list-row composition file. No dependency, feature, toolchain or authored-format
change is bundled.

## Regression coverage

Twenty-six shared checks exercise actual Activate and ValueChange events with
production dispatch, semantic and history consumers:

- Explicit timeline/choreography activation queues without immediately editing,
  then dispatches once and clears pressed/pending state. Repeated activations
  coalesce before dispatch but work again on a later transition. A raw press,
  pending-only state, unmarked/native/unrelated action or removed action cannot
  dispatch through the tested Feathers route. Pending added after an unchanged
  press waits for another pressed-state change, preserving existing policy.
- Dispatch closes the action menus and tab context without changing unrelated
  menu preferences; the tab-context revision increment happens only once.
- Actual list identity/header filters route emitter, effect-clip and referenced
  emitter selection. Invalid source/row identities and stale rows are ignored.
  Existing non-final ValueChange selection behavior is unchanged. Referenced
  selection resolves a real on-disk catalog source rather than a stub.
- Live color updates matching swatches and all matching region fills without
  changing the canonical document/history; unrelated emitters remain untouched.
  Disjoint query filters handle dual-tagged entities without aliasing mutable
  BackgroundColor access.
- Final color targets the picker emitter rather than current selection, creates
  one reversible semantic edit and uses the current English/French status text.
  Unchanged/missing targets do not invent edits. Reset-to-default follows the
  existing semantic command policy. Marker/color edits preserve global undo order.
- The actual debug control audit accepts classified/native buttons and rejects
  unclassified timeline and choreography buttons.

Fixtures provide upstream events and actual ECS state, not complete native
Feathers button/list/picker or TimelinePlugin composition. Pointer/keyboard input
generation, native layout/picking and full panel acceptance remain separate.

## Preserved limitations

The dispatchers retain their existing caller contract: disabled-control gating
belongs to upstream controls; no new guard against manually injected Activate
events is claimed. Color None events do not themselves restore live fills;
semantic reset/normal presentation rebuilding remain responsible for default
appearance. No new color-preview cancellation guarantee is claimed.

Previously recorded automation cancellation/resize cleanup, host-motion pointer
ownership, stale-header menu and reorder-click hardening remain separate.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/50d2b90160e6469982918e9c1c5de6aa/`.
It executes **1,199 checks**, including **780 graph/session/history/consumer checks**,
with all **821 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 15.80 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`,
and `native_executed: false`. Shipping passes **1,866 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting checks and whitespace validation. Root manifest/toolchain/lockfile
diffs remain empty. Existing Windows incremental-cache hard-link fallback warnings
are non-fatal. The two audit-panic checks are debug-only, matching the production
audit; no release-profile execution is claimed.

## Remaining scope

Next: timeline asset-drop/reorder and remaining presentation consumers, followed
by complete timeline/Curves panel/plugin integration. Remaining viewport/
application handlers and full native workflow acceptance stay open. B20-2 remains
in progress; shipping switches engine graphs only after B20-3.
