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

## M3c implementation — Focused presentation history

- The existing session edit-order journal now carries presentation transactions and
  optional placement deltas on semantic actions. There is no second user-facing history
  stack or separate layout Undo button. Shared program/function tabs use their existing
  project/document context; embedded material edits keep the effect-history route.
  Neutral panels preserve focus, code editors retain text Undo, and the asset-deletion
  ordering bridge continues to sequence document actions and filesystem recovery.
- The generic graph widget emits one completed drag transaction, not one per pointer
  motion. It records the previous **base** position even when displayed with an offset.
  Collapse emits the same shared node-presentation event. Individual and all-node material
  preview toggles each record one visibility action. Camera pan/zoom/framing, passive UI
  rebuilds and sibling-view synchronization create no history items.
- Material add/duplicate/extract/delete and function body/signature/drop actions capture
  the previous placement and attach the resulting delta to the successful semantic entry.
  Insertion placement, connection changes and removed-node placement therefore undo/redo
  together. Material node deletion also restores removed preview visibility. Invalid
  insertion positions are rejected before semantic mutation. No-op function commands
  cannot overwrite the preceding transaction. Replay validates first, executes semantic
  history when needed, and only applies the presentation delta on success.
- Transactions identify the project root/generation, graph and stable node keys. They
  check the expected semantic fingerprint, base placement, collapse and relevant preview
  visibility before applying; unknown/removed sources and stale state fail without
  mutating either side. Explicit saved-layout reload and confirmed source removal
  invalidate old layout entries. Source discard/reload clears only its document order.
- Layout-only actions do not invoke authoring commands, compile shaders, mark material
  drafts dirty or change content revisions. They update shared base memory and request
  a UI refresh. Per-view cameras remain independent, and existing M3a/M3b persistence
  serializes the restored base/collapse/visibility state without a schema change.
  Undo clears derived offsets instead of replaying stale displacement. There is still no
  production offset solver: M4 supplies bounded candidates; M5 supplies reversible causes.

Regression coverage includes mixed semantic/move/collapse/preview order, material and
function context isolation, redo branching, compound insertion/deletion, deleted preview
restoration, no-op attachment, stale source/project/reload protection, and real shared
widget drag coalescing with base-vs-effective state and sibling-view restoration.
The full editor suite also exercises existing code-editor and asset-deletion history.

Validation with `cargo +1.98.1-x86_64-pc-windows-msvc`: **869 editor tests passed,
6 existing GPU tests ignored**; **9 project layout tests** and the **architecture test**
passed. Strict editor/project Clippy (`--all-targets -- -D warnings`), workspace
formatting and `git diff --check` pass.

Native acceptance (not claimed by headless tests): in material and function canvases,
move and collapse a node, Undo/Redo, then create/delete and Undo/Redo. In a material
canvas, also toggle an individual preview and all previews between shader edits. Repeat
in two views: positions must agree while cameras stay independent; layout-only actions
must leave material/effect dirty indicators unchanged. Verify save/reopen placement.

Next: **M4 — Local resize collision resolver**, internal/test-only until M5 makes its
temporary displacement reversible. No automatic layout or new Arrange action is enabled.

## M4 implementation — Internal bounded resize solver

The shared graph widget now has a pure `resize` planner, compiled **only for tests**.
It has no ECS systems, material/function semantics, history or persistence dependencies.
There is no live preview pushing, new Arrange action, or layout file/schema change.

- Inputs use stable ordered node keys, an adapter-supplied project/document/view identity,
  generation/topology/geometry/placement revisions, effective logical positions, measured
  positive sizes and frozen flags. Explicit resize roots include previous sizes; the
  snapshot holds current sizes. Missing/invalid measurements reject the request.
- All resize roots, including coalesced shrinking roots, remain anchored. Frozen nodes
  include pins, current drags and protected manual placements. Width growth pushes right;
  height growth pushes down; growth on both axes chooses the smaller separation (right
  wins ties). Cascades inherit that direction. A movable node can clear a frozen obstacle
  along its push axis; two conflicting hard anchors fail. This monotonic heuristic may
  reject a scene another algorithm could solve; it never silently relaxes constraints.
