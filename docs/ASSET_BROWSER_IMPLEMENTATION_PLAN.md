# Asset Browser delivery plan

Status: updated 2026-09-07. AB0 contracts/inventory recorded and AB1
read-only content model implemented. AB2a background refresh/editor adapter and AB2b1
cached semantic queries implemented. AB2b2 explicit-open/post-write background operations
implemented. AB3a folder navigation and grid/list browsing are implemented in the existing
Assets dock, with a transitional Library switch. AB3b snapshot inspection and locate-source
routing are implemented. AB3c persistence and the 10,000-source benchmark are implemented;
native acceptance is still in progress. Platform caveats are recorded in
[the migration checklist](ASSET_BROWSER_MIGRATION_CHECKLIST.md).

This is the repository-specific delivery plan for
[`aestra_asset_browser_plan.md`](aestra_asset_browser_plan.md). That proposal remains
the UX/product reference; the decisions, dependencies and gates below take precedence
over its suggested implementation sequence. This is an M6 authoring/project-content
track, independent of the P2 trail-compaction benchmark work.

## Review verdict

Keep the proposed direction: **Asset Browser** is the right user-facing name, one
dockable panel should contain a source tree and reusable grid/list view, and filesystem
locations must remain separate from semantic identity. Keep `ProjectAssetIndex`;
do not introduce another semantic registry in the editor.

The proposal needs these adjustments before implementation:

| Finding in current code | Delivery decision |
| --- | --- |
| [`ProjectSourceId`](../crates/aestra-project/src/lib.rs) already exists, hashes a root-relative location and intentionally is not serializable. | Extend/reuse its location-handle contract; do not invent a second ID or persist it as stable identity. Restore browser state by relative locations, semantic IDs where available, and safe fallbacks. |
| The index already covers effects, material programs, functions and presets, with duplicate-ID and source-change protection. Duplicate semantic IDs have multiple source rows. | Source → optional semantic ID; semantic ID → **all** sources. Unique open/resolve returns a structured ambiguity error, never an arbitrary source. Invalid files remain addressable. |
| `ProjectAssetIndex::scan` already traverses the entire root, not just type folders; discovery currently accepts all `.ron` files. | Preserve arbitrary organization. Separate recognized compound suffixes, legacy effect compatibility and generic RON classification; don't turn unrelated RON files into broken effects. Characterize old behavior before changing it. |
| [`ProjectEffectCatalog`](../apps/aestra-editor/src/library.rs) owns the index, default effect destination and unsaved material drafts; its polling watcher filters semantic sources. | Replace scanning through a shared content snapshot, but retain a thin editor adapter for drafts/document coordination. A rename alone is not this migration. |
| Effect/material file operations and usage queries already exist in `aestra-project`. | Reuse audited behavior behind a common operation service. Dependency analysis must precede destructive operations, not wait for a later UI milestone. |
| [`MaterialAuthoringDocument`](../crates/aestra-authoring/src/material_authoring.rs), graph projection and [`MaterialProgramEditHistory`](../apps/aestra-editor/src/history.rs) assume an effect context. | Standalone program editing and standalone function editing are separate milestones, including command history, validation, save/recovery and conflict handling. An open-request enum alone is insufficient. |
| [`MaterialDrafts`](../apps/aestra-editor/src/material_drafts.rs) and [`persistence.rs`](../apps/aestra-editor/src/persistence.rs) already protect unsaved shared sources and exact disk baselines. | Extend this protection to document targets; don't create a second independent save path or silently discard drafts when changing browser selection. |
| Texture/mesh `AssetDefinition` and `FlipbookDefinition` are effect-local declarations; ordinary files have no project semantic ID. | Display texture/mesh files as sources. Don't infer flipbook metadata from a PNG or mint project texture/mesh/flipbook IDs in this migration. Register/import local resources only through authoring commands. |
| [`DockPanel::Assets`](../apps/aestra-editor/src/docking.rs) and reusable [`Feathers`](../apps/aestra-editor/src/feathers/README.md) controls already exist. | Reuse the persisted dock identity; change its visible title to Asset Browser. Avoid an additional permanent Library/Browser panel pair. |

Two safety corrections are mandatory: **duplicate creates a new semantic identity**
(rename/move preserves it), and unsupported reference rewriting **blocks** a move/delete.
The first browser will not offer a warning-only “unsafe” bypass.

