# Aestra GPU Architecture and Future Engine Portability

## Status

**Current implementation target:** Bevy only.

**Architectural constraint:** Aestra must avoid unnecessary Bevy-specific assumptions in its core, compiler, runtime, and shader-generation layers so that additional engine integrations can be implemented later without redesigning the effect format or runtime semantics.

This document does **not** propose implementing Godot, Unity, GameMaker, Unreal, or any other engine now. It defines boundaries that keep those ports feasible while allowing development to remain focused on the Bevy implementation.

## Implementation status vocabulary

This document describes both the current repository and its intended architecture. Each substantial architectural item should be understood using the following status vocabulary:

- **Existing** — implemented and exercised by the current repository;
- **Partial** — a useful implementation exists, but the portable boundary is incomplete;
- **Target** — an agreed direction that is not implemented yet;
- **Deferred** — intentionally postponed until there is a concrete product need.

The current high-level status is:

| Area | Status | Current state |
| --- | --- | --- |
| Engine-neutral semantic model and stable IDs | Existing | Core, project, authoring, compiler, and runtime contain no Bevy dependency. |
| Compiled execution model and CPU reference runtime | Existing | Compiled effects, emitters, execution plans, resources, events, and parameter data already exist. |
| Engine-neutral renderer plan | Partial | Sprite and flipbook plans exist; the portable renderer contract is not yet broad enough for every planned renderer. |
| Backend/device compatibility contract | Existing | The compiler retains portable effect requirements; `aestra-bevy-render` converts Bevy/WGPU discovery into `BackendCapabilities` and produces structured compatibility reports before selecting a presentation path. |
| Editor/runtime-adapter isolation | Existing | `aestra-editor` and `aestra-bevy` are sibling consumers of `aestra-bevy-render`; an architecture test forbids editor imports or a Cargo dependency on the runtime adapter. |
| Engine-neutral GPU lowering | Existing | `aestra-gpu` owns the packed GPU ABI, artifact lowering, reference WESL sources, WESL composition, and explicit Naga validation without Bevy or WGPU. |
| Generated WESL/WGSL and explicit Naga validation | Partial | Representative artifacts produce inspectable, snapshotted, Naga-validated WGSL. The current runtime-sized shaders are shared rather than specialized per effect. |
| CPU/GPU semantic conformance suite | Partial | Deterministic particle fixtures compare CPU-reference and native-compute readback for alive count, identity, position, lifetime progress, rotation, size, and color across once, restart-loop, and continuous-loop playback. Coverage includes emitter regions, surviving earlier-cycle particles, emitter-time spawn curves, particle-life motion curves, and deterministic scalar/vector random ranges. The suite still needs broader module, event, and parameter coverage. |
| Serialized compiled artifact | Deferred | The in-memory compiled representation comes first. |

---

## 1. Goals

Aestra should be designed as a standalone VFX authoring system with a reusable effect model and runtime.

For the foreseeable future:

- Bevy is the only supported runtime integration.
- Bevy is the reference implementation.
- WGPU is the reference graphics implementation.
- WESL/WGSL is the primary shader authoring path.
- No engineering effort should be spent maintaining unimplemented engine adapters.

However, the architecture should ensure that:

- `aestra-core` does not depend on Bevy.
- `aestra-project` does not depend on Bevy.
- `aestra-authoring` does not depend on Bevy.
- `aestra-compiler` does not depend on Bevy.
- `aestra-runtime` does not depend on Bevy.
- VFX semantics do not depend on WGSL, WESL, WGPU, Bevy ECS, or Bevy renderer concepts.
- GPU shader logic can eventually be translated to other shader languages.
- engine-specific resource management and rendering remain behind adapters.
- coordinate-system conventions are explicitly defined by Aestra rather than inherited accidentally from Bevy.

The repository has two independent Bevy consumers. The editor is an authoring application; `aestra-bevy` is the isolated game-runtime adapter. Neither depends on the other. Their shared Bevy-specific presentation code lives in the lower-level `aestra-bevy-render` integration crate.

```text
┌──────────────────────┐      ┌──────────────────────┐
│    Aestra Editor     │      │     aestra-bevy      │
│  authoring app/Bevy  │      │ game runtime adapter │
└──────────┬───────────┘      └──────────┬───────────┘
           │                             │
           └─────────────┬───────────────┘
                         ▼
             ┌──────────────────────┐
             │ aestra-bevy-render   │
             │ shared presentation  │
             └──────────┬───────────┘
                        ▼
             ┌──────────────────────┐
             │     aestra-gpu       │
             │ ABI / data lowering  │
             └──────────┬───────────┘
                        ▼
             ┌──────────────────────┐
             │ Portable Aestra APIs │
             │ compiler / runtime   │
             └──────────────────────┘
```

In the future, other engine adapters could consume the same compiled/runtime representation:

```text
                     Aestra
                       │
              engine-independent
                       │
        ┌──────────────┼──────────────┐
        ▼              ▼              ▼
   aestra-bevy    aestra-godot   aestra-unity
```

Only `aestra-bevy` is a current product goal.

---

# 2. Core Principle: Separate VFX Semantics from GPU Implementation

The most important rule is:

> Aestra's effect model must describe **what an effect means**, not how Bevy or WGPU happens to execute it.

For example, an Aestra update stage should conceptually contain operations such as:

```rust
ParticleUpdate {
    operations: [
        ApplyGravity(...),
        ApplyDrag(...),
        CurlNoise(...),
        IntegratePosition,
        SampleSizeOverLife(...),
    ],
}
```

It should **not** contain the equivalent of:

```text
"execute this WESL snippet"
```