- Only growing roots seed work. Shrinking/no-change/sub-tolerance requests do not tidy
  manual overlaps. Cascades inspect changed nodes against all obstacles, leaving unrelated
  overlapping pairs untouched. BTree key order makes candidates and failures independent
  of container/ECS insertion order. Spacing is 22 logical units on each axis; separation
  tolerance is 0.01 logical units (not the geometry collector's stability tolerance).
- Default limits are 4,096 input nodes, 256 translations, 64 affected nodes, 200,000 pair
  checks, 2,048 logical units per-node L1 displacement, and 8,192 total L1 displacement.
  Failures return a structured conflict and operation counts, never partial candidates.
  Pair-check bounds include the final affected-region constraint scan. The maximum
  affected count intentionally rejects long cascades rather than rearranging a graph.
- Candidates keep private validated positions and the exact input. Atomic application
  affects only a detached effective snapshot; no base-memory API is called. It rejects
  changed identity, any revision, node membership, positions, sizes or frozen flags before
  any write, even if an adapter failed to increment a revision. Placement-generation
  overflow also rejects without mutation. No-op application does not bump revisions.

Regression cases cover width/height/two-axis resize, multi-root anchoring, shrinking,
fractional/negative coordinates, intentional unrelated overlap, frozen obstacles, every
budget, invalid measurements, stale inputs, deterministic ordering and a seeded dense
stress corpus. The benchmark fixture lives in `benchmarks/graph-layout/` and is included
by the private solver test module, like the Asset Browser benchmark. Operation-count
gates run normally; wall-clock profiling is opt-in and does not assert flaky time limits.

Validation: **882 editor tests passed**, with 6 existing GPU tests and the opt-in
timing probe ignored in the normal run. The timing probe was run separately and passed:
25/50-node cascades averaged 0.105/0.874 ms; 100–500-node full-row cascades stopped at
200,000 pair checks in about 3.2 ms with no candidate. See the recorded benchmark for
scope and toolchain. The architecture test, strict editor Clippy (`--all-targets --
-D warnings`), workspace formatting and whitespace checks pass.

Next: **M5 — Reversible preview displacement**. Adapt the authoritative stable geometry
and explicit resize causes, compose temporary offsets, validate against current manual
placements/history and remaining causes, restore safely, and surface compact conflicts.
Native M3 Undo/Redo acceptance and the combined M4/M5 DPI/multi-view/restart gates remain
required before enabling automatic movement. These pure tests do not claim native UI QA.

## M5 implementation — Reversible session overlays

- M4's pure planner is now consumed by the shared graph widget. The geometry collector
  also measures compact node bounds by removing the actual preview block height and its
  logical margins, so already-visible previews reconstruct after restart without relying
  on estimated material node heights. Collapsed bodies have no visible preview extent.
- `GraphWidgetSync` restores nodes, reconciles the last stable owner snapshot, and then
  synchronizes effective positions to all views before UI layout. Both material and
  function wires explicitly update after UI PostLayout. Stale/missing/rebuilt geometry
  waits; active dragging suspends reconciliation for that document. Owner/DPI-only
  remeasurement, camera navigation and passive rebuilds do not create a resize cause.
- Each document has a session-only composition model. It starts from bases and compact
  measured sizes, grows active causes in stable node-key order, and anchors each root at
  its already-composed position. Earlier causes may have displaced a later root. Closing
  either of two previews recomputes the remaining composition; it never subtracts an old
  delta. Iteration/pair-check and aggregate movement/affected-node budgets apply across
  all causes, not independently with unlimited work per preview.
- Initial ordinary body measurements are baselines, not a request to tidy a manual
  graph. A measured collapse followed by expansion establishes a reversible body cause;
  genuine stable content growth is also local. Preview causes are reconstructed from
  measured compact bounds and saved visibility. Body/content baselines are session-only;
  ordinary already-expanded bodies are not treated as new expansions on first opening.
- Placement revisions protect manual moves for the active cause period. The displayed
  drop position becomes the new authored base, removes that node's temporary offset, and
  older causes cannot move it again. Protection is not persistent pinning. History clears
  offsets through an epoch boundary and recomputes them against restored bases/visibility;
  this adds no second Undo entry and does not execute a semantic command or compilation.
- Returning nodes are validated against current geometry and inserted/manual obstacles.
  A failed solve or unsafe restoration keeps the remaining safe offsets, publishes no
  partial new movement, and displays a localized in-canvas conflict notice. Moving an
  obstacle or changing visibility retries against the new state. No silent global Arrange,
  unpinning, file mutation or native alert is used as a fallback.
- Project/document identities, input revisions and live UI incarnation are checked.
  Project switches discard journals even when material IDs and memory keys match. Node
  removal prunes claims; document generation changes rebaseline. Closed/rebuilt views
  consume shared placement while retaining independent cameras. Persistence still writes
  only base/collapse/visibility metadata; the schema stays at version 2.

Coverage includes both preview-closing orders, repeated composition, manual protection,
unsafe restoration after insertion, frozen conflicts, removed nodes, aggregate budgets,
history epoch replay, actual shared-widget preview geometry at three DPIs/two zooms,
two-view synchronization and owner hiding, real function collapse/expand, project A/B/A,
and disk save/reopen with visible previews at a different DPI. The existing real material
and function adapter geometry tests now run with overlays and still assert unchanged
effect state. Native pointer/visual acceptance is not claimed by these headless tests.

Outstanding M5 native check: open a preview near another node, close it, then repeat with two
previews closing in either order. Move a displaced neighbor manually before closing.
Exercise mixed move/collapse/preview Undo/Redo in material/function tabs; confirm the
dirty indicators stay unchanged for presentation-only actions. Save/reopen with a preview
visible, then close it. Repeat with two views and different zoom/DPI; wires must remain
attached and cameras independent. Confirm the compact notice appears for blocked cases.

Validation with `cargo +1.98.1-x86_64-pc-windows-msvc`: **895 editor tests passed**,
7 opt-in tests ignored (6 GPU tests and the M4 timing probe). The architecture test,
strict editor/project Clippy (`--all-targets -- -D warnings`), workspace formatting
and `git diff --check` passed. Changes remain subject to the native acceptance above.

## M6 implementation — shared manual drag assistance

- Material and function toolbars now call the same shared drag-control builder. Grid
  snapping is off by default; alignment snapping/guides are on. Both are independently
  switchable with icon buttons and localized tooltips. Hold either Alt key to bypass
  snapping. These are application-session preferences, not asset or project metadata.
- Drag capture reads the originating view's stable, mounted geometry snapshot, never
  a different view's measurement owner. Nodes' measured edges, centers and socket rows
  are candidates; grid alignment uses the existing 32-logical-unit lattice. The soft
  radius is six logical screen pixels divided by the current canvas zoom. DPI conversion
  remains at the existing physical-pointer to logical-graph boundary.
- Alignment has priority over grid, with deterministic geometric tie breaking. Targets
  more than 320 logical screen pixels away on the cross axis are excluded. Candidate
  work is capped at 512 measured nodes and 32 port rows per node; missing/incomplete or
  oversized geometry falls back to free movement (or the independent grid option).
- The unsnapped position accumulates pointer deltas; the displayed snapped position
  never becomes the next pointer origin. The user's displayed drop location becomes
  the authored base through the existing one-gesture `GraphPresentationEdit`. Neighbor
  bases, semantics, shader compilation and history are not modified by assistance.
- Capture is session-only and discarded on release, node/view destruction or rebuild.
  Changes to captured target position, size, collapse/preview/content, identity or the
  history/reload epoch invalidate alignment for the rest of the gesture. This avoids
  stale magnets while the moving graph no longer has a stable geometry snapshot.
- At most two passive guide lines are projected into the originating viewport and
  clipped by it. Lines remain one logical screen pixel thick at every zoom, ignore
  picking and disappear on release, Alt bypass or view teardown. Other views follow
  shared positions, without inheriting the originating view's guide overlay or camera.

Native acceptance still required (including the outstanding M5 checks): in both graph
types, drag near edges/centers/port rows; enable grid; move slowly out of a snap; hold Alt
for arbitrary placement; release and Undo/Redo once. Repeat with a visible preview and
a second view at a different zoom/DPI. Verify wires follow, guides never capture input,
and no asset becomes semantically dirty. No native acceptance is claimed by unit tests.

Validation with `cargo +1.98.1-x86_64-pc-windows-msvc`: **904 editor tests passed**
(7 existing opt-in tests ignored), including nine new pure/actual-widget drag tests.
The architecture test, strict editor/project Clippy (`--all-targets -- -D warnings`),
workspace formatting and `git diff --check` passed.

## M7a implementation — bounded new-node placement

- The shared `node_graph::placement` module consumes stable, mounted view geometry.
  It rechecks current effective positions, dimensions, collapse and content metadata;
  stale, unavailable, wrong-view or oversized snapshots do not drive automatic placement.
- Interactive material palette creation and function toolbar creation use the same
  bounded free-space search. A free cursor position remains exact, including fractional
  or negative coordinates. Material socket creation constrains the new node to the
  source's right or consumer's left. Toolbar requests use the originating view and
  logical DPI-normalized center, not the first view of a document.
- Existing effective rectangles (including previews) are fixed obstacles. The solver
  tries nearby obstacle boundaries with 24 logical units of clearance, orders candidates
  deterministically by distance and coordinates, and caps search at 512 obstacles,
  256 tested candidates and 1024 logical units of travel. No global arrange or existing
  neighbor displacement is invoked in this slice.
- New nodes have no measured bounds yet: adapters use their bootstrap row-height
  estimates. These are not persisted geometry or a guarantee about later content
  resizing. A creation batch reserves each new rectangle before placing the next helper.
- If measurements or local space are unavailable, creation keeps a local fallback and
  displays a localized status advisory. Batch helpers still avoid one another when
  possible. Users can position the nodes manually; semantic creation is not rolled back
  solely because spacing cannot be established.
- After successful creation only, unremembered existing bootstrap positions are frozen
  as bases to prevent unrelated nodes moving during projection rebuild. Existing bases
  and temporary offsets are never overwritten. New placements and any bootstrap seeds
  attach to the existing compound semantic history entry: one Undo/Redo, not a second
  placement action. Failed semantic commands leave memory untouched.

M7 remains incomplete: wire-drop insertion, insertion conflict pushing, function
socket-created nodes and non-interactive semantic insertion remain future slices.
Native check: add into occupied space in material/function views; create from both
material socket directions; repeat with an expanded preview and a split view at another
zoom/DPI. Existing nodes must stay put, helper nodes should not stack, and one Undo/Redo
must remove/restore the entire creation at the same positions. M5/M6 native gates also
remain outstanding.

M7a validation with `cargo +1.98.1-x86_64-pc-windows-msvc`: **911 editor tests passed**
(7 existing opt-in tests ignored). Coverage includes bounded placement, helper batches,
view/DPI isolation, stale geometry rejection, and creation Undo/Redo for both adapters.
The architecture test, strict editor/project Clippy (`--all-targets -- -D warnings`),
workspace formatting and `git diff --check` passed. Native acceptance is still pending.

## M7b implementation — validated drag-on-wire insertion

- The shared widget captures the originating view's mounted geometry at drag start,
  including typed ports and entity identities. It checks current content, dimensions,
  collapse/preview state and stationary node positions before hit testing. Rebuilds,
  missing geometry and unrelated views cannot supply an insertion candidate.
- The dragged node's center targets the rendered cubic wire within 14 logical viewport
  units, excluding the 18-unit endpoint neighborhoods. Hit testing is bounded to 512
  nodes/wires per view and uses the same cubic convention at all zoom/DPI values.
- Both graph adapters expose typed wire endpoints to the widget. The common semantic
  adapter tries at most 16 input choices on detached authoring documents. Exactly one
  compiler-validated choice is required. Cycles and type/domain errors are rejected by
  the existing transaction executor; outgoing connections and non-literal incoming
  branches are not silently replaced. Ambiguous nodes require manual socket wiring.
- Green/red wire feedback and a status explanation preview the operation. Holding Alt
  suppresses insertion without disabling normal node movement. Leaving the wire or
  tearing down the view clears feedback. Function wires now project through their own
  viewport, including socket resolution, rather than the first function view found.
- Port offsets and wire fragment coordinates are normalized from physical layout pixels
  to logical units. A per-view inverse-scale uniform keeps the visible cubic and the
  insertion hit test in the same coordinate system; stroke width retains screen-pixel
  anti-aliasing. Regression checks compare rendered offsets with measured ports at
  100%, 125% and 200% DPI and verify scale-only uniform updates.
- Drop recomputes the target and revalidates current semantics, rather than trusting a
  cached green result. A valid insertion replaces the original edge with source → node
  → target, keeps the dropped position, and records a single compound Undo/Redo action.
  Semantic rejection restores the authored base and original temporary display offset.
  Missing or stale hit-test geometry falls back to an ordinary move without rewiring.
- Existing neighbors are never pushed in this slice. New nodes from socket gestures,
  explicit choice among ambiguous inputs, and bounded insertion conflict pushing remain
  later M7 work. No full M7 completion or native acceptance is claimed.

Native acceptance: in both graph types, drag an unused single-input node onto a wire;
check green feedback, release, and Undo/Redo once. Repeat with incompatible and ambiguous
nodes (red feedback, release restores position), an expanded preview, two views, and
different zoom/DPI. Hold Alt to move without inserting. Confirm no other nodes move and
that switching/closing views cannot commit an old insertion candidate.

M7b validation with `cargo +1.98.1-x86_64-pc-windows-msvc`: **919 editor tests passed**
(7 existing opt-in tests ignored). Eight new tests cover shared hit testing/scale,
semantic compatibility and ambiguity, preserved branches/cycles, stale candidates,
compound Undo/Redo and failed-drop offset restoration across both graph kinds. Strict
editor/project Clippy (`--all-targets -- -D warnings`), the architecture test, workspace
formatting and `git diff --check` passed. Native UI/GPU acceptance remains pending.

## M7c implementation — bounded spacing on wire insertion

- Wire insertion shares M4's deterministic collision solver with an explicit anchored
  insertion seed, rather than manufacturing a resize or invoking a global arrangement.
  Measured effective rectangles use logical graph units, including expanded previews.
- The drop remains fixed; conflicts cascade rightward only. The wire source, nodes left
  of the drop, other active drags, nodes beyond a 1024-unit local radius and nodes with
  temporary offsets are frozen. Preview offsets are never baked into permanent placement.
  Explicit pin UI remains future work; the pure solver already enforces frozen constraints.
- Insertion uses 24-unit spacing and limits of 512 measured nodes, 16 moved neighbors,
  256 iterations, 200,000 pair checks, 1024 units of movement per node and 4096 total.
  Only the conflict chain moves. Untouched pairs keep intentional overlaps; this is a
  bounded directional heuristic, not a global minimum-displacement guarantee.
- Planning does not move neighbors during hover. Green/red wire feedback reflects both
  semantic validity and local space; status includes how many neighbors will move.
  Alt bypasses both rewiring and spacing. Release recomputes the candidate using the
  originating view and revalidates stored neighbor bases before any semantic change.
- Successful rewiring, the dropped base and all neighbor bases form one compound history
  action for material and function graphs. Bootstrap neighbors acquire a before-base
  for exact Undo and to prevent unrelated fallback-column jumps after rewiring. A failed
  commit removes those seeded entries again. Conflicts and stale
  candidates restore the dragged base/offset and never apply partial neighbor movement.
- Missing/stale hit-test geometry still falls back to an ordinary move without rewiring.
  A valid wire hit with an unsatisfied spacing plan is rejected, not silently arranged.

Native acceptance (pending): insert beside a consumer with a second neighbor behind it;
verify rightward local movement, fixed source/drop and one-step Undo/Redo in both graph
types. Repeat with expanded previews, protected displaced neighbors, two views and varied
DPI/zoom. Red insertion must restore the original drop position; Alt must remain a plain
move. M5/M6 and M7a/M7b native gates are still pending, not implicitly passed by this slice.

M7c validation with `cargo +1.98.1-x86_64-pc-windows-msvc`: **922 editor tests passed**
(7 existing opt-in tests ignored). Three new tests cover deterministic conflict chains,
frozen/budget failures, stale solver candidates and bootstrap preservation. Extended
shared-widget tests cover spacing and preview-offset protection at multiple DPI/zoom
values; both semantic adapters cover rejected spacing, stale neighbor bases and exact
compound neighbor Undo/Redo for stored and bootstrap positions. Strict editor/project
Clippy (`--all-targets -- -D warnings`), the architecture test, workspace formatting and
`git diff --check` passed. Native acceptance remains pending.
