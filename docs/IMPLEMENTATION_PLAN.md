# Aestra implementation plan

Status: initial execution plan

This plan turns `aestra_ai_native_vfx_editor_plan.md` into a build sequence for the
current repository. The source document remains the long-term product and
architecture vision; this file is the shorter delivery plan.

## Current delivery status

- M0 reference behavior and architecture decisions: complete.
- M1 semantic core and format v3 foundation: complete for the native 3D module set.
- M2 deterministic authoring operations: complete for current editor operations.
- M3 module registry, compiler frontend, and CPU runtime: complete for the initial module set.
- M4 first GPU production slice: complete. The deterministic WESL compute substrate,
  bounded particle pools, compaction lists, counters, native indirect alpha/additive
  sprite drawing, conservative visibility bounds, and explicit CPU/readback fallbacks
  are implemented. Deterministic image regression with approved references, tolerant
  metrics, and diff reports is implemented. Adapter capability reporting, automatic
  backend selection, particle budgets, and per-effect fallback diagnostics close the milestone.
- M5 professional authoring workflow: in progress. The editor has a searchable,
  registry-driven stage/module stack, metadata-defined typed property controls,
  undoable module and renderer structure edits, inline compiler diagnostics, and
  dedicated curve/gradient authoring with draggable keys and fine-grained semantic
  commands. Transactions can now be executed against a temporary document, previewed,
  reviewed as semantic changes with compiler diagnostics, and applied or discarded as
  one history entry. A shared fixed-step playback clock now gives the runtime, editor,
  and viewer exact 60 Hz frame addressing, deterministic seed controls, restart/frame
  stepping, tick-snapped scrubbing, and frame-addressed visual captures. A bounded,
  context-keyed checkpoint store now supports backward restore-and-replay for stateful
  effects, while stateless effects seek directly and snapshotless backends explicitly
  restart and replay. Backend-native GPU snapshot capture remains part of future
  stateful GPU module work. The editor workspace now uses an in-code BSN shell and a
  persisted recursive dock tree. Every authoring panel can be inserted and reordered
  through tab strips or split through directional panel-content drop regions; empty branches collapse automatically,
  splitters expose directional cursors, closed panels can be restored from View, and
  docked tabs can become persisted native secondary windows through an explicit
  context action, enabling multi-monitor layouts. Structured diagnostics now have a
  dedicated severity-filtered dock panel with navigation to owning semantic objects,
  a persistent compile-state footer shortcut, and panel visibility controls under View.
  The live compiler artifact is now exposed through a source-mapped Generated Code
  panel covering execution stages, particle layout, parameter slots, renderer plans,
  optimizations, and WESL entry points. A dockable Profiler now consumes public,
  machine-readable runtime snapshots with measured CPU/live-particle data, rolling
  history, per-emitter peaks, and explicitly labeled compiler estimates; native GPU
  timestamps remain a capability-dependent extension. A dockable Settings workspace
  now owns versioned `settings.ron` persistence, live grid, particle-limit, and UI-scale
  controls, capture defaults, protected recovery from malformed or newer files, and a
  persisted locale. The Settings workspace and editor chrome now form the first
  complete Bevy Feathers migration slices, using the upstream dark theme and
  BSN-backed menus and tooling controls for consistent buttons, checkboxes,
  number entry, scrollbars, focus, cursors, and accessibility. Embedded
  `en-US` and `fr-FR` Fluent catalogs now localize the editor
  shell live with English fallback, semantic message IDs, interpolation, and catalog
  coverage tests. Metadata-defined Inspector scalars, integers, vectors, ranges,
  and toggles now use Feathers controls, commit final values through semantic
  undoable commands, retain in-progress text across live preview frames, and expose
  localized built-in names and descriptions. Semantic emitter transforms now flow
  from persisted format-v3 assets through commands, compiler artifacts, CPU/GPU
  execution, compact Inspector controls, and viewport gizmos. Gizmo drags preview
  temporary transactions and commit one undoable command on release. M5 workflow now
  includes debounced atomic recovery snapshots, startup restore/discard,
  stale-snapshot cleanup, save/discard cleanup, and persisted autosave enablement and
  interval controls. The
  remaining M5 work is localization of the other deep authoring workspaces.
