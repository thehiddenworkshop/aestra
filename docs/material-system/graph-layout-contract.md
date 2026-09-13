# Material/function graph layout contract — M0 audit

Date: 2026-09-13. Scope: the editor's current graph adapters, shared Feathers widget,
project-local layout metadata, and presentation/history boundary.

This is the baseline for the [layout roadmap](../new/AESTRA_MATERIAL_GRAPH_LAYOUT_ROADMAP.md).
M0 adds regression tests and this audit only. It does not add a layout engine, move
nodes automatically, change the persistence format, or implement the gaps below.

## Current implementation

| Concern | Material graph | Function graph |
| --- | --- | --- |
| Adapter | `apps/aestra-editor/src/material_graph.rs` | `apps/aestra-editor/src/material_function_editor/graph.rs` |
| Document memory key | `material:{MaterialProgramId}` | `function:{catalog.root().display()}:{MaterialFunctionId}` |
| Expression node key | `expression:{MaterialExpressionId}` | `{MaterialExpressionId}` |
| Output node key | `output` | `outputs`; each output socket has a `MaterialFunctionOutputId` |
| Signature inputs | Semantic expression/input ports | Function-input expressions retain expression IDs; labels resolve through signature input IDs |
| Camera key | `material:{id}#view:{EditorViewId}` for document views; document key for the tool panel | Document key, without an independent view suffix |
| Shared node widget | `spawn_graph_node`, `FeathersGraphNode`, `GraphViewportMemory` | Same widget and memory |
| Saved metadata | Positions, collapse, output position/collapse, visible previews, one document camera | Session memory only; no function entry in `ProjectEditorLayout` |

Material `MaterialGraphLayout` is a bootstrap result containing an expression-position
map, output position and estimated canvas size. `layout_graph` groups expressions by
dependency depth, places columns 282 logical units apart and stacks rows using
`node_height` plus a 22-unit gap. Sources precede consumers; outputs occupy the next
column. Inline constants are semantic inputs displayed on a socket, not independently
placed nodes. Ordering follows the projection within each column, not a global optimizer.
The depth walk has a visitation guard, not a replacement for semantic validation.

Preview estimates add `MATERIAL_PREVIEW_LAYOUT_HEIGHT`; disabled/unreachable state
adds a status row. These are estimates, not a post-layout geometry snapshot. Function
bootstrap also uses dependency depth and 282-unit columns, but separate row/height
estimates. Custom WESL functions bypass the graph-body adapter and use a source editor.
They must not be mistaken for graph-body measurement coverage.

Saved/manual node positions override bootstrap positions. The shared widget restores
position and collapsed body state on entity creation, then propagates memory changes
to other entities with the same document/node keys. Rebuilding UI is not a semantic
graph edit. Drag uses the zoom of the node's ancestor viewport, rather than looking up
the node's document key as a view key.

`GraphViewportMemory` currently stores one `(position, collapsed)` per document/node
and one `(pan, zoom)` per camera key. There is no base/effective split, movement reason,
pin, guard revision, measured-size registry or presentation transaction.

## Units and measurement contract

The target layout unit is **unzoomed logical graph units**. Positions in `Node.left/top`
and persisted metadata must remain in these units, including at non-100% DPI.

- Bevy 0.19.1 `ComputedNode.size()` is physical layout pixels. Convert measured size
  using that node's `inverse_scale_factor` before combining it with graph positions.
- Canvas `UiTransform` zoom is separate from layout size. Do not divide an untransformed
  `ComputedNode` size by zoom a second time.
- Pointer drag currently converts physical delta by `inverse_scale_factor / zoom`.
  The regression test exercises the actual observer at three DPI scales and three zooms.
- Screen-space socket/rectangle measurements must explicitly remove viewport pan,
  zoom and UI scale as appropriate to their source. Never mix them with layout sizes.
- M1 collects after Bevy layout. Zero/non-finite/unmounted measurements are unavailable,
  not authoritative zero-sized rectangles. Generation/revision tags must prevent stale
  entities or asynchronous results from replacing current geometry.

## Ownership and persistence contract

The future shared key is project identity + typed graph document identity + typed node
identity. Persistent identity cannot use a Bevy `Entity`, tab title, display name or
transient view ID. Material/function output nodes require distinct stable variants;
signature labels may change without losing placement.

Geometry and cameras belong to a view; base placement, collapse and preview intentions
belong to a document. Multiple views must not race to drive one shared placement:

1. Use the focused visible view of that document when its measurement is valid.
2. Otherwise retain a valid visible owner; if none exists, choose a stable fallback.
3. With no valid owner, defer geometry-dependent work. Remove snapshots on view close,
   node removal, document generation change and project switch.

Owner selection must not itself move nodes. It must have material/function parity and
avoid hidden dock-tab measurements. Camera persistence needs an equally explicit owner;
iteration order through ECS queries is not a focus policy.

Current `.aestra/editor-layout.ron` is optional version-1 metadata, separate from
semantic assets. The project crate writes through a temporary file, defaults missing
optional fields, normalizes invalid geometry, and rejects newer versions on load.
Two project roots can store different layouts for identical material IDs. Editor
integration restores on startup and saves material memory through a debounce/exit flush.
Expression metadata and preview IDs are pruned against the current semantic expressions.

Before displacement is enabled (M3), persistence must distinguish:

- **Base position:** user-authored placement, committed layout actions and insertion.
- **Effective position:** base plus deterministic session-only displacement.
- **View geometry:** transient measurement, never serialized as authoritative placement.

