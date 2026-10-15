# B20-2 slice 30: shared clipboard payload and function graph consumers

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate imports the
actual immutable graph clipboard payload, per-view material/function selection,
function graph identity/target activation, and complete function clipboard
execution/keyboard consumer. It uses real session/catalog publication, function
body validation, presentation snapshots and compound undo/redo routing.

- `material_graph/clipboard/model.rs` shares capture, inline-default ownership,
  identity remapping, external-wire validation, placement and shortcut decoding.
  The material adapter retains its existing entry points through re-exports.
- `material_graph/selection.rs` shares the actual scoped selection model, not a
  candidate-specific imitation. Material and function selection remain distinct.
- `material_function_editor/graph/clipboard_context.rs` shares actual view/scope/
  node identities and function target activation with all native graph consumers.
- `material_function_editor/graph/clipboard/execution.rs` shares copy/cut/paste,
  duplicate/delete, insertion, selection and keyboard routing. Shipping calls its
  real registration once, retaining Update scheduling without adding an order
  relative to wire attachment or menu dismissal that did not previously exist.
- Source comparisons confirm the model, selection and execution/keyboard bodies
  are unchanged apart from visibility and imports. No behavior fixes, dependency
  changes, authored-format changes or widget replacement are bundled here.

The two existing pure clipboard regressions move to the shared model test module;
they are not dropped or duplicated. Existing material and function context-menu
tests remain in the shipping suite. A test-only constructor creates actual
FeathersGraphViewport components for headless consumer fixtures; it does not change
production viewport construction or replace the native type.

## Regression coverage

**21 new regressions** (seven payload and fourteen function-consumer cases), plus
the two relocated regressions, form **23 shared clipboard checks** on the candidate.

Payload coverage includes immutable snapshots, disjoint fresh identities, internal
wire remapping, retained selected metadata, visible constant nodes, independent
inline defaults, missing external sources, empty/stale selections, non-finite
positions/offsets, placement overflow and existing Ctrl/shortcut priority policy.

Function consumer fixtures run the actual keyboard registration with real
InputPlugin, InputFocusPlugin, InputDispatchPlugin and EditorKeyboardPlugin.
KeyboardInput messages exercise:

- Same-frame copy/cut/paste with fresh identities and remapped wires; actual
  compound undo/redo restores semantic and presentation state.
- Repeated pastes, idle non-replay, duplicate/delete selection and layout recovery.
- Cursor placement through actual viewport pan/zoom, preserving relative offsets.
- Per-view selection isolation and activation of the hovered function rather than
  editing the previously active target; no hover/despawn prevents editing.
- Logical letters, event-time Ctrl, held/repeated chord suppression, editable/code
  ancestor ownership, document protection and transient function menus.
- Output-protected cut remains atomic but retains a clipboard copy. Foreign
  dependencies reject paste without partial edits or selection changes. Empty
  clipboard/selection creates no undo entry.

The integration fixture uses production Routing for undo/redo. Its filesystem
callback rejects unexpected deletion routing; filesystem recovery is not simulated
as success. Tests use actual component/resource types but synthetic keyboard
messages and explicit computed geometry, not a native event loop or complete
graph-panel layout/rendering.

## Validation

Accepted frozen headless run (2026-10-14):
`target/bevy-020-qualification/runs/f0e6ef6c78bd4e858c1fb56539b2901b/`.
It executes **905 checks**, including **486 graph/session/history/consumer checks**,
with all **761 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 13.69 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false. The final shipping snapshot passes **1,579 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace all-target checking with
`aestra-bench/gpu`, both formatting checks and whitespace validation. Root manifest/
toolchain/lockfile diffs remain empty. Existing Windows incremental-cache hard-link
fallback warnings do not fail these checks.

## Remaining scope

Function clipboard keyboard/execution and shared payload semantics are qualified.
The full material keyboard/selection-edit consumer (including extraction and
connection deletion), material/function menu activation/composition, timeline,
remaining viewport/application execution and full editor workflows remain open.
No new native-window/GPU acceptance is claimed. The engine switch still requires
B20-3; shipping remains on 0.19.1.

Next: the material graph keyboard/selection-edit consumer, then timeline and
remaining application execution/full panel composition.