## Architecture and scope decisions

- `aestra-project` owns filesystem discovery/classification, source/semantic joins,
  immutable content snapshots, change reconciliation and project-aware operation plans.
  It stays independent of Bevy, editor widgets and rendering.
- `apps/aestra-editor/src/asset_browser/` owns navigation, filtering, retained UI,
  routing and drop/action adapters. Reusable presentation primitives belong under
  `feathers/`, not in project-domain code. Add modules as used, not empty scaffolding.
- Keep an editor `EditorProjectContent` adapter for the current snapshot plus existing
  drafts/document coordination. The index is owned once; Library/compiler adapters
  read it rather than scan or copy their own semantic database.
- Use one project asset root. Preserve current “project containing `assets/` or chosen
  asset directory” behavior in [`project.rs`](../apps/aestra-editor/src/project.rs).
  Conventional `effects/`, `materials/`, etc. are creation defaults only. Explicit
  folder selection chooses destinations; changing root is a guarded document action.
- “Complete hierarchy” means user content under that root. Exclude Aestra's internal
  `.aestra` metadata/recovery/trash and source-control internals from content discovery.
  Other hidden/unsupported files remain representable. Show unreadable-directory and
  root-unavailable errors; never represent a failed scan as a successful empty project.
- Do not follow symlinks/junctions/reparse-point directories in initial discovery or
  mutation. Represent them as unsupported links. Reject out-of-root traversal,
  root deletion, destination collisions and recursive self-moves. Preserve actual
  filesystem casing and handle Windows case-only rename explicitly.
- Keep native paths in the domain and lossy strings for display only. Runtime row
  keys remain transient; persisted navigation uses validated project-relative paths.
  Internal moves supply a relocation map; external semantic moves can rebind by unique
  semantic ID. Generic external moves may be remove/add; fall back to a surviving
  ancestor instead of promising stable identity that the filesystem cannot supply.
- Selection and activation differ: a single click inspects; Enter/double-click opens.
  Browser selection must not destroy effect/emitter selection, switch documents, or
  dirty the project. Built-ins and Current Document are explicit virtual sources,
  never fabricated disk entries. Built-in materials/presets remain available.
- First external-file behavior is metadata inspection/reveal. Opening with the OS is
  a separate explicit action; selecting or double-clicking unknown/script files must
  not execute them. Supported texture preview can be added without a full importer.
- No new dependency on the `bevy/aestra-bevy` runtime integration. Shared render/preview
  services may be reused through the existing editor rendering boundary.

## Milestone sequence

Priorities are within this track: **P0** correctness/data-safety prerequisites,
**P1** usable migration, **P2** subsequent polish. AB0/AB1 are the first implemented
slice; AB2a/AB2b1/AB2b2 and AB3a/AB3b are implemented. AB3c acceptance is in progress;
AB4–AB9 are pending.
Do not count unavailable platform tests as verified.

| Milestone | Priority | Depends on | Deliverable / exit gate |
| --- | --- | --- | --- |
| AB0 — Contracts and migration inventory | P0 | — | Contracts, characterization coverage and Library parity inventory recorded in the migration checklist. |
| AB1 — Project content model | P0 | AB0 | Implemented: source tree joined to the existing semantic index, deterministic tests, no UI replacement. Native link/Unix verification caveats remain. |
| AB2 — Coherent refresh and editor adapter | P0 | AB1 | Implemented: background refresh, cached read-only queries, serialized explicit-open/source-write jobs, guarded publication and exact-byte/partial-save protection. Native-dialog/platform acceptance caveats remain in the checklist. |
| AB3 — Read-only Asset Browser | P1 | AB2 | Tree/grid/list/navigation/search/inspection and existing effect opening usable through retained Feathers UI. |
| AB4 — Standalone material programs | P1 | AB3 | Open/edit/undo/save a project material without an effect prerequisite. |
| AB5 — Standalone material functions | P1 | AB4 | Function inputs/outputs/body editing, validation and guarded persistence. |
| AB6 — Safe content operations | P0 | AB2, AB3, AB4, AB5 | Preflighted create/rename/move/duplicate/recoverable delete; reference and draft safety. |
| AB7 — Typed drops, pickers and local resources | P1 | AB3, AB4, AB5; AB6 for source creation | Existing authoring workflows migrate through typed payloads and shared compatibility checks. |
| AB8 — Parity, migration and legacy removal | P1 | AB3–AB7 | All Library capabilities have a tested home; existing dock/settings migrate; old scanner/UI removed. |
| AB9 — Visual browsing and organization | P2 | AB8 | Bounded thumbnail cache, favorites/recent, then saved searches/collections. |