or:

```text
"bind Bevy buffer X to bind group Y"
```

The semantic representation must remain usable by:

- a CPU interpreter;
- a GPU compiler;
- validation tools;
- preview tooling;
- future engine adapters;
- future alternative GPU backends.

This creates two distinct layers:

```text
Aestra semantic/execution IR
            │
            ▼
      GPU shader lowering
            │
            ▼
      WESL / WGSL / Naga
```

The first layer belongs to Aestra.

The second layer is an implementation detail.

---

# 3. Recommended Crate Responsibilities

The exact crate names may evolve, but compile-time dependencies should point from the two independent Bevy consumers toward the portable contracts they consume.

```text
aestra-editor ───────────────► { aestra-authoring, aestra-compiler,
                                 aestra-project, aestra-runtime,
                                 aestra-bevy-render, bevy }

aestra-bevy ─┬───────────────► aestra-bevy-render
             ├───────────────► bevy
             ├───────────────► aestra-compiler
             ├───────────────► aestra-runtime
             └───────────────► aestra-core

aestra-bevy-render ──────────► { aestra-gpu, aestra-runtime, bevy }
aestra-gpu ──────────────────► { aestra-core, aestra-runtime }

aestra-compiler ─────────────► aestra-project
       │
       ├─────────────────────► aestra-runtime
       └─────────────────────► aestra-core

aestra-project ──────────────► aestra-core
aestra-authoring ────────────► aestra-core
aestra-runtime ──────────────► aestra-core
```

This diagram expresses the required architectural dependency direction, not effect-data flow. The editor is an authoring application and `aestra-bevy` is a game-runtime adapter. They share Bevy presentation infrastructure below both consumers, but `aestra-editor` does not depend on `aestra-bevy` and `aestra-bevy` does not depend on `aestra-editor`.

The direct `aestra-editor` → `aestra-bevy` Cargo dependency has been removed. `aestra-bevy-render` owns Bevy/WGPU capability selection plus CPU and GPU presentation, while `aestra-bevy` retains game-runtime playback and lifecycle. A dedicated architecture test guards this boundary. The repository still keeps compiled contract types in `aestra-runtime`, so `aestra-compiler` depends on `aestra-runtime`.

If shared compiled contracts eventually need independent ownership, the target may become:

```text
aestra-compiler ───► aestra-compiled ◄─── aestra-runtime
                           ▲
                           │
                 aestra-gpu / aestra-render
                           ▲
                           │
                      aestra-bevy
```

A future `aestra-compiled` or `aestra-ir` crate is optional. Do not split it out until the current ownership creates concrete friction.

A more explicit future structure could be:

```text
crates/
    aestra-core
    aestra-project
    aestra-authoring
    aestra-compiler
    aestra-gpu
    aestra-runtime

    aestra-bevy-render

    aestra-bevy

apps/
    aestra-editor
```

`aestra-gpu` is the engine-neutral GPU ABI and artifact-lowering crate. `aestra-bevy-render` is the shared Bevy-specific presentation crate that uploads those artifacts, owns shaders and pipelines, dispatches compute work, and submits draws.

## Dependency rule

The following dependencies should remain forbidden:

```text
aestra-core      ─X─► bevy
aestra-project   ─X─► bevy
aestra-authoring ─X─► bevy
aestra-compiler  ─X─► bevy
aestra-runtime   ─X─► bevy
aestra-gpu       ─X─► bevy / wgpu
aestra-editor    ─X─► aestra-bevy
aestra-bevy      ─X─► aestra-editor
```

Bevy dependencies may terminate in the two consumers and their shared lower-level integration crate:

```text
aestra-bevy ───► bevy
aestra-editor ─► bevy
aestra-bevy-render ─► bevy
```

Sharing Bevy as an implementation dependency does not make either boundary consumer an API dependency of the other. Shared preview/runtime presentation is reached through `aestra-bevy-render`, never through the game-runtime adapter.

---

# 4. Shader-Language Abstraction

## 4.1 Do not invent a low-level cross-language shader compiler

Rust already has a strong solution for low-level shader parsing, validation, and language translation: **Naga**.

Naga provides a shader intermediate representation and supports multiple shader input/output formats used by modern graphics APIs.

Conceptually:

```text
                 WGSL
                  │
                  ▼
                Naga IR
       ┌──────────┼───────────┬─────────┐
       ▼          ▼           ▼         ▼
     SPIR-V      HLSL        MSL       GLSL
```

Naga can reduce the need for separate handwritten implementations of the same low-level shader logic. It does not make a complete renderer or engine integration portable by itself.

In particular, Naga does not abstract:

- binding layouts and resource models;
- engine camera, depth, and render-target integration;
- render-pass and synchronization lifecycles;
- device limits and optional GPU features;
- engine shader reflection and registration conventions.

The portable target is therefore **Aestra semantic GPU lowering plus a documented backend ABI**. Naga handles shader validation and language translation below that boundary.

---

# 5. Role of WESL

WESL and Naga solve different problems and are complementary.

## WESL

WESL should be treated as Aestra's shader composition/authoring layer.

It provides useful capabilities around:

- reusable shader modules;
- imports;
- composition;
- conditional compilation;
- generated shader fragments;
- maintainable WGSL-based shader source.

Conceptually:

```text
Aestra GPU lowering
        │
        ▼
      WESL
        │
     wesl-rs
        │
        ▼
      WGSL
```

## Naga

Naga should be treated as the shader compiler/translation layer:

```text
WGSL
 │
 ▼
Naga
 │
 ├── SPIR-V
 ├── HLSL
 ├── MSL
 └── GLSL
```

