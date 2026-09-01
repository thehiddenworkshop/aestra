# Aestra architecture

## Product direction

Aestra is an effect choreography system, not only a particle editor. A production effect may combine bursts, trails, ribbons, mesh animation, material parameters, screen-space accents, sound/event cues, and child effects on one timeline. The editor therefore treats an effect as layered, timed data and keeps simulation modules separate from rendering modules.

The UI is built directly with Bevy UI. Bevy Feathers supplies the editor-facing
widget, theme-token, focus, cursor, and accessibility foundation so standard
controls track the visual language of the future Bevy editor. Its stable shell
and reusable structural and Feathers widget scenes use Bevy Scene Notation
(BSN), while dynamic authoring lists remain normal ECS-backed builders.
Workspace placement, tab stacks, and recursive split ratios live in a persisted
dock-tree resource rather than in effect assets. Specialized controls such as
docking, the viewport, timeline, and curve editor remain Aestra-owned while
consuming the shared theme foundation.

## Boundaries

```text
Authoring UI (Bevy UI)
              │ discovers/resolves
              ▼
Project asset index (aestra-project)
              │ loads stable EffectAssetRef dependencies
              ▼
EffectAsset + validation (aestra-core)

Authoring UI (Bevy UI)
              │ submits
              ▼
Semantic commands + transactions (aestra-authoring)
              │ validates and edits
              ▼
EffectAsset + validation (aestra-core)
              │ compiles
              ▼
Typed execution plan (aestra-compiler)
              │ instantiates
              ▼
EffectInstance (aestra-runtime) ──► CPU reference interpreter
                            └─────► packed GPU artifact (aestra-gpu)
                                          │
                                          ▼
                                  Bevy/WGPU presentation
```

### `aestra-core`

- Owns stable serialized authoring data and format versions.
- Owns the engine-independent `EffectClip` and serializable `EffectAssetRef` value types; project
  source discovery remains outside the semantic model.
- Validates assets at import/save boundaries.
- Has no dependency on Bevy, editor UI, or an AI provider.

### `aestra-project`

- Owns engine-independent project source discovery and typed asset-reference resolution.
- Uses the persisted `EffectId` inside an effect as semantic identity; paths are locations and may
  change without invalidating `EffectAssetRef`.
- Retains invalid and unsupported files as indexed sources, rejects duplicate effect IDs as
  ambiguous, and returns structured missing/duplicate/unavailable resolution errors.
- Exposes source-row IDs only for addressing files that cannot provide a valid semantic ID. Those
  IDs are never serialized into effect dependencies.
- Resolves complete transitive effect dependency sets and rejects missing, ambiguous, changed, or
  cyclic references with the owning effect and clip identity preserved in structured diagnostics.

### `aestra-bevy`

- Re-exports the public semantic types for convenient Bevy integration.
- Adapts compiled runtime instances and particle samples to Bevy ECS and rendering.
- Exposes `AestraPlugin` and `EffectPlayer` for direct integration in Bevy applications.
- Owns no editor or viewer state.

### `aestra-compiler`

- Owns the extensible module registry and metadata used for discovery and validation.
- Validates stages, supported renderer capabilities, and particle attribute flow.
- Lowers authored constants and parameter bindings into immutable typed expressions.
- Compiles curves and gradients, folds constants, removes dead particle storage, and retains source mapping.
- Compiles an indexed effect project into a root plan plus its unique transitive child-effect plans.

### `aestra-runtime`

- Owns `CompiledEffect`, `EffectInstance`, particle layouts, and execution instructions.
- Interprets compiled effects deterministically for a seed and time without Bevy.
- Maps active `EffectClip` windows into child-effect time, derives stable per-instance seeds, and
  returns nested CPU-reference samples with effect and clip-path provenance.
- Owns the fixed-step playback clock and bounded, context-keyed checkpoint contract
  shared by games, editor preview, viewer playback, and visual capture. Compiled
  effects declare direct seek, checkpoint restore, or restart-and-replay semantics so
  stateful backends cannot silently use stateless seeking.
- Resolves indexed, type-checked parameter overrides without recompiling an effect.
- Defines the engine-independent contract that future CPU and GPU backends must preserve.

