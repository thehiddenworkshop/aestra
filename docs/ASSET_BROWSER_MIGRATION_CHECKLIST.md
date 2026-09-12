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

### AB4c2 active-target recovery and keyboard history — 2026-09-07

Recovery v3 stores the standalone material target (project root and semantic ID) with
the effect and shared drafts. Legacy v1/v2 snapshots restore effect context. Autosave
tracks target and draft changes independently of effect revisions, fixing repeated
material-only edits not reaching the snapshot. A clean standalone target remains
recoverable; returning to a clean effect clears the tracked snapshot.

Restore prepares the exact recorded project before publishing the session, validates
draft paths/identities, and rebinds moved sources only through unique semantic IDs.
Original bytes remain the save-conflict baseline. External edits therefore remain
conflicts, while missing/duplicate sources retain the explicit target and recovered
drafts with diagnostics. Unsafe paths and inconsistent roots reject restoration without
replacing the session. Rejected/unsupported snapshots remain untouched, including when
a new session autosaves the same effect ID. The restored target reveals Material Graph.

Keyboard focus traverses the same panel history scopes as pointer focus. Neutral menus
and Assets retain the last editing scope; focus-only changes do not rebuild graph UI.
Ten new active regressions cover recovery, conflict preservation, legacy versions,
autosave and keyboard focus. Editor validation: 581 tests pass, two opt-in utilities
ignored; strict workspace/all-targets Clippy, the editor/runtime-independence architecture
test, formatting and diff-whitespace checks pass. The new ignored
`native_material_recovery_fixture` test produces isolated assets and an
`AESTRA_CONFIG_DIR` for native restart verification without touching user settings.

Native recovery-dialog/graph/preview and lifecycle acceptance have not been completed;
approval to accept the isolated recovery prompt is pending. AB4 stays open until that
gate passes. Normal quit/discard cleanup is unchanged: this is interrupted-session
recovery, not automatic workspace reopening after clean exit, and undo stacks are not
serialized. Browser opening remains snapshot-backed; explicit Reload reads fresh disk
state. User material sources and local `.aestra` preferences are untouched.

### AB4c2 native verification pass — 2026-09-07 (partial; exit gate still open)

Ran the ignored fixture generator successfully, then launched the actual editor with
`AESTRA_CONFIG_DIR` pointing at isolated `target/material-recovery-smoke-*` settings.
All source mutations below were confined to synthetic test assets, not bundled/user
materials. Observed in the native UI:

- Startup restored `Recovered edit` as the active unsaved shared material, revealed
  Material Graph, and continued rendering the separate `Editor Test Effect`.
- Editing the material name and File > Save Material wrote the material file, reported
  zero unsaved shared materials, and left the effect untitled/unsaved. No effect source
  was created in the synthetic project.
- Moving the source within the project and changing its name on disk retained the
  semantic target. File > Reload Material completed and displayed the disk name.
- Show All Previews produced square constant/output previews. Menu Undo and Redo
  reversed/reapplied a shared-material rename without changing the effect identity.
- An external edit while a draft was active produced a conflict. File > Save Material
  failed; the external disk name and the unsaved draft were both retained. Reload showed
  material-specific Save/Discard/Cancel text; Cancel retained the draft.
- Duplicate semantic IDs and missing sources displayed diagnostics and rejected Save.
  The shared-draft count remained one; no arbitrary source was selected or recreated.
- A subsequent UI rename appeared in the autosave snapshot alongside `material_target:
  Program`. Stopping only the isolated editor and restarting it produced a recovery
  prompt. Acceptance of that second prompt awaits confirmation; restoration of this
  second UI-authored edit is not yet counted as passed.

Findings blocking full acceptance:

1. Injected Ctrl+A inserted `a`, and the automation paste operation inserted `v` in the
   Feather name field. Enter and menu Undo/Redo worked. Code inspection shows both
   Feather's text-input observer and editor shortcuts consult frame-level
   `ButtonInput::pressed` modifier state after input collection; press/release in one
   frame can therefore lose the modifier. This is a plausible mechanism, not yet an
   instrumented reproduction of the native event stream. Keyboard-only history/focus
   acceptance remains pending; do not count the menu checks as keyboard validation.
2. Long duplicate/missing/conflict diagnostics overflow the narrow Properties/status
   areas. The data-safety behavior works, but the presentation needs bounded wrapping
   and a readable diagnostic detail surface.

Effect-context return/reopen, built-in read-only native acceptance, and the remaining
reload confirmation branches still need verification. Do not advance to AB5 on the
basis of this partial native pass.

### In-app recovery popup — 2026-09-07

Replaced the startup OS recovery alert with a retained Feather modal once the editor
UI is available. It shows effect/material identity, relative snapshot age and shared
draft count in English/French. Restore, Discard Recovery and Decide Later are explicit;
Escape/X preserve the candidate for a later startup. Only Discard Recovery deletes it.
Restore/discard failures retain the dialog and pause autosave for retry. Closing the
application while recovery is pending also preserves the snapshot. Modal protection
blocks editing/shortcuts, viewport navigation and the transform gizmo.

Six automated regressions cover deferred dismissal, explicit discard, restoration,
failure/retry retention, autosave/window-close protection and retained/localized modal
focus. Native inspection with a fresh isolated material recovery fixture confirmed
readable details/buttons and Escape dismissal back to the existing effect. The snapshot
SHA-256 was unchanged after dismissal and clean application exit. Restore/discard are
automatically tested; this popup pass does not claim new native acceptance of those
branches or the outstanding AB4 lifecycle/keyboard checks above.

A second native launch confirmed the same candidate was offered again, the transform
gizmo no longer drew over the popup, and closing the application with recovery still
pending again preserved the snapshot hash. Final validation: 587 editor tests passed
(two opt-in tests ignored), architecture isolation passed, strict workspace Clippy and
formatting/diff checks passed.

### AB4c2 keyboard modifiers and OS clipboard — 2026-09-07

The earlier modifier hypothesis is now reproduced: stock Bevy 0.19.1 dispatch of
Ctrl-down/A-down/A-up/V-down/V-up/Ctrl-up in one frame queued literal `a` and `v` in
Feather's editable text. The same regression now queues SelectAll and Paste.

- A pinned, licensed `vendor/bevy_input_focus` compatibility patch captures pre-frame
  key state, supplies per-event state during focused dispatch, and restores frame state
  afterward. Other focused widget behavior remains upstream. Its scope and removal
  criteria are documented in `vendor/bevy_input_focus/AESTRA_PATCH.md`.
- History, document, material graph, timeline, viewport and transport shortcuts use
  ordered physical keypress snapshots instead of end-of-frame modifier state. Repeats
  are excluded from discrete actions and retained for deliberate frame stepping.
- Regressions cover plain text around a chord, held/left/right modifiers, selection,
  focus loss, repeats, text-focus history suppression, ordered Undo/Redo, Save/Save As,
  and Alt+arrow navigation not leaking into transport stepping.
- Native testing found the second paste failure: Bevy's `system_clipboard` feature was
  not enabled. Enabled it for aestra-editor, preserving upstream clipboard behavior.
  Before this change, Ctrl+A selected text correctly but OS paste inserted nothing.
  Afterward, pasting `prism` replaced selected text and filtered the Asset Browser to
  Prism Bloom. The temporary search filter was cleared; no material source was edited.

Validation: 595 editor tests passed, two opt-in tests ignored; architecture isolation
passed; 33 vendored input-focus tests passed. Strict workspace and vendored-dependency
Clippy, plus formatting checks, passed. Eight new editor regressions supplement the
native text-entry check. Native shared-material keyboard Undo/Redo and lifecycle
acceptance are not claimed by this focused search-field pass. Overflowing diagnostics
and the remaining AB4 exit checks still precede AB5.

### AB4c2 bounded diagnostic presentation — 2026-09-07

- The footer presents a single-line, bounded operation summary and a Feather Details
  action. Full status text is retained; long paths no longer determine footer width.
- Standalone material source errors and validation messages use wrapped, bounded
  summaries with Details actions. The Properties scrollbar is beside its viewport,
  not below it. Inline effect diagnostics and validation paths also wrap long tokens.
- Details opens the Diagnostics dock with the complete original message in a wrapped,
  vertically scrollable view. It snapshots the selected message, so later operations
  do not silently replace the error being inspected. Back to validation preserves the
  existing severity filter; the compile-status action returns to validation as well.
- English/French actions are localized. This changes presentation only, not source
  loading, recovery, material drafts or effect history.

Native diagnostic presentation and the remaining AB4 lifecycle checks are still
pending; this implementation does not advance AB5 or claim native acceptance.

Validation: 600 editor tests passed (two opt-in tests ignored), plus architecture
isolation. Five new regressions cover Unicode/unbroken-path summaries, retained
status-action visibility, lossless snapshots/back navigation and real text layout at
160/240/400-pixel widths and 1×/2× target scale. The existing retained-footer test now
also checks long messages without a UI rebuild. Strict workspace Clippy and formatting
checks passed.

### AB4c2 native keyboard and interrupted-session follow-up — 2026-09-08

Used a generated, isolated project/configuration under `target/material-recovery-smoke-*`;
the user's material source edits were not changed.

- Restored the in-app recovery candidate, renamed its standalone material through the
  Feather name field, and confirmed that the UI-authored name reached autosave. After
  terminating only that isolated editor process and restarting, the popup offered the
  new name and Restore recovered the same draft and selected material target.
- Native Tab exposed a missing normal `TabGroup` on the editor root. Added that group
  and excluded retained hidden/disabled controls from tab order, restoring their original
  indices when they become eligible again. Tab now leaves the name field for a visible
  control without the former "No focusable entities found" warning.
- Native Ctrl+Z failed on AZERTY while Ctrl+W reached Undo: editor shortcuts were using
  physical QWERTY letter positions. Discrete letter shortcuts now follow logical OS
  letters while retaining per-event modifiers. Physical number-row/numpad shortcuts
  remain intact. Raw focused-widget input is unchanged.
- After a fresh name edit, Ctrl+Z restored the previous name and Ctrl+Shift+Z reapplied
  it. Ctrl+S saved the selected standalone material; disk contents matched the UI name,
  the material became Saved, and the effect remained Unsaved. The moved source was
  resolved by its unique semantic identity instead of recreating its old path.
