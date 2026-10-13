# B20-2 slice 25: history controls, availability and keyboard ingress

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated Bevy **0.20.0** candidate now
executes the actual history button handler, menu availability rules, control
classification audit, keyboard snapshot collector and history shortcut dispatcher.
This is a compatibility port, not adoption of replacement widgets.

- `history/controls.rs` shares the production handler and availability policy.
  Actual focused view/document registries, code buffers, global operation order,
  effect/material/function histories, compound layouts, document protection and
  project I/O ownership determine availability. Existing activation consumption,
  native button colors and transient menu closure remain unchanged.
- `history/control_input_019.rs` retains shipping's original `Changed<Interaction>`
  query. The 0.20 adapter uses actual `Pressed` and `Hovered` components, including
  removal transitions, and the actual headless Button for classification. It does
  not depend on deprecated Interaction/Button aliases or surrogate components.
- `input/keypresses.rs` shares the existing event-time snapshot collection and
  layout-aware logical-letter mapping, registered after input-focus dispatch.
  Repeated snapshots remain available to repeat consumers; discrete shortcuts
  do not repeat. Modifier sides, punctuation and number-key policy are unchanged.
- `history/keyboard.rs` shares actual Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y dispatch.
  The native wrapper still supplies its real palette and `ShortcutContext`
  ownership decision. Candidate dispatcher tests pass an explicit blocking signal;
  this does **not** qualify the complete application ownership context.
- Actual menu state/markers and ToolPanel move to `menus/state.rs` and
  `docking/panels.rs`. Shipping reexports preserve existing consumers; serialization
  variants/aliases, defaults, panel identities and behavior are unchanged. The
  candidate imports these production types instead of menu marker substitutes.
- The frozen runner includes both version-specific control adapters, shared
  controls/tests, keyboard ingress/dispatch, native input/menu consumers and panel
  identities. No manifest or lockfile change is needed in this slice.

## Regression coverage

Twenty-two new shared regressions cover:

- History-only availability updates, optional catalog handling, actual effect
  undo/redo, focused material/function ownership and stale-view fallback.
- Active, empty and stale code-buffer ownership; every document-protection branch;
  real busy I/O until completion; global chronology without local fallback; and
  actual layout-only undo without a semantic step.
- Real Feathers Activate consumed exactly once, pressed-without-Activate rejected,
  disabled activation preserved, and native hover/press/release colors without
  held-press redispatch. Removed hover markers are explicitly covered on 0.20.
- Closure of File/Edit/View/Help and panel/tab-context menus without changing grid
  or About state; only removal of tab context increments the UI revision.
- Actual Button classification rejects unclassified controls and accepts explicit
  native/Feathers controls.
- Real KeyboardInput messages through input-focus snapshot dispatch, including
  same-frame AZERTY chords, ordering, both modifier sides, held modifiers across
  frames, repeat suppression/retention and logical punctuation. Accepted shortcuts
  execute the actual history router; blocked chords do not replay after unblocking.
- Direct ButtonInput fallback remains usable when no snapshot collector exists.

## Remaining scope

The complete application `ShortcutContext` (all modal/palette/window/focus states),
tab-navigation ownership and other application shortcut/action handlers remain
open. Full panel/plugin composition and interactive native workflow acceptance
also remain open. These tests do not qualify a substitute ownership model, the
native event loop or whole-editor parity. The root engine switch still requires
the full B20-3 release gate.

Next: share and qualify the actual application shortcut ownership context and
remaining keyboard/action consumers, then full panel composition and native workflows.

## Validation

The final candidate graph/session/history gate passes **347 tests**, with one
existing timing probe ignored. Shipping passes **1,480 editor unit + 3 architecture
tests**, with ten existing ignores unchanged. Strict all-target Clippy passes on
both engines, as do workspace all-target checking with `aestra-bench/gpu`, both
formatting checks and whitespace validation.

Accepted final frozen headless run (2026-10-13):
`target/bevy-020-qualification/runs/40f3e2ac5a0f4ff9832dad400b082945/`.
It executes **766 checks**, including **347 graph/session/history checks**, with
all **725 input hashes** unchanged. Counts are executions across gates, not unique
tests. The graph/session/history test process takes 9.05 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false. Root manifest/toolchain/lockfile diffs remain empty.
Existing Windows incremental-cache hard-link fallback warnings do not fail the gates.