- M6 renderer, material, and asset breadth: in progress. Renderers now reference stable,
  reusable material definitions instead of owning presentation state. The first sprite
  material domain compiles blend state, typed softness/tint inputs, explicit particle-color
  consumption, stable texture assets, and UV regions into backend-independent material
  plans consumed by native GPU, readback, and CPU presentation. The Assets and Inspector
  workspaces expose material creation, assignment, and undoable editing. Imported
  flipbooks now add explicit atlas frames, particle-age/effect-time animation,
  forward/reverse/ping-pong playback, looping, deterministic random starts, native WESL,
  CPU/readback parity, authoring controls, and a viewer example. Procedural recipes,
  mesh/ribbon production paths, renderer sorting, and richer material domains remain.

## Stability-hardening milestone

Complete this milestone before expanding the editor or renderer feature surface further.

1. **Correct native-GPU 3D view integration — complete.** GPU draws now consume Bevy's
   per-view visible-entity classes, preserving inherited visibility, render-layer, and
   frustum decisions. World-space bounds centers feed camera-relative transparent
   sorting with deterministic renderer-order bias. Focused tests cover per-view
   selection, transformed sort centers, depth ordering, and renderer tie-breaking; the
   `--editor-viewport-smoke` capture remains the acceptance gate for constrained preview
   and overlay-camera changes.
2. **Harden recovery cleanup — complete.** Active recovery snapshots remain tracked until
   deletion succeeds, with throttled retries after transient filesystem failures.
   Cleanup is driven by the persisted snapshot rather than a revision marker, and
   document switches wait for the previous snapshot to be removed. Focused tests cover
   failed deletion and retry, save, document switch, startup discard tracking, and
   autosave-disable behavior.
3. **Make settings replacement crash-safe — complete.** Settings now stage into unique,
   same-directory temporary files and atomically replace the canonical file without first
   moving it away. File contents are synchronized before replacement, directory metadata
   is synchronized where supported, and startup recovers interrupted legacy replacements
   or initial writes. Tests cover unique staging, canonical-file continuity, legacy and
   first-write recovery, protected malformed/newer files, and repeated replacements.
4. **Remove multiplicative renderer work — complete.** GPU compaction now writes each
   emitter into its own bounded alive-index segment and maintains one indirect command
   per emitter. Renderers share only their emitter's command and segment, so particles
   from unrelated emitters are no longer submitted and discarded for every draw.
   Profiles expose submitted instances alongside live particles, the buffer estimate
   includes the indirect command table, focused tests cover command/range isolation,
   and the native editor-viewport GPU smoke remains the visual acceptance gate.
5. **Decompose the editor application — in progress.** The timeline, viewport,
   inspector, and docking lifecycle are extracted domain plugins with explicit
   system-set boundaries. `DockingPlugin` owns persisted layout loading, transient
   drag/resize state, drop-affordance reconciliation, and native floating-window
   synchronization. The
   inspector owns module-stack construction, semantic property controls, numeric scrub
   transactions, renderer fields, focus navigation, contextual help, and focused tests.
   The reusable Bevy Feathers layer is now extracted into `src/feathers/`, with
   one plugin owning the upstream Feathers setup, theme, activation auditing, and
   scrollbar synchronization. Action buttons, combo/action menus, compact field
   rows, numeric scrub policy, panel headings, BSN scenes, separators, status
   surfaces, and scroll areas no longer live in `main.rs`. Finish moving dock-tree
   entity construction behind the docking boundary, then separate menus, settings,
   persistence, and localization while
   keeping `main.rs` as composition and startup wiring.
6. **Establish automated quality gates.** Run formatting, workspace checks, strict
   Clippy, and tests in CI. Add at least one supported GPU visual smoke job and keep the
   tolerant reference-image workflow for broader rendering regression coverage.
7. **Refresh contributor documentation.** Mark the prototype assessment below as a
   historical baseline or rewrite it to match the implemented semantic/compiler
   architecture.

## 1. Assessment of the current prototype

The existing vertical slice already proves several useful contracts:

- three top-level products: `aestra-editor`, `aestra-bevy`, and `aestra-viewer`;
- a versioned RON effect asset;
- deterministic CPU evaluation;
- Bevy playback through `AestraPlugin` and `EffectPlayer`;
- an editor session with selection, file workflows, undo, redo, and preview;
- a standalone viewer and visual capture workflow.

It is not yet the semantic/compiler architecture described by the evolution
document:

- `aestra-bevy` currently owns the authored model, evaluator, persistence, and
  Bevy adapter in one crate;
- `EffectAsset`, compiled data, and runtime instance state are not distinct;
- emitter behavior is a fixed Rust struct rather than an ordered stage/module
  program;
- IDs are user-facing strings and some references use vector indices;
- validation returns one enum error rather than a structured diagnostic report;
- editor history stores whole-document snapshots around unrestricted closures,
  not serializable semantic commands and atomic transactions;
- the editor and viewer call the evaluator directly; there is no compiler IR;
- renderer declarations exist, but simulation, assets, materials, and renderer
  instances do not yet have independent models.

The prototype's visual behavior should be preserved as a conformance reference,
but its format-v1 schema is not retained.

## 2. Build-order decisions

1. AI is deferred until the semantic model, commands, transactions, diagnostics,
   compilation, and preview APIs work without a UI.
2. The visual node graph is a projection of semantic data. It is never the
   canonical saved effect.
3. Keep the three product packages at the workspace root. Put reusable internal
   libraries under `crates/`.
4. Do not create every future crate immediately. Extract a crate when its public
   boundary is needed and tested.
5. Keep the CPU evaluator as the conformance oracle while introducing a compiler
   and GPU backend.
6. Replace the planar prototype formats with native-3D format v3. Only v3 is supported; no legacy
   parser or migration layer is maintained.
7. Implement commands immediately after the v3 semantic foundation. Although the
   source document lists commands later in one revised priority list, commands do
   not depend on the GPU runtime and are necessary to make subsequent editor
   changes safe and automatable.

## 3. Target repository boundaries

```text
aestra-editor/                 Bevy UI authoring product
aestra-bevy/                   Public Bevy integration and GPU backend
aestra-viewer/                 Playback, capture, and analysis product
crates/
  aestra-core/                 Semantic source model, IDs, values, diagnostics
  aestra-compiler/             Validation, lowering, IR, optimization, artifacts
  aestra-runtime/              Runtime contracts and deterministic CPU backend
  aestra-authoring/            Commands, transactions, history, diff, locks
  aestra-graph/                Custom module/subgraph language, added when needed
  aestra-ai/                   Optional AI client layer, added only at its milestone
```

Dependency direction:

```text
aestra-core
    ^
    +-- aestra-authoring
    +-- aestra-compiler ----> aestra-runtime ----> aestra-bevy
    +-- aestra-graph -----------^

aestra-editor ----> authoring + compiler + runtime/Bevy preview
aestra-viewer ----> compiler + runtime/Bevy playback
```

No internal semantic crate depends on Bevy UI. Bevy-specific asset loaders, data
interfaces, render extraction, GPU resources, and ECS components remain in
`aestra-bevy`.

## 4. Delivery milestones

### M0 — Freeze the reference behavior

Goal: make the current prototype a safe baseline for refactoring.

Deliverables:

- architecture decisions for terminology, IDs, the format reset, and crate
  boundaries;
- a golden v3 asset and deterministic evaluator snapshots at selected times;
- save/load, evaluator determinism, and viewer capture smoke tests;
- performance baseline for the bundled example;
- explicit statement that `EffectAsset`, `CompiledEffect`, and `EffectInstance`
  are different types with different lifecycles.

Exit gate: the current sample can be compared mechanically with every subsequent
runtime implementation.

### M1 — Semantic core and format v3

Goal: define a complete authored effect independently of Bevy and editor layout.

Start `crates/aestra-core` with:

- typed stable IDs for effects, emitters, modules, renderers, curves, parameters,
  events, materials, and referenced assets;
- `EffectAsset` containing parameters, emitters, events, dependencies, metadata,
  and quality profiles;
- emitters with an explicit simulation domain, ordered module instances, and
  zero or more renderer instances;
- explicit stages such as emitter spawn/update and particle spawn/update;
- open `ModuleTypeId` and `RendererTypeId` values rather than a permanently
  closed enum of all possible modules;