- Removing the isolated source from indexing preserved its draft and showed readable
  bounded Properties/footer diagnostics. Details displayed the complete missing-source
  path/message with wrapping. This pass does not claim a native extreme-length scrolling
  or detached/high-DPI check.

Further computer control was paused when concurrent desktop input was detected.
Effect-context return/reopen, remaining reload confirmation branches and built-in
read-only native acceptance are still pending. AB4c2 is not marked complete and AB5
has not started. Regression coverage includes the actual editor root/tab eligibility,
AZERTY/QWERTZ logical shortcuts and ordered AZERTY Undo/Redo with text-focus suppression.

Validation: 602 editor tests passed (two opt-in tests ignored), including the final
number-row/numpad compatibility cases. Strict workspace Clippy, formatting and diff
checks passed. Architecture isolation also passed during this acceptance run.

### AB4c2 native material lifecycle follow-up — 2026-09-08

- Returning to the effect and reopening the same standalone material retained the
  draft and effect context in the isolated acceptance project.
- Fixed retained File menu entries failing to follow the active material/effect target.
  The menu now exposes Save Material/Reload Material and hides effect-only Save As
  without rebuilding its controls; bidirectional target-switch regression coverage added.
- Reload Cancel retained the draft. Save initially wrote the source but falsely
  cancelled the following Reload: preview recompilation advances the checkpoint revision.
  A deterministic regression reproduced the exact native error. Material Reload now
  checks authored content, target, catalog, drafts, locks, proposal and document identity
  without rejecting preview-only invalidation. Other I/O retains revision checks.
- Native Save-and-Reload then passed: the footer confirmed Reload, the name matched disk,
  the material became Saved, and the effect stayed Unsaved.
- Reload Discard and built-in read-only native acceptance remain pending. No AB5 work
  started; AB4c2 is not complete. All native edits were confined to a generated fixture
  under `target/material-recovery-smoke-*`, not the user's material files.

Validation: 604 editor tests passed (two opt-in tests ignored), including all 12 material
lifecycle cases. Strict workspace Clippy passed.

### AB4 user acceptance / AB5 authoring start — 2026-09-08

The user accepted AB4 and explicitly requested AB5. The pending native Discard and
built-in checks above remain unexecuted; acceptance does not turn them into test evidence.

AB5 begins at the shared authoring boundary: `ReplaceMaterialFunction` preserves asset
identity and uses existing atomic validation/history. `plan_function_edit` inspects a
candidate without mutation, reports direct program/function call sites in the supplied
document, and retains validation diagnostics for incompatible signatures and cycles.
This is not yet a complete project usage inventory or editor function UI. No fake
effect/program is needed. Tests cover standalone signature/body edits and undo/redo,
identity rejection, caller preservation, recursive-body diagnostics, and exact custom-WESL
source preservation during metadata edits. Function-native projection/defaults, editor
target integration and guarded persistence remain subsequent AB5 work.

Validation: the complete `aestra-authoring` test suite passed, including five new
function-document contracts. Strict workspace Clippy, formatting and diff checks passed.

### AB5a function projection and input defaults — 2026-09-08

Added a function-native projection with stable signature IDs, authored expression nodes
and input/output edges. Invalid sources are retained for repair; custom WESL returns its
unaltered source/entry-point representation rather than a fake editable graph. This is
structural projection, not per-node inferred-type/preview analysis or editor UI integration.

Function inputs now carry an optional typed default. Existing files deserialize as required
inputs and serialize without new fields when unset. Validation rejects mismatched/non-finite
defaults; compiler expansion uses defaults only for omitted arguments (including custom WESL),
while explicit arguments retain precedence. Node creation uses declared defaults before its
existing type fallback. Existing Rust constructors explicitly retain required-input behavior.

Core/compiler/authoring suites passed, followed by all 15 function compiler contracts;
workspace all-target checking and strict Clippy passed. Granular function-body commands,
editor target/UI and function persistence remain pending; no native function UI is claimed.

### AB5a granular function-body commands — 2026-09-08

`EditMaterialFunctionBody` adds a function-native command vocabulary for expression
add/remove/replace/rewire, output assignment and explicit call-argument connection or
disconnection. It reuses the existing clone/validate/commit executor and bounded history;
inverse snapshots preserve expression order, signature IDs, defaults and source metadata.
Graph commands reject custom-WESL bodies. Transactions may repair links and remove nodes
together; deleting a connected node alone fails without implicitly removing any link.
Disconnecting an optional call input restores its default, while disconnecting a required
input fails validation. No filesystem or active-effect changes are introduced.

Regression cases cover all command kinds, complete undo/redo round trips, invalid IDs,
indices, duplicate expressions, cyclic edits, redo preservation after rejected edits,
default restoration and read-only custom WESL. AB5a authoring contracts are implemented;
AB5b editor opening/controls and AB5c persistence/native acceptance remain pending.

### AB5b function opening and inspection — 2026-09-08

Function assets now open via Enter/double-click into an explicit root-scoped target,
including unused functions. The active effect and emitter selection are preserved.
The read-only inspector shows typed signatures/defaults, graph structure or custom-WESL
source, and projection diagnostics; it does not yet provide an editable node canvas.
Save/Save As and Undo/Redo cannot accidentally modify the effect from this target.
Source ambiguity and wrong-root resolution fail explicitly. Existing recovery target
serialization recognizes functions, but full function persistence/recovery acceptance
remains AB5c. Native visual acceptance has not been performed for this slice.

Automated coverage added for browser activation/reopening, preserved effect selection,
source/root validation, target serialization and guarded Save/Save As. Interactive
signature/body controls, function-scoped history and dependent previews remain AB5b work.

Validation: editor suite passed (607 passed, 2 ignored); strict workspace/all-targets
Clippy, formatting and `git diff --check` passed. Native UI inspection remains pending.

### AB5b graph-function signature drafts and history — 2026-09-08

The function inspector now offers Feather name fields, type menus, add/remove controls,
and typed defaults. Blank defaults retain required-input semantics; scalar defaults use
the shared draggable number behavior. Vector/color defaults use comma-separated fields.
New outputs initially return zero; body rewiring and output-expression selection are not
part of this slice. Custom WESL remains read-only, including its signature.

Edits preflight indexed material programs and functions, report direct callers and
validation errors, and retain existing state/history when rejected. Stable port IDs survive
rename/type/default edits. Draft replacement uses exact disk baselines and optimistic
current-value checks. Undo/Redo uses separate project-root/function-ID stacks and does not
edit the active effect. Dedicated function Save/Reload remains AB5c; existing shared-draft
protection and Save All continue to cover these drafts.

Automated coverage includes final Feather text/activation events, signature creation,
typed defaults, invalid references/default rejection, stale Undo protection, custom-WESL
read-only handling, caller preflight/dependent recompilation, and per-function Undo/Redo
with unchanged effect selection and source bytes. Native UI acceptance remains pending.

Validation: editor suite passed (613 passed, 2 ignored); strict workspace/all-targets
Clippy, formatting and whitespace checks passed.

### AB5b initial function-body canvas — 2026-09-08

Graph functions open into a function-native Feather canvas with movable/collapsible nodes,
pan/zoom/frame-all, rendered connections and a declared-output node. Dragging between
input/output sockets routes to function-body rewiring or output mapping, in either direction.
The initial add menu offers Float, declared inputs, Add, Multiply and Smoothstep. Operations
and their initial scalar constants are added atomically. Scalar constants use the shared
draggable numeric input; deletion rejects remaining references. Signature controls moved
to Properties, while custom WESL retains read-only inspection.

Body edits use the authoring body-command executor, candidate caller preflight and the
same root/function-scoped history as signature edits. No surrogate material or effect is
created. Graph view/node positions are session memory, not new persisted source fields.
Automated coverage includes add-button/socket-drop events, atomic output remapping,
referenced-node deletion rejection, cyclic rewire rejection, Undo/Redo and untouched effect
and source bytes. Native visual/interaction verification is pending. Full creation catalog,
non-scalar editing, connection drag previews and canvas parity remain AB5b follow-up work;
function-only persistence remains AB5c.

Validation: editor suite passed (616 passed, 2 ignored); strict workspace/all-targets
Clippy and formatting passed. Native interaction testing remains pending.

### Material/function graph presentation alignment — 2026-09-08

Both graph adapters now share toolbar chrome and white icon buttons in addition to the
existing common viewport, node, socket and wire widgets. Function graphs use one toolbar
with Back/Add/Frame/Locate, compact header node actions instead of full-width Delete
buttons, the material graph's input labels and dependency-depth layout. Capability gaps
(preview controls, full catalog and gesture feedback) remain tracked in AB5b; they are not
represented by non-functional toolbar buttons.

Dissolve Edge is node-authored. Pulse Wave is a custom-WESL source function with no stored
expression graph. Its read-only view now explicitly identifies a code function and explains
why source is displayed. Neither asset representation nor source contents were converted.
Native visual acceptance of the updated presentation is still pending.

### AB5b typed constants and connection feedback — 2026-09-08

The function canvas now offers vector/color/Boolean literal creation, component-wise
Feather numeric controls and a Boolean selector. Component edits preserve the literal
type and untouched channels and continue through function-scoped draft/history validation.
Default layout accounts for multi-component constant rows. The creation popup uses a
reusable bounded searchable Feather action menu with scrolling/navigation isolation.
Socket drags display a transient wire in either direction; release or removal of the
origin clears its feedback without changing the document.

This is an incremental AB5b slice, not full parity: the function menu still has its
limited native creation catalog rather than the material compiler's full catalog.
Sharing that catalog/creation API without a surrogate MaterialProgram, category groups,
connection snapping/compatibility feedback, and native visual acceptance remain open.
Function-only Save/Reload/recovery remains AB5c. User asset files are not changed.

Validation: all 104 authoring tests passed, along with strict workspace Clippy,
formatting and diff checks.

### AB5b shared creation recipes and socket targeting — 2026-09-08

Function graphs now use the compiler-owned material node catalog and construction
recipes without constructing a surrogate MaterialProgram. Function signature texture
inputs supply texture recipes; unsupported entries and direct recursive calls are
excluded. Creation remains an atomic, validated function-draft transaction.

The bounded Feather creation menu groups searchable entries by category. Native
interaction testing exposed and fixed popup focus/activation timing and a compressed
scroll area. Mouse opening, searching for Multiply, creating its default expressions,
and Undo back to a clean draft were verified in Dissolve Edge. Float creation and Undo
were also verified. No test edits were saved to source assets.

