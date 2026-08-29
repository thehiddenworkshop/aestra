# Aestra — Pre-M6 Professional UI Implementation Plan

Status: Complete; all six pre-M6 slices and the professional acceptance gate are implemented

This document turns the pre-M6 portion of
`aestra_ui_choreography_library_ux_plan.md` into a repository-specific delivery plan.
It intentionally stops before reusable `EffectClip` semantics, project asset references,
or any format/runtime change.

## 1. Goal

Improve the current editor's information architecture and daily usability so M6 can add
reusable effect composition onto stable interaction surfaces.

The pre-M6 result should communicate:

```text
Library
    project and current-document resources

Choreography
    current-document emitter hierarchy and timing

Inspector
    behavior of the selected semantic object

Viewport
    visual result and direct manipulation
```

## 2. Hard boundary

This plan is editor-only.

Allowed areas:

```text
aestra-editor/src
aestra-editor/locales
editor layout/settings persistence where required
editor tests
documentation
```

Not changed in this phase:

```text
EffectAsset format v3
aestra-core semantic data
aestra-authoring command serialization
aestra-compiler plans
aestra-runtime execution
aestra-bevy public runtime API
effect files or migrations
```

Existing semantic commands remain the only mutation path. UI work may move command
ownership between editor plugins, but it must not bypass `EditorSession` or
`aestra-authoring`.

## 3. Architecture audit

### 3.1 Assets currently owns four unrelated responsibilities

`aestra-editor/src/assets.rs` currently presents and controls:

1. the open document summary;
2. a one-time project-effect directory scan;
3. embedded render assets, materials, and flipbooks from the open `EffectAsset`;
4. the current effect's emitter hierarchy and emitter actions.

This is the main information-architecture problem. Project discovery, embedded document
resources, and document hierarchy have different ownership and interaction rules.

Decision:

```text
EditorLibraryPlugin
    owns project-effect discovery, Library query/filter state, project entries,
    and presentation of current-document resources

TimelinePlugin
    owns the emitter track-header hierarchy, emitter selection, and emitter-level
    choreography actions

EditorPersistencePlugin
    remains the only owner of document replacement and unsaved-change protection
```

Material and flipbook creation remain semantic document actions displayed under Current
Document Resources until project-level reusable assets exist.

### 3.2 Project discovery is not yet a project asset system

`EffectCatalog::scan()` currently:

- reads only `assets/effects`;
- runs once when `EditorAssetsPlugin` is built;
- identifies rows by vector index;
- silently omits unreadable, invalid, or unsupported files;
- stores only a display name and path;
- has no refresh, query, type, status, or diagnostic model.

Pre-M6 should improve this into an honest project-effect catalog, not pretend that a full
M6 project asset index already exists.

Required pre-M6 catalog entry data:

```rust
struct ProjectEffectEntry {
    id: ProjectEffectEntryId,
    path: PathBuf,
    display_name: String,
    status: ProjectEffectStatus,
}
```

`ProjectEffectEntryId` is an editor catalog identity derived from or associated with the
normalized path. It is not the future semantic `ProjectAssetId` and must not be serialized
into effects.

Unreadable and unsupported files remain visible with an explicit status. Search/filter
operations never silently remove invalid entries except when the user's active query does
not match them.

### 3.3 Current resources are embedded document data

The open `EffectAsset` owns:

```text
assets: Vec<AssetDefinition>
materials: Vec<MaterialDefinition>
flipbooks: Vec<FlipbookDefinition>
emitters: Vec<Emitter>
```

Pre-M6 must label assets/materials/flipbooks as **Current Document Resources**. It must not
describe them as reusable project assets. Effects discovered from disk are the only
project-level content shown in the first Library slice.

### 3.4 Timeline already projects semantic emitter timing

Each `Emitter` already owns `start_time` and `duration`. `TimelineState` owns editor-only
zoom, pan, snapping, drag, and scrollbar state. Timeline clips use stable `EmitterId`, but
the left track labels are passive text and are indexed visually.

The timeline body currently contains:

- a fixed 224 px passive label column;
- one row per emitter;
- a separate clip canvas;
- move and trim handles with correct cursors;
- seek, zoom, pan, snapping, and horizontal scrolling;
- no synchronized vertical scrolling for overflow;
- no active track-header selection, status, or emitter actions.

Decision: make the existing label column an interactive track-header tree. Do not add a
second semantic track model.

### 3.5 Selection is semantic but the editor assumes an emitter

`aestra-authoring::Selection` uses stable semantic IDs. `EditorSession`, however, assumes
an emitter can always be resolved through `selected_layer_index()` and `selected_layer()`.

