# Material/function graph layout contract — M0 audit

Date: 2026-09-13. Scope: the editor's current graph adapters, shared Feathers widget,
project-local layout metadata, and presentation/history boundary.

This is the baseline for the [layout roadmap](../new/AESTRA_MATERIAL_GRAPH_LAYOUT_ROADMAP.md).
M0 adds regression tests and this audit only. It does not add a layout engine, move
nodes automatically, change the persistence format, or implement the gaps below.

## M0 implementation baseline

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
M1/M2 introduce live measurement and interactive bounds.

## M1 implementation — Live Graph Geometry Registry

The baseline audit above describes the state before M1. The shared widget now registers
an observational collector in `feathers/node_graph/geometry`, ordered after
`UiSystems::PostLayout`. Both material and function adapters provide typed document,
node and socket markers. Function rendering uses the requested view's editing target,
not the unrelated active session target.

- `GraphDocumentKey` combines the catalog's project root with `DocumentKey`.
  `GraphViewKey` adds the editor-view ID (or the graph tool panel). Expressions and
  synthetic material/function outputs have separate `GraphNodeKey` variants.
- Node sizes are physical `ComputedNode.size()` multiplied by inverse UI scale.
  Port centers are transformed back through the node's inverse global transform,
  shifted from center to top-left, then normalized by inverse UI scale. They are not
  divided by canvas zoom twice.
- Each visible view gets an independent snapshot after two stable PostUpdate samples.
  A 0.5-logical-unit tolerance absorbs rounding; the comparison anchor is retained so
  cumulative drift cannot hide forever. An incomplete view exposes no usable snapshot.
- Focused valid view wins; otherwise a valid previous owner is retained, then a stable
  view-key fallback is used. A newly measured/reopened document or owner handoff sets
  a baseline without insertion/resize requests. Hidden/closed/project-mismatched views
  are removed; unavailable measurements do not count as deleted semantic nodes.
- Changes are coalesced into moved, resized, inserted and removed observations for
  consumption in the following Update. Snapshots/events carry a monotonic geometry
  revision and open-document generation. Preview and collapse flags classify resize
  observations; UI rebuild/DPI changes alone do not manufacture preview actions.
- No collector writes graph memory, semantic assets, persistence or Undo. No solver
  consumes events yet, so no automatic movement can occur. Explicit resize-cause
  journals and layout transactions still belong to M3–M5.

This resolves measurement/identity collection in GL01–GL04, not every item in those
rows. M2 still needs to replace estimated/mixed-unit interactive bounds. M3 still owns
project-scoped placement memory, camera persistence/independent function cameras,
function layout serialization, unsupported-file protection and presentation Undo.

M1 regression coverage includes:

- Real Bevy UI layout at 100%, 125% and 200% DPI, each with 0.5, 1.0 and 1.75 canvas
  zoom: normalized node/socket measurements, preview growth/shrinkage once per toggle,
  unchanged placement memory and cleanup when hidden.
- The actual material canvas rebuilding for preview open/close: one correctly classified
  resize, stable positions and unchanged effect content.
- The actual function canvas: measured expression/output nodes and signature ports,
  stable manual positions, explicit non-active document target, and separate view keys.
- Registry ownership/fallback, document generation changes, same-ID project isolation,
  insertion/removal vs missing measurements, collapse/move detection, transient sizes,
  rounding noise, and invalid/non-finite measurement rejection.

Validation: **847 editor tests passed, 6 existing GPU tests ignored**; the architecture
test, strict editor Clippy (`--all-targets -- -D warnings`), workspace formatting and
`git diff --check` pass. Native-window visual acceptance remains separate from headless
layout/ECS coverage. Next is
**M2 — Use measured geometry for interactive bounds**; M1 does not change framing or
enable automatic node movement.

## M2 implementation — Measured interactive bounds