Connection feedback marks validated compatible sockets and snaps within 18 logical
pixels, recomputing the target at release to avoid stale-frame targeting. Automated
coverage checks compatible targets and function-only edits. Full native gesture
acceptance (including near-socket release, pan and zoom) remains pending; a native
near-socket drag attempt did not establish a changed connection.

Validation: 620 editor tests passed (2 ignored), compiler and authoring suites passed,
and strict workspace/all-targets Clippy passed. Shared catalog recipe parity has a
compiler regression test. This does not complete all canvas parity work or AB5c
function-only persistence/recovery.

### AB5b gesture regression follow-up — 2026-09-08

Socket gestures now resolve child hit targets to their semantic socket consistently
through start, movement, drop and release. The regression covers a near-socket release
at a 2x UI scale without a DragDrop event or intervening preview frame, and verifies
that only the function changes. Node action-menu wrappers have bounded header geometry
and structural menu roots ignore picking.

Validation: 620 editor tests passed (2 ignored), strict workspace/all-targets Clippy
passed, and diff checks passed. Native node movement and zoom were observed with
aligned wires; native direct/near-socket drags still did not demonstrate a changed
connection. The native failure's root cause remains unresolved; these defensive fixes
do not establish that it is fixed. Compatibility highlighting, reconnection and its
Undo/Redo, and complete pan/drop acceptance remain pending. No source asset test edits
were saved. AB5b remains open.

### Manual gesture acceptance and AB5c scoped Save — 2026-09-08

The user reports that socket dragging works when tested manually and authorizes
starting the next step. The unsuccessful automated native drag is not considered
evidence of a remaining interaction defect.

First AB5c slice: File Save / Ctrl+S on a function target saves that function and
its transitive function dependencies through the existing guarded background I/O
and exact-byte baseline checks. It does not save the effect or unrelated program
or function drafts. Save As remains unavailable; custom-WESL inspection does not
introduce edits. Regression coverage verifies function save isolation and preservation
of external disk changes and unsaved drafts on conflict.

Function-target guarded Reload, history reset, recovery routing and their native
acceptance remain the next AB5c work; this slice does not close AB5c.

### AB5c guarded function Reload — 2026-09-08

File Reload Function now uses the existing in-app Save/Discard/Cancel guard.
Save persists the scoped function dependency set before re-requesting Reload, so
concurrent edits require fresh confirmation. Discard resolves the selected function
from a fresh project snapshot before removing its draft. Missing or ambiguous sources
and stale completions retain the draft. A moved source is resolved by stable identity.
Only the successfully reloaded function's history/report is cleared; other drafts
and the active effect are preserved. English and French labels explain the scope.

All 17 focused persistence tests pass, including function Cancel/Save/Discard,
missing/moved source handling, history reset and concurrent-edit protection.
Native dialog acceptance has not been exercised in this slice. Function-target
recovery routing and acceptance remain next; AB5c is not complete.

### AB5c function recovery routing — 2026-09-08

Recovery retains clean function targets as well as dirty function drafts, restores
their recorded project and reveals the graph panel through the existing in-app
dialog. Function targets do not enter the legacy effect-only project fallback.
The localized dialog identifies a function by its recovered draft name when available.
Missing/ambiguous identities warn without dropping drafts or choosing another asset.
Moved sources retain their exact original byte baseline, so external edits still
conflict rather than being overwritten.

Regression coverage exercises the dialog restore action, clean-target autosave,
function drafts alongside unrelated material drafts, moved/missing/duplicate sources,
and external-file conflict preservation. Native restart → Restore → Save/Reload
acceptance remains pending; this does not claim the full AB5c exit gate is complete.

### AB5c manual acceptance / AB6a operation foundation — 2026-09-08

The user confirms the manual workflow works and authorizes AB6a. AB5c is accepted.

Initial AB6a backend slice adds `content::operations` typed requests, opaque plans,
results and explicit blocked-operation errors. Folder creation validates portable
names, parent/root membership, link provenance, read-only destinations and case-folded
collisions, then rechecks before exclusive directory creation. It never recursively
creates parents or overwrites destinations and does not claim document Undo support.
Filename Rename is explicitly separate from authored Display Name. Rename, move and
duplicate requests remain blocked until draft-aware reference preflight is implemented;
the legacy mutation APIs are not newly exposed through this layer.

Project tests pass, including destination races, case collisions, unsafe names and
missing parents. Browser operation controls, reference inventory/preflight and semantic
operations remain pending. This is a backend foundation, not completion of AB6a.

### AB6a browser folder creation — 2026-09-08

The browser toolbar now opens an in-app New Folder name prompt, with Create,
Cancel and Escape. The prompt captures the current folder and project version;
changed snapshots reject stale submissions. Filesystem work runs through the typed
project planner on serialized background I/O, never directly in a widget. Completion
refreshes the catalog while preserving shared drafts and reports errors in the status
bar. Names/collisions/link checks remain owned by the project operation layer.

Automated action coverage exercises Create and Cancel without changing the effect.
Native prompt acceptance remains pending. Draft-aware reference preflight and semantic
rename/move/duplicate remain open; this does not complete AB6a.

### AB6a draft-aware reference inventory — 2026-09-08

The project operation module now exposes read-only reference preflight with typed
effect/program/function draft overlays and an explicit host-draft completeness flag.
Draft references replace saved references, source identity is checked, directory
descendants are included, and unknown formats, discovery diagnostics, custom-WESL
include semantics and unresolved/contextual references mark analysis incomplete.
Relation indexing now includes custom-WESL function calls and function texture defaults.

Project tests cover draft-only usages, incomplete host inventories, unknown shader
files and directory descendants. This is not mutation authorization: rename/move/
duplicate remain blocked. Editor draft collection, user-facing preflight and guarded
semantic operations are still pending. No source assets are rewritten by this slice.

### AB6a editor preflight report — 2026-09-08

The Asset Inspector includes a read-only Preflight tab with affected-source/usage
counts and bounded lists of known owners and incomplete-analysis reasons. Editor
collection overlays dirty indexed effects and shared program/function drafts.
Untitled/unresolved drafts, pending proposals, deletions and mismatched draft roots
prevent a complete report. Authored draft changes invalidate the inspector's report;
playback alone does not rebuild it. The report explicitly does not authorize mutation.
Rename/move/duplicate remain blocked pending guarded semantic operation implementation.

### AB6a saved material duplication backend — 2026-09-08

An explicit draft-aware planner now supports a single saved material program or graph
function duplicate. The filename stem is separate from the authored display name.
The copy receives a fresh asset identity; owner-local expression/parameter/signature
IDs, internal wiring and shared asset references remain unchanged. Dirty targets,
incomplete host draft inventories, ambiguous identities, stale source documents,
unsupported asset kinds and custom WESL are rejected. No existing users are rewritten,
so unknown reverse references in unrelated assets do not block this additive operation.

Publication stages and syncs a temporary file, rechecks source bytes, parent/link
provenance and case-folded collisions, then uses exclusive no-overwrite persistence.
Regression tests cover identity/wiring preservation, dirty/stale/deleted sources,
invalid destinations, ambiguity, custom WESL and collision preservation/cleanup.
This is not a multi-file journal or document Undo. The host supplies its draft
inventory at submission and preserves later edits on completion. Browser Duplicate UI integration is next;
generic operation dispatch still blocks duplication, and rename/move remain unavailable.

### AB6a browser saved-asset Duplicate — 2026-09-08

Material and function context menus now open the shared in-app name prompt in Duplicate
mode. The prompt describes saved-copy semantics, accepts a filename stem, validates
case-folded full-filename collisions, and disables confirmation for empty names or
blocked drafts with an explanation. Cancel/Escape reuse the folder prompt behavior.
The project queue checks the submitted snapshot, invokes the guarded duplicate planner,
refreshes discovery and locates/selects the created asset without opening another
document. Later draft edits survive catalog publication and are not included in the copy.
Backend failures (including unsupported custom WESL) are reported in the status bar.

Regression coverage exercises the queued duplicate, selection, independent copy edits,
preservation of the original/active effect and concurrent original drafts, and collision/
dirty-source controls. Native right-click → Duplicate → open/edit acceptance remains
pending. This slice does not enable effect duplication, rename or move.

### AB6a conservative single-material filename Rename — 2026-09-08

Material/function context menus and the focused asset-list F2 shortcut open Rename.
The shared modal starts with the current filename stem; same-name/case-only collisions
and empty names disable confirmation. Escape cancels. Display names, semantic IDs,
graph signatures and source bytes are unchanged. Successful publication refreshes and
selects the new source without replacing the active effect or material identity.

The project planner accepts only same-directory material/program function renames in
fully understood projects. Unknown formats/includes, contextual/unresolved references,
path-backed effect assets (including potential path aliases), dirty target drafts,
incomplete draft inventory and ambiguous identities block the operation with a reason.
This is intentionally conservative: projects containing such assets need AB6b analysis
before Rename can proceed. There is no warning-only bypass or legacy rename fallback.

Background planning inventories project bytes; completion rechecks the editor guard
before synchronous final backend revalidation/publication. New/deleted/edited files,
read-only sources, links and destination collisions cancel. Windows uses MoveFileExW
without replacement; Linux uses RENAME_NOREPLACE. Other platforms explicitly reject
apply. No multi-file rewriting, folder rename, move, or Ctrl+Z support is claimed.
Tests cover preserved effect resolution/signatures, original bytes/display names,
collision refusal, incomplete/stale analysis and draft changes during preparation.
Windows automated verification is provided; Linux and native F2/menu acceptance remain
pending.

### AB6a target-specific Rename analysis and inline blockers — 2026-09-08

Rename now uses an operation-specific reference policy rather than the broad inventory
report's global completeness rules. Raster textures and a parsed drawing-only SVG
subset cannot reference semantic material filenames and no longer block this operation.
SVG scripts/styles/external links/unknown constructs, mesh formats and unknown shader
include behavior remain conservative blockers. Typed semantic/contextual IDs are unchanged
by filename-only rename. Effect path references from saved documents and current draft
overlays are compared against the target using canonical paths where available and
normalized root-relative aliases otherwise; outside-root/unresolvable paths stay blocked.
The generic read-only Preflight report retains its broader conservative policy.

