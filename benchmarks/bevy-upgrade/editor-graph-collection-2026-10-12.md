# B20-2 slice 11 — Post-layout collection and document/view authority

The required Bevy 0.20 graph gate now imports the actual post-layout geometry
collector and the production document/view registries. Shipping remains on
Bevy 0.19.1; root manifests, toolchain and lockfile are unchanged.

## Implementation

- `document_registry.rs` contains the existing document lifecycle, IDs, dirty
  state and revision tracking. Its four existing tests move with it. Host
  display/save code remains in `document.rs`, reexporting the manager.
- `editor_view_registry.rs` contains the existing view kinds, registry, active
  context and open/focus/new-tab paths. Host docking persistence, target routing,
  save/close handlers and their tests remain in `editor_view.rs`. Existing
  consumer paths still resolve to the same production types through reexports.
- `geometry/collect.rs` reads the real registries and actual UI components.
  Measurement, stable observation, duplicate rejection, port normalization and
  preview subtraction remain in the production collector, not a test copy.
- The current project root is an explicit `In<Option<PathBuf>>` system input.
  Shipping `geometry.rs` pipes it directly from `ProjectEffectCatalog` immediately
  before collection, after `UiSystems::PostLayout`. There is no mirrored root
  resource or extra synchronization schedule. A shipping regression checks
  absent, current and replaced catalogs. The candidate controls this explicit
  root input; it does not import the full catalog discovery/draft adapter.
- The actual preview marker moves to the shared node module, with its existing
  host path reexported. The candidate wrapper only imports production modules;
  it does not define stand-in nodes, documents, views or geometry registries.
- Source freezing now includes both registry modules and the remaining host
  editor-view adapter, as well as the complete graph subtree and candidate files.

## Authority fixes

A docked view cannot fall back to tool-panel lookup when only one of its two
registries remains during teardown. Missing views/documents and wrong asset
bindings remain unavailable. The existing resource-free widget measurement mode
is preserved when neither registry is installed; ordinary tool panels still
resolve documents without requiring docked views.

A focused view only influences registry ownership when its active document
matches the real view mapping and remains open. An inconsistent active context
cannot steal a stable snapshot from another split view. Valid owner handoffs
and document close/reopen establish fresh baselines rather than resize edits.

## Regression coverage

Eleven new shared collector tests cover:

- Actual Bevy UI layout, computed transforms, port centers and preview height
  at 1x/1.25x/2x display scale, using the real post-layout schedule. Fractional
  child/parent rounding is bounded to one physical pixel per axis.
- Controlled physical measurements at three scales and three zooms; exact
  graph-unit normalization without changing authored placement, dirty state or
  document revisions.
- Stale tabs, wrong assets, missing/partially removed registries, changed project
  scope, document reopen and restored view generation.
- Duplicate live view/node/port identities, hidden ancestors, invalid transforms
  and measurements, collapsed ports and measured preview margins.
- Valid split-view focus, inconsistent active context and tool-panel ownership.

The existing invalid-measurement collector test and four document lifecycle
tests now also run in the candidate. Existing shipping graph composition,
insertion, framing, docking and save/close tests remain enabled. A separate new
shipping test covers the full catalog-to-root projection.
The two existing material/function camera-persistence tests now resolve their
restored split-view IDs through the real document/view registries and a matching
active document; their focus, fallback and idle-change assertions are retained.

## Remaining scope

This slice qualifies collection and authority, not complete editor acceptance.
The candidate layout test uses ordinary UI nodes carrying real graph components;
full production panel composition and material/function adapters still need
end-to-end qualification. Catalog discovery/draft behavior is shipping-tested,
not imported here. Semantic action dispatch, rewiring, undo/redo and full native
editor workflows remain open. Legacy node-drag cancellation/ownership and native
pointer/DPI behavior remain the explicit limitations recorded in slice 10.

Next: application action dispatch and semantic graph-edit transactions, followed
by complete native workflow acceptance before switching the root engine graph.
No optional widget replacement is part of this compatibility slice.

## Validation

Validation on 2026-10-12:

- Shipping editor: **1,278 unit + 3 architecture tests pass**; ten existing
  ignores are unchanged. The initial run exposed two old camera-focus fixtures
  without document/view authority; the corrected real-registry fixtures and
  full suite pass on rerun.
- Strict workspace and candidate all-target Clippy pass, as do all-target
  workspace checking with `aestra-bench/gpu`, both formatting checks and
  whitespace validation.
- Accepted source-frozen headless run:
  `target/bevy-020-qualification/runs/d077104cb0d746c08ad25d0d0fbffcf1/`.
  It executes **490 checks**, including **82 graph checks** and one existing
  ignored graph timing test. All **607 source-input hashes** remain unchanged.
  The graph test process takes 0.56 seconds after compilation. Executed-check
  totals include shared tests exercised by multiple gates, not unique tests.
- Candidate: Bevy **0.20.0**, Rust **1.98.1**. Shipping stays Bevy **0.19.1**;
  root manifest/toolchain/lockfile diffs are empty. Root lock SHA-256 remains
  `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
- `native_executed` is false: no new native graph or full-editor workflow
  acceptance is claimed by this headless run.