**First usable delivery:** AB0–AB3. **Canonical-browser migration:** AB0–AB8.
Thumbnails, native OS watchers, full import pipelines and multiple browser instances
must not block those gates. Do not implement disabled-looking toolbar promises for
features that have no action yet.

### AB0 — Contracts and migration inventory

Before modifying discovery, add/record characterization coverage for:

- Existing create/rename/move, duplicate-ID, invalid/newer format, dependency/usage and
  draft-conflict contracts in `crates/aestra-project/tests/` and editor tests. Reuse
  them rather than treating the proposal's milestone 0 as entirely missing work.
- Compound suffixes: `.aestra.ron`, `.aestra.material.ron`,
  `.aestra.material-function.ron`, `.aestra.material-preset.ron`; document legacy
  plain `.ron` effect handling separately from generic RON. Known malformed formats
  remain typed error rows. Keep valid legacy source references resolvable.
- Supported texture/mesh/shader extensions from existing loaders, with generic
  fallback for everything else. Classification is not proof that loading succeeded.
- A mixed-content temporary project with empty/nested folders, same filenames in
  different directories, duplicates, invalid semantic sources, generic files and
  project-local resource references. Tests must not modify user's `assets/` fixtures.
- A parity checklist covering effect open/create/duplicate/extract/explode/repair,
  presets, local material/flipbook creation, source operations, relations, drag/drop,
  keyboard navigation and all unsaved-change dialogs.

Exit: tests and documented policies agree on identities, scope and blocked actions.
No new asset formats, UUID sidecars or asset database are required.

### AB1 — Source tree and semantic join (first implementation PR)

Add `crates/aestra-project/src/content/` with `source_tree`, `classification` and
`mod` as needed. Model directories, files, links and unavailable entries. Expose
ordered children, source metadata, typed diagnostics and one-to-many semantic lookup.
Canonical disk rows are keyed by source, so duplicate-ID files do not collapse into
one selectable item. `ProjectAssetEntry` from the proposal is conceptual, not an
existing type; add a typed borrowed view over current entries only if needed.

Introduce a discovery result consumed by both source tree and semantic index; keep
`ProjectAssetIndex::scan` as a compatibility wrapper. Avoid a second independent walk
with different exclusions. Initially a complete snapshot rebuild is acceptable.
Do not split all of `aestra-project/src/lib.rs` as unrelated cleanup in this PR.

Exit tests: nested/empty folders, invalid assets still visible, ordinary files,
deterministic ordering, unambiguous/duplicate mappings, root unavailable, unreadable
subtree, case/path handling and links excluded from traversal. Moving a semantic
source preserves its asset ID but updates its source-location handle. No editor
UI change or file mutation in this PR.

### AB2 — Refresh and project coordination

Delivered as reviewable slices. AB2b separates read-policy integration from changes to
open/write transaction timing, so cached presentation data never becomes mutation authority:

- **AB2a implemented:** `content/refresh.rs` owns file stamps/fingerprints, diffs and
  two-observation settling. `EditorProjectContent` owns one source-tree/index snapshot
  plus shared-source drafts; `ProjectEffectCatalog` is a temporary type alias. A single
  polled worker discovers generic content, prepares changed-source reload/compilation,
  and validates its observation again before publication. Root generations, internal
  write revisions and document/draft checks reject stale results. Generic-only updates
  advance content revision without triggering legacy Bevy catalog/UI invalidation.
  A localized Refresh action requests a full fingerprint check. Existing write/save
  preflight remains exact-byte-based, never hash-authorized.