Rename keeps its modal open on failure, displays the backend reason inline, clears it
when the name changes, and permits retry. Confirmation is disabled during preparation;
closing the dialog or changing the submitted name cancels pending publication. Source,
draft and project-byte revalidation and exclusive no-overwrite publication remain intact.
Tests cover unrelated image/icon assets, alias and draft-only filename references,
unsafe SVG constructs, rename/reopen/save with effect resolution, and inline error/retry.

### AB6a inline filename Rename — 2026-09-08

F2 and the browser Rename action now replace the selected asset's caption with a focused,
selected filename input in both list and thumbnail views, without a modal overlay. Enter
submits, unchanged Enter exits without writing, and Escape cancels. Focus loss or an
outside press submits through the same guarded path; invalid names remain editable. The
extension remains protected. Validation blockers mark the inline field and expose their
full message on hover; backend failures also remain in the status bar. Existing draft,
reference, identity and exclusive no-overwrite checks are unchanged. New Folder and
Duplicate retain their dialogs. Native visual/input acceptance remains pending.

Verification: editor tests pass (637 unit tests plus the architecture test; two opt-in
tests ignored), and strict all-target editor Clippy passes. Regression coverage includes
list/grid field layout, Enter/Escape, unchanged names, blank/colliding names and blur.

Inline submission reads EditableText after its queued edits have been applied in
PostUpdate. Focus changes no longer cancel a pending preflight, and same-project catalog
refreshes no longer silently discard the field. Tests exercise queued edits plus Enter,
focus loss, outside press and focus changes while awaiting publication, including the
new visible filename after success.

### AB6a project-wide Rename reference checks — 2026-09-08

The reference check now recognizes the bundled icons' harmless export structures
(`defs`, Sketch shape metadata, XML whitespace hints, the standard SVG 1.1 declaration,
and literal fill/stroke styles). Descendants and attributes are still checked: scripts,
links, general CSS, custom DTDs/entities and unknown constructs block Rename.

Self-contained core JSON glTF is parsed and accepted only when its buffers/images are
embedded and it has no extension payloads. WGSL and the WGSL-compatible subset of WESL
are parsed with Naga to prove there are no imports; this also applies to custom-function
sources and draft overlays. Custom calls use stable function IDs, so they do not alone
block filename-only Rename. The general Preflight report remains conservative.

Regression coverage copies the full bundled project to a temporary directory and renames
both Dissolve Edge and Material Graph Lab there, preserving bytes and asset identity.
Additional cases retain blockers for external mesh URIs, extensions, shader imports,
unsafe SVG content and imported custom-function drafts. Workspace assets are not renamed
by these tests. Native inline-rename acceptance remains pending.

Verification: all 78 project tests and 637 editor unit tests plus the architecture test
pass (two opt-in editor tests ignored); strict all-target Clippy and formatting checks pass.

### AB6a shared semantic asset operations and effects — 2026-09-08

Effects, material programs and graph functions now use the same browser menu/F2 routes,
inline Rename controls, saved-copy dialog, project queue and guarded filesystem planners:
`plan_asset_rename` and `plan_saved_asset_duplicate`. A single backend capability/suffix
query replaces repeated UI type lists. Rename's stale-document comparison is shared too;
there is no effect-specific rename implementation in the Browser. The transitional
Library's authored-name rename remains separate until AB8 (it changes both display name
and filename, unlike filename-only Rename).

`ProjectAssetId` remains the common stable identity abstraction. Filename-only Rename
preserves it and exact source bytes. Duplication allocates a fresh project-level ID;
effect-owned emitter, resource, event, curve and choreography IDs retain their local
wiring, just as graph-owned expression/signature IDs do. Referenced project effects,
materials and functions remain shared. This is not a global ID/sidecar migration for
textures, meshes or arbitrary files, nor support for duplicating custom-WESL functions.

The only new effect-session hook retargets Save after successfully renaming the currently
open clean effect. It preserves the display name, exact saved-byte conflict baseline,
history, playback state and selection rather than reopening or resetting the document.
Dirty sources block submission; edits during rename preflight cancel publication. Saved
Duplicate never replaces the active document and preserves later original edits.

Tests cover the same list/grid menus and inline Rename for all three types, parent-effect
reference resolution, canonical/legacy effect suffixes, duplicate identity/local wiring,
source changes and collisions, open-effect Save after Rename, history/playback retention,
dirty-before/dirty-during rejection and later draft edits during Duplicate. Native effect
Rename/Duplicate acceptance remains pending; AB6b folder/move/recovery work is not enabled.

Verification: 81 project tests and 640 editor unit tests plus the architecture test pass
(two opt-in editor tests ignored). Strict all-target project/editor Clippy, formatting
and diff checks pass. User-authored asset files remain untouched.

### AB6b first slice: folder drops and atomic single-asset moves — 2026-09-08

The user confirmed AB6a works and requested UE-style folder drag/drop. Browser assets
can now be dropped onto tree folders or folder entries in list/grid views. A highlighted
target and status explain basic destination eligibility; releasing opens the shared
Feathers pointer menu with Move Here / Copy Here. Escape/outside-click cancels the menu;
Escape also cancels a drag. Release clicks do not accidentally open the dragged asset.
No filesystem mutation happens on drop alone.

Effects, material programs and functions share the relocation planner used by Rename.
Moves retain exact bytes and stable identity; copies use saved Duplicate with a new
owner identity. Full preflight runs after choosing an action and rechecks current drafts,
catalog/session guards, source bytes, project inventory, references, destination collisions,
root containment and links before publication. A moved open effect retargets Save without
resetting its authored data or playback. The catalog and browser locate the new source.

Each move writes a byte-backup journal under `.aestra/asset-moves` before one exclusive,
same-filesystem rename. This slice has only two atomic outcomes, not a multi-file rollback:
source retained or destination published. On project open (and before another move),
recovery verifies either exact-byte outcome and acknowledges the journal without changing
asset files. Ambiguous/edited outcomes block further moves, preserve all files/backups,
and report the journal location in the app status. Completed backup journals are retained;
there is no destructive journal replay, automatic backup cleanup, or Ctrl+Z promise.

Scope remains one supported semantic file within the same project/filesystem. Folder
moves, arbitrary file moves, path-reference rewriting, multi-selection, recoverable delete,
interactive ambiguous-recovery resolution and full multi-file rollback are not enabled.
Native tree/grid/list drag/drop acceptance remains pending. Tests use temporary projects,
including interrupted publication on both sides of the atomic rename, ambiguity/traversal
rejection, unchanged byte/ID checks, menu cancellation and queued-edit guards.

Verification: 85 project tests and 643 editor unit tests plus the architecture test pass
(two opt-in editor tests ignored). Strict project/editor all-target Clippy, formatting
and diff checks pass. No native interaction verification is claimed for this slice.

### Asset drag visual feedback — 2026-09-08

Dragging now displays a floating copy with the browser's asset icon, filename and type.
It follows pointer events directly in the originating window's logical coordinates,
outside the browser scroll clip. Every preview element ignores picking so it cannot
obscure folder drop targets. The original row/tile keeps its position and dimensions;
release, Escape, stale content or removal of the source row removes the copy before
UI rendering. A drop still only opens Move Here / Copy Here; it does not mutate files.

Layout regressions cover list/grid views at 1x, 1.5x and 2x display scale, immediate
pointer tracking, non-intercepting descendants, unchanged source layout/bytes and
release/Escape cleanup. Native visual acceptance remains pending.

Verification: 644 editor unit tests plus the architecture test pass (two opt-in tests
ignored); strict editor all-target Clippy and formatting checks pass.

### AB6b semantic-file batch transaction foundation — 2026-09-08

The user accepted the single-asset folder drop and floating drag-preview workflow.
This slice adds a bounded backend batch-move API, not folder mutations or multi-select UI.
`plan_asset_moves` composes the shared semantic relocation preflight; all source identities,
drafts, references, bytes and destinations must pass before publication. Duplicate owners,
overlapping paths, same-name destinations, case-only collisions and unsupported paths
are rejected. Results return each stable identity and its before/after source location.

Before the first file moves, a staged, synced, no-clobber journal is published to
`.aestra/asset-transactions/active.pending`. It holds root-relative paths, canonical root,
typed identities and exact byte backups (128 files / 64 MiB serialized journal limit).
Each publication is an exclusive same-filesystem rename with inventory revalidation.
Only a completed batch archives the journal as committed. Normal failures restore known
completed moves in reverse order; failures/conflicts during rollback retain the pending
journal and every recoverable file. Completed/rolled-back backup journals are retained.

Restart inspection (`pending_asset_move_batch`) does not mutate asset files. Explicit
rollback validates the complete journal and every before/after file state before moving
anything, requires a complete draft-free host inventory, and rechecks each reverse step.
It can resume after an interruption during rollback without relying on a saved step
counter. External edits, missing files, occupied paths, links, read-only paths, root moves,
unknown versions, altered journals and identity mismatches fail closed. New browser
rename/move/copy/folder-create operations cannot apply over a pending transaction.
Startup reports the condition in-app but does not automatically replay it.

Existing single-file Move Here retains its accepted atomic-move journal. Batch publication
is not wired to a new user action yet. Next: typed path-rewrite planning and staged content
replacement, folder plans, and guarded in-app recovery actions with document-path
reconciliation. Folder rename/move/delete remain disabled until their full gates pass.
No semantic Ctrl+Z, cross-project moves, power-loss guarantee or backup-cleanup policy is
introduced by this slice. Failure tests use temporary projects, not the user's assets.

Coverage includes failure after journal preparation and each file publication, commit
archive collision, restart after every publication boundary, interrupted reverse steps,
external edit/collision preservation, stale plans, dirty/incomplete drafts, competing
destinations, malformed/relocated/traversing journals and read-only startup reporting.

Verification: all 95 project tests and 645 editor unit tests plus the architecture test
pass (two opt-in editor tests ignored). Strict all-target project/editor Clippy, formatting
and diff checks pass. Platform-specific Windows/Linux publication code is shared with
the existing exclusive-rename primitive; this run does not claim Linux execution or
native fault/power-loss testing. User-authored assets remain untouched.

### AB6b typed path rewrites and staged document replacements — 2026-09-09