That assumption is acceptable for the pre-M6 emitter-only hierarchy. Pre-M6 actions must
use `EmitterId`, not row indices, so filtering/reordering cannot target the wrong object.
M6 must extend selection for `EffectClipId` separately rather than weakening this phase
with placeholder variants.

Selection synchronization that must remain intact:

```text
Timeline track header / clip
Inspector
Curves
Diagnostics and Compiler Inspector navigation
Viewport gizmo
Profiler and Changes navigation where applicable
```

### 3.6 UI rebuilding constrains search and focus behavior

`session.ui_revision` currently triggers replacement of the complete editor content and
dock tree. Semantic scroll memory repairs deep panel positions, but Assets and Timeline
have no dedicated scroll-memory keys. Emitter selection increments `ui_revision`, even
though `update_layer_selection` can update row styling in place.

Consequences:

- transient Library query state cannot live only in spawned text entities;
- search must not increment `ui_revision` on every keystroke;
- selection styling should synchronize in place where possible;
- structural changes may still rebuild the workspace in this phase;
- Library query/filter state must survive any unavoidable rebuild.

Decision: add persistent editor resources for Library and choreography presentation state,
then update row visibility/style through focused systems rather than full rebuilds for
query and hover changes.

Full per-panel reconciliation is valuable later but is not required to complete this
pre-M6 plan.

### 3.7 Docking already supports the target layout

The persisted recursive dock tree already provides resizable and floating panels. The
default vertical split is `0.71`, leaving roughly 29% for Timeline. Existing user layouts
must not be overwritten.

Decision:

- retain the serialized `DockPanel::Assets` variant during pre-M6 to avoid a layout
  migration solely for a label/source rename;
- present its localized title as `LIBRARY`;
- use approximately `0.64` for the new/reset default split, giving choreography roughly
  36% of the content height;
- apply the new ratio only to defaults and explicit workspace reset.

### 3.8 Widget support is sufficient but missing list/tree compositions

`src/feathers/` already owns buttons, text inputs, scroll areas, combo boxes, field rows,
panel cards, tooltips, separators, sliders, and status surfaces. It intentionally does not
yet contain reusable list/tree rows.

Decision: add only the shared primitives exercised by Library and choreography:

```text
search field composition with clear action
compact selectable list row
section/disclosure header
compact icon/status slot
empty/error result surface
```

Timeline clips, track headers, and future Library drag/drop remain domain-owned controls.

### 3.9 Keyboard routing needs a professional boundary

Assets currently handles `Ctrl+Enter`, `Ctrl+D`, and `Delete` globally whenever the module
palette is closed. Once emitter hierarchy moves to Timeline, these shortcuts must not fire
while a user edits text or interacts with an unrelated panel.

Decision: choreography shortcuts are owned by `TimelinePlugin` and gated by relevant
focus/hover context plus editable-text focus. All visible actions remain accessible through
buttons or context menus.

### 3.10 Transport ownership remains unchanged

Timeline currently consumes shared `TransportAction` values for frame stepping and seed
adjustment. `EditorTransportPlugin` remains the single playback-state owner. Visual changes
may make transport feel closer to choreography, but this phase does not duplicate its
commands or state.

## 4. Target pre-M6 state

### Library

```text
LIBRARY
├── search + type/origin filters
├── PROJECT EFFECTS
│   ├── valid effect rows
│   └── invalid/unsupported rows with status
└── CURRENT DOCUMENT RESOURCES
    ├── Textures / Meshes
    ├── Materials
    └── Flipbooks
```

The current effect identity already appears in document chrome and is not repeated as a
large card in Library.

### Choreography

```text
CHOREOGRAPHY
├── toolbar/ruler
└── synchronized rows
    ├── interactive emitter track header
    └── timed emitter clip
```

Track headers provide selection, enabled status, semantic diagnostics, and emitter actions.
The clip canvas remains the timing editor.

## 5. Delivery slices

Each slice should be independently reviewable, keep the workspace green, and end in one
focused commit.

### Slice 1 — Establish Library domain state

Outcome: project discovery and Library presentation state have explicit models independent
of spawned UI entities.

Work:

1. Rename source/plugin concepts from Assets to Library where this does not alter persisted
   `DockPanel::Assets` compatibility.
2. Replace `EffectCatalog` with an injectable-root `ProjectEffectCatalog`.
3. Add stable editor-only entry IDs and `Valid`, `Invalid`, and `Unsupported` status.
4. Preserve invalid entries instead of dropping them.
5. Add `LibraryState` with query and supported type/origin filters.
6. Keep opening routed through `DocumentAction` and persistence.
7. Add complete English/French message IDs.

Likely files:

```text
aestra-editor/src/assets.rs -> library.rs
aestra-editor/src/main.rs
aestra-editor/src/persistence.rs
aestra-editor/src/dock_ui.rs
aestra-editor/src/docking.rs
aestra-editor/locales/*/editor.ftl
```

Tests:

- scan a temporary root deterministically;
- stable ordering and entry identity;
- valid, invalid, unsupported, and empty catalog states;
- query matching is case-insensitive and includes authored name/path;
- catalog activation still routes through unsaved-change protection.

### Slice 2 — Add reusable searchable-list primitives

Outcome: Library uses shared professional list/search compositions rather than ad hoc rows.

Work:

1. Add a Feathers search-field composition with clear action and accessible label.
2. Add compact selectable/status list-row and section-header compositions.
3. Add localized empty/error result surfaces.
4. Keep query text in `LibraryState` and filter existing rows without `ui_revision` rebuilds.
5. Preserve keyboard focus while typing and after clearing a query.

Likely files:

```text
aestra-editor/src/feathers/search_field.rs
aestra-editor/src/feathers/list_row.rs
aestra-editor/src/feathers/mod.rs
aestra-editor/src/library.rs
aestra-editor/locales/*/editor.ftl
```

Tests:

- input/change/clear contract;
- accessible labels and activation classification;
- filtering does not mutate catalog order or semantic document state;
- Library query survives structural UI rebuild.

### Slice 3 — Rebuild the Library information architecture

Outcome: project content and current-document resources are visibly separated. The
existing emitter subsection remains temporarily available until Slice 4 installs its
replacement, so this commit does not remove an authoring workflow.

Work:

1. Replace the current effect card and mixed headings with `PROJECT EFFECTS` and
   `CURRENT DOCUMENT RESOURCES`.
2. Present only real supported types: Effects, Textures/Meshes, Materials, Flipbooks.
3. Keep embedded material/flipbook creation under Current Document Resources.
4. Show source/status metadata on demand or as secondary text without displaying raw UUIDs
   as primary information.
5. Make invalid catalog entries visible but non-openable, with actionable tooltip/status.
6. Hide origin/type sections that have no content unless an explicit empty state teaches the
   user how to add/import that content.

Tests:

- valid effect rows emit stable document-open actions;
- invalid entries cannot replace the document;
- current resources reflect the open document after New/Open/Undo/Redo;
- localized labels are complete in English and French.

### Slice 4 — Make track headers the emitter hierarchy

Outcome: the timeline track-header column is the authoritative current-document hierarchy.

Work:

1. Replace passive label text with interactive `EmitterTrackHeader` rows keyed by
   `EmitterId`.
2. Add selected, muted, soloed, and diagnostic visual states using icon plus style, not
   color alone. Mute remains an authored, undoable emitter-enabled change; Solo is explicitly
   preview-only and never dirties or rewrites the effect asset. Lock remains deferred until
   its editing-state contract is defined.
3. Selecting a header or its clip uses one `ChoreographyAction::SelectEmitter(EmitterId)`
   path and clears incompatible curve selection consistently.
4. Move add, duplicate, delete, and enabled actions from Library into `TimelinePlugin`.
5. Remove the former emitter subsection, rows, and actions from Library only after the
   track-header equivalents exist in the same commit.
6. Add hover actions and a compact context menu without permanently overloading every row.
7. Move emitter keyboard shortcuts to context-aware choreography input.
8. Keep all mutations routed through existing semantic commands and deletion review.

Likely files:

```text
aestra-editor/src/timeline.rs
aestra-editor/src/library.rs
aestra-editor/src/session.rs
aestra-editor/src/menus.rs or a focused choreography-menu module
aestra-editor/locales/*/editor.ftl
```

Tests:

- header and clip select the same stable emitter ID;
- Library contains no `LayerRow`/emitter selection controls after the move;
- filtered Library state cannot affect emitter targeting;
- add/duplicate/delete/enabled remain undoable;
- deletion retains the minimum-emitter guard and Changes review;
- shortcuts do not fire while editing text or outside choreography context;
- selection synchronization with Inspector/Curves/Viewport remains intact.

### Slice 5 — Choreography overflow and default proportions

Outcome: choreography remains usable with more tracks and has professional default space.

Work:

1. Add one synchronized vertical scroll model for track headers and clip rows.
2. Show a vertical scrollbar only on overflow.
3. Preserve current horizontal timeline zoom/pan/scroll behavior.
4. Keep ruler, playhead, snap guide, header rows, and clips vertically aligned.
5. Change only the default/reset workspace vertical split from `0.71` to approximately
   `0.64`.
6. Preserve existing saved user layouts unchanged.
7. Remember choreography scroll state across unavoidable structural rebuilds.