- **AB2b1 implemented:** retain effect/program/function/preset documents from the same parse
  that creates each index entry, keyed by source location. Snapshot typed reads use the
  single semantic index for identity/status/ambiguity; they never reopen files. One shared
  dependency/usage traversal supports explicit cached and disk-validating read policies.
  Graph/preset queries, Properties, timeline source labels, dependency diagnostics,
  read-only usage inspection and preview compilation use cached sources plus draft overlays.
  Existing source commands, deletion-confirmation usage checks and exact-byte save/edit
  preflight retain current-disk validation. Tests exercise cache reads after source removal,
  all four duplicate kinds, dependency parity, replacement and draft/conflict preservation.
- **AB2b2 implemented:** a serialized `project_content/io.rs` job runner prepares explicit
  folder/effect opens, saves and Library rename/move/delete/extraction operations on the
  I/O pool. File/destination pickers remain on the UI thread; discovery, source preflight,
  writes and post-write reconciliation run on workers. Completed writes are drained,
  never abandoned by a second operation or window close. Publication is memory-only and
  checks project/document identity; opens and owner replacement also check revisions,
  effect/draft contents, pending proposals and locks. Save completion merges only saved
  baselines, preserving newer edits, undo and transport. Partial saves retain failed
  drafts; undo during a save is rebased onto the bytes actually written. Save/Discard/Cancel
  and source back/forward navigation remain guarded; cancelling pending navigation does
  not interrupt its save. Deletion rediscovers current owners before checking confirmed
  usages. Tests cover prepared publication, root switches, failures, concurrent edits,
  partial saves, source operations and the production completion poller.

Interactive project discovery, periodic refresh and explicit-open/post-write scans now
run off the UI thread. Cached UI semantic queries and operation-result publication are
memory-only, including compilation dependency resolution. This is not a claim that all
editor filesystem work is asynchronous: initial startup/recovery bootstrap, settings/
recovery persistence and narrow first-edit/explode disk validation retain their existing
paths. Native dialog behavior still needs manual platform acceptance; no browser UI
acceptance is claimed.

Move pure snapshot/diff/debounce logic out of Library into project-domain code.
Keep scheduling/task execution in the editor; use bounded polling first. Discover
folder and ordinary-file changes as well as semantic sources. Publish source tree
and index together under a project generation/revision; discard stale worker results
after root switches and editor writes. Do not reparse assets on search/hover.

Metadata polling is a trigger, not a content identity guarantee: unchanged timestamps
and sizes must not authorize overwriting disk. Preserve exact-byte save preflight;
use changed-source fingerprints plus explicit Refresh/full reconciliation to handle
coarse timestamp filesystems. Polling may collapse rapid events; do not infer an
unambiguous generic rename from filename similarity alone.

Use the thin `EditorProjectContent` adapter and temporarily forward old catalog APIs.
Keep clean-source reload, dirty-source conflicts, deletion status, dependent preview
recompile and texture-root invalidation. Generic-file-only changes must not reset
playback, rebuild every panel or discard graph drafts.

Exit tests: external add/change/delete/folder move, partial writes settling, permission
failure/recovery, stale scan on project switch, same-size edits via explicit refresh,
internal-write watcher echo and dirty material/function/effect preservation. No project
discovery or read-only semantic parsing on the interactive UI frame path; unchanged
snapshots cause no UI invalidation. Explicit mutation preflight must still validate disk.

### AB3 — Read-only browser, inspection and basic routing

Delivery slices:

- **AB3a — Navigation and browsing (implemented):** `asset_browser/{state,panel,actions}.rs`
  reads the published `EditorProjectContent` snapshot. The existing Assets dock now has
  Browser/Library tabs within its content, preserving presets, current-document tools and
  legacy operations without another catalog or permanent panel. Includes expandable,
  resizable/collapsible folders; back/forward/up and compact breadcrumbs; clearable search,
  recursive scope, multiple type filters and name/type sorting; shared grid/list projection;
  independent single selection and double-click/Enter guarded effect activation. Unknown
  files and links are never executed. White type/view icons and English/French strings are
  included. Both result and expanded-folder lists are paged at 96 entries; file rows have a
  192-entry retained cache, with hidden rows removed from keyboard navigation. Selection,
  filtering and view changes do not invalidate the editor shell; toolbar entities retain
  keyboard focus across navigation/view changes. Scroll/pan starts are blocked behind the
  browser. Tests cover cached browsing with deleted disk sources, root-generation and
  deleted-folder fallback, filtering/layout parity, bounded entities, retained rows/controls,
  keyboard folders and effect activation, duplicate rejection and preview input isolation.