`plan_asset_moves` now prepares supported path-reference edits with its semantic-file
moves. A shared relocation preflight permits known path usages only inside the batch
planner; the public single-file rename/move API still blocks them. Full saved-source
inventory, identities, destination checks and unknown-reference gates remain in force.
All drafts must be saved/discarded and the host must recheck its session guard at apply.

The current loader stores file paths in effect resource declarations
(`EffectAsset.assets[].path`). Material/function texture defaults and constants use
resource IDs and are not rewritten. Resolution validates root-relative aliases against
actual indexed files; missing/escaping/linked paths, unknown formats/include semantics,
read-only owners, case collisions and stale inventories block publication. Both stationary
and moved reference owners are supported, including multiple bindings to one source.

RON path tokens are located by borrowed typed deserialization and edited in place;
comments, whitespace, raw-string surroundings, names, unknown fields and unrelated
strings are preserved. The rewritten document is reloaded and compared to the intended
typed result. Journal validation recomputes these edits, rejecting unrelated changes.
Both plan and result expose rewritten owner paths for future host reload reconciliation.

Version-2 journals store exact before/after bytes. Every replacement is staged and synced
before journal publication. Publication exclusively moves the original into a retained
backup, then publishes the staged file without overwriting an occupied path. This is
recoverable multi-step publication, not a claim of filesystem-wide atomicity. Recovery
validates every touched path against a complete sequence prefix, then reverses the same
steps. It handles a missing owner between detach/publication and interrupted rollback;
unknown edits, missing backups or occupied destinations retain the pending journal and
block the entire recovery. Version-1 move-only journals remain supported. Staging files
orphaned before journal publication and archived backups are retained; cleanup is separate.

This backend slice does not yet enable folder/resource-file moves, multi-select UI,
reference-rewriting Move Here, or new recovery buttons. Folder plans, host reconciliation
and guarded in-app recovery actions remain the next gates. No native UX, Linux execution
or power-loss verification is claimed; tests modify temporary fixtures only.

Verification: 104 project tests and 645 editor unit tests plus the architecture test
pass (two opt-in editor tests ignored). Strict all-target project/editor Clippy,
formatting and diff checks pass. Regressions exercise all eight forward interruption
boundaries for three moves and two replacements, interrupted reverse recovery, external
edits to every owner/backup/staging file, dirty and read-only owners, stale bytes,
unresolved/escaping references, malformed rewrite journals, lossless RON token edits
and version-1 restart recovery. User-authored asset files remain untouched.

### AB6b audited folder and resource relocation plans — 2026-09-09

`plan_content_relocations` accepts source-ID-based same-project Move/Rename requests
for folders, saved effects/programs/functions/presets and supported non-referencing
resources. `plan_asset_moves` now delegates to that shared planner with its existing
semantic-only request boundary. Files keep their suffix, typed assets keep their IDs,
and resource files gain no invented semantic identity. Results expose all file and
folder location changes plus rewritten owner paths for future host reconciliation.

The folder planner physically inspects every descendant, including directories that
contain no files. It rejects hidden/excluded descendants rather than silently moving
content omitted by the normal index. Unknown formats/include semantics, links,
read-only paths, root moves, self-descendants, ancestor destinations, merges, overlapping
selections, case collisions, incomplete drafts and stale sources remain blockers.
The resource format proof is shared between normal reference preflight and journal
validation: textures, drawing-only SVG, self-contained JSON glTF and import-free WGSL/WESL.
This proves reference scope, not image decoding or shader rendering correctness.

Each selected folder moves with one exclusive same-filesystem directory rename;
empty descendants and native directory metadata travel with it. Loose file moves and
typed reference replacements use the same ordered transaction. Original resource IDs,
semantic bindings, comments and untouched bytes are preserved. Whole-project file and
directory inventories are revalidated before apply and every publication boundary.
The bounded gate permits up to 128 files and 128 directories, with a 64 MiB serialized
backup journal. No recursive deletion, folder merging or cross-project move is added.

Version-3 journals include full directory inventories and optional semantic identities
for file moves. Pending inspection reports both moved files and moved folders. Recovery
validates entire before/after subtrees, including unexpected new files and empty
directories; it never moves external additions back as if they were original contents.
The same prefix-state recovery supports directory moves, mixed loose files, missing
owners during replacement and interrupted reverse publication. Versions 1 and 2 remain
recoverable. Unknown/tampered states retain the journal and backups without replay.

This remains backend-only. Browser folder/resource rename/drop actions stay gated until
guarded in-app recovery and document/session path reconciliation are implemented and
tested. No native browser UX acceptance, Linux execution or power-loss guarantee is
claimed. Tests use temporary projects; user-authored assets remain untouched.

Verification: all 117 project tests and 645 editor unit tests plus the architecture
test pass (two opt-in editor tests ignored). Strict project/editor all-target Clippy,
formatting and diff checks pass. New regressions cover mixed semantic/resource folders,
inside/outside reference owners, empty folders, directory/resource rename, combined
folder and loose-file batches, every directory/replacement interruption boundary,
interrupted reverse recovery, hidden/excluded descendants, external additions, stale
empty-directory inventories, read-only folders, overlaps/collisions and tampered or
legacy journals. Existing file-only transaction and reference-edit tests remain green.

### AB6b guarded in-app relocation recovery — 2026-09-09

Interrupted batch journals are inspected without replay on project open and on explicit
Assets Refresh. The in-app Asset recovery dialog shows moved file/folder counts and the
backup location, with Restore original locations, Check again and Later actions.
Escape defers recovery without changing files or deleting the journal; the conditional
Assets toolbar button reopens it. Unreadable, altered and ambiguous journals remain
visible with errors and cannot be restored until a fresh inspection succeeds.

Restore uses the serialized project I/O queue and checks the captured project, document,
target, proposal, locks and draft inventory immediately before rollback. All drafts must
be saved/discarded. The exact inspected journal is retained through preparation; a new
journal at the same path cannot silently replace it. Backend before/after validation
still rejects external edits and additions. Save/open shortcuts respect the modal;
window close defers it before the normal unsaved-document exit prompt.

After rollback the catalog is refreshed, including ID-based material/function targets.
The clean open effect is resolved by unique identity within its project, its restored
resource paths are reloaded, and its exact-byte Save baseline is refreshed. A path-only
change preserves history; a changed effect reloads through the normal document path
while preserving standalone graph context. Same-ID documents outside the project are
not adopted. A failed reload reports the error and retains existing source-conflict
protection rather than accepting a stale baseline. Backups remain archived.

Browser folder/resource Move/Rename wiring is still the next gate. This change does not
enable deletion, automatic recovery replay, cross-project moves or journal cleanup.
No native UX acceptance, Linux execution or power-loss guarantee is claimed. Regression
tests use temporary projects and the real transaction writer; user assets are untouched.

Verification: 653 editor unit tests, the editor architecture test and all 117 project
tests pass (two opt-in editor tests ignored). Strict all-target project/editor Clippy,
formatting and diff checks pass. Eight new regressions cover read-only discovery,
Later/Escape/reopening, restored folder/resource bindings and exact Save baselines,
shared graph context, dirty and mid-preparation edits, project switches, altered
journals, external additions, explicit retry, shared drafts, path-only history
preservation, outside-project same-ID documents and modal focus/disabled controls.

### AB6b browser folder/resource relocation — 2026-09-09

Inline Rename and drag/drop Move now call the same journaled source relocation planner
for semantic assets, folders and supported resources. The shared capability supplies
the preserved suffix (empty for directories), not mutation authority; the backend still
proves current bytes, reference completeness, destinations and the draft-free inventory.
Unsupported formats, unsafe descendants, root/self-descendant targets, collisions and
stale or dirty state remain blockers. No cross-project move or silent merge is enabled.

F2 edits the existing label in list/grid views; Enter and blur use the existing commit
path, Escape cancels. Folder/resource rows retain the floating drag preview and drop onto
folder rows or tree destinations. Their drop menu offers Move Here only; semantic asset
Copy/Duplicate retains its separate fresh-ID implementation. Tree-source dragging and
folder/resource Copy are not part of this slice.

Forward publication and recovery share clean-document reconciliation: preserve IDs,
reload rewritten resource bindings, adopt exact Save baselines, retain graph targets and
preserve history for path-only changes. Forward moves also remap browser history,
expansion, selection and inspection before path-derived source IDs expire. A post-move
reload error is reported as a warning after publication rather than a failed move that
could be retried. Failed transactions trigger read-only pending-journal inspection;
rollback still requires the explicit guarded in-app recovery action.

Regression coverage extends both views' inline Rename and folder-row/tree drop tests
to folders/resources, verifies resource suffix preservation and Move-only menus, and
tests real folder/resource moves with active effects, rewritten bindings, empty folders,
selection/inspection, safe subsequent Save and mid-preparation draft rejection. Shared
state tests exercise relocated history/expansion; capability tests verify unsupported
formats stay unavailable and format recognition cannot bypass reference proof.

Native UX acceptance remains pending: rename a folder and texture; move a folder from
the content view onto a folder tree destination; verify its effect still plays/saves,
then verify dirty drafts/collisions block without partial moves. Recoverable deletion
is the next implementation gate after acceptance. User assets are not test fixtures.

Verification: 655 editor unit tests and the architecture test pass (two opt-in tests
ignored), along with all 118 project tests. Strict all-target project/editor Clippy,
formatting and diff checks pass on the supported Windows MSVC toolchain.

### AB6b mesh subasset relocation regression — 2026-09-09

Manual folder Rename/Move exposed an overly literal resource check: a valid binding
such as `meshes/lab_cube.gltf#Mesh0/Primitive0` was treated as a filesystem path,
blocking even unrelated empty folders during project-wide preflight. The shared typed
rewriter now separates the physical file from the final `#` label delimiter, validates
the file against the same canonical project inventory, and preserves the label exactly
when rewriting a moved file or folder. Journal validation/recovery uses the same logic.
Missing files, foreign roots, escaping paths and invalid labels still block operations.

Regression fixtures include the real self-contained lab cube, unrelated folder Rename
and Move, labeled mesh file Rename/folder Move, exact-byte recovery and invalid paths.
The list/grid inline Rename regression now includes an unrelated mesh subasset owner.

### AB6b tree gestures and recoverable deletion — 2026-09-09

Folder-tree rows now originate the same browser drag payload/preview as content rows.
Navigation waits for click release, so a press does not rebuild the dragged row. Drops
use shared destination checks and the guarded Move Here flow. F2 and the tree context
menu use the shared inline editor, with Enter/blur validation and Escape cancellation;
tree focus follows a navigation-triggered rebuild.