### `aestra-gpu`

- Owns the packed curves, gradients, emitter, renderer, particle, global, and indirect-draw ABI.
- Lowers compiled effect instances and parameter values into GPU artifacts without Bevy or WGPU.
- Owns the reference WESL modules, composes inspectable WGSL, and validates it explicitly with Naga.
- Keeps generated WGSL snapshots tied to representative compiled artifacts.
- Derives conservative effect bounds and stable GPU seed/index contracts.
- Depends only on portable Aestra contracts plus engine-neutral data-layout and math libraries.

### `aestra-bevy-render`

- Uploads `aestra-gpu` artifacts into Bevy shader buffers.
- Registers the portable WESL sources with Bevy and owns render-world extraction, WGPU pipeline
  setup, compute dispatch, readback, texture resolution, and draw submission.
- Exercises generated simulation WGSL through a deterministic native-compute conformance harness;
  fixed-time particle readback is compared with the CPU semantic reference across once,
  restart-loop, and continuous-loop playback, including emitter regions and surviving prior-cycle
  particles. The same harness covers emitter-time and particle-life curves plus deterministic
  scalar and vector random-range sources. Live scalar, range, vector, curve, and gradient instance
  parameters—and compiler-validated reusable-clip overrides—are compared without recompiling the
  source effect. Event-aware advancement verifies that deterministic choreography dispatch and
  native GPU simulation consume the same playback clock across once, restart-loop, and continuous
  playback, while backend-independent tests cover exact boundaries, multi-loop steps, seek, pause,
  restart, and equal-time event ordering.
- Is shared by the editor preview and `aestra-bevy`; neither consumer depends on the other.

### `aestra-authoring`

- Owns semantic commands, atomic transactions, inverse-command history, locks, selection, and diffs.
- Treats reusable effect clips as stable semantic targets with atomic create, delete, timing, and
  seed commands; clip locks, diffs, selection repair, and undo/redo never depend on timeline rows.
- Executes independently of Bevy UI so scripts and future AI clients use the same path as the editor.
- Validates a complete transaction before replacing the working document.
- Stores forward and inverse commands for undo/redo rather than document snapshots.

### `aestra-editor`

- Owns panels, viewport controls, timeline state, and an authoring-backed session.
- Presents Bevy-native UI and editor interactions.
- Uses Bevy Feathers for standard tooling controls and theme semantics. The
  editor menu bar, dropdowns, primary toolbar, Settings workspace, and
  metadata-driven Properties inputs use Feathers menus, buttons, pane/group
  containers, checkboxes, editable numeric inputs, bounded sliders, themed typography, keyboard
  focus, cursors, and accessibility labels. Properties edits commit only when an
  interaction is final and enter the semantic command history as undoable changes.
- Owns a dedicated `src/feathers/` widget layer above Bevy Feathers. Reusable
  button activation, combo/action menus, compact field rows, scrub-number policy,
  bounded slider/number pairs,
  panel chrome, semantic-keyed collapsible cards, delayed window-aware tooltips,
  BSN scenes, scroll areas, separators, and status surfaces live there rather than
  in application or panel builders.
  Domain plugins retain only semantic state and commands; docking, timeline,
  viewport, and effect-specific Properties behavior do not leak into generic widgets.
- Owns a persisted recursive workspace dock tree with tab-strip insertion and
  directional panel-content splitting,
  closable/recoverable panels, collapsing empty branches, transient drop targets,
  persisted native secondary windows for multi-monitor panels, draggable splitter
  gutters, and directional resize cursors.
- Runs that workspace through a dedicated `DockingPlugin`. Its explicit input,
  reconciliation, recursive entity construction, and native-window synchronization sets
  keep serialized layout state separate from transient ECS pointer state. The editor shell
  declares only a transparent `DockTreeHost`; the plugin populates it and refreshes floating
  panel roots when the editor UI revision changes.
- Keeps window chrome stable while rebuilding only effect-dependent workspace content.
- Runs top-level menu chrome through `EditorMenusPlugin`. The plugin owns popup state,
  hover switching, delayed submenu opening, outside-click dismissal, the panels submenu,
  tab context menus, and menu synchronization; document commands remain editor actions
  handled by the shell.
