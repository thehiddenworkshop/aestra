# Aestra Asset Browser — Implementation Plan and Milestones

## Status

**Proposal:** Replace the current editor `Library` with a general-purpose, project-aware **Asset Browser**.

The Asset Browser should behave more like a professional game-engine/DCC content browser than a palette of insertable objects. It must represent the project filesystem while preserving Aestra's semantic asset identity and dependency model.

The key design principle is:

> **Filesystem paths are locations; semantic asset IDs are identities.**

The browser therefore needs to combine two views of the project:

1. **Filesystem/source hierarchy** — directories and files as they exist in the project.
2. **Semantic asset index** — Effects, Material Programs, Material Functions, Presets, and future typed assets identified independently from their paths.

The existing `ProjectAssetIndex` is a strong foundation and should be retained. The main work is to add a generic project-content layer around it and migrate the current effect-centric `Library` UI to a reusable Asset Browser.

---

# 1. Goals

## 1.1 Primary goals

The Asset Browser must:

- Replace the current `Library` as the main project-content UI.
- Browse the complete project asset filesystem.
- Support arbitrary folder organization.
- Display both recognized Aestra assets and ordinary files.
- Preserve semantic identity when semantic assets are renamed or moved.
- Open assets in the appropriate editor:
  - Effect → Effect editor/timeline.
  - Material Program → Material Graph.
  - Material Function → Material Graph in function mode.
  - Texture/Mesh/Flipbook → preview/inspector initially.
  - Other files → inspect, reveal externally, or open externally.
- Support grid and list views.
- Support drag-and-drop based on semantic asset payloads.
- Support safe rename/move/delete operations.
- Integrate dependency/usages information.
- Be reusable for asset-picker dialogs and embedded selectors.
- Prepare for future asset types without redesigning the browser.

## 1.2 Secondary goals

Later versions should support:

- Thumbnails.
- Favorites.
- Tags.
- Saved searches.
- Collections.
- Asset validation state.
- Unused asset discovery.
- Source-control status.
- Multiple Asset Browser instances.
- Import pipelines.
- Asset creation templates.
- Reference repair.
- Bulk operations.

---

# 2. Non-goals for the first milestone

Do **not** block the initial migration on:

- Full thumbnail rendering for every asset type.
- Tags and Collections.
- Source control integration.
- Advanced saved queries.
- Full texture/mesh import pipelines.
- Full external-file editors.
- Multi-selection batch editing.
- Multiple simultaneous Asset Browser panels.
- Asset virtualization for extremely large projects.

The first objective is a correct project-content model and a usable professional browser.

---

# 3. UX Model

The default UI should be one dockable **Asset Browser** panel containing two internal panes:

```text
┌──────────────────────────── Asset Browser ────────────────────────────┐
│ + Add   Import   ‹  ›   Assets / Materials / Fire   Search   Filter │
├───────────────────┬──────────────────────────────────────────────────┤
│ SOURCES           │ ASSET VIEW                                       │
│                   │                                                  │
│ ★ Favorites       │  [Material] Flame                                │
│                   │  [Texture ] Noise                                │
│ Assets            │  [Function] DissolveEdge                         │
│ ├─ Effects        │                                                  │
│ ├─ Materials      │                                                  │
│ │  ├─ Fire        │                                                  │
│ │  └─ Smoke       │                                                  │
│ ├─ Textures       │                                                  │
│ └─ Meshes         │                                                  │
├───────────────────┴──────────────────────────────────────────────────┤
│ 3 items                            Grid ▦   List ☰   thumbnail size  │
└──────────────────────────────────────────────────────────────────────┘
```

## 3.1 One panel, two reusable components

The tree and asset view should remain inside the same dockable panel.

Internally they should be separate reusable widgets:

```text
AssetBrowserPanel
├── AssetBrowserToolbar
├── AssetBrowserBreadcrumb
├── AssetBrowserBody
│   ├── SourceTreeView
│   └── AssetView
└── AssetBrowserStatusBar
```