Delete is available from row/tree context menus and the Delete key. A native in-app
modal prepares a read-only preflight, shows blockers and file/folder counts, and requires
explicit confirmation. Known outside references, unknown dependency semantics,
unsupported descendants, active documents/resources, unsaved drafts, pending relocation,
stale project inventories and unsafe paths block deletion. Selected folders may contain
internal references because the whole subtree is retained together.

The project backend stages/syncs an exact-byte journal, then exclusively renames the
source into `.aestra/deleted/entry-*/payload`. This is project-local recovery storage,
not the OS recycle bin. Empty folders and semantic identities survive restore. The
Deleted Items toolbar action opens a bounded, scrollable in-app list; Restore validates
both endpoints, journal and payload before moving anything. Collisions/changed payloads
stay listed with a blocker. Missing original parents, duplicate identities or missing
external dependencies also block restoration. Restored records are retained for audit;
no purge, automatic replay or Ctrl+Z integration is provided. Operations refresh browser
selection/catalogue without replacing the active effect.

Regression coverage includes tree-origin drops in both views, F2 after tree navigation,
exact bytes/IDs/empty-folder round trips, reference blockers (including mesh-style
subasset labels), read-only payloads, stale inventories, interrupted publication/restore,
collisions, changed journals, duplicate IDs and restore dependency ordering. Editor tests
exercise preview/cancel/confirm/restore, draft changes before and during queued apply,
active resources/documents, modal focus, disabled confirmation and long-message scrolling.

Native acceptance checklist (use disposable project copies, not user assets):

- Drag a folder from the tree onto another folder; check preview, Move Here and final tree.
- F2/right-click Rename in the tree; Enter and click-away commit, Escape cancels.
- Delete an unused folder containing assets and an empty subfolder; confirm it disappears.
- Reopen the project, open Deleted Items and restore; verify contents and asset IDs.
- Check a referenced/active asset and an unsaved draft block deletion; check a conflicting
  destination blocks restore and preserves the recovery payload.

Implementation is ready for native acceptance; AB6 is not marked user-accepted yet.

Verification: 661 editor unit tests plus the architecture test and all 131 project
tests pass (two opt-in editor tests remain ignored). Strict all-target project/editor
Clippy, formatting and diff checks pass on the supported Windows MSVC toolchain.

### Delete ergonomics and ordered Undo/Redo — 2026-09-09

This supersedes the confirmation flow described in the initial AB6b slice above.
Delete removes the selected asset, or folder and all descendants, immediately after
safety preflight. Only blocked deletion opens an explanatory in-app panel with Close
and Check again; there is no force-delete action. Successful deletion offers Ctrl+Z
through the status message. English and French UI wording is updated.

Recent deletions join the action order of effect, standalone material and function
edits. Undo restores the exact original bytes, IDs and empty directories; Redo checks
the restored contents, current dependencies and drafts before deleting again. Collisions,
external changes or affected dirty drafts keep the history entry available for retry without
undoing an unrelated edit. New edits invalidate the redo branch. Explicit Deleted Items
restore removes the matching deletion entry, and recovery remains available after restart.

The ordering bridge is bounded to 256 entries and resets when opening/reloading another
effect or switching projects. Before the first deletion, existing scoped document history
behavior is unchanged. Rename/move Undo and permanent purge are not part of this slice.

Automated regressions cover repeated exact Delete/Undo/Redo, mixed document history,
focus changes, redo invalidation, collisions and edited restored contents, project switches,
draft changes during queued I/O, active-resource blocking and scrollable error panels.
Native acceptance remains pending: use a disposable folder to check Delete, Ctrl+Z,
Ctrl+Shift+Z/Ctrl+Y, menu Undo/Redo, restored selection and Deleted Items after restart.

Verification: 668 editor unit tests plus the architecture test and all 132 project
tests pass (two opt-in editor tests remain ignored). Strict all-target editor/project
Clippy, formatting and diff checks pass on Windows MSVC 1.98.1. No user assets were
mutated by these tests; all filesystem mutation fixtures use temporary projects.

### Deletion with unrelated drafts — 2026-09-09

Removed the blanket save/discard requirement. Delete, Undo/Redo and Deleted Items
restore preserve unrelated effect/material/function drafts. Untitled effects need no
invented source identity: their current references are checked separately, while mapped
drafts are validated against their actual source and semantic ID. Both saved references
and newly introduced draft references block deletion; an unsaved reference removal does
not authorize breaking the saved document. Drafts inside the target folder or belonging
to the target asset still require saving/discarding that specific draft. Existing I/O
guards reject edits made after preflight and retain the drafts unchanged.

Verification: 669 editor tests, the architecture test and 134 project tests pass;
two opt-in editor tests remain ignored. Regression coverage includes unrelated drafts
through Delete/Undo/Redo/manual restore, unsaved-only semantic/resource usages, affected
draft protection and queued edit cancellation. Strict Clippy and formatting pass.

### Assets Delete shortcut ownership — 2026-09-09

A consumed focused Delete event was still present in the editor's global keyboard
snapshot. With the timeline visible, its shortcut handler could create an emitter
deletion proposal behind the Assets panel. That pending proposal then marked deletion's
draft inventory incomplete even with zero unsaved materials. Timeline shortcuts now
defer to focused Assets surfaces (tree, list and descendants); global Undo/Save remain
available. An unrelated existing proposal also no longer blocks asset deletion: its
candidate references are checked alongside the active effect, and the proposal is
preserved during Delete/Undo. Proposed usages still block, and the I/O guard still
cancels if the proposal changes during preparation.

Regression coverage dispatches native keyboard messages with Assets focus, checks the
focused handler receives Delete without creating a timeline proposal, then returns
focus to the timeline to verify normal deletion still works. Temporary-project tests
cover preserved unrelated proposals and references introduced only in a proposal.

Verification: 671 editor unit tests and the architecture test pass (two opt-in tests
ignored), including the keyboard-dispatch regression. Strict editor Clippy, formatting
and diff checks pass. Native user acceptance remains pending.

### Delete open documents without losing drafts — 2026-09-09

Being open is no longer a deletion blocker. Material/function views close after
successful deletion of their source or containing folder. The same synced recovery
journal retains affected drafts and the graph target, so Undo and Deleted Items restore
both original file bytes and unsaved edits. Journal data survives a restart and Redo;
live draft collisions or changed restored drafts block recovery/Redo without overwrite.
An open effect is retained in memory as Untitled, requiring Save As; Undo reattaches
its original source without replacing unsaved work. References from surviving saved
documents and draft documents still block deletion. Failed/stale operations leave views
and drafts untouched. This supersedes the open-document and affected-draft restrictions
recorded in earlier slices.

Temporary-project regressions cover file/folder deletion with a dirty open material,
cross-document edit Undo/Redo, restart-style draft recovery, open dirty effects, function
drafts, failed publication preserving views, and recovery payload retention. Native
manual acceptance of this open-document workflow remains pending.

Verification: 674 editor unit tests, the architecture test and 135 project tests pass;
two opt-in editor tests remain ignored. Strict Clippy for both packages, formatting
and diff checks pass. No user assets were deleted during verification.

### AB7a — Typed Asset Browser Effect → Timeline — 2026-09-09

The user accepted the open-document deletion P0 check and requested AB7a. Browser
drag origins now publish a shared project-root/version/source/semantic-identity payload;
timeline hover and drop re-resolve it against the published content snapshot. The
transitional Library and browser feed the same cycle-safe placement command, rather
than maintaining independent clip insertion logic. Canvas drops place at pointer time,
track-header drops at playhead time, with existing track insertion and duration bounds.
One Undo reverses both clip insertion and track order; Redo restores the same clip.
Successful drops explicitly transfer history focus to the effect while leaving an open
material/function graph in place. Returning to graph/standalone Properties restores
graph history focus; merely having a function open no longer intercepts effect Undo.

The existing browser ghost remains non-pickable and does not remove the source item.
Escape/end/stale-source cleanup removes the payload and timeline preview. Invalid
types, project switches, changed/missing/ambiguous sources, self/cyclic references,
protection dialogs and pending proposals reject without editing or discarding work.
Hover uses the existing timeline preview/rejection feedback. Source files are not
changed by authoring drops. Texture/mesh/material/function targets and reusable picker
UI are not part of this slice; Library remains available.

Regression coverage includes native picking events for canvas/header drops, child-label
targets, secondary-button no-ops, a single transaction's Undo/Redo, hover/cancel cleanup,
source validation and protected-document rejection. Existing grid/list ghost tests also
check typed-payload cleanup at multiple UI scales. Native manual acceptance is pending:
drag an effect from Assets onto timeline space and track headers, inspect insertion
feedback, Undo/Redo, and check self/invalid drops and Escape leave the document intact.

Verification: 678 editor unit tests and the architecture test pass (two opt-in tests
ignored). Strict editor Clippy and formatting pass. Tests use temporary assets; no
user asset files were moved, copied, edited or deleted for this slice.

### AB7b — Material Program → Renderer assignment — 2026-09-09

Properties renderer cards consume the shared snapshot-bound AssetPayload. Material
program drops are authoring operations, not file moves. Hover gives an accent/red
outline and assignment/rejection label. Primary drops resolve child labels to their
owning card and revalidate source identity, current effect/renderer, domain, compiler,
locks, pending proposals and document-operation guards. Sprite/Flipbook, Ribbon/Trail
and Mesh require their corresponding program domain. Unsupported assets are rejected.

One effect-history transaction adds a default MaterialInstance if needed and changes
the renderer binding. A matching default instance is reused; another renderer's
overrides are never silently adopted. Dropping the already-assigned program preserves
overrides and adds no history entry. Undo removes only the newly created instance and
restores the prior binding; Redo restores the same identities. The shared source file
and any shared draft remain unchanged. Successful assignment routes Undo to the effect
without navigating away from an open graph.

Automated regression coverage includes pointer child-target propagation, atomic
assignment Undo/Redo and RON round-trip, reuse/no-op/override isolation, domain mapping,
draft-aware planning, invalid/stale/locked/protected target rejection and hover cleanup.
Authoring command coverage checks insertion/removal serialization, inverse commands,
duplicate IDs, invalid indices and rollback after partial transaction failure.

