# B20-2 slice 27: transport keyboard, actions and playback

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now executes
the actual transport keyboard consumer, action observer and playback advancement,
using the production keyboard plugin, ownership context and EditorSession.

- `transport/playback.rs` contains the unchanged TransportAction/TransportSet
  identities, keyboard handler, repeated-frame-step predicate, action execution
  and playback advancement. Its shared registration installs the real observer
  and systems in their original Input/Playback sets.
- The native EditorTransportPlugin calls that registration and retains its
  toolbar activation, button handling, classification, icon synchronization and
  scene composition. Main application's set ordering and session policy are
  unchanged. Candidate integration tests exercise the same relative transport-set
  order, not a substitute session/action implementation.
- Seven existing transport tests move into the shared source gate with their
  original assertions preserved. Native toolbar/UI regressions remain native.
  Nineteen new shared integration regressions use actual KeyboardInput messages,
  event-time snapshots and real action/session execution.
- No new Bevy-version adapter, dependency, authored-format change or widget
  replacement is needed. Live playback still uses the existing session advancement;
  the compatibility port adds no checkpoint/replay work to that path.

Source comparisons confirm action/keyboard/repeat/advancement bodies are unchanged
apart from internal visibility and formatting. The seven moved tests preserve
their assertions; native toolbar activation/handlers/composition are unchanged.

## Regression coverage

All **26 shared transport tests** cover:

- Same-frame Space/restart/arrow input ordering and exactly-once application.
  Restart follows logical letters rather than physical QWERTY positions.
- Arrow repeats step both directions; Space and restart do not repeat. Fast Alt
  navigation and repeats use each event's modifier state, including modifiers held
  across frames and releasing only one physical Alt side.
- Focus loss drops ambiguous input. Clearing a modal/menu/palette does not replay
  blocked discrete or repeat input; a fresh released-and-pressed chord works.
- Every document-protection branch, actual extraction dialog, all material/function
  menus, all top-level/panel/tab/About menus and module palette block transport.
- Editable/code/menu descendants own input; despawn releases ownership. Assets
  permits global transport except while its search owns input. All-unfocused windows
  block while a focused floating window permits global shortcuts.
- The raw-message fallback works without the snapshot collector and drains blocked
  repeats. Snapshot collection plus the raw reader cannot double-dispatch an event.
- Real stop/restart/frame stepping preserve the seed and authored effect, without
  adding undo steps. Once playback restarts while loop modes resume their position.
- Actual registration advances from Bevy time at session speed. Keyboard pause is
  applied before the Playback set; paused/stopped playback does not advance.
  Once playback stops at its end; continuous playback crosses duration boundaries.
- Modal keyboard ownership does not implicitly pause the already-running preview.
  Existing mode-change undo and playback-position/running-state behavior is retained.

## Remaining scope

This qualifies transport keyboard/action/playback systems, not the complete toolbar
plugin or every keyboard consumer. Toolbar activation, buttons, icons and scene
composition still need their candidate composition gate. Timeline, viewport,
material/function clipboard and other application action handlers remain open, as
do full panel composition and interactive native workflow acceptance.

Next: remaining keyboard/action consumers and transport toolbar composition, then
complete editor workflows. The root engine switch still requires B20-3.

## Validation

Shipping passes **1,521 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, as do workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Existing Windows incremental-cache hard-link fallback warnings do not
fail the gates.

Accepted final frozen headless run (2026-10-13):
`target/bevy-020-qualification/runs/dfc67dd552434ae4802cf26e78966d61/`.
It executes **822 checks**, including **403 graph/session/history checks**, with
all **744 input hashes** unchanged. One existing graph timing probe remains ignored.
Counts are executions across gates, not unique tests. The graph/session/history
test process takes 10.34 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false. Root manifest/toolchain/lockfile diffs remain empty.
