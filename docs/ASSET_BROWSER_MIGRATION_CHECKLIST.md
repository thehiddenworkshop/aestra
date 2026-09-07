# Asset Browser contracts and migration inventory

Recorded for AB0/AB1 and updated through AB3c acceptance work, 2026-09-07. This is a parity inventory,
**not** a claim that every Library workflow has migrated. Follow
[the delivery plan](ASSET_BROWSER_IMPLEMENTATION_PLAN.md).

## Implemented discovery contract

`aestra-project::ProjectContent::scan(root)` builds a read-only source tree and the
existing semantic index from one directory discovery. It does not choose a project
root, mutate files, create metadata, load textures, watch changes or publish UI events.
The editor's adapter schedules recurring discovery and explicit root-open/post-write
preparation on I/O workers. Read-only queries use the published snapshot.

- `ProjectSourceTree` retains the root, nested/empty folders, ordinary files, native
  paths/names, metadata and discovery errors. Children are folders-first then native
  name order; whole-tree enumeration uses native relative-path order.
- `ProjectSourceId` is a transient location handle, not serialized identity. Hash
  collisions are disambiguated within discovery; the semantic index uses those same
  handles. A move or changed collision set can change a handle. Keep native paths for
  I/O; do not use lossy display names or persist a row handle across snapshots.
- A source may have a readable semantic ID. Each semantic ID maps to **every** source
  declaring it. `unique_source_for_asset` refuses missing/unavailable/ambiguous
  locations; it is not a validation or mutation API. Typed index loaders retain their
  validation, duplicate-ID and source-change checks.
- Recognized compound suffixes retain typed rows on parse failure. Valid legacy
  effect `.ron` files still resolve. Other RON files remain generic in `ProjectContent`;
  the old `ProjectAssetIndex::scan` wrapper retains its broad RON-candidate behavior.
- Texture (`png`, `jpg/jpeg`, `webp`, `bmp`, `tga`, `dds`, `ktx2`, `exr`, `hdr`), mesh
  (`gltf`, `glb`, `obj`) and shader (`wesl`, `wgsl`, `glsl`, `hlsl`) classifications are
  filename hints only. They **do not advertise loader support**. Current project
  fixtures exercise PNG, glTF and WESL; a future Open/Preview action must consult its
  actual loader capabilities, not enable itself solely from this hint.
- Effect-local texture/mesh declarations and flipbook metadata stay inside the effect.
  Discovery does not invent project IDs or standalone flipbooks for these sources.
- `.aestra`, `.git`, `.hg` and `.svn` entries are excluded case-insensitively. Other
  hidden files remain visible. Encountered symlinks and Windows reparse points are
  retained as Link rows but not traversed/parsed; a linked root is unavailable.
- Missing/non-directory/unreadable roots are unavailable, not successful empty scans.
  Unreadable subdirectories retain a row and diagnostic while readable siblings remain
  visible. Enumeration is a best-effort filesystem observation, not an atomic snapshot
  of disk or a security boundary against concurrent path replacement. AB2 reconciles
  revisions; AB6 must separately preflight mutations and reference completeness.

The editor's existing `project.rs` still chooses a conventional containing `assets/`
directory or an explicit asset directory. AB2a rejects an explicitly selected link/reparse
root (and a conventional linked `assets/` root) before canonicalization loses that
information. This checks the selected entry, not every ancestor or concurrent replacement;
mutation containment/preflight remains a separate AB6 requirement.

## Library parity inventory

The polling/catalog service moved in AB2a; AB3a now supplies source browsing and guarded
effect activation, with a Library switch for all remaining legacy workflows.
Retain each existing workflow until its replacement
passes both command and interaction checks; AB8 is the removal gate.