- **AB3a follow-up:** compact navigation/breadcrumb chrome, content-local search/filters,
  folder icons, denser list rows and larger grid type tiles. Selection shows cached path,
  type, size/read-only metadata and source diagnostics; full dependency/usage inspection
  is supplied by AB3b below. Pointer activation is keyed by source row rather than descendant hit entity,
  explicitly focuses the list, and has an Open-button fallback. Material activation reveals
  the existing graph when a renderer in the current effect uses that program; unused
  programs explain the standalone-editing limitation, without fabricating an effect.
  Effect-open regression coverage includes the real background loader and viewport
  synchronization: requesting an open must not mark the catalog changed, reinstall the
  current preview, and falsely cancel publication as a concurrent edit. The footer now
  shows operation results/errors as well as save state; genuine concurrent edits remain
  protected.
- **AB3b — Inspection and locate-source routing (implemented):** an explicit, closable
  Asset Details dock provides Details, Dependencies and Used By tabs. Single-click only
  selects; double-click/Enter opens. Right-click or the keyboard context-menu command
  offers Asset Details and References. Inspection pins its source until explicitly
  retargeted, never consuming browser height or following ordinary selection. Metadata
  and warnings also appear in delayed row tooltips. Reference rows have compact Locate
  icons; the snapshot-scope explanation is an information tooltip, not persistent prose.
  Snapshot-time direct-reference indexing covers nested effects, project/built-in material
  programs and functions, and declared resource files; texture values without an effect
  instance are explicitly contextual. Missing, ambiguous, unavailable and unknown states
  are distinct. All duplicate candidates remain inspectable; no arbitrary winner is chosen.
  Relations are saved-snapshot reports, not unsaved-draft or shader/script analysis and
  not destructive-operation preflight. Lists use scrollbars and 24-row pages.
  Version-guarded Locate actions reveal sources, reset obstructing filters, expand ancestors,
  choose the correct page and focus/scroll the selected row without opening it or changing
  emitter selection. Entry points include references, the current effect and material graph.
  Native inspection/locate acceptance remains part of AB3c.
- **AB3c — Persistence implemented; acceptance in progress (P1):** versioned preferences
  in each root's `.aestra/asset-browser.ron` restore folder/expanded paths, search/filter,
  sort, grid/list and source-pane visibility/width. Selection, inspection and authored
  state are not persisted there. Writes are debounced and atomic; malformed/future
  settings are preserved, and stale folders fall back to a surviving parent. Regression
  tests cover root changes, same-root generation changes, restart and unsafe paths.
  The 10,000-source harness and recorded baseline live in
  [`benchmarks/asset-browser`](../benchmarks/asset-browser/README.md).
  Layout/drag regression checks cover 320/640/1440-pixel panels and 100/150/200% target
  scaling using an explicit secondary-window camera. Resizing starts at the rendered
  pane width, avoiding a dead zone when the saved width exceeds the panel's 55% cap.
  Native wide browsing and restart restoration have been observed; native divider
  dragging remains inconclusive, and narrow/high-DPI/floating-window visual/input
  acceptance remains open. Headless target/layout tests are not native-window passes.
  Full AB3 is not complete until these gates pass.

Build `asset_browser/{state,panel,source_tree,asset_view,filtering,actions}.rs` as needed
using shared Feathers buttons, search fields, breadcrumbs, list rows, scrollbars,
tooltips, context menus, focus and accessibility. Grid/list are layouts over the same
filtered items. Reuse white-tinted asset/type icons with tooltips and locale strings.

Implement resizable/collapsible source pane, navigation history, breadcrumbs,
current-folder/recursive search, multi-type filters, deterministic name/type sorting,
single selection, keyboard arrows/Enter/Escape and bounded scrolling. Virtualization
is not an initial feature, but avoid eagerly spawning an entire large project tree.
Keep rows/entities stable across selection/filter updates; scroll over the browser
must not zoom/pan the viewport behind it.

Add `EditorSelection`/inspection routing without replacing effect selection. Show
source name/path/type/ID/diagnostics and semantic dependencies/usages with explicit
“incomplete/unknown” status where indexing failed. Support locate-source navigation.
Use the existing guarded Effect-open path; material/function open actions arrive in
AB4/AB5. Presets and Current Document retain their legacy route during transition.