- typed values, parameter bindings, curves, gradients, and custom particle
  attributes;
- editor layout metadata in a separate document keyed by semantic IDs;
- deterministic serialization and explicit format-version rejection;
- structured diagnostics with severity, code, message, semantic path, and optional
  remediation.

Initial built-in semantic set:

```text
Particle Spawn: Shape, InitialLifetime, InitialVelocity, InitialRotation
Particle Update: Gravity, Drag, Turbulence, ColorOverLife, SizeOverLife,
                 OpacityOverLife, Integrate
Renderer:        SpriteRenderer
```

Exit gate: a v3 effect can be created, validated, serialized deterministically,
loaded without Bevy, and can represent the current bundled example.

### M2 — Deterministic authoring operations

Goal: make every meaningful change callable without simulating UI input.

Start `crates/aestra-authoring` with:

- `EffectCommand` for add/remove/move/enable operations, value changes,
  connections, renderer changes, and curve edits;
- `EffectTransaction` with all-or-nothing validation and rollback;
- `CommandExecutor` and bounded undo/redo history;
- semantic selection and lock state based on typed IDs;
- structured semantic diffs suitable for UI review and tests;
- command preconditions so stale IDs, type mismatches, and locked targets fail
  without mutating the document.

Refactor `EditorSession` to own document, editor metadata, selection, locks,
history, diagnostics, and preview state. UI systems may read session state and
submit commands, but may not mutate the effect directly.

Exit gate: editor edits, tests, and a small CLI test harness all use the same
commands; multi-command failure leaves the document unchanged; undo and redo are
tested across structural and parameter edits.

### M3 — Module registry, compiler frontend, and CPU runtime

Goal: compile semantic effects into an immutable executable representation.

Start `crates/aestra-compiler` and `crates/aestra-runtime` with:

- a module registry exposing category, stages, typed inputs/outputs, read/write
  attributes, tags, capability requirements, and approximate cost;
- graph validation for stage compatibility, bindings, dependencies, cycles, and
  required attributes;
- a typed IR representing values, attributes, parameters, stages, math, and
  renderer dependencies;
- lowering from built-in module instances into IR;
- initial constant folding and dead-attribute elimination;
- `CompiledEffect` with particle layout, compiled curves, execution plans,
  renderer plans, and source-to-IR diagnostic mapping;
- `EffectInstance` with time, seed, transform, parameter overrides, active
  emitters, and runtime allocation handles;
- a deterministic CPU interpreter for the initial IR.

Exit gate: format-v3 examples compile and match the frozen CPU reference within
defined tolerances; editor and viewer preview only through compile/instantiate/
update APIs.

### M4 — First GPU production slice

Goal: prove the architecture with one complete, professional-looking GPU path.

Implement in `aestra-bevy`:

- Bevy asset loading and compiled-effect caching;
- particle storage derived from compiler attribute liveness;
- bounded allocation, spawn, update, kill, and compaction;
- seeded Particle Spawn and Particle Update compute passes;
- one additive/alpha sprite renderer with typed material bindings;
- explicit bounds and basic culling;
- indirect dispatch/draw where supported, with a documented fallback;
- conformance tests against the CPU interpreter and a headless render smoke test;
- the same runtime path in games, editor preview, viewer, and capture mode.

Shader policy: Aestra authors and tests WESL modules. Bevy's WESL loader performs
the final lowering required by wgpu; checked-in runtime shader sources are not WGSL.

The demo effect for this milestone should exercise a timed burst, continuous
emission, forces, curves, gradients, at least two emitters, and multiple sprite
renderers without relying on product-specific examples.

Exit gate: the GPU path renders the demo deterministically for a seed, stays
within authored particle bounds, reports unsupported capabilities, and visually
matches the CPU reference closely enough for approved image snapshots.

### M5 — Professional authoring workflow

Goal: expose the semantic model efficiently to technical artists.

Deliver:

- effect hierarchy and searchable module stack;
- properties generated from module metadata;
- timeline, curve, diagnostics, generated-code, and profiler tabs;
- node graph as an optional projection for dataflow-heavy authoring;
- preview transaction, semantic diff, accept/reject, and undo;
- semantic viewport selection and gizmos that submit normal commands;
- autosave/recovery and explicit asset migrations;
- a dockable Settings workspace backed by a versioned, persistent editor-settings
  document stored separately from effect assets and workspace layout;