| Existing workflow / owner | Existing evidence to retain | Destination / acceptance |
| --- | --- | --- |
| Project root/open folder — `project.rs` | `project_folders_and_external_effects_use_the_same_asset_root`, invalid-folder preservation | AB2/AB3 guarded root switch, generation-scoped navigation; no ancestor-wide implicit scan. |
| Effect open and source back/forward — `persistence.rs` | `catalog_open_action_uses_stable_id_and_document_protection_path`, `source_navigation_round_trip_restores_playhead_selection_and_parent_context` | AB3 explicit activation through document protection; selection alone never opens. |
| New effect/save and reusable source creation — `persistence.rs`, `library.rs`, project index | Creation collision/root tests and `project_catalog_creates_effects_in_the_effect_authoring_folder` | AB6 selected-folder destination with the same source safety. |
| Effect source rename/move/delete and usage inspector — `library.rs`, project index | Open/dirty rename tests, reverse-usage navigation, project operation/dependency contracts | AB6 common preflight and recoverable operations; existing permanent-delete behavior is not the new default. |
| Extract selected emitters / explode referenced clip — `library.rs` | `reusable_effect_extraction_replaces_selected_emitters_and_is_undoable`, recursive explode and boundary-link tests | AB7 effect-context commands retain undo, source creation and resource remapping. |
| Broken effect-reference repair — `properties/referenced_effect.rs`, `properties.rs` | `repairing_a_clip_source_preserves_instance_state_and_is_undoable`, candidate-window/cycle rejection | AB7 typed picker adapter; retain current Properties route and instance state. |
| Built-in + project material presets — `library.rs`, material preset actions | Catalog merge, metadata/scope filtering tests | AB3 transitional route, AB7 virtual Built-ins source and compatible preset application. |
| Current Document resources; Add Sprite Material / Add Grid Flipbook — `library.rs`, session commands | Resource projection across new/open/undo/redo; localized resource labels | AB7 explicit document-local section/Properties; no fabricated file rows or IDs. |
| Project effect drag to timeline — `library.rs`, timeline drop handlers | Drag cleanup/propagation and existing timeline command tests | AB7 typed payload and one undoable insertion; invalid/cancelled drop is a no-op. |
| Shared material programs/functions and unsaved drafts — `material_drafts.rs`, `material_graph/`, `history.rs` | Exact-byte conflict and material-only-save tests | AB4/AB5 standalone targets; do not discard an effect-context draft during browser selection. |
| Search/filter/list activation/context menu — `library.rs`, shared Feathers list/focus controls | In-place filtering, live-search, semantic activation and keyboard-context tests | AB3 tree/grid/list parity, scroll isolation and keyboard-only workflow. |
| Unsaved-change/migration/recovery dialogs — `persistence.rs` | Save/Discard/Cancel routing, failed navigation, external changes, autosave/cleanup and migration backup tests | AB2–AB5 shared document coordinator; keep source navigation and window-close protection. |
| Docking/settings/localization — `docking.rs`, settings/persistence and locale resources | Persisted `DockPanel::Assets`, compact surfaces and localized Library tests | AB3/AB8 reuse dock identity; preserve closed/floating panels and restart state. |
| Polling, clean reload, dirty conflicts, texture root — `project_content/`, `library.rs` | Stable-observation debounce, project-switch baseline, internal-save echo and moved dirty source tests | AB2a background generic refresh, AB2b1 cached queries and AB2b2 explicit-open/post-write workers implemented; texture-root sync retained. |

There is no general project-file Duplicate action in the current Library action enum.
Do not confuse duplicate-ID diagnostics or emitter duplication with source duplication.
AB6 adds source duplication with a fresh semantic identity and explicit remapping rules.

## Characterization and remaining verification

- Existing project tests cover all four semantic kinds, duplicate IDs, invalid/future
  formats, semantic source moves, exact source baselines, creation collisions,
  out-of-root operations, dependency cycles and reverse usages.
- New `project_content_contract` tests cover mixed generic/semantic content, local
  resources, hierarchy/ordering, source/index joins, ambiguity, legacy RON, unavailable
  roots, metadata/case preservation, exclusions and read-only scan behavior.
- Source-tree unit tests inject directory-read failures without relying on platform
  ACLs. A Unix-only test covers invalid-UTF names and case-folded hash collisions.
- Local Windows symlink creation returned error 1314 (privilege unavailable). The
  link traversal test reports this explicitly; real symlink/junction traversal and
  Unix-native-path validation remain platform acceptance checks, not local passes.
- No browser UI or filesystem operation has been added. Manual browser acceptance,
  remaining foreground-read removal, mutation preflight and migration checks remain in AB2–AB8.

