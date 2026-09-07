# Bevy 0.19.1 keyboard dispatch compatibility patch

This directory vendors the published `bevy_input_focus` 0.19.1 crate, retaining its
MIT/Apache-2.0 licenses. Original crates.io checksum:
`c4d5573eb503387f73d12acde3010d49c808c790d34691e2fb410fc54b8c72b9`.

## Reason

The default dispatcher queues an entire frame of keyboard events after `InputSystems`.
Feather's stock editable-text observer reads logical modifiers from `ButtonInput<Key>`.
A Ctrl-down/A-down/A-up/Ctrl-up batch therefore inserts `a` instead of selecting all.
Using `just_released(Control)` as a substitute also misclassifies ordinary characters
before/after the chord in the same frame.

## Local changes

- `src/keyboard_dispatch.rs` captures pre-frame key state, replays each keyboard event
  in order, and supplies that event's physical/logical state while synchronous focused
  observers run. It restores the normal frame state afterward. Stock Feather editing,
  clipboard commands, IME handling and tab-navigation observers remain in use.
- `InputDispatchPlugin` installs this keyboard dispatcher. Mouse/gamepad and the public
  generic `dispatch_focused_input` are unchanged. Callers manually installing the generic
  keyboard dispatcher do not receive this fix; the editor uses `InputDispatchPlugin`.
- `KeyboardInputSnapshot` provides deferred shortcut consumers with per-event physical
  key state. The editor uses this for history, document, graph, timeline and viewport
  shortcuts, excluding repeat events except for intentional transport frame stepping.
- Physical left/right modifiers are combined correctly for focused observers. Ambiguous
  keyboard events in a frame with `KeyboardFocusLost` are dropped, and logical held
  keys are released too, preventing stale commands and stuck Ctrl state after focus loss.

No registry files are modified and no new external package is required. This is a
temporary compatibility patch, not an independent fork of widget behavior. On a Bevy
upgrade, verify equivalent event-time modifier behavior with the editor regressions,
then remove the patch, this directory and the snapshot adapter together.

Regression coverage lives in `apps/aestra-editor/src/input/tests.rs` and the editor's
history/transport tests. The first Ctrl+A/Ctrl+V test was observed failing against the
unpatched crate with `Insert("a"), Insert("v")`. Clipboard command mapping is tested
without reading or writing the user's OS clipboard.