This allows `AssetView` to later be reused for:

- Texture pickers.
- Material selectors.
- Mesh selectors.
- "Choose Effect" dialogs.
- Drag/drop popovers.
- Search-only asset palettes.

## 3.2 Source tree behavior

The Source Tree should:

- Show folders under the project asset root.
- Expand/collapse folders.
- Support keyboard navigation.
- Support single-folder selection initially.
- Support drag/drop reorganization later.
- Remember expansion state per project.
- Be collapsible.

The browser must still work when the Source Tree is hidden using breadcrumbs and Back/Forward navigation.

## 3.3 Asset View behavior

The Asset View should support:

### Grid mode

Best for:

- textures,
- effects,
- materials,
- flipbooks,
- visually identifiable assets.

### List mode

Best for:

- large projects,
- technical assets,
- diagnostics,
- sorting by type/name/status.

The view mode is part of browser state, not part of individual assets.

---

# 4. Architectural Direction

The Asset Browser should not directly scan files and independently infer semantic state.

Introduce a project-content layer.

```text
                        Aestra Project
                             │
                ┌────────────┴────────────┐
                │                         │
       ProjectSourceTree          ProjectAssetIndex
                │                         │
          filesystem                semantics
                │                         │
                └────────────┬────────────┘
                             │
                     ProjectContent
                             │
                       Asset Browser
```

---

# 5. Preserve `ProjectAssetIndex`

The existing semantic asset model should remain the source of truth for semantic identity and dependency relationships.

Conceptually:

```rust
pub enum ProjectAssetId {
    Effect(EffectId),
    MaterialProgram(MaterialProgramId),
    MaterialFunction(MaterialFunctionId),
    MaterialPreset(MaterialPresetId),

    // Future:
    // Texture(TextureAssetId),
    // Mesh(MeshAssetId),
    // Flipbook(FlipbookAssetId),
}
```

The important invariant is:

```text
Assets/Materials/fire.aestra.material.ron
                    │
                    │ move
                    ▼
Assets/VFX/Fire/fire.aestra.material.ron

MaterialProgramId remains unchanged.
```

The browser must never assume that a path is the identity of a semantic asset.

---

# 6. Add `ProjectSourceTree`

Introduce a filesystem-oriented representation of project content.

Suggested model:

```rust
pub struct ProjectSourceTree {
    pub root: ProjectSourceId,
    pub entries: HashMap<ProjectSourceId, ProjectSourceEntry>,
}

pub struct ProjectSourceEntry {
    pub id: ProjectSourceId,
    pub parent: Option<ProjectSourceId>,
    pub path: PathBuf,
    pub name: String,
    pub kind: ProjectSourceKind,
}

pub enum ProjectSourceKind {
    Directory,
    File(ProjectFileInfo),
}

pub struct ProjectFileInfo {
    pub classification: ProjectFileClassification,
    pub semantic_asset: Option<ProjectAssetId>,
}
```

Classification:

```rust
pub enum ProjectFileClassification {
    Effect,
    MaterialProgram,
    MaterialFunction,
    MaterialPreset,

    Texture,
    Mesh,
    Flipbook,
    Shader,

    Generic,
    Unknown,
}
```

The classification should describe what Aestra currently understands about the source file.

It must remain possible to represent unsupported files.

---

# 7. Add `ProjectContent`

Create a project-level model joining source and semantic information.

Suggested API:

```rust
pub struct ProjectContent {
    source_tree: ProjectSourceTree,
    asset_index: ProjectAssetIndex,
}
```

Possible queries:

```rust
impl ProjectContent {
    pub fn source_tree(&self) -> &ProjectSourceTree;
    pub fn asset_index(&self) -> &ProjectAssetIndex;

    pub fn source(&self, id: ProjectSourceId) -> Option<&ProjectSourceEntry>;
    pub fn asset(&self, id: &ProjectAssetId) -> Option<&ProjectAssetEntry>;

    pub fn source_for_asset(
        &self,
        id: &ProjectAssetId,
    ) -> Option<ProjectSourceId>;

    pub fn asset_for_source(
        &self,
        id: ProjectSourceId,
    ) -> Option<&ProjectAssetId>;
}
```

Avoid creating a second semantic database inside the editor.

---

# 8. Remove Type-Based Folder Assumptions

Aestra can retain conventional default destinations:

```text
assets/effects/
assets/materials/
assets/textures/
```

but they must become **defaults**, not restrictions.

Valid organization should include both:

```text
assets/
├─ effects/
├─ materials/
└─ textures/
```

and feature-based layouts such as:

```text
assets/
└─ fireball/
   ├─ fireball.aestra.ron
   ├─ flame.aestra.material.ron
   ├─ flame_noise.png
   └─ sparks.wesl
```

The Asset Browser is specifically valuable because it lets project structure reflect authoring intent rather than asset type alone.

---

# 9. Generic Project Watcher

The current effect-oriented watcher should evolve into a generic project-content watcher.

Target:

```text
Filesystem Change
      │
      ▼
ProjectContentWatcher
      │
      ├─ update ProjectSourceTree
      │
      └─ recognized Aestra source?
                    │
              yes   ▼
              update ProjectAssetIndex
```

Suggested name:

```rust
ProjectContentWatchState
```

The watcher should detect:

- folder creation,
- folder deletion,
- file creation,
- file deletion,
- file rename/move,
- semantic Aestra asset changes,
- textures,
- shaders,
- generic files.

The first implementation can continue using the existing polling strategy if necessary.

The architectural change matters more than immediately moving to native filesystem notifications.

---

# 10. Centralize File Operations

The Asset Browser UI must **not** directly call filesystem operations for semantic project assets.

Introduce:

```rust
pub struct ProjectContentOperations;
```

or:

```rust
pub struct ProjectAssetOperations;
```

Operations:

```rust
create_folder(...)
rename(...)
move_entry(...)
duplicate(...)
delete(...)
import(...)
```

Each operation must have enough project context to preserve references and validate results.

---

# 11. Safe Moves and Renames

Semantic-ID-backed references make moves relatively safe:

```text
MaterialProgramId → unchanged after move
```

Raw path references require more work.

Example:

```text
textures/fire.png
        │
        │ move
        ▼
textures/fire/fire.png
```

If an Effect contains:

```text
textures/fire.png
```

Aestra must not silently break it.

Eventually:

```text
move()
  │
  ├─ discover references
  ├─ prepare edits
  ├─ perform filesystem move
  ├─ rewrite references
  ├─ validate
  └─ commit
```

For the initial browser milestone, when a path-backed asset has known references and safe rewriting is not implemented:

- either block the move,
- or show a warning and require an explicit unsafe operation.

Never silently break known references.

---

# 12. Asset Browser State

Suggested editor-side state:

```rust
pub struct AssetBrowserState {
    pub current_source: ProjectSourceId,

    pub history_back: Vec<ProjectSourceId>,
    pub history_forward: Vec<ProjectSourceId>,

    pub selected_items: Vec<AssetBrowserItemId>,

    pub search_query: String,
    pub filters: AssetFilterSet,

    pub view_mode: AssetViewMode,

    pub sources_visible: bool,
    pub sources_width: f32,

    pub thumbnail_size: f32,
}
```

View mode:

```rust
pub enum AssetViewMode {
    Grid,
    List,
}
```

Item abstraction:

```rust
pub enum AssetBrowserItemId {
    Source(ProjectSourceId),
    Asset(ProjectAssetId),
}
```

Prefer one canonical browser-item abstraction rather than passing raw paths through UI code.

---

# 13. Asset Open Routing

The Asset Browser should dispatch asset opening through a generic editor request.

Suggested:

```rust
pub enum AssetOpenRequest {
    Effect(ProjectAssetId),
    MaterialProgram(ProjectAssetId),
    MaterialFunction(ProjectAssetId),

    Texture(ProjectSourceId),
    Mesh(ProjectSourceId),
    Flipbook(ProjectSourceId),
    Shader(ProjectSourceId),

    ExternalFile(ProjectSourceId),
}
```

Or stronger typed variants if convenient.

Routing:

```text
Effect
   → Effect document

Material Program
   → Material Graph

Material Function
   → Material Graph / function mode

Texture
   → Preview / Properties

Mesh
   → Preview / Properties

Flipbook
   → Preview / Properties

Shader
   → source editor later
   → external editor initially

Generic File
   → inspector / reveal / external editor
```

---

# 14. Decouple Material Graph from Active Effect

This is one of the most important architecture changes.

A project material must be editable directly from the Asset Browser:

```text
Asset Browser
     │
     │ double click
     ▼
my_material.aestra.material.ron
     │
     ▼
Material Graph
```

It should not require an Effect to already be active.

Introduce a graph editing target:

```rust
pub enum MaterialGraphTarget {
    Program(MaterialProgramRef),
    Function(MaterialFunctionRef),

    Instance {
        effect: EffectId,
        material: MaterialId,
    },
}
```

and a corresponding session:

```rust
pub struct MaterialEditorSession {
    pub target: MaterialGraphTarget,
    // graph state...
}
```

This cleanly distinguishes:

- editing reusable project material source,
- editing reusable function source,
- editing a material instance in an Effect context.

---

# 15. Current Document Resources

Do not treat all current-document resources as filesystem assets.

The existing Library currently mixes:

```text
Project Assets
+
Current Document Resources
```

These are different concepts.

The Asset Browser should primarily represent project content.

Effect-local objects belong primarily in:

- Properties,
- Renderer inspector,
- Timeline/track hierarchy,
- Material inspector,
- Effect-local resource views.

Later the Asset Browser may expose a virtual source:

```text
Sources
├─ Project
├─ Favorites
└─ Current Document
```

but `Current Document` should be explicitly a **virtual view**, not a fake folder.

---

# 16. Search

## 16.1 Initial search

Implement:

- current-folder search,
- recursive search toggle,
- filename/display-name matching,
- semantic asset name matching.

Examples:

```text
fire
smoke
dissolve
```

## 16.2 Type filters

Initial filters:

- Effect
- Material
- Material Function
- Material Preset
- Texture
- Mesh
- Flipbook
- Shader
- Other

The filter model should already support multiple active types.

## 16.3 Future semantic query syntax

Design the query API so it can evolve toward:

```text
type:material fire
type:effect smoke
usedby:Fireball
uses:DissolveEdge
status:error
unused:true
path:/Fire/
```

Do not make the first UI dependent on implementing the full query language.

---

# 17. Drag-and-Drop

Use semantic drag payloads instead of raw filesystem paths wherever possible.

Suggested:

```rust
pub struct AssetDragPayload {
    pub source: ProjectSourceId,
    pub asset: Option<ProjectAssetId>,
}
```

Targets decide whether the payload is accepted.

Examples:

```text
Effect
  → Timeline
  → create EffectClip / reusable effect instance

Material Program
  → Renderer
  → assign material

Material Function
  → Material Graph
  → create function-call node

Texture
  → Material Graph texture input
  → Renderer/material property

Mesh
  → Renderer mesh slot

Flipbook
  → Flipbook/sprite renderer slot
```

Centralize compatibility logic instead of embedding it independently in every drag source.

---

# 18. Context Menu

Initial context menu:

```text
Open
Open With...
Rename
Duplicate
Delete
Move To...
────────────────
Copy Path
Copy Asset ID
Reveal in Explorer/Finder
────────────────
Dependencies...
Usages...
```

Conditional actions:

```text
Effect
  Explode reusable effect
  Create instance

Material
  Create preset
  Duplicate as new material

Texture
  Reimport        [future]

Generic File
  Open externally
```

Asset-specific actions should be registered by asset type rather than accumulated into one giant `asset_browser.rs`.

---

# 19. Inspector Integration

Selection in the Asset Browser should publish a generic editor selection.

Example:

```rust
EditorSelection::ProjectSource(ProjectSourceId)
EditorSelection::ProjectAsset(ProjectAssetId)
```

Properties panel can then show:

```text
Name
Type
Path
Semantic ID
Dependencies
Used By
Validation State
File Metadata
```

This avoids duplicating a separate inspector inside the Asset Browser.

---

# 20. Dependency and Usage Integration

Existing semantic dependency infrastructure should become accessible from the Asset Browser.

For semantic assets show:

```text
Dependencies
Used By
```

Potential future badges:

```text
⚠ validation error
● modified
↗ external dependency
0 usages
```

Later search can expose:

```text
unused:true
status:error
```

---

# 21. Thumbnails

Do not make thumbnails a blocker for the browser architecture.

## Phase 1 icons

Use asset-type icons:

```text
Effect            ✦
Material          ◉
Material Function ◇
Texture           ▣
Mesh              △
Flipbook          ▤
Shader            </>
Folder            📁
```

Actual icon design should match Aestra's UI.

## Future thumbnails

Thumbnail providers should be asset-type plugins/services:

```rust
trait ThumbnailProvider {
    fn supports(&self, item: &AssetBrowserItem) -> bool;
    fn request_thumbnail(&mut self, ...);
}
```

Potential thumbnail rendering:

- Effect → representative VFX frame.
- Material → shaded sphere/plane preview.
- Texture → image preview.
- Mesh → neutral mesh preview.
- Flipbook → selected frame/animated preview.
- Material Function → graph icon initially.

Cache thumbnails independently from the browser widget.

---

# 22. Code Organization

Do not rename `library.rs` into another monolithic file.

Target:

```text
apps/aestra-editor/src/
└─ asset_browser/
   ├─ mod.rs
   ├─ state.rs
   ├─ panel.rs
   ├─ toolbar.rs
   ├─ source_tree.rs
   ├─ asset_view.rs
   ├─ grid_view.rs
   ├─ list_view.rs
   ├─ selection.rs
   ├─ filtering.rs
   ├─ actions.rs
   ├─ drag_drop.rs
   ├─ context_menu.rs
   └─ thumbnails.rs
```

Project-domain code should live outside the editor:

```text
crates/aestra-project/src/
├─ content/
│  ├─ mod.rs
│  ├─ source_tree.rs
│  ├─ classification.rs
│  ├─ watcher.rs
│  └─ operations.rs
│
└─ asset_index/
   └─ ...
```

Exact file organization can follow the current crate style, but the responsibility split matters.

---

# 23. Refactor the Existing `ProjectEffectCatalog`

The existing catalog abstraction already serves more than Effects.

Rename/refactor it.

Avoid:

```rust
ProjectEffectCatalog
```

for a structure that also provides:

- material programs,
- material functions,
- presets,
- project indexing.

Potential names:

```text
ProjectCatalog
ProjectContentCatalog
EditorProjectContent
```

Prefer exposing the project-domain `ProjectContent` model directly where practical instead of maintaining another duplicate editor catalog.

---

# 24. Migration Strategy

Do not rewrite the entire Library in one step.

Use an incremental migration.

---

# Milestone 0 — Define Invariants and Tests

## Goal

Lock down project-content behavior before changing the UI.

## Tasks

- Document the distinction between:
  - filesystem source,
  - semantic asset identity,
  - document-local resource.
- Add tests proving semantic IDs survive source moves.
- Add tests for material program/function/preset indexing.
- Add tests for duplicate semantic IDs.
- Define what happens when:
  - files disappear,
  - files fail to parse,
  - semantic IDs collide,
  - unsupported files exist in the asset root.