AB2a adds `project_content_refresh_contract` tests for generic changes, settling,
same-size/restored-time edits, stale versions, folder relocation and unavailable roots.
Editor refresh tests cover generic/unchanged Bevy change detection, captured-byte apply,
stale document/project/write results, and preservation of program/function drafts and
their exact-byte save guards. Background snapshots are observations, not filesystem
transactions; changes after validation are reconciled by subsequent polls.

AB0/AB1 local validation used `+1.98.1-x86_64-pc-windows-msvc`: project tests (47 reported
passes, including the native-link capability skip above), compiler tests (102), editor
binary tests (483), workspace/all-targets strict Clippy, formatting and diff whitespace
checks. These are model/regression checks, not manual acceptance of a browser UI.

AB2a local validation: project suite 54 reported passes (the native-link privilege
caveat still applies), editor suite 491 passes, workspace/all-targets Clippy with
warnings denied, formatting and diff whitespace checks. Windows dialog/verbatim path
lookup is tested without I/O during lookup. No manual browser UI acceptance is claimed.

AB2b1 retains parsed source documents in the immutable content snapshot. These are
source-keyed contents, not a second semantic registry: all typed queries still resolve
through its single index. Dependencies and usage inspection share traversal logic with
the existing disk-based APIs but use an explicit snapshot read policy. Editor graph,
Properties, timeline, presets, diagnostics and preview dependency resolution now read
the published snapshot plus unsaved drafts. Cached data never authorizes a disk change;
delete confirmation and source mutation/save/edit preflight still validate current disk.

AB2b1 local validation: project suite 58 reported passes (native-link privilege caveat
unchanged), compiler suite 102 passes, editor suite 494 passes, workspace/all-targets
Clippy with warnings denied, formatting and diff whitespace checks.
New tests remove source files after indexing and exercise all cached semantic kinds,
dependency/usage resolution and compilation; disk-validating operations still fail.
Duplicate-ID tests cover all four kinds. Refresh tests verify cache replacement and
draft overlays, and first-edit preflight rejects a newer external program.

AB2b2 moves explicit project/effect opening and source save/create-extraction/rename/move/
delete preparation into serialized background jobs. Workers keep existing exact-byte
preflight, recheck deletion usages against freshly discovered owners, reconcile after
partial failures and prepare coherent snapshots. Main-thread publication does not read
disk, replace newer effect/material edits or silently accept undo during a save. Completed
writes cannot be dropped by another operation or native window close. Save/Discard/Cancel,
source navigation and failure/root-switch recovery are covered by the asynchronous routes.

AB2b2 local validation (`+1.98.1-x86_64-pc-windows-msvc`): editor suite 518 passes;
project suite 58 reported passes and compiler suite 102 passes; workspace/all-targets
Clippy with warnings denied, formatting and diff whitespace checks. Tests pause between
worker preparation and publication to exercise races deterministically. Native-link
privilege/Unix caveats above remain. Native file/folder/save and migration dialogs have
not been manually revalidated with these workers; the folder-preparation path is tested
without opening a dialog. Startup/recovery bootstrap, settings/recovery writes and narrow
first-edit/explode disk preflight remain synchronous and outside this scan-removal slice.
The legacy Library remains available through AB3a's transitional implementation switch.

AB3a adds folder navigation, grid/list browsing, recursive search, multi-type filters,
deterministic sorting and independent source selection in `DockPanel::Assets`. Both
folder and result entities are bounded by pages; file rows survive selection/filter/view
changes within a bounded cache. No new filesystem scan, semantic catalog or runtime-adapter
dependency was added. Explicit effect activation keeps the existing dirty-document guard;
invalid/duplicate effects cannot open an arbitrary same-ID file. Standalone material/function
editing, source relations, root-scoped restart persistence and thumbnails are not included.

AB3a automated checks cover snapshot-only browsing after disk removal, navigation/root
fallback, filtering/layout parity, row/control identity, bounded rows, keyboard folders and
guarded effect routing, duplicate rejection, locale coverage and preview scroll/pan isolation.
Native narrow/wide/high-DPI/floating-panel checks and the 10,000-source benchmark are still
pending AB3c acceptance; automated entity/observer tests do not substitute for visual QA.