For the current Bevy backend, it may not always be necessary to explicitly emit these alternate formats because WGPU already uses Naga internally.

Keeping this pipeline conceptually clean prevents the semantic effect model from becoming intrinsically tied to WGSL. WESL/WGSL remains the reference GPU source family, and another backend may still require ABI glue or a backend-specific lowering step even when Naga can translate the shader module.

---

# 6. Recommended Shader Pipeline

The target architecture should be:

```text
                   Aestra effect
                        │
                        ▼
                 Semantic model
                        │
                        ▼
                 Execution plan
                        │
            ┌───────────┴───────────┐
            │                       │
            ▼                       ▼
       CPU execution           GPU lowering
                                    │
                                    ▼
                              Generated WESL
                                    │
                                 wesl-rs
                                    │
                                    ▼
                                   WGSL
                                    │
                                    ▼
                                  Naga
                                    │
                                    ▼
                              WGPU / Bevy
```

The crucial design constraint is that the upper half does not understand:

- bind groups;
- WGPU pipeline layouts;
- Bevy render-world entities;
- shader-language syntax;
- Vulkan;
- Direct3D;
- Metal.

Those concepts belong below the GPU/backend boundary.

**Current state:** `aestra-gpu` lowers compiled effect instances into an engine-neutral packed ABI, owns the reference WESL, composes WGSL with `wesl-rs`, and parses and validates that output explicitly with Naga. Representative artifact tests retain reviewable WGSL snapshots without launching Bevy. The current shaders use runtime-sized buffers and are therefore shared across effects rather than specialized from each execution plan. `aestra-bevy-render` only registers the portable source and owns backend resource and pipeline integration.

---

# 7. Do Not Make Naga IR the Aestra VFX IR

Aestra should **not** represent VFX semantics directly using types such as:

```rust
naga::Module
naga::Expression
naga::Statement
naga::GlobalVariable
```

Naga IR is a compiler representation for GPU programs.

Aestra needs a domain representation for visual effects.

For example:

```rust
enum ParticleOperation {
    Gravity(GravityOp),
    Drag(DragOp),
    CurlNoise(CurlNoiseOp),
    IntegratePosition,
    SizeOverLife(CurveId),
    ColorOverLife(GradientId),
}
```

Then:

```text
ParticleOperation[]
        │
        ▼
  shader generator
        │
        ▼
      WESL
```

This gives Aestra several advantages:

- effects remain understandable without GPU knowledge;
- CPU execution remains possible;
- validation operates on meaningful VFX concepts;
- optimizations can happen before shader generation;
- shader implementation can change without changing the asset format;
- testing is significantly easier.

---

# 8. Why Generate WESL/WGSL Instead of Building Naga IR Directly

Aestra could theoretically construct `naga::Module` programmatically.

That should not be the initial design.

Generated WESL/WGSL has several practical advantages:

- human-readable shader dumps;
- easier debugging;
- easier profiling;
- easier comparison with handwritten shaders;
- simpler generated-code testing;
- easier shader authoring for custom modules within Aestra's supported WESL/WGSL subset;
- less coupling to Naga's internal IR API.

A useful debugging workflow becomes:

```text
Effect
  ↓
Compiled GPU plan
  ↓
Generated .wesl
  ↓
Generated .wgsl
  ↓
GPU pipeline
```

When an effect fails, developers can inspect the generated source directly.

Direct Naga-IR generation may eventually be useful for optimization, but should remain an implementation option rather than a foundational architectural dependency.

---

# 9. GPU API Abstraction Is a Separate Problem

Naga largely solves **shader-language portability**.

It does not solve engine integration.

A renderer still needs to perform operations such as:

```text
create buffers
upload parameters
create textures
resolve texture resources
create pipelines
bind resources
dispatch compute work
synchronize compute/render stages
issue indirect draws
access scene depth
access camera matrices
handle render targets
```

These responsibilities should live in the engine/backend integration.

For the current implementation:

```text
Compiled Aestra plan
         │
         ▼
    aestra-gpu
 packed GPU artifact
         │
         ▼
 aestra-bevy-render
         │
         ▼
    Bevy + WGPU
```

In a hypothetical future:

```text
Aestra GPU/render plan
         │
    ┌────┼─────┐
    ▼    ▼     ▼
 Bevy  Godot Unity
```

The shader logic can be shared.

The resource/pipeline integration cannot necessarily be shared.

---

# 10. Do Not Force WGPU onto Other Engines

WGPU is an excellent choice for Aestra's Bevy implementation.

However, future engine ports should not necessarily create a second independent WGPU device inside another renderer.

Avoid making the architecture:

```text
Unity
  │
  ▼
Aestra
  │
  ▼
WGPU
```

The preferred model is:

```text
               Aestra runtime
                     │
                     ▼
              Render/GPU contract
                     │
      ┌──────────────┼──────────────┐
      ▼              ▼              ▼
 Bevy adapter   Godot adapter   Unity adapter
      │              │              │
   WGPU/Bevy    RenderingDevice  Unity renderer
```

Each engine should ideally use its native graphics resources.

---

# 11. Engine-Neutral Render Plan

Aestra should progressively establish an engine-neutral render description.

For example:

```rust
struct ParticleRendererPlan {
    topology: ParticleTopology,
    blend_mode: BlendMode,
    depth_mode: DepthMode,
    sort_mode: SortMode,
    material: MaterialPlan,
    attributes: ParticleAttributeLayout,
}
```

A renderer plan may describe concepts such as:

```text
Sprite particles
Mesh particles
Ribbon particles
Blend mode
Depth testing
Depth writing
Sorting
Texture bindings
Material parameters
Indirect drawing requirement
Scene-depth requirement
```

It should not describe:

```text
Bevy RenderPipelineDescriptor
wgpu::BindGroupLayout
wgpu::Buffer
Bevy Entity
RenderAssets<T>
```

The Bevy adapter translates from the neutral plan into those concrete types.

---

# 12. Engine-Neutral GPU Resource Descriptions

Where useful, expose semantic GPU resource requirements instead of engine objects.

Example:

```rust
enum GpuResourceRequirement {
    ParticleStorage {
        stride: u32,
        capacity: u32,
    },

    CurveTable {
        samples: u32,
    },

    SceneDepth,

    CameraUniform,

    Texture2D {
        id: AssetId,
    },
}
```

Then:

```text
Aestra requirement
       │
       ▼
Bevy adapter
       │
       ▼
wgpu / Bevy resource
```

This also makes capability validation possible before runtime.

---

# 13. Capability System

Aestra should not assume that every backend or GPU device supports every rendering feature.

Keep two related contracts separate:

1. `EffectRequirements` is portable, compiler-derived data describing what an effect needs.
2. `BackendCapabilities` is adapter/device-discovered data describing what a concrete backend can provide.

For example:

```rust
struct BackendCapabilities {
    compute_particles: bool,
    indirect_draw: bool,
    sprite_particles: bool,
    mesh_particles: bool,
    ribbons: bool,
    scene_depth: bool,
    soft_particles: bool,
    storage_textures: bool,
}
```

Effects should be able to derive their required capabilities:

```rust
EffectRequirements {
    required: [
        ComputeParticles,
        SpriteParticles,
        SceneDepth,
    ],
}
```

Compatibility is evaluated as:

```text
EffectRequirements
        │
        ▼
compatibility check
        ▲
        │
BackendCapabilities
```

The compiler may validate against an explicitly supplied target profile. It must also retain requirements in compiled data so the runtime adapter can validate the actual device and produce a `CompatibilityReport`.

For now the only concrete capability provider is the shared Bevy render integration:

```text
Bevy/WGPU device → BackendCapabilities
```

**Current state:** the compiler derives particle-capacity and renderer requirements into every `CompiledEffect`. `aestra-bevy-render` converts compute, workgroup, storage-buffer, readback, indirect-draw, vertex-storage, and application-budget discovery into the portable `BackendCapabilities` contract. Backend selection retains a structured `CompatibilityReport` with stable issue codes when it falls back.

This still has immediate value because it:

- documents renderer assumptions;
- prevents unsupported combinations;
- allows graceful diagnostics;
- prepares the compiler for future backends;
- allows future hardware/platform capability differences even within Bevy.

---

# 14. Coordinate System Contract

Aestra must explicitly define its own coordinate conventions.

Do not let the asset format implicitly inherit Bevy's conventions.

The contract should explicitly specify:

- handedness;
- world up direction;
- forward direction;
- angle units;
- distance units;
- UV origin;
- texture coordinate conventions;
- matrix conventions where externally visible;
- color space;
- alpha convention;
- quaternion convention.

Existing serialized semantics must be the starting point for this contract. In particular, the current model already uses:

```text
Emitter rotation: normalized quaternion in [x, y, z, w] order
Particle spread: degrees
Time and durations: seconds
UV rectangles: normalized [0, 1] coordinates
```

Handedness, axis directions, UV origin, distance units, color space, alpha convention, and externally visible matrix conventions still need to be documented from current behavior and then frozen as part of a versioned file/runtime specification.

Changing an existing convention is a data-format migration, not a documentation-only decision. Such a change requires a format-version rule, source migration, and compatibility tests.

The engine adapter is responsible for converting where necessary:

```text
Aestra space
     │
     ▼
 Bevy space
```

Future adapters could implement:

```text
Aestra → Unity
Aestra → Godot
```

without modifying effects.

---

# 15. Asset and Runtime Data Must Avoid Engine Handles

Persistent or compiled Aestra data must not contain engine-specific identifiers such as:

```text
bevy::Handle<Image>
bevy::Handle<Mesh>
Entity
AssetId<Mesh> from a specific engine
RenderEntity
```

Instead use Aestra-owned logical identifiers:

```rust
TextureId
MeshId
MaterialId
CurveId
EffectId
```

The adapter resolves them:

```text
TextureId
   │
   ▼
Bevy Handle<Image>
```

This ensures compiled effects remain portable.

---

# 16. Runtime Events

Runtime events should also remain engine-independent.

For example:

```rust
enum RuntimeEvent {
    EffectStarted,
    EffectFinished,
    EmitterStarted(EmitterId),
    EmitterFinished(EmitterId),
    UserEvent(EventId),
}
```

The Bevy adapter can map them into:

```text
Bevy Events / Messages
```

A future Godot adapter could map them to signals.

A future Unity adapter could map them to C# callbacks/events.

---

# 17. Stable Engine-Facing Runtime Contract

The internal Rust API can evolve freely for now, but the architecture should naturally converge toward an engine-facing contract.

Conceptually:

```rust
load_effect(...)
create_instance(...)
destroy_instance(...)

play(...)
pause(...)
stop(...)
seek(...)

set_parameter(...)
get_parameter(...)

tick(...)

poll_event(...)
```

This does **not** require creating a C ABI today.

For Bevy, normal Rust APIs should be used.

However, keeping the conceptual surface small and explicit will make a future FFI layer straightforward.

A future crate could provide:

```text
aestra-ffi
```

without redesigning `aestra-runtime`.

---

# 18. Compiled Effect Artifact