- Define project asset root behavior.
- Decide which file extensions/classifications are initially recognized.

## Exit criteria

A documented and tested invariant exists:

> Moving a semantic project asset changes its source location, not its semantic identity.

---

# Milestone 1 — Project Source Tree

## Goal

Create a generic filesystem representation independent from the editor UI.

## Tasks

Implement:

```rust
ProjectSourceId
ProjectSourceTree
ProjectSourceEntry
ProjectFileClassification
```

Support:

- directories,
- Aestra effect files,
- material program files,
- material function files,
- material preset files,
- common texture files,
- WESL/shader source,
- generic files.

Add mappings:

```text
ProjectSourceId ↔ ProjectAssetId
```

where applicable.

## Tests

- nested folders,
- empty directories,
- unsupported files,
- recognized assets,
- duplicate filenames,
- same asset type in arbitrary directory,
- mapping from semantic asset to source.

## Exit criteria

A project can be scanned and represented as one generic source tree without using the editor's existing Library model.

---

# Milestone 2 — Generic Project Content Watcher

## Goal

Make project changes update the source tree and semantic asset index consistently.

## Tasks

Replace or supersede effect-specific watching with:

```rust
ProjectContentWatchState
```

Detect:

- new file,
- deleted file,
- modified file,
- renamed file,
- moved file,
- folder changes.

Update:

```text
ProjectSourceTree
ProjectAssetIndex
```

from one watcher pipeline.

## Tests

- external file creation appears in model.
- external file deletion disappears.
- material function modification refreshes semantic index.
- folder rename updates paths.
- invalid semantic source remains visible as source even when parsing fails.

## Exit criteria

The editor no longer requires separate effect/material-specific filesystem discovery logic.

---

# Milestone 3 — Asset Browser UI Skeleton

## Goal

Introduce the new panel without removing the Library yet.

## Tasks

Create:

```text
AssetBrowserPanel
SourceTreeView
AssetView
```

Implement:

- dockable Asset Browser panel,
- resizable Source Tree / Asset View splitter,
- folder tree,
- folder selection,
- breadcrumbs,
- Back/Forward navigation,
- Source Tree collapse,
- grid/list toggle,
- type icons,
- status item count.

No advanced editing required yet.

## UX default

```text
Sources: ~20–30%
Asset View: ~70–80%
```

Persist splitter position and selected view mode.

## Exit criteria

Users can browse all project files and folders from inside Aestra.

---

# Milestone 4 — Search, Filters, and Selection

## Goal

Make the browser useful in medium-sized projects.

## Tasks

Implement:

- search field,
- current-folder search,
- recursive search,
- multi-type filters,
- sorting:
  - Name,
  - Type,
  - Modified time if readily available.
- single selection,
- multi-selection if easy to add cleanly,
- keyboard navigation,
- Properties panel integration.

Type filters:

```text
Effect
Material
Material Function
Material Preset
Texture
Mesh
Flipbook
Shader
Other
```

## Exit criteria

Users can efficiently locate assets without navigating every folder manually.

---

# Milestone 5 — Asset Opening and Material Graph Decoupling

## Goal

Allow semantic assets to be first-class editable project documents.

## Tasks

Add:

```rust
AssetOpenRequest
```

Implement double-click routing.

Most importantly:

- decouple Material Graph from active Effect,
- implement `MaterialGraphTarget`,
- open `.aestra.material.ron` directly from Asset Browser,
- open material functions directly,
- preserve Effect-instance material editing.

## Tests

- open Effect from browser.
- open Material Program without active Effect.
- open Material Function without active Effect.
- switch between Effect and Material editing.
- reopen same asset safely.

## Exit criteria

A standalone project material can be discovered and edited from the Asset Browser with no Effect prerequisite.

---

# Milestone 6 — Centralized Rename / Move / Delete

## Goal

Make the Asset Browser a safe project-management tool.

## Tasks

