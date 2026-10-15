# B20-2 slice 29: discrete viewport shortcut consumers

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now imports
the actual production viewport keyboard system, its registration, canvas marker,
action identities and system-set identities. The shipping plugin calls that same
registration in `ViewportSet::Input`; it does not register a second shortcut system.

- `viewport/keyboard.rs` retains the complete discrete shortcut consumer. A source
  comparison against the previous commit confirms its signature and body are
  unchanged except for module visibility.
- `viewport/shortcuts.rs` owns the real `ViewportAction`, `PreviewCanvas`,
  `PreviewDisplayMode` and `ViewportSet` types, re-exported at their existing paths.
  Actions gain equality derives for regression assertions, with no payload changes.
- The candidate uses Bevy's actual `TransformGizmoMode`, not a substitute enum.
  No engine dependency, feature, toolchain, authored-format or lockfile changes
  are needed for this slice.
- The qualification runner hashes both extracted modules, their shared tests and
  the original viewport plugin, in addition to existing keyboard/ownership inputs.

Grid and maximize stay editor-wide when shortcuts are not blocked. Frame and gizmo
modes require a hovered actual preview canvas. Existing modifier policy is retained:
Ctrl blocks G, not F/Home, F11 or 1/2/3; this slice does not redesign keybindings.
Logical letters follow the OS layout while physical number-row keys remain usable
on AZERTY. No persistent viewport navigation hint is added.

## Regression coverage

Eighteen new shared tests execute the same production registration and keyboard
system on both engines. Most use real InputPlugin, InputFocusPlugin,
InputDispatchPlugin and EditorKeyboardPlugin with actual KeyboardInput messages.
The fallback test deliberately injects ButtonInput without the keyboard plugin.
An action observer records actual ViewportAction events; it is not a rendered
viewport execution substitute.

Coverage includes:

- Global versus canvas-local ownership, both framing keys and all three native
  transform-gizmo modes, event ordering and multiple taps within one frame.
- Logical/non-QWERTY letters, punctuation, AZERTY number-row behavior, no numpad
  remapping, event-time left/right Ctrl and the existing modifier policy.
- Held keys and OS-repeat suppression, no stale replay on idle frames or after
  closing a modal, and direct ButtonInput fallback with no canvas.
- Actual editable/code-buffer ancestor focus, module palette, all document
  protection branches, asset extraction, material/function transient menus,
  top-level/panel/tab-context/About menus and focused menu items.
- Lost window focus and recovery via another focused window, unrelated hovered
  entities, multiple canvases without duplicate dispatch, hover loss and despawn.
- WASD remaining in the held-key resource without dispatching discrete actions;
  this does not claim qualification of the camera-flight/navigation system itself.

Tests are headless and use synthetic native-type keyboard messages. They do not
create an OS event loop or claim native-window/GPU/complete-main scheduling parity.

## Validation

Accepted frozen headless run (2026-10-14):
`target/bevy-020-qualification/runs/923829019a364a8a874b358625919ced/`.
It executes **882 checks**, including **463 graph/session/history/consumer checks**,
with all **751 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 12.76 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false. Strict all-target Clippy passes on both engines, as do
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. The full shipping snapshot passes **1,558 editor unit + 3
architecture tests**, with ten existing ignores unchanged, including all 18 new
shortcut tests.

An initial sandboxed full run encountered access-denied errors in existing
filesystem fixtures. The accepted full rerun used normal filesystem access; no
tests were disabled or weakened. Windows incremental-cache hard-link fallback
warnings are harmless. Root manifest/toolchain/lockfile diffs remain empty.

## Remaining scope

Discrete viewport shortcut dispatch is now qualified. Rendered viewport action
execution/persistence, toolbar activation, continuous camera navigation, pointer
and transform-gizmo consumers, timeline and material/function clipboard consumers
remain open, along with full panel/main-plugin and interactive workflow acceptance.
The engine switch still requires B20-3.

Next: timeline and material/function clipboard keyboard/action consumers, then
remaining viewport/application execution and full editor workflows.