The shared framing path now unions `GraphGeometrySnapshot` node rectangles from the
exact mounted `GraphViewKey` and viewport entity, rather than matching document memory
strings against camera strings or adding physical dimensions to logical positions.
Both the node bounds and viewport size use logical units. Cursor-centered wheel zoom
also normalizes the viewport size by inverse UI scale.

- Frame All uses every measured node, including synthetic material/function outputs.
  Frame Selection uses selected nodes under that viewport only; an empty selection
  frames its measured whole graph. Function graphs use the same widget behavior, but
  this milestone does not add a function-node selection UI.
- Material selection height estimates have been removed. Preview expansion, diagnostic
  content, collapsed bodies and manually moved nodes are included by measurement.
  Depth/height estimates remain initial placement inputs, not live interaction bounds.
- Function camera/toolbar keys now include the editor-view ID, while node placement
  remains document-scoped. This small part of GL03 is brought forward so Frame All in
  one function view cannot also frame its sibling. Camera disk persistence and focused
  camera-save ownership still belong to M3.
- New, hidden, resizing and rebuilt graph views retain their frame request until their
  own stable, complete snapshot exists. Other views and old same-key entities cannot
  supply a substitute. Empty graphs/unadapted widget clients retain bootstrap bounds.
- Post-layout framing stages only a camera result. The next Update applies it with the
  canvas transform, before UI layout and wire/grid rendering. Wheel/pan navigation can
  cancel a pending frame. There is no node movement, semantic edit, new layout history
  entry or persisted measured size. Existing zoom limits are unchanged.

Regression coverage uses real Bevy layout for both typed graph kinds at 100%, 125% and
200% DPI, each at 0.5, 1.0 and 1.75 initial zoom. It covers All/Selection, negative/manual
positions, expanded content, collapse, separate views, rebuilds, hidden views and empty
selection. Actual material-preview and function-body adapter tests now assert measured
initial framing as well as geometry and semantic isolation. A wheel-navigation system
test checks DPI-normalized anchoring and cancellation of a staged frame.

Validation with `cargo +1.98.1-x86_64-pc-windows-msvc`: **850 editor tests passed,
6 existing GPU tests ignored**; architecture test, strict editor Clippy
(`--all-targets -- -D warnings`), workspace formatting and `git diff --check` pass.

Native-window visual acceptance remains separate from these headless tests. Next is
**M3 — Base placement, persistence and presentation Undo**; GL03–GL08's placement,
camera persistence, project-isolation and history work is not claimed by M2.

## M3a implementation — Base placement and safe function persistence

M3 is split into persistence foundations (M3a), project placement lifecycle (M3b), and
focused presentation history (M3c). This entry does not claim the full M3 gate is done.

- `GraphViewportMemory` keeps authored positions separately from an internal offset
  map. `node()` returns persistent base state; `node_position()` returns effective display
  placement. UI restore/synchronization uses effective positions, while persistence
  reads only base state. Collapse preserves base and offset. A manual placement clears
  the old offset; removal clears both. Offsets require an existing base, reject invalid
  values, replace rather than accumulate, and are never serialized. No production
  caller sets offsets; cause composition and displacement remain M4/M5 work.
- `ProjectEditorLayout` format 2 adds `function_graphs`, reusing the material graph
  metadata structure. Expressions (including signature-input expressions) retain stable
  IDs; the synthetic output node has a separate entry. Position, collapse and camera
  round-trip for graph-bodied functions. Custom WESL stays outside this canvas metadata.
  Function preview fields are currently unused. Version-1 material data reads with an
  empty function map and migrates on the next successful save, not on read.
- Function restore uses the same project-qualified memory key as the canvas. Saving
  visits catalog functions including retained drafts, independent of the active editor
  target; removed expression IDs are pruned from metadata. Semantic function/material
  source and effect state remain unchanged.
- Function next-open cameras mirror only the registry's visible owner (focused, retained,
  stable fallback). Sibling view cameras remain independent. Material camera mirroring
  still has the baseline multi-view limitation and is assigned to M3b.