Saving, rebuilding or reopening must not promote a temporary offset to the base. Old
position-only files migrate as base positions. Failed/newer-version metadata must not
silently be replaced on a later save. Project switches must not reuse another project's
same-ID graph memory or write into its metadata file.

## Undo and movement contract

Current node movement writes presentation memory directly; it does not edit the effect,
increment its document revision or create a semantic Undo entry. That is tested, but
**does not mean node movement is currently undoable**. `history.rs` has effect/material
history, separate function history lives in `material_function_editor.rs`, and asset
deletion ordering uses `history/asset_order.rs`. `editor_view.rs` supplies active
document/view context; there is no layout transaction integrated with these paths yet.

M3 must route presentation actions through the focused document's chronological history,
preserving text-input Undo and existing asset recovery ordering. A semantic insertion
and its initial placement are one compound action. Drag commits once on release;
measurement, pan/zoom, rebuild and derived displacement add no history entries. Undo
must not accidentally address whichever graph happens to be rendered last.

For M4–M5, overlapping preview/resize causes must compose deterministically. Recompute
remaining causes on close/Undo; do not restore stale absolute snapshots. Revision guards
protect a manually moved neighbor from an old restoration. Frozen context nodes, the
cause node and explicit pinned nodes are hard constraints. Bound the affected set and
solver work; stage and validate a complete candidate before applying it. On stale data,
conflict or exhausted budget, keep current positions and report an explicit non-moving
fallback. No partial movement and no unrequested whole-graph arrangement.

## Gaps and milestone owners

| ID | Observed gap | Owner / acceptance requirement |
| --- | --- | --- |
| GL01 | No post-layout geometry registry. `graph_node_bounds` adds physical `ComputedNode.size()` to logical positions. Selection/bootstrap bounds also use estimates. | M1 collects logical geometry; M2 uses it consistently for selection/framing. Test DPI, collapse and preview sizing. |
| GL02 | Shared bounds match node document keys against viewport keys; material document views use different keys. Multiple views have no measurement authority. | M1 adds explicit document/view adapters and snapshot lifecycle; M2 tests the correct view, not fallback or another view's bounds. |
| GL03 | Function viewport key lacks `EditorViewId`; material camera mirroring loops over all views without focus selection. | M1 makes view identity/authority explicit for both adapters; M3 persists the chosen camera. Test two simultaneously visible views. |
| GL04 | Material memory keys omit project identity; function keys contain a displayed path string. Layout loading is registered at Startup and merges into memory; the persistence resource retains its root. | M1 scopes registry identities; M3 tests project switching, identical IDs, memory cleanup and correct save root. Disk-file isolation alone is not editor-session isolation. |
| GL05 | Only material layout has a disk schema. Current memory is serialized directly and cannot distinguish derived displacement from base. | M3 adds backward-compatible material/function base persistence, output/signature identity and stale-key handling. |
| GL06 | Project loader rejects future versions, but editor load failure falls back to an empty document that later saves could overwrite. | M3 preserves unsupported/corrupt metadata until explicit recovery; test through the editor save path. Current loader test only guarantees that reading leaves bytes untouched. |
| GL07 | No presentation Undo or movement guards; function, material and asset history are separate existing routes. | M3 implements focused chronological routing and compound insertion before any automatic movement. |
| GL08 | No composable offsets, constraint solver or pin controller exists. | M3 establishes data/history boundaries; M4–M5 implement bounded displacement and guarded restoration. Explicit pin UI remains M12. |

## Regression coverage

New or strengthened M0 tests:

- `feathers/node_graph/layout_contract_tests.rs`: material/function key and output-node
  restoration across entity rebuilds; collapsed body visibility; independent material
  view cameras; real pointer drag across DPI/zoom, preserving effect/revision/history.
- `material_function_editor/graph.rs`:
  `function_graph_rebuild_uses_expression_ids_for_manual_placement_and_output_ids_for_sockets`
  spawns the actual Dissolve Edge graph twice, verifies saved expression placement,
  shared widget/output sockets and unchanged function/effect content.
- `material_graph.rs`: deterministic depth-column bootstrap with source/consumer/output
  ordering; the existing semantic-isolation round trip now passes through disk and
  verifies output placement as well as node/camera/collapse/preview restoration.
- `crates/aestra-project/src/editor_layout.rs`: same-ID cross-project file isolation,
  omitted legacy fields, and byte preservation when rejecting a future layout version.

Retained coverage includes preview-height estimates, shared-node synchronization,
cursor-centered zoom, canvas/wire projection, graph depth, per-view material keys,
single-view camera mirroring, invalid geometry and stale-expression pruning. These
tests characterize current behavior, not the missing guarantees in GL01–GL08.

Validation completed with `cargo +1.98.1-x86_64-pc-windows-msvc`:

- `test -p aestra-editor --bin aestra-editor -- --quiet`: 839 passed, 6 existing GPU
  tests ignored, no failures.
- `test -p aestra-project editor_layout -- --quiet`: 7 layout tests passed.
- `test -p aestra-editor --test architecture -- --quiet`: 1 passed.
- `clippy -p aestra-editor -p aestra-project --all-targets -- -D warnings`: passed.
- `fmt --all -- --check` and `git diff --check`: passed.

Native-window visual acceptance is not claimed by headless ECS tests; exercise it when
M1/M2 introduce live measurement and interactive bounds. **Next: M1 — Live Graph
Geometry Registry**, without automatic movement.