AB3a local validation (`+1.98.1-x86_64-pc-windows-msvc`): 529 editor tests plus the
runtime-independence architecture test pass; workspace/all-targets Clippy with warnings
denied, formatting and diff whitespace checks pass. Popup-isolation coverage includes
removing hidden-menu blockers even when their relative cursor data is stale.

AB3a follow-up adds compact browser chrome, folder icons, denser list rows and larger
grid type tiles (not rendered thumbnails). Single-click selection shows cached source
metadata/diagnostics. Row-based double-clicks survive a change of descendant hit target,
focus the list for Enter, and have an explicit Open button. Materials used by the current
effect open through its existing renderer graph; unused programs explain that standalone
editing remains AB4. Duplicate material IDs remain rejected and browsing does not alter
the effect. Automated coverage now includes real Bevy text measurement/layout at multiple
pane widths/UI scales and pointer/material activation. Native visual QA remains pending.

Effect-open follow-up: reproduced inert double-click/Enter/Open in the native editor.
The shared open dispatcher marked the catalog changed merely by taking a mutable
reference; viewport synchronization reinstalled the current preview and invalidated
the background loader's revision guard. Normal and discard-confirmed opens now queue
using an immutable catalog reference. The regression includes real background loading
and viewport synchronization (failed before the fix, passes after it), plus the Open
button route, discard confirmation and visible operation-status updates. Genuine
concurrent-edit rejection tests remain passing. Local validation: 535 editor tests,
the architecture test, workspace/all-targets strict Clippy and formatting pass. The
fixed executable is rebuilt; final native retesting paused because Windows was locked.

AB3b adds independent Details/Dependencies/Used By inspection with snapshot-only direct
references and bounded, scrollable pages. It distinguishes missing files, duplicate IDs,
unavailable sources, built-ins and context-dependent texture bindings. Unsupported formats
have unknown dependencies; incomplete indexing produces an explicit partial-usage warning.
Locate routes from reference rows, the current effect and the material graph reveal the
source through filters/pagination without opening it or modifying emitter/document selection.
Regression coverage includes immutable reports after disk removal, function/material/effect
inverse references, duplicate candidates, cycles, stale-version rejection, retained browser
rows, list/grid locate focus and bounded inspector pagination. Native validation remains
paused by the Computer Use skill because Windows is locked; no visual success is claimed.

AB3b local validation (`+1.98.1-x86_64-pc-windows-msvc`): 540 editor unit tests and
60 project tests pass. Workspace/all-targets Clippy with warnings denied and formatting
checks pass. Locate focus is consumed once, so subsequent shell rebuilds do not steal
focus back from another control. Existing user material and editor-layout edits are preserved.

Ergonomics follow-up: the browser no longer embeds an automatically visible inspector.
Selection only highlights; double-click and Enter still open. Context-menu Asset Details
and References explicitly open a closable/dockable inspector, normally in the Properties
tab stack, without retargeting the active effect/emitter. Later browsing leaves that source
pinned. The browser footer only shows counts/paging. Metadata lives in delayed hover
tooltips; the snapshot explanation is an info tooltip. Healthy references use compact rows
with Locate icons; missing/ambiguous warnings remain visible. The duplicate list/inspection
icon and redundant Open/project buttons have been removed from the toolbar; asset Open
and background Open Project/Refresh remain in context menus. File-menu workflows remain.
Native visual verification of this layout is still required; automated tests are not visual QA.
The ergonomics follow-up passes 543 editor tests, the runtime-independence architecture
check, workspace/all-targets strict Clippy, formatting and diff whitespace checks.

### AB3c persistence and acceptance — 2026-09-07

Per-root preferences now persist under `.aestra/asset-browser.ron`, independently of
semantic assets and editor docking layout. Automated coverage includes round-trip,
fallback from missing folders, root switches, same-root generation refresh, corrupt/
future formats, path escapes, debounce and exit flush. Selection and inspection do
not cause preference writes or authored edits. The 10,000-source measurement and its
host/profile/revision are recorded in [the benchmark](../benchmarks/asset-browser/README.md).

Divider investigation reproduced a real capped-width dead zone: at a 320-pixel panel
width, a saved 360-pixel source pane is displayed at 176 pixels; dragging left 20
pixels previously left the divider unchanged. The drag now anchors to the rendered
width and uses cumulative distance. Layout/observer regression coverage spans
320/640/1440 logical pixels at 100/150/200% target scaling, with a secondary-window
camera. Folder captions retain their earlier UI-scale/layout checks. These tests
do not run OS pointer delivery, native picking or detached-window presentation.