Native acceptance is pending: drag a compatible material onto expanded and collapsed
renderer cards; inspect feedback and preview, Undo/Redo, repeat the drop, and try a
mismatched domain and Escape. Test both list/grid origins and high-DPI/detached panels.

AB7b is **partial**: presets are graph-building recipes, not assignable programs.
The user has been asked whether a renderer preset drop should create a new named
material or modify its existing shared material. No preset-drop mutations are enabled
pending that decision. Reusable pickers and other typed consumers remain pending.

Verification: 686 editor unit tests and the architecture test pass (two opt-in tests
ignored); all aestra-authoring tests pass. Strict Clippy for both packages, formatting
and diff checks pass. User-authored assets were left untouched. Native manual
acceptance is not claimed by these automated checks.

### AB7b — Project preset → new material → renderer — 2026-09-09

The user accepted material-program drops and approved the proposed preset flow:
create a new named material rather than modifying an existing shared program.
Project presets now use the shared typed payload and renderer-domain planner. Hover
explains that a material will be created; release opens a modal in-app dialog with
name and existing project-relative folder fields. It focuses the name, shows the
resulting path or error, disables invalid/colliding destinations, and supports Enter
and Escape/Cancel. Submission waits until native text edits have been applied.

Creation builds a distinct program from a clean domain-compatible base and the preset
recipe. Serialized project I/O rechecks the saved preset, compiles the candidate, and
uses project-layer portable-name/path/link/read-only/case-collision checks plus staged
no-clobber publication. Existing presets and shared programs are never rewritten.
The resulting source is published to the catalog and assigned by the existing single
effect transaction. Undo/Redo restores the prior/new binding and local instance with
stable IDs; the named reusable material file remains in Assets, explicitly stated in
the dialog. Source creation and document assignment retain separate lifetimes, as
with existing source-creation workflows. Delete/Deleted Items remain available for
removing the created source explicitly.

Cancel before submission makes no file or authored edit. During publication controls
are disabled; a failed operation stays in the dialog with an explanation. Changed
document/project state never receives a late assignment: a successfully created source
is retained and reported as not assigned. Native acceptance is pending for list/grid
origin drops, name/folder editing, collision feedback, Enter/Escape, live preview and
Undo/Redo. Built-in virtual-source migration and reusable picker UI remain pending.

Verification: 691 editor unit tests and the architecture test pass (two opt-in tests
ignored), together with all 136 project tests. Strict editor/project Clippy, formatting
and diff checks pass. New tests cover real picking-event propagation into the modal,
latest native field submission, creation/assignment/Undo/Redo, original preset bytes,
empty and colliding names, cancellation, stale presets, late collisions, missing folders,
new locks and a document change during I/O. Tests use temporary project roots; existing
user asset edits were not included or changed. Native manual acceptance is still pending.

### AB7c review / AB7d — Texture → Properties input — 2026-09-12

Reviewed the current committed function-drop implementation (`d80a3a0`) and ran its six
regression tests successfully. The plan's missing next consumer was texture assignment;
mesh drops, reusable pickers and Library cutover are not yet complete. Existing manual
acceptance gaps are not retroactively marked tested.

Properties now accepts project textures on sprite Texture rows and exposed material
Texture2D parameters. Unassigned parameters have a visible drop target. Closest-input
routing prevents the parent renderer's material-drop handler from also processing a
texture-field drop. Existing typed-payload validation, drag ghost, hover outline/label,
document guards and effect history are reused. A single transaction adds a local asset
registration when needed and sets the input; same paths are reused, including slash/dot
aliases and Windows case aliases. Already assigned identities win over duplicate legacy
registrations. Undo removes only a newly registered local reference and restores the
previous binding; Redo restores the same IDs. No disk file is moved, copied or deleted,
and no shared material/function source/default or flipbook metadata is changed.

Validation rejects non-textures, loader-disabled formats, reserved loader-path characters,
stale payloads, changed bindings, locked material consumers and pending/protected edits.
Release rechecks file existence, metadata and link/reparse status before assignment.
Image bytes are decoded asynchronously by the renderer loader, not on the UI hover path.

Native acceptance checklist: list/grid drag origins; a legacy sprite texture field;
an exposed Texture2D input with/without a value; repeat assignment; Undo/Redo; wrong-type
and locked-target rejection; Escape; live preview; detached/high-DPI panels. Native
acceptance is pending. AB7's next implementation slice is Mesh → mesh renderer.

Verification: all 771 editor unit tests pass (two opt-in tests ignored), including seven
texture-drop tests. Strict editor Clippy with all targets and warnings denied passes.
Formatting checks for the touched Rust files and `git diff --check` pass. Whole-repo
formatting has pre-existing differences outside this slice; unrelated diagnostics edits,
new icons and roadmap files were left untouched. Native acceptance remains pending.

### AB7 shared asset-drop infrastructure — 2026-09-12

Moved the browser-owned payload into the editor-level `asset_drop` module. Graph and
Properties drops now share primary-button source/nearest-target resolution, hover
outline/message construction, leave/end/stale-source cleanup and release cancellation.
Feedback registration is idempotent and scoped by consumer type. Graph-specific target
invalidation remains in its adapter. The Properties adapter and its existing tests/preset
workflow were renamed from `material_drop` to `asset_drop`.

Timeline placement and browser folder targets use the same ancestor-routing helper;
timeline preview cleanup shares the payload lifetime check, and both timeline release
targets now recheck Escape. Browser drag ghosts still originate in the browser and
follow the pointer across consumers. File move/copy keeps its existing serialized I/O,
confirmation and recovery; document assignments and graph edits keep their own history.
No mesh drop or new asset mutation behavior is introduced by this refactor.

Added shared routing/lifecycle regression coverage and a same-frame Escape/release test
for both timeline targets. Native drag/drop acceptance remains pending.

Verification: all 776 editor unit tests pass (two opt-in tests ignored). Strict editor
Clippy with all targets and warnings denied, touched-file formatting and diff checks pass.
The refactor preserves existing material/preset, texture, function graph, timeline and
folder-move regression coverage. Unrelated user changes remain untouched.

### AB7e — Mesh → mesh renderer — 2026-09-12

Follow-up: the Render add menu now includes **Mesh Renderer**, with a project mesh-source
chooser followed by shared primitive inspection/selection. Creation registers geometry,
adds a fresh local instance of the built-in unlit Default Mesh program and selects the renderer in one Undo/Redo transaction.
Escape/Cancel creates nothing. The sample project now includes **Mesh Material Lab** /
**Mesh Cubes**, so native acceptance does not require hand-authoring a renderer.
Manually verify menu creation, source/primitive selection, and Undo/Redo as well as drops.

Mesh renderer cards and a dedicated Mesh input now accept glTF/GLB drops through the
shared asset-drop infrastructure. The input appears for semantic material renderers too.
Source metadata is inspected on the background project I/O worker. One triangle primitive
assigns directly; multiple primitives require an explicit selection in a scrollable
in-app chooser. Cancel/Escape can dismiss the loading or selection stage; late completion
does not revive a cancelled request or apply to a changed document.

The assignment registers/reuses an effect-local mesh reference with its exact runtime
primitive label and updates geometry in one history entry. Undo/Redo includes the new
registration, with stable IDs. Material bindings, material drafts and disk files stay
unchanged. Mesh and texture assignment share lexical path normalization and regular-file/
metadata/link checks; no duplicate hover/release observer was introduced.

Supported metadata: glTF/GLB triangle primitives with position attributes, up to 64 MiB
per source and 256 eligible primitives. OBJ, scenes and animation import are outside
this slice. External buffers and decoding are checked by the asynchronous runtime loader,
not by the chooser. Native acceptance pending: list/grid origin, card/row targets,
single/multiple primitives, chooser scrolling and keyboard navigation, Escape/Cancel,
live geometry preview, unchanged material and one-step Undo/Redo.

Verification: 785 editor unit tests pass (two opt-in tests ignored), and all core/compiler/project
tests pass (327 tests). Strict editor/core/compiler/project Clippy with all targets and warnings denied, touched-file
formatting and diff checks pass. New tests cover card/child-input routing, single and
multiple primitives, explicit selection, source preservation, reuse/no-op assignment,
Undo/Redo and RON roundtrip, cancellation and late document/project/lock changes,
modified/missing/invalid/wrong-format sources, and GLB metadata/primitive labels.
Creation tests additionally cover starting from a sprite-only effect, the source chooser,
single/multiple primitives, built-in default material compilation in standalone/project
contexts, renderer selection, atomic Undo/Redo and sample-project dependency validation.
Native acceptance remains pending; unrelated user edits were left untouched.

### AB7f — Shared Properties asset picker — 2026-09-12

Material, Mesh and Texture value buttons now open the same bounded, scrollable, searchable
in-app picker. Project sources are filtered for the field; local materials/instances and
registered textures remain available, including Procedural for sprite textures. Search is
case-insensitive; Enter in Search selects the first match and result buttons retain normal
keyboard activation. Escape, Cancel and backdrop clicks dismiss without edits. The modal
blocks graph navigation; its search input receives initial focus.

Project-source choices and drag release share AssignAsset, source checks, target validation,
preset creation and asynchronous mesh inspection/primitive selection. Local texture choices
reuse the same binding planner as drops. Selection is snapshot guarded against document,
project, lock and draft changes; failed validation does not silently assign to a different
target. Repeated assignments are no-ops; successful changes use one effect Undo/Redo entry.
Material value fields are distinct typed drop targets, so meshes cannot fall through to
the surrounding renderer card. The old texture/material combo actions were retired.

Automated coverage includes field filtering, search/empty results, initial focus, project
material/texture selection, local texture/Procedural parity, repeat/no-op and Undo/Redo,
stale/cancelled/locked requests, mesh-picker handoff and unchanged material bindings,
inside-versus-backdrop click behavior and mesh rejection on Material fields.

Verification: 792 editor tests passed, two opt-in tests ignored. Strict editor Clippy
with all targets and warnings denied, touched-file formatting and diff checks passed.

Native acceptance pending: field click, popup sizing at narrow docks/DPI, search, scrollbar,
keyboard/Enter, Escape/outside click, preset/primitive handoff, live preview and Undo/Redo.
This does not complete virtual-source or Library parity migration. Keep Library until its
remaining built-in presets and local resource creation workflows have tested replacements.

### AB7g — Built-ins and Current Document virtual sources — 2026-09-12

