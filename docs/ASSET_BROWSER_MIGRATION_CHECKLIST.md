# Asset Browser contracts and migration inventory

Recorded for AB0/AB1 and updated for AB2a, 2026-09-06. This is a parity inventory, **not** a claim that
the Library UI has migrated. Follow [the delivery plan](ASSET_BROWSER_IMPLEMENTATION_PLAN.md).

## Implemented discovery contract

`aestra-project::ProjectContent::scan(root)` builds a read-only source tree and the
existing semantic index from one directory discovery. It does not choose a project
root, mutate files, create metadata, load textures, watch changes or publish UI events.
The editor's AB2a adapter now schedules recurring discovery on an I/O worker. Explicit
root-open/post-write scans and legacy read-through query helpers remain for AB2b.

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

UI destinations below are pending; the polling/catalog service has moved in AB2a.
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
| Polling, clean reload, dirty conflicts, texture root — `project_content/`, `library.rs` | Stable-observation debounce, project-switch baseline, internal-save echo and moved dirty source tests | AB2a background generic refresh and AB2b1 cached semantic queries implemented; texture-root sync retained. AB2b2 removes explicit-open/post-write foreground scans. |

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
Explicit open/write transaction timing, foreground post-write scans and manual browser
acceptance are not covered as completed work; AB2b2 is the next integration slice.