Add project-content operations:

```rust
create_folder()
rename()
move_entry()
duplicate()
delete()
```

Rules:

- semantic assets keep their IDs,
- known path-backed references must not silently break,
- conflicts produce structured errors,
- operations refresh source tree and asset index.

Add context menu.

Implement:

```text
Rename
Duplicate
Delete
Move To...
Reveal externally
Copy Path
Copy Asset ID
```

## Important

Do not implement these as direct `std::fs` calls from the UI.

## Exit criteria

The Asset Browser is safe enough to become the canonical way to reorganize Aestra project content.

---

# Milestone 7 — Semantic Drag-and-Drop

## Goal

Turn the browser into the main source for authoring reusable assets.

## Tasks

Create generic:

```rust
AssetDragPayload
```

Implement initial targets:

```text
Effect → Timeline
Material → Renderer/material property
Material Function → Material Graph
Texture → Material input/property
Mesh → Renderer
Flipbook → compatible renderer
```

Centralize compatibility checks.

Add invalid-drop feedback.

## Exit criteria

Common project assets can be assigned/instanced by dragging them from the Asset Browser.

---

# Milestone 8 — Dependency and Usage Tools

## Goal

Expose semantic project relationships in the new browser.

## Tasks

Add:

- Dependencies action.
- Used By action.
- asset validation badges.
- broken-reference badges.
- parse/compile diagnostic badges.
- optional details in Properties panel.

Possible later search operators:

```text
usedby:
uses:
status:
unused:
```

## Exit criteria

Users can understand the consequences of changing or deleting an asset from the Asset Browser.

---

# Milestone 9 — Remove Legacy Library

## Goal

Complete the migration.

## Tasks

Move remaining useful Library functionality to appropriate homes:

### Project assets

→ Asset Browser

### Current-document resources

→ Effect/renderer/material Properties or explicit virtual source

### Effect-specific actions

→ typed Asset Browser context actions or Effect editor

### Search/filtering

→ Asset Browser

### Dependency inspector

→ Asset Browser / Properties

### Drag/drop

→ generic Asset Browser payload

Delete obsolete Library state and UI.

Rename/refactor `ProjectEffectCatalog`.

Remove duplicated project scanning/indexing logic.

## Exit criteria

`library.rs` is removed or reduced to no meaningful legacy functionality.

The editor uses Asset Browser as its canonical project-content interface.

---

# Milestone 10 — Professional Asset Browser Features

## Goal

Reach professional DCC/game-engine ergonomics after the architectural migration is stable.

## Features

### Thumbnails

- Texture image thumbnails.
- Material previews.
- Effect previews.
- Mesh previews.
- Flipbook previews.

### Favorites

Virtual source:

```text
★ Favorites
```

### Saved searches

Example:

```text
Broken Assets
Unused Materials
Fire Effects
All Material Functions
```

### Collections

Non-filesystem organizational groups.

Collections must not move assets or change source paths.

### Recent assets

Virtual source:

```text
Recent
```

### Multiple Asset Browser instances

Allow users to keep:

```text
Browser 1 → Effects/Fire
Browser 2 → Materials/Shared
```

and drag between workflows.

## Exit criteria

The Asset Browser reaches the flexibility expected from Unreal/Unity-class professional tooling.

---

# 25. Suggested Delivery Order

Recommended implementation sequence:

```text
0. Invariants/tests
        ↓
1. ProjectSourceTree
        ↓
2. ProjectContent watcher
        ↓
3. Asset Browser UI shell
        ↓
4. Search/filter/selection
        ↓
5. Direct asset opening + Material Graph decoupling
        ↓
6. Safe filesystem operations
        ↓
7. Semantic drag/drop
        ↓
8. Dependencies/usages
        ↓
9. Remove Library
        ↓
10. Thumbnails/Favorites/Collections
```

The most important dependency is:

> Do not implement complex browser-side file operations before the project content model exists.

---