Native observations across the persistence and acceptance slices:

- Wide docked folder navigation and grid browsing work without changing the active
  effect. Folder/grid preferences were restored on editor restart; selection was not.
- Explicit References opens the separate inspector while browsing stays independent.
- Divider drags through Computer Use landed on folder rows rather than reliably
  exercising the handle. This is inconclusive, not a native pass for the resize fix.
- Native narrow/high-DPI and detached-window visual/input gates remain outstanding.
  The earlier locked-desktop interruption is historical, not the current blocker.
  Tooltip path overflow and the inspector's info glyph also need visual follow-up.

At this point AB3 remained in acceptance, not complete (superseded by the user
acceptance below). Headless layout/benchmark results were not native verification.
Existing material edits were preserved; native browsing only changed local browser
preferences, returned to the Effects folder before closing the QA editor.

Local validation for this acceptance slice: 551 editor unit tests pass (the opt-in
benchmark is ignored by the normal suite and passed separately); workspace/all-targets
Clippy with warnings denied passes on `+1.98.1-x86_64-pc-windows-msvc`.

AB3c polish follow-up: shared tooltip text now uses word wrapping with a character
fallback and a zero flex minimum width, so long unbroken paths/identifiers cannot
force text beyond the popup. Layout tests preserve the full text and verify measured
bounds/multiple lines at 180/280-pixel widths and 100/150/200% UI scales. The inspector
scope hint uses `icons/info.svg` with explicit light tint and a fixed 16-pixel image,
not a missing font glyph; its hover description and accessible name are unchanged.

The updated native editor built and reopened with the Effects folder/grid preference
restored and no selected asset. Divider automation still selected an asset rather than
reliably hitting the thin handle; subsequent native attempts encountered screenshot-ID
and concurrent-user-input errors. A manual divider check was requested. Narrow/high-DPI,
floating-window, tooltip/icon visual confirmation and docking restart acceptance are
were unverified by the agent at that point. They remain historical verification limits,
not automated or agent-observed native passes; see the subsequent user acceptance below.

Polish validation: 552 editor tests pass (one opt-in benchmark ignored), the editor
build and workspace/all-targets strict Clippy pass, and formatting/whitespace checks
pass. Existing user material edits and local browser/docking preferences are excluded
from this code change.

### AB3c user acceptance and AB4a foundation — 2026-09-07

The user explicitly confirmed "AB3c is good do AB4". AB3c is now accepted on that
basis. The native automation limitations above are retained as provenance, not as a
current blocker or as newly verified platform tests. The transitional Library remains.

AB4a adds a standalone material authoring document without an effect. Program/function
commands, graph planning, validation, inspection, compilation and command history share
the existing machinery. Effect-local operations fail with `EffectContextRequired` and
leave the document/history unchanged. Compilation reports now include the document's
function library; a regression reproduced the previously missing-function report after
successful extraction before the fix. Existing effect-context serialized snapshots retain
their original representation; standalone snapshots omit the effect. Semantic source
formats are unchanged; Rust consumers account for the optional effect context.

Six standalone contract tests cover edits/undo/redo, validation failure and retained redo,
all effect-only command families, binding API errors, migration/instance inspection,
duplicate IDs, function extraction and missing functions, legacy snapshot compatibility
and standalone/source serialization. This is a foundation slice, not an editor UI pass:
AB4b explicit target/open/edit/history routing and AB4c save/reload/recovery/conflict
handling plus native acceptance remain pending.

AB4a local validation on `+1.98.1-x86_64-pc-windows-msvc`: the full authoring suite
passes, including all six new standalone contracts; 552 editor unit tests pass (one
opt-in benchmark ignored), and both portable-GPU legacy-material migration contracts
pass. The editor/runtime-independence architecture test, workspace/all-targets Clippy
with warnings denied, formatting and diff-whitespace checks pass. User-authored material
changes and local `.aestra` preferences are preserved and are not part of this slice.

### AB4b standalone editor target and editing — 2026-09-07

