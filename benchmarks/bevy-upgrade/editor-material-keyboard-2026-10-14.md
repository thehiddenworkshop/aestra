# B20-2 slice 31: material graph keyboard and selection-edit consumers

## Scope and implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate imports the
complete production material selection-edit keyboard consumer, clipboard capture/
insertion adapters, selection-edit execution, semantic command preparation and
target activation. It uses actual graph view/node identities, inspector selection,
session/catalog publication, material history and presentation/preview snapshots.

- `material_graph/edit_context.rs` shares the real viewport and node action types.
- `material_graph/keyboard.rs` shares the actual copy/cut/paste, duplicate/delete,
  connection reset and Ctrl+Shift+E extraction consumer. Shipping calls the same
  registration once in Update, without adding new ordering against the remaining
  palette/menu systems.
- `material_graph/execution.rs` shares selection edits, typed tool-command
  preparation, target activation and publication through the real material history.
- `material_graph/clipboard/execution.rs` shares material capture/insertion while
  the native clipboard module retains its existing entry points and tests.
- `properties/material_state.rs` shares the actual inspector selection resource.
- Source audits confirm the keyboard, edit/preparation/activation and clipboard
  function bodies are unchanged apart from visibility/imports. No behavior fix,
  dependency, feature, toolchain or authored-format change is bundled here.

A test-only FeathersGraphNode constructor complements the earlier real viewport
fixture. It creates the actual component type, with no production scene change.
Existing native graph menu, clipboard and extraction regressions remain intact.
The runner adds all new material consumer modules/tests and inspector state to its
frozen input inventory; node-graph sources are already recursively hashed.

## Regression coverage

Fifteen new shared tests run the production keyboard registration with actual
InputPlugin, InputFocusPlugin, InputDispatchPlugin and EditorKeyboardPlugin. They
use real material documents, catalog/drafts, live graph node components, viewport
geometry, scoped selection, inspector, previews and compound history routing.

Coverage includes:

- Ordered same-frame copy/cut/paste, fresh identities, remapped internal wires and
  semantic/presentation undo/redo, with the source file left unchanged on disk.
- Duplicate selection followed by delete, restoring both layout and preview
  visibility on undo.
- Real Ctrl+Shift+E extraction: function publication, call selection/placement,
  reversible material edits and no accidental source-file save. Logical letters and
  event-time modifier release are exercised too.
- Delete on a selected connection creates the correctly typed output default;
  undo restores the original wire and redo restores the disconnected state.
- Pointer paste unprojection through actual pan/zoom; live node positions take
  priority over stale stored positions without changing relative placement.
- Invalid live positions, mismatched tab targets and foreign dependencies reject
  edits without partial semantics, selection or ordered history mutations.
- Hovered-tab activation rather than editing the previously active program;
  per-view selection/hover isolation, logical shortcuts and repeat suppression.
- Editable/code-editor ancestor ownership, material palettes, document protection,
  fresh-chord recovery and empty-selection/payload no-ops.

History uses production Routing; an unexpected filesystem-history callback fails
the fixture. Synthetic keyboard messages and explicit geometry qualify these real
consumers, not native OS input, complete graph-panel layout/rendering or full main
plugin scheduling parity.

## Validation

Accepted frozen headless run (2026-10-14):
`target/bevy-020-qualification/runs/b0d14d1d7492448da9cde8288a11993a/`.
It executes **920 checks**, including **501 graph/session/history/consumer checks**,
with all **767 input hashes** unchanged. One existing graph timing probe remains
ignored. Counts are executions across gates, not unique tests. The graph test
process takes 12.07 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, and unchanged shipping lock
SHA-256 `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
`native_executed` is false. The final shipping snapshot passes **1,594 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace all-target checking with
`aestra-bench/gpu`, both formatting checks and whitespace validation. Root manifest/
toolchain/lockfile diffs remain empty. Existing Windows incremental-cache hard-link
fallback warnings do not fail these checks. All 767 frozen inputs still match after
the validation/documentation work.

## Remaining scope

Material and function clipboard/selection-edit keyboard consumers are now in the
candidate gate. Palette-opening keyboard behavior, palette/menu action handling
and full graph-panel composition remain separate, as do timeline consumers,
remaining viewport/application execution and full editor workflow acceptance.
No new native-window/GPU acceptance is claimed; the engine switch still requires
B20-3.

Next: timeline keyboard/action consumers, followed by remaining application
handlers, graph menu/palette composition and complete editor workflows.