- editor localization through Fluent message catalogs, including runtime locale
  switching, fallback, and localized diagnostics without translating asset data;
- fixed-step preview, restart, frame step, seeded playback, and checkpoint-based
  backward scrubbing.

Exit gate: the initial demo can be authored from an empty effect without editing
RON, all mutations are undoable, and invalid operations produce targeted
diagnostics without damaging the document.

Persistent settings slice:

1. Define a serde-defaulted `EditorSettings` model with an explicit format version.
2. Store it as `settings.ron` in Aestra's platform configuration directory, separate
   from `editor-layout.ron`, using atomic temporary-file replacement.
3. Add a Settings command and dockable Settings panel with General, Preview,
   Performance, Capture, Appearance, Language, and Keybindings categories.
4. Apply reversible settings live where possible; clearly mark restart-only values.
5. Preserve unknown/newer files without destructive rewrites, recover malformed files
   to defaults with diagnostics, and test defaults, round trips, and persistence.

Localization slice:

1. Introduce a small editor localization service backed by Fluent bundles and stable,
   semantic message identifiers; UI code must not use visible English text as keys.
2. Ship an embedded `en-US` catalog as the complete fallback and load additional locale
   catalogs from editor resources without coupling them to effect assets.
3. Persist the selected locale in `EditorSettings`, allow live switching from the
   Settings panel, and fall back from region to language and then `en-US`.
4. Localize menus, commands, panels, tooltips, status text, validation messages, and
   variable interpolation while leaving user-authored names, paths, and code untouched.
5. Add catalog validation and tests for missing or malformed messages, fallback,
   interpolation, and coverage of every registered editor command and panel title.

The Fluent runtime, English fallback, French catalog, live persisted switching, and
editor-shell coverage are complete. Inspector metadata, validation messages, profiler
details, generated-plan descriptions, and remaining authoring content migrate next;
user-authored names, file paths, semantic IDs, and generated code stay untranslated.

### M6 — Renderer, material, and asset breadth

Goal: move from a sprite particle tool to effect choreography.

Add incrementally:

- independent sprite, texture, mesh, flipbook, and material asset IDs/registry;
- typed material inputs and attribute/parameter bindings;
- multiple renderers per emitter;
- mesh instancing, ribbons/strips, flipbooks, and renderer-controlled sorting;
- procedural mesh and sprite recipes before external generation;
- effect dependencies, child effects, parameter forwarding, and event routing;
- modular collision and event outputs.

Each renderer ships with compiler validation, CPU/reference semantics where
meaningful, GPU tests, profiler counters, and a viewer example.

### M7 — Scale, profiling, and platform quality

Goal: make runtime cost visible and controllable.

Deliver:

- per-effect/emitter/module counters and GPU timing where available;
- debug attribute/event inspection;
- renderer batching, culling, bounds, sorting, and indirect draw refinements;
- quality tiers and compiler-visible platform capability profiles;
- a global VFX budget manager with deterministic degradation decisions;
- performance regression scenes and CI thresholds on a defined reference target.

Exit gate: an effect can declare quality behavior, the runtime enforces aggregate
budgets, and the editor explains both estimated and measured costs.

### M8 — Stable tool API and AI transactions

Goal: let scripts and AI use the same safe public operations as the editor.

Expose versioned tools for inspection, discovery, commands, validation,
compilation, profiling, preview, and semantic diff. First test the API through
Rust integration tests and scripts. Then add `crates/aestra-ai` as an optional
client that:

1. inspects context and selection;
2. proposes a typed transaction;
3. validates and compiles it in a temporary document;
4. produces a semantic diff and preview;
5. commits only after acceptance.

Provider SDKs, local models, retrieval, MCP, multimodal input, and external asset
generation are later adapters. None may define or bypass Aestra semantics.

Exit gate: a provider can be removed entirely and all editor/runtime workflows
still function; rejected or invalid AI proposals never modify the working asset.

## 5. First implementation slice

The first build slice should end after M2 and be delivered in small, reviewable
changes:

1. Add architecture decisions and golden reference-behavior tests.
2. Create `aestra-core` with typed IDs, diagnostic types, and deterministic value
   primitives.
3. Add the minimal v3 `EffectAsset`, stage, module instance, and sprite renderer
   model.
4. Replace the example with format v3 and add golden serialization tests.
5. Create `aestra-authoring` with commands, atomic transactions, selection, locks,
   history, and semantic diff.
6. Refactor `EditorSession` to submit commands while retaining the existing UI.
7. Move persistence and semantic validation out of `aestra-bevy`.
8. Update editor/viewer loading so both accept and save only v3 assets.

This slice intentionally does not redesign the UI, generate WESL, add an LLM, or
implement every future module.

## 6. Verification required at every milestone

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test --workspace`
- stable serialization fixtures and unsupported-version tests
- structured validation snapshots
- deterministic seed/time tests
- CPU/compiler conformance tests once the IR exists
- headless GPU and image regression tests once the GPU backend exists
- viewer capture smoke tests for every public example
- no dependency from semantic crates to Bevy UI or an AI provider

## 7. Planning rules

- A milestone is complete only when its exit gate is automated where practical.
- New semantic features require commands, diagnostics, serialization, migration
  consideration, compiler support, runtime support, and an example.
- New renderer/runtime features require explicit capability and fallback behavior.
- Experimental advanced features stay behind narrow interfaces until the P0 and
  P1 acceptance criteria in the source plan are satisfied.

## 8. Docking design reference

The `jackdaw_panels` crate was reviewed as a Bevy-native reference. Aestra adopts its
most useful architectural ideas without copying its broader editor feature set:

- compose docking as a plugin with named scheduling phases instead of registering
  unrelated systems from the application root;
- keep a serializable layout model distinct from transient drag, hover, resize, and
  spawned-entity state;
- reconcile visible dock entities and native windows from that model after structural
  changes;
- keep tabs, splits, drag/drop, and window synchronization as separable concerns even
  while they share one public plugin boundary.

Jackdaw's workspace tabs, sidebars, and add-window popup are intentionally deferred:
Aestra currently needs one VFX workspace and already restores panels from the View
menu. Its descriptor/registry approach becomes worthwhile when third-party panels are
supported; until then the closed `DockPanel` enum gives exhaustive behavior and simpler
persistence migrations.
- The source plan's large checklists are product acceptance criteria; this file's
  milestones are the implementation order.

## 9. Editor Feathers design reference

Jackdaw's current `main` branch was reviewed after its Bevy 0.19 migration. Aestra
adopts the architectural ideas that improve consistency without importing editor
domain assumptions:

- keep every reusable editor widget below one Feathers plugin and source folder;
- expose data-driven option/props types instead of rebuilding combo and field-row
  structure at each call site;
- use a stable label/control column that wraps on narrow panels;
- preserve collapse and scroll state by semantic keys rather than transient entities;
- build on Bevy Feathers focus, accessibility, menus, text editing, and theme tokens;
- keep numeric scrub mechanics independent from the semantic command committed by
  the owning Inspector field.

The first slice is implemented in `aestra-editor/src/feathers/`. Its widgets cover
Aestra's current shared controls. The following Jackdaw primitives become useful as
the corresponding Aestra feature lands, rather than being copied unused:

1. replace remaining hand-built collapsible cards with remembered panel cards;
2. add slider and swatch rows when continuous scalar and color authoring expand;
3. add list/tree primitives for the future searchable content browser;
4. evaluate Jackdaw's Bevy 0.19 scrub input as a replacement for the remaining
   Inspector-owned pointer state after its semantic preview/commit adapter is isolated;
5. add dialogs, toasts, progress, and file-browser widgets only when those workflows
   need in-editor non-native surfaces.

The generic tooltip slice is complete: Inspector and viewport help now share delayed,
popover-based content with optional titles, shortcuts, and footers. Text remains localized at
the call site, and parenting the popup to the hovered control keeps placement scoped to the
correct native window.

Dock tabs, splitters, timeline clips, curve keys, and viewport gizmos remain specialized
Aestra controls. They consume the shared widget and theme layer but are not generic
Feathers primitives.