Long term, Aestra should consider compiling authoring data into a runtime artifact rather than requiring every engine to interpret the authoring representation.

Conceptually:

```text
project / .aestra source
          │
          ▼
     Aestra compiler
          │
          ▼
        .aestrac
          │
          ▼
      game runtime
```

A compiled artifact could contain:

```text
Header
    format version
    compiler version
    feature flags

Effect metadata

Runtime execution plans

Emitter plans

Parameter layouts

Curve/gradient tables

Renderer plans

Resource references

Required renderer capabilities

Shader-generation metadata
```

The serialized artifact must be a stable, explicitly versioned data-transfer representation. It must not be a dump of Rust object layout or a direct serialization of in-memory ownership details such as `Arc` graphs.

At minimum, the artifact design must specify:

- schema and semantic versions;
- byte order and scalar encodings;
- required feature/capability tables;
- a logical resource manifest;
- deterministic canonicalization rules;
- validation and resource-consumption limits for untrusted files;
- compatibility behavior for unknown optional and required features.

The compiled artifact should remain free of:

- Bevy handles;
- WGPU objects;
- platform-specific GPU binaries unless stored as optional caches.

## Important

This is a long-term format direction.

Do not block current development on designing a permanent binary file format prematurely.

The current `CompiledEffect` and `CompiledEffectProject` are the in-memory compiled contract. Start by keeping those representations portable and stable enough to inform a separate artifact DTO.

Serialization can come later.

---

# 19. Optional Shader Caching

Eventually Aestra may cache shader outputs.

For example:

```text
.aestrac
    semantic/runtime plans

optional shader cache
    WGSL
    SPIR-V
    HLSL
    MSL
```

But the canonical effect representation should not depend on a single backend shader binary.

Caches should be:

- optional;
- reproducible;
- versioned;
- discardable.

A cache key must cover every input that can affect compatibility, including the Aestra GPU ABI version, shader-generator version, WESL/Naga versions, target backend, relevant device/driver identity, enabled features, and compiled effect content hash.

---

# 20. CPU and GPU Semantic Equivalence

Aestra should maintain a contract where CPU and GPU execution implement the same logical semantics wherever possible.

Example:

```text
Gravity
Drag
Lifetime
Velocity
Position integration
Curve sampling
Random generation
```

should behave predictably regardless of backend.

Perfect floating-point equality is not always realistic, but semantic equivalence matters. Each operation should declare whether conformance is bit-exact, tolerance-based, or statistical. Random streams should be tested separately for deterministic seed mapping and for distribution quality; visual similarity alone is not a conformance contract.

This has several benefits:

- deterministic tests;
- headless validation;
- effect debugging;
- fallback execution;
- future engine ports;
- easier compiler verification.

---

# 21. Randomness Contract

Randomness must be owned by Aestra rather than delegated implicitly to shaders or Bevy.

A deterministic random contract should specify inputs such as:

```text
effect seed
instance seed
emitter id
particle id
spawn sequence
simulation step
random stream id
```

A generated value should be reproducible according to a documented determinism tier. The contract must say whether reproducibility is guaranteed across runs, CPU/GPU implementations, platforms, and compiler versions. When exact cross-backend sequences are not guaranteed, stable seed derivation and statistically equivalent sampling still need explicit tests.

Avoid depending on backend-specific random functions.

---

# 22. Material Architecture

Aestra material definitions should remain semantic.

For example:

```rust
Material {
    blend_mode,
    shading_model,
    textures,
    parameters,
    feature_flags,
}
```

Do not serialize:

```text
Bevy MaterialPipeline
BindGroupLayoutEntry
wgpu::TextureFormat
```

unless those are explicitly isolated backend-specific caches.

The Bevy renderer translates an Aestra material into the appropriate Bevy/WGPU pipeline.

---

# 23. Custom Shader Modules

If Aestra eventually supports user-authored shader modules, prefer an Aestra/WESL-facing API rather than exposing Bevy shader internals.

For example:

```text
@aestra_module
fn modify_velocity(...)
```

The custom shader contract should define:

- allowed inputs;
- allowed outputs;
- accessible particle attributes;
- accessible parameters;
- lifecycle stage;
- deterministic restrictions;
- supported GPU features.

This keeps custom modules portable across backends that implement Aestra's GPU ABI and support the required Naga-translatable subset. A custom WESL module is not automatically engine-neutral: bindings, engine-provided resources, optional GPU features, and backend integration remain constrained by the declared contract.

Avoid exposing:

```text
Bevy-specific shader imports
Bevy bind-group numbering
Bevy renderer globals
```

unless isolated behind explicitly non-portable extensions.

---

# 24. Bevy-Specific Extensions Are Allowed

Portability must not cripple the Bevy implementation.

Aestra can support optional Bevy-specific functionality.

The important requirement is that such functionality be explicit.

For example:

```rust
enum ExtensionRequirement {
    Bevy(BevyExtension),
}
```

or through capability namespaces.

An effect using an engine-specific extension should simply become:

```text
Portable: no

Supported targets:
    Bevy ✓
```

rather than forcing the entire Aestra model to depend on Bevy.

---

# 25. Suggested High-Level Architecture