- Keeps versioned editor preferences separate from both semantic effect assets and the
  persisted dock layout. A dockable Settings panel edits that dedicated document,
  applies supported values live, and protects malformed, unknown, or newer files from
  implicit replacement.
- Runs Settings presentation and interaction through `EditorSettingsUiPlugin`. The
  plugin owns category navigation, Feathers controls, locale selection, reset, and
  live preference application; `settings.rs` remains the versioned persistence,
  migration, validation, and atomic-replacement boundary.
- Runs the Library workspace through `EditorLibraryPlugin`. The plugin projects the shared
  `aestra-project` index into Bevy resources and owns search plus clearly separated current-document
  resource projections; it does not own the emitter hierarchy. Opening a valid catalog
  entry resolves a typed `EffectAssetRef` before emitting the shared `DocumentAction` contract, so
  source moves retain identity while document replacement and
  unsaved-change protection remain exclusively owned by persistence. Invalid and
  unsupported catalog entries remain inspectable, localized status rows rather than actionable
  document controls; duplicate effect IDs are visible but deliberately non-resolvable.
- Runs emitter hierarchy, inline track naming, selection, timing, authored track color,
  Mute/Solo preview state, overflow, and track navigation through `TimelinePlugin`.
  Stable emitter IDs connect track headers and clips to semantic commands, so Timeline
  and Properties naming stay synchronized through the same undoable document field. Track
  swatches open an anchored picker composed from Bevy Feathers' shader-backed color plane,
  lightness/alpha sliders, RGB/HSL number inputs, and editable RGBA hex input. Intermediate
  values preview directly on the track; only final picker values enter semantic undo history.
  Before EffectClip authoring UI exists, a project-effect drag over the
  Timeline produces explicit transient rejection feedback; the drop path cannot mutate
  the effect, history, dirty state, or selection and does not create a synthetic clip.
- Keeps compact Library rows and Timeline panes shrinkable with clipped, non-wrapping
  labels and reserved native scrollbar gutters. Automated composition coverage exercises
  blank and current documents at constrained width without adding a GPU requirement to CI.
- Runs effect-property authoring through `PropertiesPlugin`. Properties controls emit the
  dedicated `PropertiesAction` contract; the plugin owns module-palette navigation,
  effect/emitter identity and execution settings, typed emitter-event links, module and
  renderer mutations, renderer configuration, persisted disclosure state, semantic
  selection, and localized validation/status outcomes. Undoable edits still
  enter the shared `EditorSession` command history, while `main.rs` only schedules the
  Properties action set with the other editor domains.
- Runs curve and gradient authoring through `EditorCurvesPlugin`. The plugin owns the
  Curves workspace, semantic property/key selection, graph interaction, key controls,
  and its Feathers action bridge. Properties links emit `CurvesAction`, while every key
  mutation still enters the `EditorSession` command history as one undoable semantic edit.
- Runs document lifecycle through `EditorPersistencePlugin`. File-menu controls,
  keyboard shortcuts, and project-effect rows emit one `DocumentAction` contract;
  the plugin owns startup recovery, open/save/save-as/exit workflows, unsaved-change
  confirmation, recovery autosave and cleanup, explicit legacy-asset migration, and
  primary-window close handling. Migration detects the format before deserialization,
  uses typed core transforms, compiles the candidate before replacement, preserves a
  unique synchronized backup, and writes the migrated asset atomically only after user
  confirmation.
  Document operations return domain state while the plugin translates structured
  outcomes into localized dialogs and status messages, preserving file paths and
  technical errors as Fluent arguments rather than embedding presentation in the session.
  `recovery.rs` remains the atomic snapshot storage boundary.
- Resolves editor-facing text through `EditorLocalizationPlugin` and Fluent bundles
  with stable semantic message IDs, an embedded complete English fallback, live locale
  switching, and the selected locale persisted in editor settings. Diagnostics retain
  exact compiler detail and semantic paths while localizing workspace chrome, severity,
  stable diagnostic-code titles, Assets, Timeline, Curves, Compiler Inspector, Profiler, and
  Changes presentation. Asset-authored names, paths, IDs, and generated instructions
  remain locale-independent.
