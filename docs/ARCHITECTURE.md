# Aestra architecture

## Product direction

Aestra is an effect choreography system, not only a particle editor. A production effect may combine bursts, trails, ribbons, mesh animation, material parameters, screen-space accents, sound/event cues, and child effects on one timeline. The editor therefore treats an effect as layered, timed data and keeps simulation modules separate from rendering modules.

The UI is built directly with Bevy UI. Its stable shell and reusable structural
scenes use Bevy Scene Notation (BSN), while dynamic authoring lists remain normal
ECS-backed builders. Workspace placement, tab stacks, and recursive split ratios live
in a persisted dock-tree resource rather than in effect assets. Reusable editor widgets remain
independent of effect semantics so they can later move into any shared Bevy
editor-UI library.

## Boundaries

```text
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
                            └─────► WESL GPU compute runtime + Bevy renderer
```

### `aestra-core`

- Owns stable serialized authoring data and format versions.
- Validates assets at import/save boundaries.
- Has no dependency on Bevy, editor UI, or an AI provider.

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

### `aestra-runtime`

- Owns `CompiledEffect`, `EffectInstance`, particle layouts, and execution instructions.
- Interprets compiled effects deterministically for a seed and time without Bevy.
- Owns the fixed-step playback clock and bounded, context-keyed checkpoint contract
  shared by games, editor preview, viewer playback, and visual capture. Compiled
  effects declare direct seek, checkpoint restore, or restart-and-replay semantics so
  stateful backends cannot silently use stateless seeking.
- Resolves indexed, type-checked parameter overrides without recompiling an effect.
- Defines the engine-independent contract that future CPU and GPU backends must preserve.

### `aestra-authoring`

- Owns semantic commands, atomic transactions, inverse-command history, locks, selection, and diffs.
- Executes independently of Bevy UI so scripts and future AI clients use the same path as the editor.
- Validates a complete transaction before replacing the working document.
- Stores forward and inverse commands for undo/redo rather than document snapshots.

### `aestra-editor`

- Owns panels, viewport controls, timeline state, and an authoring-backed session.
- Presents Bevy-native UI and editor interactions.
- Owns a persisted recursive workspace dock tree with tab-strip insertion and
  directional panel-content splitting,
  closable/recoverable panels, collapsing empty branches, transient drop targets,
  persisted native secondary windows for multi-monitor panels, draggable splitter
  gutters, and directional resize cursors.
- Keeps window chrome stable while rebuilding only effect-dependent workspace content.
- Projects structured validation reports into a dockable diagnostics workspace with
  severity filtering and semantic-path navigation; the persistent footer exposes
  compile health and opens that workspace directly.
- Projects the immutable compiler artifact into a dockable Generated Code workspace:
  execution stages, source-mapped instructions, particle layout, runtime parameter
  slots, renderer plans, optimization statistics, and the WESL backend entry points.
  Compiled rows navigate back to their semantic emitter, module, renderer, or parameter.
- Uses one native Bevy scroll-area/scrollbar composition across Inspector, Diagnostics,
  Generated Code, Profiler, Changes, and curve lists. Scrollbars only participate in
  layout while their content overflows. Semantic Inspector anchors support exact
  scroll-to-source navigation from compiled instructions with a transient highlight
  that fades back to the normal or diagnostic border.
- Projects machine-readable `EffectProfile` snapshots into a dockable Profiler workspace
  with measured CPU/live-particle data, resettable peaks, rolling history, per-emitter
  counts, and clearly marked compiler estimates. Uninstrumented GPU values remain
  unavailable rather than being synthesized.
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
- Runs compute simulation, live-particle compaction, alpha/additive sprite presentation,
  visibility culling, and indirect drawing without a per-frame CPU readback.
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
- ordered modules assigned to explicit execution stages;
- module inputs with authored fallback values and optional typed effect-parameter bindings;
- stable IDs for modules, curves, gradients, and renderer instances;
- one or more renderers with independent blend behavior;
- typed event links between emitters.

The current file format is version 2. Prototype version 1 is intentionally unsupported and has no legacy loader. A compatibility policy will be defined before the asset format is declared stable.

## Roadmap

### Phase 1 — foundation (complete)

- Bevy UI workspace and professional editor layout
- deterministic CPU reference evaluator
- layer/timeline playback and basic inspector editing
- versioned RON effects, validation, save, and sample content

### Phase 2 — real authoring (in progress)

- [x] command-based undo/redo and change history
- [x] curve previews, key-value editing, and gradient presets
- [x] timeline scrubbing, trimming, moving, and effect-duration editing
- [x] native file dialogs, project asset discovery, and unsaved-change protection
- [x] module registry, typed compiler plan, runtime instances, and CPU extraction
- [x] runtime parameter slots, compiled curves, constant folding, and attribute liveness
- [ ] timeline zooming and snapping
- node/module stack for spawn, initialize, update, renderer, and events
- autosave/recovery and asset migrations
- [x] persistent recursive pane resizing and dockable authoring-panel tab stacks
- [x] persisted native floating panel windows with redocking

### Phase 3 — production renderer

- GPU compute simulation and particle buffers
- indirect draw, frustum culling, depth sorting, and bounds
- textured billboards, flipbooks, ribbons, meshes, and trails
- material/shader parameter bindings
- effect parameters, exposed inputs, and gameplay bindings
- deterministic seeds plus scalable quality tiers and budgets

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