```text
                 PORTABLE AESTRA LAYERS

┌──────────────────────────────────────────────────────┐
│ Authoring model / project                            │
└──────────────────────────┬───────────────────────────┘
                           ▼
┌──────────────────────────────────────────────────────┐
│ Compiler                                             │
│ validation / normalization / optimization            │
│ execution and requirement derivation                 │
└──────────────────────────┬───────────────────────────┘
                           ▼
┌──────────────────────┐  ┌────────────────────────────┐
│ Runtime contracts    │  │ aestra-gpu contracts       │
│ execution / events   │  │ ABI / artifact lowering    │
└──────────▲───────────┘  └─────────────▲──────────────┘
           │                            │
           └──────────────┬─────────────┘
                          │ dependencies
             ┌────────────┴────────────┐
             │  aestra-bevy-render     │
             │ shared Bevy presentation│
             └────────────┬────────────┘
                          │
             ┌────────────┴────────────┐
             │                         │
┌────────────┴────────────┐  ┌─────────┴───────────────┐
│     Aestra Editor       │  │      aestra-bevy       │
│ authoring application   │  │ game-runtime adapter   │
│ Bevy UI/preview host    │  │ playback/lifecycle     │
└─────────────────────────┘  └─────────────────────────┘
          no dependency in either direction
```

The two lower boxes are sibling boundary consumers. They share portable Aestra contracts and the lower-level Bevy presentation integration, never code through each other.

---

# 26. Delivery Gates

The roadmap deliberately keeps **Bevy as the only implementation target**. Cross-engine work consists only of establishing clean boundaries and tests.

The work is organized into four outcome-oriented delivery gates. The detailed work-item catalog below supports these gates; it is not a requirement to execute every item as a separate, sequential milestone.

## Gate A — Boundary Specification

Correct and enforce the dependency rule, audit the existing semantic boundary, and publish the versioned coordinate/numeric contract.

**Exit criteria:** the repository and specification agree on dependency direction and existing serialized semantics, with CI guarding forbidden engine dependencies.

## Gate B — Portable Compiled Contracts

Harden the existing execution, renderer, resource, event, and parameter contracts. Add compiler-derived `EffectRequirements` and compatibility diagnostics without introducing engine objects.

**Exit criteria:** the compiler produces a complete portable contract that the CPU runtime and a renderer adapter can consume.

## Gate C — GPU Lowering Extraction

Move semantic GPU packing and shader lowering behind an Aestra-owned boundary. Generate inspectable WESL/WGSL, validate it explicitly through Naga, and retain useful shader diagnostics and snapshots.

**Exit criteria:** representative compiled effects can produce validated shader modules in tests without launching Bevy.

## Gate D — Adapter, Conformance, and Artifact Prototype

Make `aestra-bevy` consume the portable GPU/render contract, add CPU/GPU semantic conformance tests, and only then prototype a versioned compiled artifact and portability CI.

**Exit criteria:** Bevy remains the production adapter, CPU/GPU behavior is tested at the semantic boundary, and a prototype compiled artifact can round-trip without engine state.

## Detailed work-item catalog

---

### Work item 0 — Architecture Rules and Dependency Audit *(existing; harden)*

### Goal

Document and enforce which layers may depend on Bevy.

### Tasks

- Audit every current Aestra crate for:
  - `bevy` dependencies;
  - `wgpu` dependencies;
  - Bevy math types;
  - Bevy asset handles;
  - Bevy ECS entities/components;
  - Bevy renderer structures.
- Categorize each dependency as:
  - legitimately adapter-specific;
  - accidental coupling;
  - temporary technical debt.
- Define the dependency rule in project documentation.
- Add comments/architecture documentation describing the engine boundary.
- Ensure new core types use engine-neutral primitives or Aestra-owned types.

### Exit criteria

- Bevy-specific code is clearly identifiable.
- No new Bevy dependency can accidentally enter core/compiler/runtime layers unnoticed.
- Existing exceptions are documented.

---

### Work item 1 — Audit Aestra Semantic Boundaries *(existing; harden)*

### Goal

Ensure the effect model describes VFX concepts rather than implementation details.

### Tasks

Review representations for:

- emitters;
- spawn logic;
- particle attributes;
- update modules;
- events;
- curves;
- gradients;
- renderer settings;
- materials;
- parameters;
- EffectClips;
- automation.

Remove or isolate concepts that directly encode:

- WGSL;
- WGPU;
- Bevy ECS;
- Bevy render pipelines.

Create explicit IDs where necessary:

```text
TextureId
MeshId
MaterialId
EffectId
EmitterId
CurveId
GradientId
```

### Exit criteria

An `EffectAsset` or equivalent semantic representation can be created, validated, serialized, and inspected without initializing Bevy or WGPU.

---

### Work item 2 — Define the Aestra Coordinate and Numeric Contract *(target)*

### Goal

Prevent subtle backend assumptions from leaking into assets.

### Tasks

Define:

- coordinate handedness;
- up axis;
- forward axis;
- distance units;
- angle units;
- quaternion convention;
- UV orientation;
- color representation;
- alpha convention;
- time units;
- simulation timestep semantics.

Document conversions at the Bevy boundary.

### Exit criteria

The Aestra specification contains an explicit coordinate/numeric contract independent of Bevy.

---

### Work item 3 — Formalize the Compiled Execution Plan *(existing; harden)*

### Goal

Separate authoring representation from runtime execution.

### Tasks

Audit and cleanly define existing compiled forms such as:

```text
CompiledEffect
CompiledEmitter
SpawnPlan
UpdatePlan
EventPlan
ParameterLayout
```

Ensure these structures:

- contain no Bevy types;
- contain no WGPU handles;
- contain no engine entities;
- can execute using the CPU runtime;
- can provide input to GPU lowering.

Add compiler validation between authoring and compiled representations.

### Exit criteria

The runtime can operate on compiled Aestra data without referencing the editor or Bevy.

---

### Work item 4 — Separate Effect Requirements from Backend Capabilities *(existing)*

### Goal