- Runs validation presentation through `EditorDiagnosticsPlugin`. The plugin owns the
  dockable Diagnostics workspace, severity filtering, semantic-path navigation, and
  the persistent compile-health footer action. Compiler validation remains an
  `EditorSession` responsibility, so presentation cannot mutate or replace reports.
- Runs advanced compiler-artifact presentation through `EditorCompilerInspectorPlugin`.
  The dockable Compiler Inspector is hidden from the default workspace but remains
  available from View, and saved `GeneratedCode` layout entries migrate without losing
  their placement. It presents execution stages, source-mapped instructions, particle layout, runtime parameter
  slots, renderer plans, optimization statistics, and the WESL backend entry points.
  Compiled rows navigate back to their semantic emitter, module, renderer, or parameter.
- Runs panel selection, visibility, floating, and workspace reset commands through the
  `DockingAction` contract owned by `DockingPlugin`. Dock tabs, context menus, and View-menu
  entries no longer depend on the global editor action enum. Command and drag/drop outcomes
  use localized panel names, while the serializable `WorkspaceLayout` remains the single
  persisted source of truth.
- Uses one native Bevy scroll-area with a BSN-backed Feathers scrollbar across Properties, Diagnostics,
  Compiler Inspector, Profiler, Changes, and curve lists. Scrollbars only participate in
  layout while their content overflows. Semantic Properties anchors support exact
  scroll-to-source navigation from compiled instructions with a transient highlight
  that fades back to the normal or diagnostic border.
- Runs runtime profiling through `EditorProfilerPlugin`. Preview producers submit borrowed
  `ProfilerFrameSample` values containing the compiled effect, particle slice, and measured
  CPU duration; the plugin exclusively owns `EffectProfile` aggregation, reset actions,
  bounded history, and dockable presentation. The workspace exposes measured CPU/live-particle
  data, per-emitter counts, and clearly marked compiler estimates. Uninstrumented GPU values
  remain unavailable rather than being synthesized.
- Consumes `aestra-bevy` through an explicit session resource.
- Must not add game-only concepts to the semantic asset schema.

### `aestra-viewer`

- Plays any valid `.aestra.ron` effect without opening the editor.
- Captures exact, evenly sampled 60 Hz frame indices across an effect lifetime.
- Produces individual PNGs, a contact sheet, and a capture manifest for visual or AI review.
- Runs deterministic, effect-only GPU regression captures against approved references,
  with tolerant foreground metrics and amplified difference images.
- Shares runtime behavior with games by using `AestraPlugin` directly.

### GPU runtime

- Resolves asset references and compiles curves/gradients into GPU-friendly tables.
- Authors compute shaders in WESL and lets Bevy lower them at the wgpu boundary.
- Detects adapter downlevel flags and device limits, then resolves native GPU,
  GPU-readback, or CPU presentation without attempting unsupported allocation.
- Applies both physical storage/dispatch limits and an application particle budget;
  oversized or unsupported effects fall back independently with a public reason.
- Runs compute simulation, live-particle compaction, alpha/additive/multiply sprite
  presentation, visibility culling, and indirect drawing without a per-frame CPU readback.
- Resolves renderer material IDs through an immutable compiled material registry. Sprite
  materials own blend state, typed softness/tint parameter expressions, particle-color
  bindings, stable texture IDs, and normalized UV regions; missing files remain visible
  through a diagnostic checkerboard fallback.
- Retains deterministic CPU and GPU-readback presentation modes as explicit
  reference and compatibility paths.
- Publishes global adapter capabilities and per-effect active-backend diagnostics
  for games, editor status UI, viewer HUD, and capture manifests.
- Attaches an `EffectProfiler` component to Bevy players so games and tools consume the
  same measured/estimated/unavailable profile contract as the editor.
- Preserves the public Bevy plugin contract: spawn, stop, parameter overrides, and events.

## Choreography model

The format begins with effect-level duration and looping, then emitters with independent start/duration windows. Each emitter contains:

- an explicit simulation domain;
- an optional editor display color used consistently by choreography views and ignored by
  simulation, compilation, and rendering;
