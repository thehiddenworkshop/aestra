# Aestra architecture

## Product direction

Aestra is an effect choreography system, not only a particle editor. A production effect may combine bursts, trails, ribbons, mesh animation, material parameters, screen-space accents, sound/event cues, and child effects on one timeline. The editor therefore treats an effect as layered, timed data and keeps simulation modules separate from rendering modules.

The UI is built directly with Bevy UI. Reusable editor widgets remain independent of effect semantics so they can later move into any shared Bevy editor-UI library.

## Boundaries

```text
Authoring UI (Bevy UI)
              │ edits
              ▼
EffectAsset + validation (aestra-bevy)
              │ compiles
              ▼
Runtime IR (planned) ──► CPU reference runtime
              └────────► GPU compute runtime + Bevy renderer
```

### `aestra-bevy`

- Owns stable serialized authoring data and format versions.
- Validates assets at import/save boundaries.
- Provides deterministic reference evaluation for previews and tests.
- Exposes `AestraPlugin` and `EffectPlayer` for direct integration in Bevy applications.
- Owns no editor or viewer state.

### `aestra-editor`

- Owns selection, undo/redo, panels, viewport controls, and timeline state.
- Presents Bevy-native UI and editor interactions.
- Consumes `aestra-bevy` through an explicit session resource.
- Must not add game-only concepts to the stable asset schema without a migration path.

### `aestra-viewer`

- Plays any valid `.aestra.ron` effect without opening the editor.
- Captures evenly sampled frames across an effect lifetime.
- Produces individual PNGs, a contact sheet, and a capture manifest for visual or AI review.
- Shares runtime behavior with games by using `AestraPlugin` directly.

### GPU compiler/runtime (next)

- Resolves asset references and compiles curves/gradients into GPU-friendly tables.
- Applies platform budgets and capability fallbacks.
- Runs compute simulation, event queues, sorting, and indirect drawing.
- Preserves the public Bevy plugin contract: spawn, stop, parameter overrides, and events.

## Choreography model

The format begins with effect-level duration and looping, then layers with independent start/duration windows. Each layer combines:

- an emitter (where and when particles are born);
- update behavior (forces, drag, turbulence, curves);
- a renderer declaration (billboard, ribbon, or mesh);
- blend behavior;
- event links between layers.

The current file format is versioned from its first commit. Future schema changes require either backward-compatible defaults or an explicit migration pass.

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
- [ ] timeline zooming and snapping
- node/module stack for spawn, initialize, update, renderer, and events
- autosave/recovery and asset migrations
- reusable, dockable Bevy UI panels

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
- profiling overlays and per-emitter cost estimates
- thumbnails, content browser search/tags, templates, and presets
- bake/compile pipeline with validation in CI

## Quality constraints

- A saved effect must be deterministic for a seed and time.
- Editor-only types never appear in runtime assets.
- Unknown format versions fail clearly; they are never silently interpreted.
- CPU evaluation is the semantic reference for GPU conformance tests.
- Runtime allocation and draw counts are bounded by authored and platform budgets.
- Visual modules degrade explicitly on unsupported platforms.