Make rendering feature requirements explicit.

### Tasks

Define portable effect requirements independently from backend/device capability discovery.

Current Bevy capabilities include:

```text
sprite particles
GPU compute
indirect draw
flipbook particles
compute workgroup limits
storage-buffer bindings
vertex-stage storage
GPU readback
particle capacity
```

Have the compiler derive effect requirements.

Validate:

```text
EffectRequirements ⊆ BackendCapabilities
```

There is currently one capability provider:

```text
Bevy/WGPU device capability provider
```

### Exit criteria

Unsupported features fail with explicit Aestra diagnostics instead of failing deep inside the renderer.

---

### Work item 5 — Expand the Engine-Neutral Render Plan *(partial)*

### Goal

Prevent compiled renderer information from becoming Bevy pipeline configuration.

### Tasks

Create semantic renderer descriptions for:

- sprite renderer;
- mesh renderer;
- ribbons;
- blend mode;
- depth behavior;
- sorting;
- required particle attributes;
- material data;
- texture resources;
- indirect draw requirements.

Implement translation:

```text
Aestra RenderPlan
        │
        ▼
Bevy Render Pipeline
```

inside `aestra-bevy-render`.

### Exit criteria

The compiler can construct a complete render plan without depending on Bevy.

---

### Work item 6 — Isolate GPU Shader Generation *(existing)*

### Goal

Make generated GPU logic an Aestra subsystem rather than a Bevy subsystem.

### Tasks

Move or define GPU lowering behind something conceptually equivalent to:

```text
aestra-gpu
```

The `aestra-gpu` crate owns packed curves, gradients, emitters, renderers, particles, globals, indirect-draw layout, bounds derivation, seed folding, artifact lowering, reference WESL source, and validated WGSL output. It has an architecture test forbidding Bevy and WGPU dependencies.

Pipeline:

```text
Compiled simulation plan
        │
        ▼
GPU lowering
        │
        ▼
Generated WESL
```

The current runtime-sized shader package is generated for an artifact without effect-specific source specialization. Future generated code must continue to use Aestra-owned bindings and semantic names rather than engine-specific shader imports.

### Exit criteria

Artifact lowering and shader generation run together in tests without Bevy.

---

### Work item 7 — Standardize WESL → WGSL Compilation *(partial)*

### Goal

Establish WESL as the compositional shader layer.

### Tasks

- Make WESL module structure explicit.
- Separate:
  - common particle functions;
  - generated effect functions;
  - renderer functions;
  - backend glue.
- Use `wesl-rs` for preprocessing/composition.
- Add shader source snapshots to tests.
- Preserve generated WESL/WGSL on compilation errors for diagnostics.

`aestra-gpu` now owns the WESL sources, compiles them with `wesl-rs`, returns structured composition errors, and retains generated WGSL snapshots. Separating shared ABI helpers and backend glue into finer WESL modules remains future hardening.

### Exit criteria

Given an Aestra compiled effect, tests can generate and validate WGSL without running the renderer.

---

### Work item 8 — Make Naga Validation an Explicit Compiler Stage *(existing)*

### Goal

Use Naga as the low-level shader validation/translation boundary.

### Tasks

After WGSL generation:

```text
WGSL
 │
 ▼
Naga parse
 │
 ▼
Naga validation
```

Report Naga errors through Aestra diagnostics.

`aestra-gpu` explicitly parses every composed WGSL module with Naga, validates it, verifies required entry points, and returns structured errors before an engine adapter consumes it.

Optionally test selected backend translations:

```text
WGSL → Naga → SPIR-V
WGSL → Naga → HLSL
```

These are **tests only**.

They do not imply supporting Unity/Godot.

### Exit criteria

Aestra CI verifies that representative generated shaders are valid Naga modules.

---

### Work item 9 — Separate Bevy Runtime Playback from Shared Presentation *(partial)*

### Goal

Keep game-runtime playback in `aestra-bevy`, keep editor preview independent from that runtime adapter, and make both hosts consume the same lower-level Bevy presentation integration.

### Responsibilities that belong in `aestra-bevy`

```text
Bevy ECS components
playback clocks and lifecycle
runtime parameter overrides
choreography event dispatch
profiling integration
syncing runtime instances to presentation
```

### Responsibilities that belong in `aestra-bevy-render`

```text
Bevy presentation components and systems
AssetServer integration
Bevy texture/mesh handle resolution
render-world extraction
WGPU resource allocation
pipeline creation
bind groups
compute dispatch
draw submission
camera/depth integration
```

### Responsibilities that should not live in `aestra-bevy`

```text
effect semantics
module semantics
curve evaluation rules
event semantics
particle operation definitions
effect validation
compiler optimization
portable renderer description
portable GPU shader generation
GPU ABI and artifact packing
```

### Exit criteria

Deleting `aestra-bevy` would remove game playback integration while leaving the editor preview, shared Bevy presentation, and portable compiler/runtime libraries functional. GPU ABI ownership and artifact packing already live in `aestra-gpu`; moving WESL composition and generation behind that boundary remains a later gate.

---

### Work item 10 — CPU/GPU Semantic Conformance Tests *(partial)*

### Goal

Ensure GPU lowering preserves Aestra semantics.

### Tasks

Create small deterministic test effects for:

- spawn position;
- initial velocity;
- gravity;
- drag;
- lifetime;
- size-over-life;
- color-over-life;
- deterministic random streams;
- event timing.

Compare CPU results against GPU results with appropriate tolerances.

### Exit criteria

Core simulation features have automated CPU/GPU conformance tests.