Persist browser layout/navigation separately from semantic assets using the existing
versioned editor-settings/project-layout infrastructure. Scope state to the root;
validate restored paths and fall back when folders disappear. Use `DockPanel::Assets`
with a transitional implementation switch, not two permanent content panels or two
catalog owners. Legacy UI remains available until AB8.

Exit: keyboard-only folder/search/open workflow; pointer/scroll isolation; grid/list
selection parity; settings restart/root-switch tests; invalid/duplicate/unavailable
sources inspect correctly; manual narrow/wide/high-DPI and floating-panel checks.
Baseline a synthetic 10,000-source project and record timings/entity counts; no
per-frame I/O or asset parsing during selection/search.

### AB4 — Standalone material-program documents

Introduce an explicit graph editing target/session: project Program, project Function
(enabled in AB5), and Effect Instance context. Keep one active graph document initially;
multiple document tabs are not required. Route open requests through the document
coordinator so dirty drafts trigger Save/Discard/Cancel consistently.

Separate reusable program commands/projection/validation from mandatory effect-local
instance state in `aestra-authoring`, then adapt graph actions, Properties, preview,
layout persistence and undo/redo routing. Reuse existing command machinery/drafts;
do not create a fake saved effect as the way to open a material. A synthetic preview
context may exist privately, but it must not become an authored document/dependency.

Programs edit shared source; instance mode edits effect-local bindings/overrides and
must make shared-source edits explicit. Built-ins are read-only with a duplicate-to-
project path when source creation is available. Preserve preview invalidation and
source compile diagnostics even if no effect references the program.

Exit: open/edit/undo/redo/save/reload an unused project material; switch back to an
effect without altering it; cancel navigation; external-write conflict; recovery;
same-ID reopen; moved/missing source; read-only built-in. Tests cover focus/history
routing and existing effect-context graph operations without link/selection flicker.

### AB5 — Standalone function documents

Add a function authoring target with typed input/output signature editing, stable
port IDs, defaults and body graph projection/commands. Reuse extraction/inlining,
function library and type/cycle diagnostics; invalidate dependent material previews
when a draft changes. Signature changes must show affected call sites and retain
actionable diagnostics instead of silently dropping arguments or links.

Custom-WESL functions are a separate representation. Initially expose signature,
diagnostics and read-only source inspection; do not present an invented editable graph
or execute an external editor implicitly. Full code editing remains a later feature.

Exit: open an unreferenced graph function without an effect/program; edit signature
and body, undo/redo, save/reload/recover; detect cycles/broken usages; preserve all
custom-WESL source on inspection; no changes to unrelated effect history.

### AB6 — Safe operations, delivered in two slices

**AB6a: plan/preflight and safe single-entry operations.** Wrap existing typed APIs
with operation requests/results in `aestra-project::content::operations`; widgets
never directly mutate disk. Distinguish filename Rename from authored Display Name
(existing effect/program rename currently changes both). Preserve semantic IDs on
rename/move; duplicate gives fresh asset identity and remaps internal IDs when their
scope requires it, retaining shared external dependencies.

Add a typed reference inventory for known path-backed `AssetDefinition.path`,
material texture/default paths and other existing loader contracts. Include current
drafts, semantic reverse dependencies and directory descendants. Parse failures,
unknown include semantics and unresolved usages mean **analysis incomplete**, not
“unused.” Block risky actions when safe treatment cannot be established. Implement
folder creation and supported semantic operations first; unsupported actions explain
why they are disabled. Do not ship permanent delete as the default.

**AB6b: audited path rewrites, folder operations and recovery.** Prepare all moves,
reference rewrites and affected draft/session path changes before mutation. Revalidate
source bytes/root membership/destinations immediately before apply; prevent collisions,
case-only rename errors, link escapes and folder self-descendants. No cross-root move
or silent replacement in this milestone.

Use staged writes, backups and an operation journal with explicit rollback/recovery;
multi-file changes are not inherently filesystem-atomic. Deletes move to a project
recovery area with restore metadata, outside content indexing. Known live references
must be repaired/reassigned explicitly before deletion. Interrupted or failed apply
must preserve originals/recoverable bytes and expose any incomplete recovery. File
operation history is separate from semantic document undo unless a coordinated
transaction explicitly owns both; never claim Ctrl+Z support for an unjournaled move.

