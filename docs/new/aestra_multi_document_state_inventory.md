# Milestone 0 — Multi-Document State Ownership Inventory

Deliverable for Milestone 0 / PR 1 of the [multi-document editor plan](aestra_multi_document_editor_plan.md).
Classifies every piece of current Material Graph / editor state so each has a clear owner under the
target `Asset → Document → View → DockTab` architecture **before** any docking change.

## Ownership categories

| Category | Owner (target) | Lifetime |
|---|---|---|
| **Document** | `OpenDocument` / `DocumentManager` | per authored asset; dirty, history, draft, diagnostics, revision |
| **View** | `MaterialGraphViewState` keyed by `EditorViewId` | per open editor instance; transient interaction + persisted pan/zoom |
| **Shared** | shared resources/caches | reused across documents/views; never duplicated |
| **Tool** | singleton tool-panel resources | one global instance regardless of open documents |
| **Dock** | `WorkspaceLayout` / dock resources | workspace structure, persisted |
| **Eliminate** | — | singleton assumption removed by the migration |

## Invariants (restated for this codebase)

1. One project asset → one open `Document` by default.
2. One `Document` may have many `EditorView`s (not exposed until M12).
3. A dock tab references either a `ToolPanel` or an `EditorViewId`.
4. Dirty state + undo history belong to the `Document`.
5. Transient graph interaction state belongs to the `EditorView`.
6. Expensive reusable caches are shared.
7. Tool panels stay distinct from asset editors.
8. Opening an already-open asset focuses its existing view.

## Current state inventory

### Session / target

| Item | Where | Category | Target owner / notes |
|---|---|---|---|
| `EditorSession.material_target: MaterialEditingTarget` | `session.rs:53` | **Eliminate** | The core singleton — "which material/function the one graph shows." Replaced by `EditorViewId → DocumentId` routing (M4/M12). `MaterialEditingTarget` (`material_document.rs:9`: `EffectInstance` / `Program{..}` / `Function{root,id}`) is the precursor to `DocumentKey`. |
| `EditorSession.effect: EffectAsset` | `session.rs:55` | **Document** (effect) | Becomes an Effect document in the later phase (M14–15); authored vs preview/runtime state split. |
| `EditorSession.ui_revision` | `session.rs` | **Tool** | Global UI rebuild signal; stays. |

### Documents (authored asset state)

| Item | Where | Category | Target owner / notes |
|---|---|---|---|
| `ProjectEffectCatalog.material_drafts.programs` | `material_drafts.rs:26` | **Document** | Per-program unsaved draft — already keyed by `MaterialProgramId`. Becomes `MaterialDocumentState.draft` + `dirty`. |
| `ProjectEffectCatalog.material_drafts.functions` | `material_drafts.rs:27` | **Document** | Per-function draft — becomes `MaterialFunctionDocumentState`. |
| `MaterialProgramEditHistory` / `EditorHistoryLedger` | `history.rs` | **Document** | Undo/redo currently routed by `material_target`; ownership moves to the document (M6). |

### View state (per open editor instance)

| Item | Where | Category | Target owner / notes |
|---|---|---|---|
| `MaterialGraphSelectionState { program, expressions, connection }` | `material_graph.rs:241` | **View** | Holds **one** program's selection; resets on program switch. Becomes `HashMap<EditorViewId, ..>` so switching never clears another view's selection. |
| `MaterialGraphPaletteState { open, node_menu, query }` | `material_graph.rs:228` | **View** | One open palette (already carries `program` + `graph_key`). Per-view. |
| `MaterialGraphGesture` | `material_graph.rs:771` | **View** (transient) | Active drag gesture; per-view, not persisted. |
| `MaterialGraphViewport { program }` (component) | `material_graph.rs:202` | **View** | Marks the viewport UI entity with its program; carries `EditorViewId` after M1. |
| `GraphViewportMemory` (pan/zoom/node positions keyed by `graph_key`) | `feathers/node_graph.rs:247` | **View** (persisted) | Already per-graph via the string `graph_key`; re-key by `EditorViewId`. Persisted to `assets/.aestra/editor-layout.ron`. |
| `MaterialGraphPreviewState.visible: BTreeSet<(program, target)>` | `material_graph.rs:286` | **View** | Which node/output previews are toggled on — per (program,target); becomes per-view. |

### Shared caches

| Item | Where | Category | Target owner / notes |
|---|---|---|---|
| `MaterialGraphPreviewState.cache: BTreeMap<(program,target), ..>` | `material_graph.rs:287` | **Shared** | Rendered preview images keyed by (program,target). Reused across views of the same program; stays shared. |
| Compiled material / preview shader pipelines | compiler + render crates | **Shared** | Reused; two views of one program must not compile twice (M30). |
| Thumbnail cache, WESL module index, dependency graph | asset browser / project | **Shared** | Shared indices; feed incremental invalidation (M8/M32). |

### Tool panels (singleton)

| Item | Where | Category | Target owner / notes |
|---|---|---|---|
| Properties / Asset Inspector / Diagnostics / Compiler Inspector / Profiler / Changes / Settings / Timeline / Assets / Viewport | `docking.rs` `DockPanel` | **Tool** | Stay singleton; become `ToolPanel` (M1). Later made contextual via `ActiveEditorContext` (M10). |
| `MaterialGraphPaletteState.is_open()` gate | `material_graph.rs:234` | **Tool→View** | Global "palette open" gate used by shortcut blocking; must become per-active-view aware. |

### Dock / workspace

| Item | Where | Category | Target owner / notes |
|---|---|---|---|
| `DockPanel` (enum) | `docking.rs` | **Dock** | Splits into `ToolPanel` + `DockTab { Tool(ToolPanel), Editor(EditorViewId) }` (M1). `DockPanel::MaterialGraph` stays a tool panel through M1, becomes an editor view in M4. |
| `DockTab(DockPanel)` (**component**) | `dock_ui.rs` | **Dock** | Name clashes with the planned `DockTab` enum — rename the component (e.g. `DockTabButton`) in M1. |
| `WorkspaceLayout` (dock tree, floating, `next_node_id`) | `docking.rs:668` | **Dock** | Stack contents become `DockTab`; serialization persists document keys + view kinds (M11). |
| `MaximizedPanel(Option<DockPanel>)` | `docking.rs` | **Dock** (transient) | Becomes `Option<DockTab>`-addressable after M1. Not persisted. |
| `MaterialGraphLayoutPersistence` (persists `GraphViewportMemory`) | `material_graph.rs` | **Dock/persist** | Extends to persist per-`EditorViewId` view state (M11). |

## Key findings

- The dominant singleton is **`session.material_target`**; most graph interaction state is *already* keyed by `MaterialProgramId` (selection, palette, preview visible/cache) or by `graph_key` (viewport memory). Re-keying those by `EditorViewId` is mechanical.
- `MaterialEditingTarget` is a ready-made precursor to `DocumentKey` (it already distinguishes program/function/effect-instance).
- `material_drafts` is effectively today's document store — `DocumentManager` formalizes it and adds a stable `DocumentId` + `by_key` lookup so "open twice → same document" is guaranteed.
- The `DockTab` component-vs-enum name clash is the one naming hazard to resolve up front in M1.

## Exit criteria

- [x] Every current Material Graph state object has a classified future owner (tables above).
- [x] Invariants restated against this codebase.
- [x] Singletons to eliminate identified (`material_target`; the one-program assumption in selection/palette/gesture).
- [x] Naming hazard (`DockTab` component vs enum) flagged for M1.