The native-compute conformance slice compiles deterministic authored fixtures through the normal
compiler and `aestra-gpu`, executes the generated WGSL through WGPU, reads the packed particles
back, and compares them with the CPU reference at fixed timestamps. It covers once, restart-loop,
and continuous-loop playback, including emitter-region offsets, previous-cycle survival, stable
cross-cycle particle identities, and cycle-derived random streams. Ordinary test runners skip only
when no compatible compute adapter exists; the self-hosted GPU workflow requires the adapter and
fails on semantic divergence. Source-mode fixtures additionally exercise emitter-time spawn curves,
particle-life drag, turbulence, and vector-gravity curves, plus scalar/vector random ranges. Random
spawn rates are derived independently for every continuous cycle on both backends. Further fixtures
remain necessary for parameters and events.

---

### Work item 11 — Audit Renderer Resource Abstraction *(existing; harden)*

### Goal

Ensure compiled data references logical resources rather than Bevy resources.

### Tasks

Add or formalize Aestra resource IDs.

Create Bevy resolution tables:

```text
Aestra TextureId → Handle<Image>
Aestra MeshId    → Handle<Mesh>
```

Do the same for material-related data.

Ensure missing-resource diagnostics originate at the adapter boundary.

### Exit criteria

No persistent compiled Aestra effect contains a Bevy resource handle.

---

### Work item 12 — Formalize the Runtime Event and Parameter Contract *(partial)*

### Goal

Make the public runtime API naturally portable.

### Tasks

Formalize APIs around:

```text
create instance
play
pause
stop
seek
tick
set parameter
read parameter
emit event
poll event
destroy instance
```

Use Aestra-owned event/parameter types.

Map those APIs into Bevy ergonomically.

### Exit criteria

The runtime API could theoretically be wrapped by FFI without redesigning its fundamental object model.

No FFI implementation is required.

---

### Work item 13 — Compiled Artifact Prototype *(deferred until portable contracts stabilize)*

### Goal

Prove that runtime data can exist independently from authoring data and Bevy.

### Tasks

Create a prototype artifact DTO informed by, but distinct from, the in-memory compiled representation.

It does not have to be the final `.aestrac` format.

Verify:

```text
source effect
    ↓
compiler
    ↓
compiled blob
    ↓
reload
    ↓
aestra-runtime
    ↓
aestra-bevy
```

### Exit criteria

A compiled effect can be saved and loaded without carrying Bevy-specific state.

---

### Work item 14 — Portability CI Without Supporting Another Engine *(target)*

### Goal

Continuously verify the architectural promise cheaply.

### Tasks

Add CI checks that:

- core crates compile without Bevy;
- GPU lowering runs without Bevy;
- generated WGSL validates through Naga;
- selected representative shaders can be translated by Naga to at least:
  - SPIR-V;
  - HLSL.
- engine-neutral structures do not expose Bevy types.

Optional compile-time architecture tests can guard forbidden dependencies.

### Exit criteria

A change that introduces forbidden Bevy coupling, invalid generated shaders, or regressions in the selected Naga translation targets is detected in CI.

---

# 27. What Not to Build Yet

To keep development focused, explicitly defer:

- `aestra-godot`;
- `aestra-unity`;
- `aestra-gamemaker`;
- Unreal integration;
- C ABI;
- C# bindings;
- GDExtension bindings;
- GameMaker extension bindings;
- multiple renderer implementations;
- permanent cross-engine packaging format;
- shader caches for every graphics API;
- engine-specific compatibility UI.

The architecture should make those possible.

It should not pay their maintenance cost today.

---

# 28. Near-Term Priority Order

For the current Aestra project, prioritize:

```text
1. Build excellent Bevy VFX tooling.
2. Stabilize Aestra semantic/runtime representations.
3. Keep Bevy behind adapter boundaries.
4. Keep shader generation outside Bevy-specific code.
5. Use WESL for composition.
6. Use Naga for shader validation/translation.
7. Add capability and portability tests.
8. Continue improving Bevy GPU performance/features.
9. Only implement another engine when there is a real product need.
```

This avoids premature abstraction while protecting against the expensive type of coupling that would require a rewrite later.

---

# 29. Architectural Decision Summary

## Existing decisions

- Bevy is the only supported engine.
- WGPU is the Bevy renderer backend.
- Aestra owns the VFX semantic IR.
- Core/compiler/runtime structures remain engine-neutral.
- Bevy integration remains in an adapter.
- Engine resource handles never enter portable assets or compiled contracts.

## Target decisions

- WESL is the shader composition layer.
- WGSL is the generated/reference shader language.
- Naga is an explicit shader parsing, validation, and translation stage below Aestra's semantic GPU lowering.
- The GPU boundary includes an Aestra-owned backend ABI; shader translation alone is not considered a complete engine abstraction.
- Coordinate conventions belong to Aestra.
- Effect requirements and backend/device capabilities are explicit, separate contracts.
- CPU/GPU semantics are tested against a common contract.

## Defer

- other engine plugins;
- stable FFI;
- final binary package format;
- multi-engine renderer APIs beyond what Bevy presently needs;
- direct generation of Naga IR;
- backend-specific shader caches.

---

# 30. Final Target

The architecture should make the following statement true:

> **Aestra is currently a Bevy VFX system, but Bevy is an integration target rather than part of the Aestra effect specification.**

Today:

```text
Aestra
   │
   ▼
 Bevy
```

Later, without changing the fundamental effect model:

```text
             Aestra
                │
      ┌─────────┼─────────┐
      ▼         ▼         ▼
    Bevy      Godot     Unity
```

There is no need to implement the lower diagram now.

The goal is simply to avoid making it impossible.