Exit tests: semantic move/duplicate identities, supported path rewrites, partial
dependency knowledge blocking, dirty drafts, naming collisions, case-only rename,
read-only source, missing destination, links/traversal, failure after each staged step,
restart recovery/restore and watcher reconciliation. Folder mutation stays disabled
until this full gate passes; warning-only unsafe operations are not an alternative.

### AB7 — Typed drag/drop, reusable pickers and document resources

Use one source/project/generation-aware payload with optional typed semantic identity.
Re-resolve on drop; reject stale/missing/ambiguous/out-of-project items with clear
feedback. Compatibility is shared by pickers and drag targets, not inferred by icons.

Port Effect → Timeline with current cycle checks, Material Program/Preset → renderer,
Function → graph call, Texture → compatible material input, Mesh → mesh renderer.
File-backed drops register/reuse effect-local assets through semantic commands and
relative paths. A PNG does not become a Flipbook automatically: route through explicit
atlas metadata creation/selection, or drag an existing document-local flipbook payload.

Move local material/flipbook creation to Properties or an explicit Current Document
virtual source; keep built-in/project presets usable. Unknown files support inspection,
not arbitrary drops. Exit: each supported drop/picker performs one undoable edit,
cancel/invalid drops do nothing, compiler/preview update and resources are not duplicated
unnecessarily. Selection survives browser refresh and document-local resources never
appear as fake project files.

### AB8 — Cutover and legacy removal

Complete AB0's parity inventory, including lesser-used extract/explode/repair/relation
actions, preset workflows and keyboard shortcuts. Switch the existing Assets dock slot
to Asset Browser by default; preserve closed/floating/split layouts and localized View
menu restoration. Migrate settings by version or tolerant aliases without losing layout.

Remove Library UI/state and old watcher/catalog wrappers only when all callers use the
new services; relocate texture-root sync, presets and effect-specific actions to their
owners. Do not remove reusable source-operation or dependency code just because its
old UI entry point disappeared. No double scanner or permanent compatibility toggle.

Exit: mixed-project acceptance workflow covering browse → open → edit → save → move →
locate usages → drop → undo → restart/restore. Workspace tests, editor regression tests,
strict Clippy/formatting and manual detached-window/high-DPI/no-flicker checks pass.
The user can perform all supported Library workflows without Library.

### AB9 — Professional browsing after migration

Deliver separately: bounded asynchronous texture thumbnails; cached material/effect/
mesh previews through existing render services; favorites/recent; saved filters/searches;
collections. Cache keys include source/content revision and rendering inputs, with
memory/disk budgets, cancellation on root switch and stale-result rejection. No preview
job may block input or dirty authored assets. Each item retains a type-icon fallback.

Native filesystem notifications, extremely large-project virtualization, arbitrary
importers, cross-project operations and multiple browser instances require separate
measured scope. Do not label this milestone complete simply because it resembles a
particular engine UI; each delivered feature needs a tested performance/correctness gate.

## Verification and handoff rules

- Pure content tests use temporary fixtures and include platform-specific path cases.
  Missing OS capabilities (e.g. symlink privileges) must be reported, not counted as
  tested. No timing-dependent sleeps in watcher unit tests; inject snapshots/time.
- Test the content core without Bevy and editor state/routing with focused ECS tests.
  Mutation tests inject filesystem failures; image/UI checks supplement, not replace,
  model/command tests. No runtime GPU benchmarks are required for the read-only model.
- Use the repository's supported toolchain. On this Windows workstation that is
  `cargo +1.98.1-x86_64-pc-windows-msvc`; run focused tests per slice and workspace
  Clippy/formatting at integration gates. Do not describe proposed tests as already run.
- Keep each milestone independently reviewable. Ship safety services before enabling
  UI mutations, preserve unrelated assets, and update this plan's status only when its
  acceptance gate actually passes.

**Immediate next step (P1):** finish AB3c native divider, narrow/high-DPI and detached-window
acceptance, retaining precise caveats in the checklist. Then begin AB4 standalone material
documents. Do not mark AB3 complete from headless checks alone, remove the transitional
Library, or introduce thumbnails/file mutations ahead of their milestones.
