# Aestra implementation plan

Status: initial execution plan

This plan turns `aestra_ai_native_vfx_editor_plan.md` into a build sequence for the
current repository. The source document remains the long-term product and
architecture vision; this file is the shorter delivery plan.

## Current delivery status

- M0 reference behavior and architecture decisions: complete.
- M1 semantic core and format v2 foundation: complete for the initial module set.
- M2 deterministic authoring operations: complete for current editor operations.
- M3 module registry, compiler frontend, and CPU runtime: complete for the initial module set.
- M4 first GPU production slice: in progress. The deterministic WESL compute substrate,
  bounded particle pools, compaction lists, counters, native indirect alpha/additive
  sprite drawing, conservative visibility bounds, and explicit CPU/readback fallbacks
  are implemented. Deterministic image regression with approved references, tolerant
  metrics, and diff reports is implemented; capability reporting remains.

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
6. Replace prototype format v1 with format v2. Only v2 is supported; no legacy
   parser or migration layer is maintained.
7. Implement commands immediately after the v2 semantic foundation. Although the
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
- a golden v2 asset and deterministic evaluator snapshots at selected times;
- save/load, evaluator determinism, and viewer capture smoke tests;
- performance baseline for the bundled example;
- explicit statement that `EffectAsset`, `CompiledEffect`, and `EffectInstance`
  are different types with different lifecycles.

Exit gate: the current sample can be compared mechanically with every subsequent
runtime implementation.

### M1 — Semantic core and format v2

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

Exit gate: a v2 effect can be created, validated, serialized deterministically,
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

Exit gate: format-v2 examples compile and match the frozen CPU reference within
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
- fixed-step preview, restart, frame step, seeded playback, and checkpoint-based
  backward scrubbing.

Exit gate: the initial demo can be authored from an empty effect without editing
RON, all mutations are undoable, and invalid operations produce targeted
diagnostics without damaging the document.

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
3. Add the minimal v2 `EffectAsset`, stage, module instance, and sprite renderer
   model.
4. Replace the example with format v2 and add golden serialization tests.
5. Create `aestra-authoring` with commands, atomic transactions, selection, locks,
   history, and semantic diff.
6. Refactor `EditorSession` to submit commands while retaining the existing UI.
7. Move persistence and semantic validation out of `aestra-bevy`.
8. Update editor/viewer loading so both accept and save only v2 assets.

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
- The source plan's large checklists are product acceptance criteria; this file's
  milestones are the implementation order.
