# B20-2 slice 50: timeline toolbar and selection-combo composition

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now builds
the production timeline toolbar: time label, frame-step buttons, view/marker/event
actions, region split/merge tools, snap selector and duration controls.

- `timeline/toolbar_composition.rs` shares the original toolbar expression and
  three existing control/heading helpers. Layout, labels, SVGs, action payloads
  and Feathers activation classification stay intact. Native remaining panel
  construction calls these production helpers through the same parent names.
- `feathers/combo_box/selection.rs` shares the actual generic combo option type
  and two selection constructors. The native combo module reexports its existing
  API and still uses the same option builder for its other action menus. Existing
  searchable-menu tests and remaining menu behavior stay separate and unchanged.
- `shell/action.rs` shares the actual two-variant EditorAction enum used by the
  combo's default generic parameter. The shell reexports the same identity; no
  synthetic default action is introduced. Native shell plugin wiring is unchanged.
- `selection_input_019.rs` retains the original `Interaction::None` initialization.
  The isolated 0.20 boundary initializes `Hovered(false)` instead of inserting a
  deprecated compatibility component. The real Feathers scene and existing
  Pressed/Activate bridge continue to own input; no alternate dispatch is added.
- The region-tool marker moves unchanged into shared interaction state. Native
  timeline plugin wiring and complete existing root tests compare unchanged.

Audits compare the toolbar expression, three helpers, both combo constructors
(accounting explicitly for the idle-state boundary), ComboOption and EditorAction,
native timeline/shell plugin wiring and the existing timeline/searchable-menu tests.
Eight remaining native combo/menu helper bodies also compare unchanged.
Root dependency versions, features, toolchain and authored formats are unchanged.
The former static time-label placeholder and English region-tool descriptions
remain unchanged; this slice does not bundle copy/art-direction changes.

## Regression coverage

Twenty new shared toolbar tests use real controls and production timeline,
transport-plugin and global-history consumers:

- Retained toolbar/control/icon identity, geometry, action classification, both
  locales, snap modes and selected indicators, focusable option metadata and the
  actual menu trigger/description.
- Single-consumption snap/view actions without semantic edits; marker/event
  creation at the playhead, duration edits and region split/merge with global
  undo/redo. Invalid region operations preserve the document/history.
- Actual frame-step transport buttons, zero-frame clamping, no semantic stepping
  edits, constructed time-label updates and duration/heading content.
- Consumed Activate does not replay on idle updates; existing loaded icon handles
  and control entities remain stable.

Three new shared combo tests verify the public reexport/default real action
identity, actual scene/menu-option classification and selection indicator, and
empty options without dummy actions. Each engine also runs one new idle-state
boundary contract test (two engine-specific definitions, not one shared test).

Fixtures use typed Activate events and actual consumers, not replacement action
handlers. They do not qualify native pointer/keyboard input, menu focus/lifecycle,
layout/clamping/rendering, floating windows or full timeline/Curves composition.
The native per-frame region-selection dismissal policy is not newly qualified
here; the marker identity it queries is preserved. Prior interaction-hardening
follow-ups and complete host shell/action-menu integration remain open.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/bc563f61b0c245d89d2ff43b045a2720/`.
It executes **1,429 checks**, including **1,009 graph/session/history/consumer
checks**, with all **857 input hashes** unchanged. Counts are executions across
gates, not unique tests. One existing graph timing probe stays ignored; the graph
process takes 25.29 seconds after compilation. Nine paths join the inventory:
seven new source files and the existing native combo/shell wrappers.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`,
and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **2,040 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. Root dependency/toolchain diffs remain empty. Existing
Windows incremental-cache hard-link fallback warnings are non-fatal.

Next: remaining track construction and full timeline/Curves panel/plugin
integration, then viewport/application consumers and native workflow acceptance.
B20-2 remains open; shipping stays on 0.19.1 until the complete compatibility gate.