Assets now exposes Project, Built-ins, Current Document and the transitional Library.
The virtual catalog is shared with Properties pickers. Built-in material presets retain
their semantic IDs; local resources carry effect/history/revision guards and never enter
the project source tree. Virtual rows have no filesystem-operation identity. Search,
list/grid layout, bounded pages and scrolling do not edit the document. Selection remains
stable by semantic ID through refresh; document switching clears local selection.

Virtual drags use the existing pointer-following non-pickable preview and AssignAsset
route. Presets open the same named material creation prompt; local materials/textures
reuse binding planners. Local mesh and flipbook drops validate matching renderer types,
locks and the candidate document, preserve materials/playback settings, and create one
Undo/Redo edit. Wrong-field drops cannot fall through to geometry. Release over empty
space, Escape, stale snapshots and removed source widgets clear the drag visual.

Current Document offers local sprite-material creation and explicit 4×1 atlas creation
from the selected registered texture. No selection/no texture disables atlas creation.
Both use the session's existing semantic commands and effect history; the source texture
is unchanged. Library remains available for the remaining parity audit and native
acceptance. Virtual source scope/selection are intentionally not persisted as file paths.

Regression coverage: virtual/file identity separation, filtering and pagination, selected-
texture creation and Undo/Redo, drag visual cleanup, stale local handles, shared picker
validation, built-in preset creation without a preset file, local geometry/atlas bindings
and lock/wrong-field rejection. Native input, detached-window/DPI, preview and end-to-end
acceptance remain pending. Verification: 800 editor tests passed, two opt-in tests ignored;
strict editor Clippy (all targets, warnings denied), touched-file formatting and diff
checks passed. The narrow list/grid layout regression at 1.5× scale confirms a bounded
scrollable viewport; it does not substitute for native high-DPI input acceptance.

### AB8a — Library parity audit and browsing gaps — 2026-09-12

Code-level audit, not native acceptance or permission to remove Library:

| Workflow | Current replacement / evidence | Cutover status |
| --- | --- | --- |
| Root selection, explicit effect activation and source navigation | Browser activation through persistence/document protection; existing guarded-open and source-navigation regressions | Retain coordinator/history services; no second scanner. |
| Create, duplicate, inline rename, move, delete, restore and file Undo | AB6 Browser actions and common project-operation preflight/history | Native mixed-project workflow remains the removal gate. Browser rename changes filenames; authored effect names remain editable in Properties. |
| Extract selected emitters | Timeline context menu → `LibraryAction::CreateReusableEffectFromSelection`; existing extraction/resource-remapping/Undo tests | Entry point already migrated; move handler and extraction dialog ownership in AB8b. |
| Explode referenced clip | Timeline context menu and referenced-effect Properties → `LibraryAction::ExplodeEffectClip`; recursive/boundary-link tests | Entry point already migrated; move shared command ownership, do not duplicate it. |
| Repair broken effect reference | Referenced-effect Properties candidate picker → `PropertiesAction::RepairEffectClipSource`; instance-state/Undo, cycle/window tests | Existing Properties route remains supported. |
| Inspect usages and dependencies | Project row context menu → Asset Details / References; `asset_browser/inspection.rs` | Preserve dependency services and externally used relation overlays. |
| Material/function/WESL editing and retained drafts | AB4/AB5 standalone editor targets and common document coordinator | Preserve protected navigation and conflicts; selection alone does not open. |
| Preset discovery and application | Project preset sources + virtual Built-ins, compatible pickers/drop creation; shared preset rasterizer | AB8a restores previews and metadata search; native selection, creation, preview and Undo still need acceptance. |
| Current Document resources and local creation | AB7g virtual catalog and shared session commands; renderer Properties edits assigned materials/atlas settings | AB8a restores legacy declarations and blend/frame-rate metadata in tooltips. Legacy declarations are inspection-only, not fake atlas definitions. |
| Docking, keyboard and localization | Existing Assets dock identity, Browser controls and document coordinator | Native narrow/floating/high-DPI, keyboard-only and locale pass required before Library/settings removal. |

The old Current Document section rendered **read-only resource summaries**, not a
general editor for unassigned resources. Renderer Properties already edits the applicable
material/texture/flipbook settings. Do not invent a new resource editor as a prerequisite
for parity, or misrepresent a legacy Flipbook asset declaration as assignable atlas metadata.

Changes in this slice:

- Project search reads authored effect/material/function names and preset display names,
  descriptions, categories and tags from the published snapshot, without I/O or cloning
  complete documents during filtering. Built-in search includes category and tags too.
- Built-in and uniquely resolved project preset rows reuse `spawn_material_preset_preview`.
  Grid/list previews stay square. Retained rows refresh on preset identity changes; recipe
  rerasterization follows content revision rather than only resource change detection.
- Local sprite blend and atlas frame count/rate appear in hover metadata, not permanent
  description panels. Legacy flipbook declarations are visible but cannot start assignment
  drags or pass compatible-field planning.

AB8b ownership/removal checklist:

1. Move `ProjectIoTasks`, watch initialization and `io::poll`/catalog polling registration
   from `EditorLibraryPlugin` to the project-content service, preserving system ordering.
2. Move `sync_project_texture_root` to project/render integration; separate its Library UI
   reset from the required renderer texture-root update.
3. Move Timeline/Properties extraction/explode actions, extraction dialogs, and retained
   relation/dependency services out of Library ownership. Preserve source history and
   document-protection entry points, existing tests, and one-transaction Undo semantics.
4. Only then remove legacy panel/filter/context-menu/drag state. Preserve Assets dock IDs,
   closed/floating layouts, locale labels and tolerant persisted-setting migration.
5. Accept browse → open → edit → save → move → locate usages → drop → Undo → restart,
   including extract/explode/repair, Built-ins, Current Document and detached high-DPI UI.
   AB7/AB8 native tests not yet performed remain pending; earlier acceptance is not extended
   retroactively. No Library removal or general AB9 thumbnail work is included here.

Verification: 805 editor tests passed (two opt-in tests ignored), including five new
regressions for snapshot-only metadata search, square project previews/identity refresh,
built-in preview/tag search, inspection-only legacy declarations and revision-driven
preview rerasterization without resource change ticks. All 139 project tests passed.
Strict editor Clippy (all targets, warnings denied), touched-file formatting and diff
checks passed. Native acceptance remains pending.

### AB8b1 — Independent project services and reusable-effect command core — 2026-09-12

AB8a was committed as `315ee5a` before this refactor. The Library tab remains available;
manual parity acceptance was not supplied and is not inferred from an implementation request.

- `project_content/plugin.rs` now registers the sole catalog/watch resources, background
  I/O completion polling, snapshot refresh polling and renderer texture-root synchronization.
  The app installs this plugin explicitly before Library. Browser reconciliation and the
  app input chain depend on `ProjectContentSet::Input`, not on a Library scheduling set.
- `ProjectEffectCatalog` remains a compatibility name for `EditorProjectContent`, exported
  from the project-content owner rather than Library. No new catalog/scanner was introduced.
- Library retains only its own root-change UI reset. Catalog refresh, root changes and
  I/O completion publication are regression-tested in an app without Library or Browser.
- Extraction planning, source creation/rollback, explode flattening, parameter baking,
  resource remapping and transform composition moved unchanged into `effect_authoring.rs`.
  Extraction accepts selected emitter IDs, a name and replacement choice, not a Library
  dialog state. Four core regressions moved with the code; legacy interaction and worker
  tests remain to characterize the adapter routes.

This is the service/core portion of AB8b, not full cutover. The remaining Library action
adapters, extraction/relationship dialogs, source-operation workers and their modal guards
must move before the plugin can be removed. Keep the Assets dock identity and saved
layouts/settings unchanged until the migration and native acceptance steps are complete.

Verification: 807 editor tests passed, two opt-in tests ignored. The two new project-plugin
regressions cover root/watch/renderer-path synchronization and real queued I/O completion
publication without Library or Browser resources. Existing extraction/explode/worker/UI
regressions still pass after relocation. Editor all-target checking, strict all-target
Clippy with warnings denied, touched-file formatting and diff checks passed. No native
restart, detached-window or Library-removal acceptance is claimed for this slice.

### AB8b2 — Shared action/dialog and worker ownership — 2026-09-12

`EditorAssetActionsPlugin` now owns action activation/dispatch, `AssetOperationState`,
source rename and extraction prompts, dependency/reverse-usage inspection, legacy source
deletion confirmation, Escape dismissal and relation-overlay rebuild scheduling. Source
operation workers and their regression tests moved from `library/background` to
`asset_actions/background`. The existing I/O guard, latest-source preflight, rollback,
draft preservation, stale-result reconciliation and history behavior are unchanged.

Timeline extraction/explode and referenced-effect Properties now emit `AssetAction`
directly; Browser refresh uses the same event. The shell hosts the shared operation
overlay and `ShortcutContext` consults the shared modal state rather than Library.
The `AssetActionsSet` action/sync phases preserve the existing app schedule. Library's
only action observer restores its own context-menu focus; it does not execute commands.

Ten action/dialog regressions moved into `asset_actions/tests` and run without the Library
plugin, covering normal/context-menu activation, rename of clean/dirty open sources,
extraction dialog synchronization, relation rebuild stability, explode/Undo, usage lookup,
exact owner-clip navigation and deletion confirmation. New regressions exercise the complete
extraction action → confirmation → worker → document Undo path, and modal shortcut blocking
with Escape cancellation, again without Library resources. Core extraction/explode and
background-operation tests remain separate from UI tests.

This is an ownership refactor, not a redesign of legacy source operations: Browser's
recoverable deletion and inline filename rename retain their existing AB6 routes; the
transitional Library still exposes its older confirmed-delete/authored-name behaviors.
Do not replace Browser commands with those compatibility actions during cutover.

Next, after native parity acceptance, remove Library's tab/filter/list/context-menu/drag
presentation and the Timeline legacy row-drag adapter, then clean up old settings without
changing the Assets dock identity or losing closed/floating layouts. No Library UI or
persisted settings are removed in AB8b2; native restart/detached-window checks remain pending.

Verification: 809 editor tests passed, two opt-in tests ignored. All-target editor checking,
strict all-target Clippy with warnings denied, touched-file formatting and diff checks
passed. No native acceptance or Library-removal verification is claimed by these tests.