Tests:

- header and clip row positions remain synchronized while scrolling;
- overflow controls appear/disappear correctly;
- horizontal seek/zoom/pan and clip drag/trim regressions remain green;
- existing serialized layouts round-trip unchanged;
- default/reset layout uses the new proportion.

### Slice 6 — Professional interaction and acceptance pass

Outcome: the pre-M6 UI foundation meets its complete ergonomic gate.

Progress:

- complete: native list-box traversal, Enter activation, auto-scroll, and visible keyboard
  focus for Library project entries and Timeline track headers;
- complete: semantic labels and tooltips for compact Timeline controls and clip handles,
  detailed invalid/unsupported Library descriptions, explicit unavailable catalog state,
  and Mute/Solo track controls with distinct authored/preview-only semantics;
- complete: explicit non-mutating project-effect drop rejection on the pre-M6 Timeline;
- complete: compact Library/Timeline shrink, truncation, and overflow contracts;
- complete: automated blank/current-document compact-width UI composition coverage;
- complete: architecture and implementation ownership documentation.

Work:

1. Add keyboard traversal and focus visuals for Library entries and track headers.
2. Add tooltips and accessible labels for icon-only controls and statuses.
3. Define and show explicit empty, invalid, and unavailable states. Do not show a synthetic
   loading state while catalog discovery remains synchronous; add loading only with a real
   asynchronous discovery lifecycle.
4. Add invalid-drop feedback hooks without advertising Effect-to-Timeline creation before
   M6 supports it.
5. Audit compact sizing, truncation, tooltip overflow, and narrow-panel behavior.
6. Add one automated blank/current-document UI acceptance scenario.
7. Update `ARCHITECTURE.md` and `IMPLEMENTATION_PLAN.md` with the completed ownership
   boundaries.

Tests:

- keyboard/focus navigation;
- accessibility metadata for all new controls;
- narrow Library and Timeline layouts do not overlap or panic;
- no invalid drop mutates document state;
- full editor tests and workspace quality gates pass.

## 6. Dependency order

```text
Slice 1: Library models
    |
    v
Slice 2: shared search/list widgets
    |
    v
Slice 3: Library IA with temporary emitter compatibility
    |
    v
Slice 4: track hierarchy/actions and final emitter move
    |
    v
Slice 5: overflow/default layout
    |
    v
Slice 6: professional acceptance pass
```

The sequence is intentionally conservative: each commit leaves every existing authoring
workflow available, even when a responsibility is in transition.

## 7. Regression constraints

Every slice must preserve:

- current effect v3 serialization byte semantics apart from normal explicit user edits;
- undo/redo and Changes review behavior;
- unsaved-change protection for project-effect opening;
- v2-to-v3 migration behavior;
- Inspector disclosure and scroll memory;
- timeline move, trim, snapping, seeking, zooming, panning, and scrollbar behavior;
- deterministic preview, direct seek, restart, frame stepping, and seed controls;
- dock layout loading, reset, resizing, floating windows, and redocking;
- English fallback and complete French localization coverage;
- CPU/GPU preview and effect rendering.

## 8. Quality gates

Run for each slice:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
git diff --check
```

UI-specific tests should assert semantic action outcomes and stable component/state
contracts rather than screenshot pixel positions. Existing deterministic visual capture
remains the renderer regression boundary; this pre-M6 editor-only work does not add a GPU
requirement to normal CI.

## 9. Exit gate

Pre-M6 UI foundation is complete when:

- Library no longer mixes project discovery with emitter hierarchy;
- current-document resources are clearly labeled as local;
- project effects can be searched and invalid entries are explained;
- timeline track headers are the authoritative emitter hierarchy;
- emitter actions and selection work from choreography using stable IDs;
- many emitter rows can be navigated with synchronized vertical scrolling;
- the first-run/reset layout gives choreography approximately 35–40% vertical space;
- existing user layouts are preserved;
- unsupported project-effect drops explain the M6 boundary without mutating the document;
- compact Library and Timeline compositions remain bounded and usable at narrow widths;
- all existing authoring, persistence, preview, and localization behavior remains green;
- no effect-format, compiler, runtime, or migration change was required.

After this gate, M6 can begin with the project asset index/resolver and minimal referenced
`EffectClip` vertical slice defined in the UX direction document.

Post-gate polish adds Ardour-style inline emitter naming to Timeline track headers and an
optional persisted emitter display color edited from an anchored native Feathers picker with
RGB/HSL channels, alpha, editable RGBA hex, and live track preview. These are undoable semantic
authoring fields; the latter is a backward-compatible
version-3 presentation hint and does not affect the compiler, simulation, renderer, or legacy
migration contract.
