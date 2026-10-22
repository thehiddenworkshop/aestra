# Complete material graph plugin registration — Bevy 0.20

Date: 2026-10-21. B20-2 slice 100. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

The complete production `EditorMaterialGraphPlugin` now lives in
`material_graph/registration.rs`, compiled directly by both engine roots. Its
build body is unchanged: source comparison preserves all registrar calls, observer
order, Update consumers and keyboard registration. Canonical candidate facades
point to the real existing node/menu, control, appearance, lifecycle, layout,
preview, insertion, presentation and keyboard consumers. There are no substitute
registration callbacks.

The last registrar dependency, browser function dropping, now compiles as a complete
production module instead of only its target-identity definition. Explicit imports
and child paths retain a single target type. Shared asset resolution and owner-scoped
feedback move unchanged into `asset_drop/feedback.rs`, re-exported by the shipping
asset-drop module. The input-row builder and filesystem consumers remain in place.

Two small native event boundaries select 0.19 Pointer wrappers versus 0.20 named
drag-enter/leave/drop events and read their native pointer locations. They do not
translate target routing, propagation, validity, history or edit policy. The five
browser-drop handler bodies retain their policy after normalizing only event type,
location accessor, qualification path and formatting changes. Six shared resolution
and feedback helper bodies are unchanged apart from the leave-event type alias.

Obsolete native root imports are removed or test-gated. All 26 original browser-drop
test assertions remain; three event constructors use the native input boundary.
An older protection fixture reuses the existing modal-state test factory to satisfy
strict candidate Clippy while respecting native private fields, without a new lint
allowance or expanded visibility. Root manifests, shipping lockfile,
toolchain, CI, authoring policy, VFX budgets and visual references are unchanged.

## Shared regressions

The candidate now executes the seven existing full browser-drop cases and four
shared asset-feedback cases. They retain program/function and effect-material
routing, custom WESL callees, typed defaults, multi-output calls, child ancestry,
restored/unfocused targets, one-step history, unchanged files, direct/indirect
recursion rejection, stale/wrong/blocked payloads, Escape/leave/drag-end cleanup,
nearest-target ownership and idempotent feedback registration. The three previously
qualified target-identity tests now run under the actual browser-drop parent module.

Three new cases run on both engines using the complete material plugin alongside
actual function-editor registration, actual input plugins, saved sources, the real
workspace builder, production UI layout/geometry collection and global history:

- An idle workspace measures real canvas geometry, owns the expected resources,
  and runs all registered lifecycle/preview/input systems without changing its
  authored material or saved bytes.
- A real native node click selects in its originating view and advances the UI
  revision once; the registered canvas menu opens, and Escape travels through the
  real keyboard-event path to dismiss it without a semantic edit.
- Registered browser hover displays one owner-scoped feedback entity. Drop commits
  once, remains unchanged across later frames, and global undo/redo restores the
  source. A second undo reaches the existing empty scoped-history status, proving
  there was not another semantic history step; the saved material is unchanged.

The first combined fixture needed the normal editor Localizer for lifecycle notices.
A canvas-only fixture also lacked the workspace-owned drop target. Both were
corrected by supplying the real application resource and using the production
workspace builder, not by relaxing validation or manufacturing drop metadata.

## Validation

The default-parallel candidate graph executable passes **2,027 tests**, with the
same one existing ignored timing probe and no failures (32.02 seconds). The three
complete-plugin cases and existing browser-drop cases pass directly too. Both
strict all-target Clippy gates, both formatting gates and whitespace checks pass.
Accepted frozen run:
`target/bevy-020-qualification/runs/97651d3a02b74e7682da43e60c439b1d/`.
It executes **2,459 passing checks**, including **2,027 editor graph checks**
(170.77 seconds for the serial graph gate). Counts are executions across gates,
not unique tests. All **1,098 input hashes** are unchanged; native execution is
false. Post-documentation input drift is zero.

The final shipping source passes **2,839 editor unit + 3 architecture tests**, with
default parallel execution, the same ten existing ignores and no failures (7.89
seconds for units). Workspace GPU-feature all-target checking passes. Some Cargo
invocations report the existing incremental-cache hard-link fallback to copying;
there are no code lint failures.

## Remaining work

Qualify remaining application handlers and shell registration, then full native
editor workflow acceptance and the coherent root engine switch. This slice runs the
actual combined ECS registration, layout and event consumers in a headless UI fixture;
it does not exercise OS picking, GPU rendering or the full native editor window.
No native/visual acceptance is newly approved. Shipping stays on 0.19.1.