- Failed/corrupt/newer layout loads latch a write block and retain an error. Debounced
  saves, direct editor saves and exit flush cannot overwrite that file. Editor save also
  rechecks disk readability/version in case it changed since startup. A newer in-memory
  format cannot be silently downgraded by the project writer.
- A catalog-root mismatch blocks automatic persistence and exit flush. This is a safety
  guard, **not hot project-switch support**: reloading/clearing placement and previews for
  a changed project is M3b. Explicit recovery for blocked metadata is also still pending;
  repairing the file and restarting the editor lets it load normally.

Tests cover base/effective separation through UI rebuild/collapse and material/function
disk round-trips, the real function canvas after restart, inactive-function persistence,
signature-label stability, stale expression pruning, focused/hidden camera owners,
version-1 migration, and byte preservation after load errors, external newer metadata,
root changes and exit flush. Native-window acceptance remains separate.

Validation: **856 editor tests passed, 6 existing GPU tests ignored**; **9 project layout
tests passed**. The architecture test, strict editor/project Clippy (`--all-targets --
-D warnings`), workspace formatting and `git diff --check` pass with
`cargo +1.98.1-x86_64-pc-windows-msvc`.

Next: **M3b — Project placement lifecycle**, then **M3c — Focused presentation Undo**.
No automatic movement or layout Undo is enabled by M3a.

## M3b implementation — Project placement lifecycle

- A catalog-root change flushes the last captured old-project layout to its own root,
  then clears the material/function memory namespace, including per-view cameras and
  temporary offsets, preview visibility and preview caches. Other graph widgets keep
  their memory. The new project's metadata loads before graph UI rebuild/sync; old
  graph viewport entities and geometry observations are discarded. Same-ID assets in
  another project cannot inherit placements. Failed writes still follow the existing
  error/overwrite protection; this does not create a recovery copy of unsaved layout.
- Persistence visits all resolvable project materials and retained drafts, independent
  of the focused tab. Function persistence retains the same behavior. Source reloads
  preserve base positions for retained expression IDs, prune removed node/preview IDs,
  and clear obsolete temporary offsets when semantic content changes. Confirmed asset
  removal clears its document and view memory plus metadata; closing a tab alone does
  not erase layout. Incomplete/invalid inventories are not evidence of removal.
- Both graph kinds now use the same registry-elected camera owner: focused visible
  view, retained valid owner, then stable fallback. The graph tool panel has its own
  `#tool` camera, separate from the saved document camera and `#view` cameras. All
  views seed from the document camera on first opening; idle siblings do not fight
  for the saved camera. Existing on-disk camera metadata needs no schema migration.
- A blocked metadata load/save displays an in-graph notice (English/French). Repair
  `.aestra/editor-layout.ron`, then choose **Reload saved layout**. The button retries
  reading, never overwrites an unsupported/corrupt file. A failed retry preserves live
  placement; a successful explicit retry replaces unsaved placement with the saved
  layout while leaving material/function/effect semantics untouched. It is not a
  reset-to-default or file-deletion action.

Coverage includes project A/B/A with identical material/function IDs, inactive material
persistence, base-vs-offset round-trip, stale view removal, unrelated-widget preservation,
reload/node removal, invalid inventory vs confirmed asset removal, real multi-view/tool
camera ownership and idle stability, and recovery activation with file-byte/semantic
preservation. Native-window visual acceptance remains separate from headless coverage.

Validation with `cargo +1.98.1-x86_64-pc-windows-msvc`: **860 editor tests passed,
6 existing GPU tests ignored**; **9 project layout tests passed**. Strict editor/project
Clippy (`--all-targets -- -D warnings`), architecture, workspace formatting and
`git diff --check` pass.

Next: **M3c — Focused presentation Undo**. Chronological presentation transactions,
compound semantic/insertion placement, history-generation guards and safe Undo/Redo
are still required before the complete M3 gate passes. No automatic node movement is
enabled by M3b.