Assets double-click/Enter now opens an unused project material in an explicit
root-scoped graph target without changing the active effect, emitter selection,
playback or effect history. Opening resolves unique identity from the published
snapshot and overlays retained shared drafts; pending I/O/protection dialogs block
opening. Switching targets invalidates stale queued navigation while allowing saves
to merge baselines for the same effect document. Duplicate, missing and cross-root
stale identities fail explicitly. Reopening the same target avoids a graph rebuild.

Graph actions and shared-source Properties (name, domain, draft state, parameter
default summaries and diagnostics) no longer require a renderer. Node previews use
default context and invalidate when the source changes independently of effect
revision. The existing toolbar includes a back-to-effect action. Graph view/layout
memory is retained; target changes clear selection and pending gestures.

Per-root/per-program undo stacks retain effect redo. Pointer interaction with graph
or shared Properties selects material history; viewport/timeline/curves select effect
history. Menus and Assets retain the last editing scope, including floating panels.
Conflicting newer drafts reject stale undo without consuming its entry. Keyboard-only
focus transitions and native interaction acceptance still need AB4c verification.

Eight new headless regression tests cover unused-source opening, target/root/duplicate
guards and draft retention, isolated histories and stale inverses, graph add/delete,
preview source invalidation, and shared Properties rename/stale-event handling. The
existing browser activation test now covers double-click, explicit Open and Enter.
Local validation on `+1.98.1-x86_64-pc-windows-msvc`: 560 editor unit tests pass (one
opt-in benchmark ignored), the editor/runtime-independence architecture test passes,
and workspace/all-targets strict Clippy passes. Formatting and diff-whitespace checks
pass. No native UI acceptance is claimed.

AB4c remains pending: opening here is a non-destructive snapshot-backed target switch,
not a fresh-disk reload. Existing first-edit baseline checks, shared-draft recovery and
combined effect/shared-draft save remain in place. Material-only save/reload, active
target recovery, fresh-source lifecycle and end-to-end conflict/reopen acceptance are
the next slice. User-authored material files and local `.aestra` settings are untouched.

### AB4c1 material-only Save and guarded Reload — 2026-09-07

Standalone File > Save Material / Ctrl+S now writes only the selected shared program
and its transitive edited function dependencies. It does not request an effect filename,
save a dirty named effect or commit unrelated drafts. Source identity is refreshed on
the serialized I/O worker before writing; exact-byte preflight and scoped partial-save
receipts reuse the existing conflict/atomic-file-write machinery. Receipts preserve
concurrent edits, target switches and undo intent. Material Save As is intentionally
unavailable until the safe duplication milestone; its shortcut cannot launch an effect
Save As dialog while a standalone target is active. Effect-context and destructive-
navigation saves retain the existing combined effect/shared-draft behavior.

File > Reload Material resolves the selected semantic ID from a fresh snapshot. A dirty
program requires Save/Discard/Cancel, with material-specific confirmation text. Discard
removes only that program's draft after successful loading, not unrelated function or
program drafts. Missing/malformed/ambiguous sources, changed targets and stale results
retain unsaved work. Save followed by Reload rechecks for concurrent edits and prompts
again. Successful reload clears that program's standalone history and transient graph
selection/gestures without replacing the effect, its selection, playback or history.

Eleven new regressions cover untitled and dirty named effect isolation, Save As routing,
unrelated drafts, transitive function saves, exact-byte conflicts, duplicate identities,
save receipts during target changes, Save/Discard/Cancel, concurrent save/reload edits,
moved/missing/malformed sources, stale completion/confirmation and undo after Save.
The retained dialog regression also checks switching between effect and material
confirmation descriptions without rebuilding the overlay. Local validation on
`+1.98.1-x86_64-pc-windows-msvc`: 571 editor tests pass (one opt-in benchmark ignored).
The editor/runtime-independence architecture test, workspace/all-targets strict Clippy,
formatting and diff-whitespace checks pass. User material files and `.aestra` preferences
are preserved outside this implementation.

AB4c2 remains pending: active-target recovery, fresh-source open/reopen and recovery
conflicts end to end, native UI acceptance and keyboard-only focus routing. Browser
opening still selects a snapshot-backed target; explicit Reload performs fresh disk I/O.
No native acceptance or restart recovery of the active material is claimed here.