- ordered modules assigned to explicit execution stages;
- module inputs with authored fallback values and optional typed effect-parameter bindings;
- stable IDs for modules, curves, gradients, and renderer instances;
- one or more renderers referencing reusable materials through stable semantic IDs;
- sprite materials with blend state, typed constant/effect-parameter inputs, explicit
  particle-color consumption, texture assets, and UV regions;
- typed event links between emitters.

Effect-level choreography is modeled separately from particle lifecycle links. Stable
`ChoreographyEvent` objects carry a typed gameplay, sound, camera, or child-effect payload at an
absolute or marker-relative time. The compiler orders them by time and semantic ID, the runtime
dispatches crossed intervals deterministically across loop boundaries, and `AestraPlugin` exposes
each result as `AestraChoreographyEvent` for normal Bevy observers.

The current file format is version 3. Prototype version 1 is intentionally unsupported
and has no legacy loader. Version 2 assets are upgraded only through the editor's explicit,
confirmed, backup-preserving migration path; core loading never silently interprets an
outdated or future format.

## Roadmap

### Phase 1 — foundation (complete)

- Bevy UI workspace and professional editor layout
- deterministic CPU reference evaluator
- layer/timeline playback and basic properties editing
- versioned RON effects, validation, save, and sample content

### Phase 2 — real authoring (in progress)

- [x] command-based undo/redo and change history
- [x] curve previews, key-value editing, and gradient presets
- [x] timeline scrubbing, trimming, moving, and effect-duration editing
- [x] native file dialogs, project asset discovery, and unsaved-change protection
- [x] module registry, typed compiler plan, runtime instances, and CPU extraction
- [x] runtime parameter slots, compiled curves, constant folding, and attribute liveness
- [x] cursor-centered timeline zooming, panning, adaptive rulers, and configurable snapping
- [x] module stack for spawn, initialize, update, and renderer stages
- [x] typed event-link authoring between emitter layers
- [x] typed, marker-relative choreography events with timeline and Bevy dispatch
- optional node-graph projection for dataflow-heavy authoring
- [x] atomic debounced autosave and startup crash recovery
- [x] typed, confirmed, backup-preserving asset migrations
- [x] versioned persistent editor settings and a dockable Settings workspace
- [x] Fluent editor-shell localization with catalog validation, fallback, and live locale switching
- [x] localized built-in Properties module inputs and descriptions
- [x] semantic emitter transforms with undoable Properties and viewport gizmo editing
- [x] localized diagnostics and profiler workspace content
- [x] localized primary authoring workspace content while preserving technical payloads
- [x] persistent recursive pane resizing and dockable authoring-panel tab stacks
- [x] persisted native floating panel windows with redocking

### Phase 3 — production renderer

- GPU compute simulation and particle buffers
- indirect draw, frustum culling, depth sorting, and bounds
- [x] textured billboards with stable texture assets and UV regions
- [x] reusable sprite materials with blend state and typed parameter/color bindings
- flipbooks, ribbons, meshes, and trails
- effect parameters, exposed inputs, and gameplay bindings
- deterministic seeds plus scalable quality tiers and budgets

### Pre-material authoring hardening gate (in progress)

- [x] replace showcase-dependent behavioral tests with deterministic semantic fixtures
- [x] split Timeline and Properties implementation details into focused internal modules
- [ ] reconcile roadmap status with the implemented renderer, composition, and automation slices
- [ ] approve current showcase changes through format/compiler and native-GPU visual gates
- [ ] deliver the animated additive-flame material vertical slice before a material node graph

### Phase 4 — professional workflow

- sub-effects, collision, decals, lights, audio/event tracks, and camera cues
- live game preview and remote parameter inspection
- [x] dockable profiling workspace, runtime snapshots, and per-emitter particle costs
- thumbnails, content browser search/tags, templates, and presets
- bake/compile pipeline with validation in CI

## Quality constraints

- A saved effect must be deterministic for a seed and time.
- Editor-only types never appear in runtime assets.
- Unknown format versions fail clearly; they are never silently interpreted.
- CPU evaluation is the semantic reference for GPU conformance tests.
- Runtime allocation and draw counts are bounded by authored and platform budgets.
- Visual modules degrade explicitly on unsupported platforms.