# 26. Recommended First PR Split

A practical PR sequence:

## PR 1 — Project content model

- `ProjectSourceTree`
- file classification
- semantic mapping
- tests

No editor UI change.

## PR 2 — Generic watcher

- `ProjectContentWatchState`
- source tree refresh
- asset index refresh
- external filesystem-change tests

## PR 3 — Asset Browser read-only UI

- new panel
- Sources tree
- grid/list asset view
- breadcrumbs
- navigation
- search
- filters

Keep Library temporarily.

## PR 4 — Open routing

- `AssetOpenRequest`
- Effect opening
- Material Program opening
- Material Function opening
- Material Graph decoupling

## PR 5 — Project operations

- rename
- move
- delete
- duplicate
- folders
- context menu
- reference safety

## PR 6 — Drag/drop and usage tooling

- semantic drag payload
- Timeline integration
- Material Graph integration
- renderer assignments
- dependency/usages UI

## PR 7 — Library removal

- migrate remaining actions
- delete obsolete state
- rename/refactor catalog
- update docs

## PR 8+ — Professional polish

- thumbnails
- Favorites
- saved searches
- Collections
- recent assets
- multiple browser instances

---

# 27. Testing Strategy

## Unit tests

Project domain:

- source classification,
- semantic/source mapping,
- rename/move logic,
- reference rewrites,
- dependency updates,
- ID stability,
- duplicate IDs,
- broken sources.

## Integration tests

Simulate:

```text
create project
create effect
create material
move material
rename folder
delete texture
modify material function
```

Verify both:

```text
ProjectSourceTree
ProjectAssetIndex
```

stay consistent.

## Editor tests

Test:

- folder navigation,
- browser history,
- search/filter combinations,
- double-click open routing,
- selection propagation,
- drag/drop compatibility,
- context actions.

## Regression test

Keep one representative sample project containing:

```text
Effects
Materials
Material Functions
Presets
Textures
Shaders
Generic files
Nested feature-oriented directories
```

Use it for browser regression tests.

---

# 28. Performance Requirements

The Asset Browser should be designed to scale without requiring immediate heavy optimization.

Initial targets:

- source-tree updates should be incremental when practical,
- browser filtering should not reparse assets,
- semantic parsing belongs to project indexing,
- thumbnails must load asynchronously,
- thumbnail rendering must never block the editor frame,
- filesystem scans must not be repeated every frame,
- search should operate on indexed metadata.

Later, if very large projects require it:

- virtualize Asset View,
- incremental search index,
- native filesystem watcher,
- background thumbnail generation,
- persistent metadata cache.

---

# 29. Design Rules to Keep

## Rule 1

**The Asset Browser owns project content navigation, not document authoring state.**

## Rule 2

**Paths are locations; semantic IDs are identities.**

## Rule 3

**The browser may show unknown files.**

Unknown does not mean invisible.

## Rule 4

**Moving files must go through project-aware operations.**

Never silently break known references.

## Rule 5

**Grid/List are two views of the same `AssetView`.**

They are not separate editor panels.

## Rule 6

**Source Tree and Asset View are separate reusable UI components inside one dockable Asset Browser.**

## Rule 7

**Material assets must be directly editable without opening an Effect first.**

## Rule 8

**Do not let `asset_browser.rs` become the next `library.rs`.**

Asset-type-specific behavior belongs behind typed services/actions.

---

# 30. Definition of Done

The migration is complete when a user can:

1. Open an Aestra project.
2. See its complete asset hierarchy.
3. Navigate folders.
4. Search/filter project content.
5. Switch between grid and list view.
6. Open Effects.
7. Open Material Programs directly.
8. Open Material Functions directly.
9. Rename/move project content safely.
10. Drag assets into compatible authoring contexts.
11. Inspect dependencies/usages.
12. Work without the old Library panel.

At that point, `Asset Browser` becomes the foundational project-content interface for future Aestra asset types and authoring workflows.
