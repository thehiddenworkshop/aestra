# Aestra candidate patch for bevy_input_focus 0.20.0

This is the actual published 0.20.0 crate, not relabelled 0.19 source. It is used
only by `benchmarks/bevy-upgrade/patches-020`; the shipping workspace still selects
`vendor/bevy_input_focus` 0.19.1 until the renderer/editor migration is ready.
`UPSTREAM_SHA256.json` records the registry archive checksum and original file
hashes. Licenses and upstream behavior outside keyboard dispatch are retained.

Intentional changes: `src/lib.rs`, added `src/keyboard_dispatch.rs`. The latter
ports the existing event-time modifier fix and `KeyboardInputSnapshot` API:

- Capture frame-start physical/logical input before `InputSystems`.
- Dispatch each focused keyboard event with that event's modifiers, then restore
  frame-end `ButtonInput` resources. Preserve text, repeat, physical and logical keys.
- Combine left/right modifier keys; drop ambiguous batches on keyboard focus loss.
- Deferred editor shortcuts receive event-time snapshots; missing focus targets
  fall back to the primary window. Other focused-input message types stay upstream.

The existing negative qualification test still fails on unpatched 0.20.0. The
patched fixture must pass both of those exact assertions plus tests using the real
0.20 `TextInputPlugin`/`TextInput`/`EditableText`. It queues `Paste` edits without
accessing the OS clipboard. This does not yet qualify the whole migrated editor.

Root Cargo patches do not propagate to downstream consumers. Do not publish or
remove this fix without deciding how dependent hosts receive equivalent behavior.
