# Aestra — Extensible Stages, Plugins, Execution Model, and Properties Redesign

**Status:** Architecture proposal  
**Target:** Aestra post-format-v3 evolution  
**Scope:** semantic effect model, stages/modules/renderers, compiler/runtime execution, plugin extensibility, missing-plugin resilience, and the Properties editor redesign.

---

## 1. Executive decision

Aestra should move toward a **generic stage infrastructure with fixed core lifecycle semantics and registry-driven extension types**.

The core rule is:

> **Aestra defines how execution is described, scheduled, validated, serialized, compiled, and presented. Plugins define what new stages, modules, resources, and renderers do.**

The architecture should therefore avoid closed enums for feature-specific concepts such as fluids, cloth, grids, pressure solvers, destruction, hair, etc.

At the same time, Aestra should **not** make the emitter lifecycle completely arbitrary. The standard lifecycle remains structural and owned by Aestra:

```text
Emitter Spawn
Emitter Update
Particle Spawn
Particle Update
        │
        ├── extension / simulation stages
        │
        └── renderers (fan-out consumers)
```

The built-in lifecycle stages and community-created simulation stages should share the same underlying `StageInstance` infrastructure, but built-in lifecycle roles have stronger invariants.

The editor should project this model as:

- a **compact ordered stack** for execution structure;
- a **focused Inspector** for the selected stage/module/renderer;
- optionally, a separate **Effect Graph** for relationships such as events and cross-emitter dependencies.

A graph should not replace a linear module stack when the semantics are actually ordered execution.

---

# 2. Why this redesign is needed

Aestra's current architecture already contains several good extension-oriented foundations:

- `ModuleTypeId(pub String)` is already a stable string identity.
- `RendererTypeId(pub String)` is already a stable string identity.
- `StageKind` already has a `Simulation(String)` escape hatch.
- `ModuleParameters::Custom(...)` and `RendererProperties::Custom(...)` provide an initial dynamic representation.
- `ModuleRegistry` already exists in `aestra-compiler`.
- the compiler/runtime/artifact boundaries are already separated cleanly.
- authored data and runtime plans are engine independent.
- the editor already has metadata-driven module controls.

However, the current model is still fundamentally built around a fixed set of built-ins.

At the time of writing, the important constraints are:

```text
aestra-core
    Emitter
        simulation_domain: SimulationDomain
        modules: Vec<ModuleInstance>
        renderers: Vec<RendererInstance>

    ModuleInstance
        module_type: ModuleTypeId
        stage: StageKind

    StageKind
        EffectSpawn
        EffectUpdate
        EmitterSpawn
        EmitterUpdate
        ParticleSpawn
        ParticleUpdate
        Simulation(String)
```

The compiler then lowers enabled modules into a fixed runtime plan:

```text
ExecutionPlan
    emitter_update: Vec<Instruction>
    particle_spawn: Vec<Instruction>
    particle_update: Vec<Instruction>
```

and `RuntimeStage` is also currently closed:

```text
EmitterUpdate
ParticleSpawn
ParticleUpdate
```

Renderer lowering similarly matches the built-in `RendererProperties` variants.

This works well for the current particle feature set, but it will not scale cleanly to a community plugin such as:

```text
Aestra Fluids
    Fluid Solver
    Grid Deposit
    Pressure Solve
    Advection
    Vorticity
    Volume Renderer
```

without either modifying Aestra itself or growing increasingly large `Custom(...)` escape hatches.

The goal of this proposal is to make those extensions **first-class**, while retaining Aestra's deterministic and portable architecture.

---

# 3. Terminology

Aestra should use these terms consistently.

## Effect

The complete authored VFX choreography asset.

## Emitter

One simulation/presentation producer within an effect.

## Stage

A semantic execution unit with:

- stable identity;
- a registered stage type;
- scheduling/execution semantics;
- configuration;
- optional ordered module instances;
- declared capabilities/resources;
- compiler lowering behavior.

A stage is **not necessarily one GPU dispatch**.

## Lifecycle stage

A stage occupying one of Aestra's standard lifecycle roles:

```text
Emitter Spawn
Emitter Update
Particle Spawn
Particle Update
```

These roles are owned by Aestra.

## Simulation / extension stage

An additional authored stage registered by Aestra itself or a plugin.

Examples:

```text
Substepped Particle Update
Grid Deposit
Fluid Solver
Constraint Solve
Neighbor Search
Cloth Solve
```

## Module

An ordered semantic operation inside a stage that supports a module stack.

Example:

```text
Particle Update
    Gravity
    Vortex
    Drag
    Appearance
```

Multiple instances of the same module type are allowed unless the module declares itself singleton.

## Renderer

A consumer of simulation state that produces presentation output.

Renderers are deliberately **not normal simulation stages**.

One simulation may fan out to many renderer instances:

```text
Particle state
    ├── Sprite Renderer
    ├── Trail Renderer
    ├── Mesh Renderer
    └── custom plugin renderer
```

## Execution IR

The portable, engine-neutral execution representation produced by the compiler.

It describes what must execute without requiring `aestra-core` to understand domain-specific concepts such as fluid pressure.

---

# 4. Architectural principles

## 4.1 Generic infrastructure, specialized semantics

Aestra should reuse the same infrastructure for built-in and plugin-provided stages, modules, and renderers.

Do not build one privileged built-in path and a second-class plugin path.

```text
Built-in Gravity
        │
        └── ModuleRegistry

Community Vorticity
        │
        └── ModuleRegistry
```

The difference is provenance and capability, not the registration mechanism.

---

## 4.2 Keep the standard lifecycle structural

Avoid an unrestricted model like:

```rust
struct Emitter {
    stages: Vec<StageInstance>,
}
```

if that permits nonsensical documents such as:

```text
Particle Update
Emitter Spawn
Particle Update
Particle Spawn
Emitter Spawn
```

Aestra knows that the standard particle lifecycle has meaning.

Represent that in the model rather than removing it and rebuilding it with validation rules later.

Recommended authored topology:

```rust
pub struct Emitter {
    // existing emitter identity/timing/transform/etc.

    pub lifecycle: EmitterLifecycleStages,

    #[serde(default)]
    pub simulation_stages: Vec<StageInstance>,

    #[serde(default)]
    pub renderers: Vec<RendererInstance>,
}

pub struct EmitterLifecycleStages {
    pub emitter_spawn: StageInstance,
    pub emitter_update: StageInstance,
    pub particle_spawn: StageInstance,
    pub particle_update: StageInstance,
}
```

All four fields contain the same generic `StageInstance` type.

They are nevertheless fixed semantic slots.

### Effect-level lifecycle

The current `StageKind` also carries two **effect-level** roles — `EffectSpawn` and
`EffectUpdate` — which run once per effect rather than per emitter. These are not emitter
lifecycle slots and must not be silently folded into an emitter's stages or dropped during
migration.

Model them as an effect-owned lifecycle with the same `StageInstance` type:

```rust
pub struct Effect {
    // existing effect identity/timing/etc.

    pub lifecycle: EffectLifecycleStages,

    pub emitters: Vec<Emitter>,
}

pub struct EffectLifecycleStages {
    pub effect_spawn: StageInstance,
    pub effect_update: StageInstance,
}
```

So the complete set of fixed lifecycle slots across the two scopes is:

```text
Effect Spawn      (effect scope)
Effect Update     (effect scope)
    Emitter Spawn     (emitter scope)
    Emitter Update    (emitter scope)
    Particle Spawn    (emitter scope)
    Particle Update   (emitter scope)
```

The v3→v4 migration (§31) must place any `StageKind::EffectSpawn` / `EffectUpdate` modules
into the effect-level slots, not the emitter lifecycle.

---

## 4.3 Do not put fluid-specific concepts in core enums

Avoid:

```rust
enum StageKind {
    EmitterSpawn,
    EmitterUpdate,
    ParticleSpawn,
    ParticleUpdate,

    Fluid,
    Cloth,
    Hair,
    Destruction,
}
```

Instead, use stable type IDs:

```text
aestra.stage.emitter_spawn
aestra.stage.emitter_update
aestra.stage.particle_spawn
aestra.stage.particle_update

org.example.fluid.stage.solver
org.example.cloth.stage.constraints
```

The same rule applies to:

- modules;
- renderers;
- domains;
- resources;
- capabilities;
- optional future authoring projections.

---

## 4.4 Plugins target Aestra contracts, not Bevy internals

The portable extension API must not require direct Bevy/WGPU handles.

The intended flow is:

```text
Plugin semantic type
        │
        ▼
Aestra compiler lowering
        │
        ▼
Aestra Execution IR / renderer artifact
        │
        ├── CPU/reference backend
        ├── Aestra GPU backend
        ├── Bevy adapter
        └── future engine adapters
```

This keeps the architecture compatible with future integrations beyond Bevy.

Engine-specific escape hatches may exist later, but they must be explicitly classified as non-portable runtime extensions.

---

## 4.5 Plugin API and plugin loading are separate problems

Do not make the semantic architecture depend on the first loading mechanism.

Aestra can first support extensions that are statically linked Rust crates, while keeping the contracts suitable for future packaged plugins.

Long term:

```text
Extension API        stable semantic/compiler contracts
Plugin package       manifest + assets + code + schemas
Plugin host          discovery/loading/versioning/security
Runtime extension    optional game/backend integration
```

This allows Aestra to stabilize the important architecture before choosing a permanent dynamic ABI.

---

# 5. Stable namespaced identities

Introduce strongly typed string IDs for extensible concepts.

```rust
#[serde(transparent)]
pub struct ExtensionId(pub String);

#[serde(transparent)]
pub struct StageTypeId(pub String);

#[serde(transparent)]
pub struct ModuleTypeId(pub String);

#[serde(transparent)]
pub struct RendererTypeId(pub String);

#[serde(transparent)]
pub struct DomainTypeId(pub String);

#[serde(transparent)]
pub struct ResourceTypeId(pub String);

#[serde(transparent)]
pub struct CapabilityId(pub String);
```

Use reverse-domain or otherwise globally namespaced plugin IDs:

```text
org.thehiddenworkshop.aestra
org.someauthor.aestra-fluid
com.vendor.vfx-cloth
```

Type IDs remain globally stable:

```text
org.someauthor.aestra-fluid::stage/solver
org.someauthor.aestra-fluid::module/vorticity
org.someauthor.aestra-fluid::renderer/volume
```

Do not use:

- Rust type names;
- source file paths;
- display names;
- numeric IDs assigned at runtime.

Display names can change. Semantic IDs must not.

---

# 6. Stage model

## 6.1 `StageInstance`

The authored model should eventually move from "each module contains a stage enum" to "a stage contains its modules".

Recommended shape:

```rust
pub struct StageInstance {
    pub id: StageId,

    /// Registered semantic type.
    pub stage_type: StageTypeId,

    pub enabled: bool,

    /// Optional user-facing name override such as "Pressure Solve 2".
    #[serde(default)]
    pub name: Option<String>,

    /// Schema-versioned type-specific authored state.
    pub payload: ExtensionPayload,

    /// Empty for opaque stages.
    #[serde(default)]
    pub modules: Vec<ModuleInstance>,
}
```

This solves several problems:

1. stage identity becomes explicit;
2. custom stages can carry properties;
3. modules no longer need to repeat their `stage`;
4. custom simulation stages can be reordered as units;
5. source mapping can target stages;
6. profiling can report per-stage costs;
7. plugins can own stage-level behavior;
8. the Properties UI has a first-class object to select.

---

## 6.2 Lifecycle role is not the same as stage type

The registry descriptor may associate a stage type with a core lifecycle role:

```rust
pub enum LifecycleRole {
    EffectSpawn,
    EffectUpdate,
    EmitterSpawn,
    EmitterUpdate,
    ParticleSpawn,
    ParticleUpdate,
}
```

This is intentionally a closed core enum, and it mirrors the six lifecycle variants that
`StageKind` already has today — the two effect-level roles included, so nothing is lost in the
v3→v4 migration.

It represents Aestra's execution lifecycle, not community feature taxonomy.

Example descriptors:

```text
aestra.stage.emitter_update
    lifecycle_role = EmitterUpdate

aestra.stage.particle_spawn
    lifecycle_role = ParticleSpawn

org.someauthor.fluid::stage/solver
    lifecycle_role = None
```

Plugins must not silently redefine the meaning of a core lifecycle role.

---

## 6.3 Stage descriptor

Runtime/editor/compiler behavior belongs in the registry, not serialized into each effect.

Conceptual API:

```rust
pub struct StageTypeDescriptor {
    pub id: StageTypeId,
    pub provider: ExtensionId,

    pub display_name: LocalizedTextKey,
    pub description: LocalizedTextKey,
    pub category: String,

    pub lifecycle_role: Option<LifecycleRole>,

    pub property_schema: PropertySchema,

    pub module_policy: ModulePolicy,

    pub required_capabilities: CapabilitySet,
    pub provided_capabilities: CapabilitySet,

    pub backend_support: BackendSupport,

    pub lowering: StageLowerer,
}
```

The exact Rust representation may differ, especially once dynamic plugin loading exists.

The important point is that these are **registered descriptors**, not serialized function pointers.

---

# 7. Stage composition

Not every stage should be forced to expose a module stack.

Support at least these semantic composition policies:

```rust
pub enum ModulePolicy {
    None,

    Ordered {
        accepted: CapabilityExpression,
    },
}
```

Possible future extension:

```text
Graph-backed stage
```

should be added only when there is a real use case.

### Module-stack example

```text
PARTICLE UPDATE
    Gravity
    Vortex
    Drag
    Appearance
```

### Opaque-stage example

A fluid plugin may expose:

```text
FLUID SIMULATION
    Solver:          FLIP
    Resolution:      128³
    Substeps:        2
    Pressure Iter.:  12
```

while lowering internally into:

```text
Grid Deposit
Barrier
Pressure Solve ×12
Advection
Barrier
Grid → Particle Transfer
```

The user should not be forced to edit twenty implementation passes merely because the runtime requires them.

---

# 8. Module model

## 8.1 Multiple instances are the default

A stage is an ordered pipeline of module **instances**.

A module type may normally appear multiple times:

```text
PARTICLE UPDATE
    Gravity
    Vortex            "Large Orbit"
    Vortex            "Fine Turbulence"
    Drag
    Collision         "Ground"
    Collision         "Character Field"
```

Recommended descriptor field:

```rust
pub enum ModuleMultiplicity {
    Multiple,
    SingletonPerStage,
}
```

Default should be `Multiple`.

Only modules with a semantic reason to be unique should opt into singleton behavior.

---

## 8.2 Module instance

Move toward:

```rust
pub struct ModuleInstance {
    pub id: ModuleId,
    pub module_type: ModuleTypeId,
    pub enabled: bool,

    #[serde(default)]
    pub name: Option<String>,

    pub payload: ExtensionPayload,

    // property source/binding data remains supported
}
```

The containing stage provides execution placement.

The long-term goal is to remove:

```rust
ModuleInstance.stage: StageKind
```

because stage membership becomes structural.

---

## 8.3 Module descriptor

Conceptually:

```rust
pub struct ModuleDescriptor {
    pub id: ModuleTypeId,
    pub provider: ExtensionId,

    pub display_name: LocalizedTextKey,
    pub description: LocalizedTextKey,
    pub category: String,

    pub multiplicity: ModuleMultiplicity,

    pub property_schema: PropertySchema,

    pub required_capabilities: CapabilitySet,
    pub provided_capabilities: CapabilitySet,

    pub reads: Vec<ResourceAccessPattern>,
    pub writes: Vec<ResourceAccessPattern>,

    pub approximate_cost: u32,

    pub backend_support: BackendSupport,

    pub lowering: ModuleLowerer,
}
```

This generalizes the metadata already present in `aestra-compiler::ModuleMetadata`.

---

# 9. Capabilities instead of concrete stage checks

Compatibility should primarily be expressed using capabilities rather than:

```rust
if stage == ParticleUpdate { ... }
```

Example built-in stage:

```text
aestra.stage.particle_update

provides:
    aestra.domain.particles
    aestra.lifecycle.update
    aestra.particle.position.read
    aestra.particle.position.write
    aestra.particle.velocity.read
    aestra.particle.velocity.write
```

A Gravity module might require:

```text
aestra.domain.particles
aestra.particle.velocity.write
```

A community stage such as:

```text
org.example::substepped_particle_update
```

could provide the same capabilities.

Then standard Gravity can work inside it without either component knowing the other's concrete type ID.

This is important for a healthy plugin ecosystem:

> Plugins should compose through declared contracts, not direct plugin-to-plugin knowledge.

## 9.1 Capability IDs are a public contract, not ad-hoc strings

Because a community stage must *provide* the built-in particle-update capabilities to host a
built-in module (and vice-versa), those capability IDs are effectively a **public ABI** that
plugins hardcode. They need the same governance as type IDs (§5):

- the built-in capability vocabulary (`aestra.domain.particles`, `aestra.particle.velocity.write`,
  …) is **owned by Aestra core**, documented, and versioned;
- capability IDs are namespaced like every other extensible identity — plugins may define their own
  under their plugin namespace, but must not invent strings inside the `aestra.*` namespace;
- the registry validates capability references the same way it validates type IDs, so a drifted or
  misspelled capability string is a load-time diagnostic, not a silent no-match.

Treat the capability set as a stability surface: renaming or removing a built-in capability is a
breaking change subject to the same API-version policy as the authored format.

---

# 10. Execution domains

Do not grow a core enum such as:

```rust
enum ExecutionDomain {
    Particle,
    Grid2D,
    Grid3D,
    Cloth,
    Hair,
    ...
}
```

Use registered IDs:

```text
aestra.domain.emitter
aestra.domain.particles

org.example.fluid::domain/grid3d
org.example.cloth::domain/constraint_graph
org.example.hair::domain/strand_set
```

However, avoid making the scheduler depend heavily on understanding every domain.

The scheduler should mainly reason about:

- declared resources;
- read/write hazards;
- dependencies;
- dispatch requirements;
- barriers;
- iteration;
- lifecycle placement.

The domain is semantic metadata useful for validation, tooling, dispatch construction, and UI.

---

# 11. Resources and dependency declarations

A powerful plugin model needs explicit resources.

> **Shared with the hybrid simulation roadmap** (`SimulationResource`, ping-pong/lifetime) — one
> resource model, governed by the bounded-allocation rule in §11.1. See §44.4.

Conceptually:

```rust
pub struct ResourceDescriptor {
    pub id: ResourceId,
    pub resource_type: ResourceTypeId,
    pub lifetime: ResourceLifetime,
    pub storage: StorageClass,
}
```

and:

```rust
pub struct ResourceAccess {
    pub resource: ResourceId,
    pub mode: AccessMode,
}
```

Example fluid stage:

```text
Fluid Grid Deposit

reads:
    particles.position
    particles.velocity

writes:
    fluid.velocity_grid
    fluid.density_grid
```

Pressure solve:

```text
reads:
    fluid.velocity_grid
    fluid.pressure_grid

writes:
    fluid.pressure_grid
```

Aestra can then:

- validate hazards;
- derive barriers;
- build execution dependencies;
- inspect resource lifetime;
- profile memory;
- expose meaningful diagnostics.

Aestra does not need to know the physical meaning of "pressure".

## 11.1 Resource declarations must respect the bounded-allocation policy

Aestra's runtime guarantees bounded allocation: pool sizes are known ahead of time and the
simulation never allocates unpredictably at runtime. Plugin-declared resources (a `128³` velocity
grid, etc.) must not become a hole in that guarantee.

Therefore:

- every `ResourceDescriptor` must resolve to a **statically knowable size** at compile time — from
  fixed dimensions or from authored properties, never from runtime state;
- total declared resource footprint is a **Layer 4 backend-validation** concern (§32): the backend
  checks the sum against adapter limits and the project's allocation budget and fails with a clear
  diagnostic rather than allocating opportunistically;
- resource `lifetime`/`storage` classes let the scheduler alias transient buffers, but aliasing is
  an Aestra-owned optimization, not a plugin capability;
- a stage whose resource sizes cannot be determined statically is rejected at Layer 3 (lowering),
  the same way a non-terminating loop would be.

This keeps the bounded-pool contract intact even for large plugin simulations.

---

# 12. Execution scheduling

## 12.1 Core lifecycle stays ordered

Aestra owns the stable relative lifecycle:

```text
Emitter Spawn
Emitter Update
Particle Spawn
Particle Update
```

Do not let plugins arbitrarily reorder these roles.

---

## 12.2 Extension stages need explicit placement

Start conservatively.

A custom simulation stage should have a stable placement relative to the lifecycle.

A practical initial contract:

```rust
pub enum StageAnchor {
    AfterEmitterSpawn,
    AfterEmitterUpdate,
    AfterParticleSpawn,
    AfterParticleUpdate,
    BeforeRender,
}
```

Most advanced simulation stages will initially live:

```text
AfterParticleUpdate
```

Example:

```text
Particle Update
    ↓
Grid Deposit
    ↓
Fluid Solver
    ↓
Grid → Particle
    ↓
Renderers
```

Later, if real use cases demand richer scheduling, evolve this into an explicit dependency DAG while maintaining core lifecycle anchors.

Do not begin with a fully arbitrary scheduler unless required.

---

## 12.3 Stage order remains authored

Within the same extension anchor, authored order matters:

```text
AfterParticleUpdate:
    Grid Deposit
    Pressure Solve
    Advection
```

The compiler may add barriers/dependencies but should not silently reorder semantic stages unless an optimization is provably equivalent.

---

# 13. Generic Execution IR

This is the most important runtime/compiler boundary for extensibility.

> **Shared with the hybrid simulation roadmap.** This IR is the *same* multi-pass compute plan the
> hybrid roadmap calls the "generic staged simulation plan" (`SimulationPassPlan`). There is one such
> IR in Aestra — see §44.1 for the type correspondence. Design it once.

A stage must **not** mean "one dispatch".

Instead:

```text
StageInstance
      │
      ▼
stage/module lowering
      │
      ▼
portable Execution IR
```

A simple particle stage could lower to one fused simulation program.

A fluid stage may lower to many passes.

---

## 13.1 Suggested first execution primitives

Conceptually:

```rust
pub enum ExecutionOp {
    Compute(ComputeOp),
    Barrier(BarrierOp),
    Copy(CopyOp),
}
```

with structured repeat support:

```rust
pub struct ExecutionBlock {
    pub repeat: RepeatPolicy,
    pub operations: Vec<ExecutionOp>,
}
```

Example:

```text
Fluid Solver stage
    ↓

Compute GridDeposit
Barrier

Repeat ×12:
    Compute Pressure
    Barrier

Compute Advection
Barrier

Compute GridToParticle
```

Do not expose raw WGPU command encoding in the portable IR.

---

## 13.2 Compute program references

Compute operations should reference portable shader/program assets:

```rust
pub struct ComputeOp {
    pub program: ComputeProgramId,
    pub entry_point: String,
    pub dispatch: DispatchShape,

    pub reads: Vec<ResourceAccess>,
    pub writes: Vec<ResourceAccess>,

    pub bindings: Vec<ResourceBinding>,
}
```

A plugin may provide WESL-based programs as part of its package.

Aestra owns validation and backend translation.

---

## 13.3 CPU reference support

Aestra currently treats deterministic CPU evaluation as an important semantic reference.

Plugin extensions should explicitly declare backend support:

```rust
pub struct BackendSupport {
    pub cpu_reference: SupportLevel,
    pub gpu_compute: SupportLevel,
}
```

Possible values:

```text
Required
Supported
Unavailable
```

Official built-in simulation functionality should continue to provide deterministic reference behavior.

Community plugins may initially be GPU-only, but Aestra must surface that clearly:

```text
⚠ This stage has no CPU reference implementation.
  CPU fallback and reference conformance are unavailable.
```

Never silently pretend unsupported backends are equivalent.

### How CPU reference actually works — the honest boundary

`BackendSupport.cpu_reference` is a real capability with a real cost, and the plan must not imply
that declaring `cpu_reference: Supported` is free. There is a fundamental asymmetry:

- **Built-in lifecycle stages** keep their existing typed CPU `Instruction` plan. `CompiledStage`
  carries that typed plan alongside its portable execution blocks (§14), and the reference
  interpreter runs it exactly as it does today. Built-ins therefore remain the deterministic
  reference and lose nothing in the migration.
- **A plugin whose lowering emits `ComputeOp`s referencing WESL programs has no CPU path by
  default.** Aestra does **not** interpret WESL on the CPU. A `ComputeOp` is a GPU-shaped
  instruction; there is no automatic CPU equivalent.

So a plugin gets a CPU reference in exactly one of these ways, and it must say which:

```text
cpu_reference = Unavailable   the stage is GPU-only; Aestra surfaces the warning above
cpu_reference = Supplied      the plugin ships a SEPARATE deterministic CPU evaluator,
                              distinct from its WESL programs, kept in sync by the
                              conformance harness (§34)
```

Aestra does **not** promise to derive a CPU reference from a plugin's GPU programs. A "Supplied"
CPU evaluator is a second implementation the plugin author writes and Aestra conformance-tests
against the GPU result within a declared tolerance.

For the initial milestones this means: built-ins are CPU-reference-complete; community compute
stages are expected to declare `Unavailable` until their author invests in a separate evaluator. A
general WESL-on-CPU interpreter is explicitly a **non-goal** (§40) — it may be revisited later, but
nothing in this plan depends on it.

---

# 14. Compiled stage representation

Replace the fixed:

```rust
ExecutionPlan {
    emitter_update,
    particle_spawn,
    particle_update,
}
```

with a representation that can express generic stages while retaining lifecycle semantics.

Conceptually:

```rust
pub struct CompiledEmitter {
    // existing identity/timing/etc.

    pub lifecycle: CompiledLifecycleStages,
    pub simulation_stages: Vec<CompiledStage>,
    pub renderers: Vec<RendererPlan>,
}

pub struct CompiledLifecycleStages {
    pub emitter_spawn: CompiledStage,
    pub emitter_update: CompiledStage,
    pub particle_spawn: CompiledStage,
    pub particle_update: CompiledStage,
}

pub struct CompiledStage {
    pub source: StageId,
    pub stage_type: StageTypeId,

    pub lifecycle_role: Option<LifecycleRole>,

    pub execution: Vec<ExecutionBlock>,

    pub requirements: StageRequirements,
}
```

The effect-level lifecycle (§4.2) compiles to the matching effect-scoped container — a
`CompiledEffectLifecycleStages { effect_spawn, effect_update }` on the compiled effect — mirroring
the authored shape. `CompiledEmitter` covers only emitter/particle scope.

For CPU-reference-friendly built-ins, `CompiledStage` can additionally carry the typed CPU instruction plan needed by the reference interpreter (§13.3). Community compute stages that declare no CPU reference carry only their portable execution blocks.

The implementation can evolve incrementally rather than replacing all current typed instructions at once.

---

# 15. Source mapping

Current source mapping targets a fixed `RuntimeStage`.

Move toward semantic IDs instead:

```rust
pub struct IrLocation {
    pub emitter: EmitterId,
    pub stage: StageId,
    pub module: Option<ModuleId>,
    pub instruction_index: usize,
}
```

This improves:

- compiler diagnostics;
- profiler navigation;
- Compiler Inspector;
- plugin stage debugging;
- editor selection;
- future effect graph navigation.

Never make a user-facing source map depend solely on a runtime stage index.

---

# 16. Renderer extensibility

Renderers must become registry-driven too.

Current built-in renderer enums are useful internally but eventually become a limitation for community renderers.

Use:

```rust
pub struct RendererInstance {
    pub id: RendererId,
    pub renderer_type: RendererTypeId,
    pub enabled: bool,

    #[serde(default)]
    pub name: Option<String>,

    /// The material binding stays a first-class structural field, not payload.
    pub material: MaterialId,

    pub payload: ExtensionPayload,
}
```

### Keep the material binding structural

Today `RendererInstance.material: MaterialId` is a core field, and structural consumers depend on
being able to traverse the renderer→material relationship **without loading the renderer's plugin**:
thumbnail generation, material drafts, the material graph, reference validation, and — critically —
missing-plugin resilience (§20), where an effect using an uninstalled renderer must still report and
preserve which material it references.

Do **not** demote `material` into `ExtensionPayload`. A renderer that legitimately needs *several*
material bindings should declare them as typed **resource references** in its schema so they remain
structurally queryable; an opaque payload blob is not acceptable for anything another subsystem must
follow. The same rule applies to any renderer property that names a project asset (mesh, texture,
flipbook): asset references are structural, not payload.

with a registry descriptor:

```rust
pub struct RendererDescriptor {
    pub id: RendererTypeId,
    pub provider: ExtensionId,

    pub display_name: LocalizedTextKey,
    pub description: LocalizedTextKey,

    pub property_schema: PropertySchema,

    pub required_capabilities: CapabilitySet,
    pub required_resources: Vec<ResourceAccessPattern>,

    pub backend_support: BackendSupport,

    pub lowering: RendererLowerer,
}
```

A renderer still remains a separate fan-out consumer rather than an ordinary simulation stage.

---

# 17. Two classes of plugin renderer/runtime integration

Aestra should distinguish:

## Portable renderer extension

Lowers entirely to an Aestra renderer/render-program contract.

Benefits:

- engine-neutral artifact;
- future Bevy/Godot/etc. portability;
- no arbitrary host access;
- potentially no runtime plugin required after baking.

## Native runtime extension

Requires engine/backend-specific integration.

Example:

```text
special proprietary hardware path
engine-specific scene query
custom platform API
```

This is allowed, but must be declared explicitly as non-portable.

Artifacts should record the runtime extension requirement and fail with a useful diagnostic when it is unavailable.

---

# 18. Extension property payloads

A plugin-defined type must remain serializable even when the plugin is not installed.

This rules out storing arbitrary plugin Rust structs directly in the effect file.

Use a schema-versioned generic payload:

```rust
pub struct ExtensionPayload {
    pub schema_version: u32,
    pub values: PropertyBag,
}
```

with deterministic, self-describing values.

The exact representation can reuse or extend Aestra's existing semantic `Value` model.

It should support the common authoring primitives required by:

- booleans;
- integers;
- floats;
- strings;
- vectors;
- colors;
- enums/choices;
- resource references;
- curves;
- gradients;
- lists;
- nested records where necessary.

Built-ins can expose typed helper accessors over the same payload.

Avoid a system in which plugin-defined values disappear simply because their Rust implementation is unavailable.

---

# 19. Property schemas

The normal plugin UI should be schema driven.

Example:

```text
resolution
    type: integer
    label: Resolution
    min: 16
    max: 512
    default: 128

solver
    type: choice
    values:
        PIC
        FLIP
        APIC

pressure_iterations
    type: integer
    min: 1
    max: 128
    default: 12
```

Aestra can then automatically provide:

- native Feathers controls;
- localization;
- validation;
- undo/redo;
- search;
- copy/paste;
- AI semantic editing;
- presets;
- animation/exposure where allowed;
- consistent accessibility.

Custom plugin UI should be an advanced escape hatch, not the default authoring mechanism.

## 19.1 `PropertySchema` generalizes existing `ModuleMetadata.inputs`

Aestra already drives module controls from `ModuleMetadata.inputs: Vec<InputMetadata>` in
`aestra-compiler`. `PropertySchema` is the **generalization and eventual replacement** of that
metadata, not a second parallel system. The migration should:

- express today's built-in `InputMetadata` (label, range, default, control kind) as `PropertySchema`
  entries, so built-ins and plugins render through one code path;
- retire the bespoke metadata-driven control code in the editor once the schema-driven renderer
  covers the existing control kinds;
- keep the typed helper accessors (§18) as a thin layer over the same schema-backed payload.

Do not ship `PropertySchema` alongside a still-live `InputMetadata` control path long-term; converge
on one.

---

# 20. Missing-plugin resilience

This is mandatory for a serious community ecosystem.

Opening an effect that contains an unavailable plugin must not destroy the unknown data.

Example:

```text
SIMULATION
    ⚠ Fluid Simulation

      Missing plugin:
      org.someauthor.aestra-fluid >= 1.4

      [Locate / Install Plugin]
```

Aestra must preserve:

```text
stage type ID
plugin requirement
payload
schema version
order
enabled state
stable semantic IDs
```

even while the plugin is unavailable.

The asset should remain inspectable and safely saveable for unrelated edits.

Compilation can fail specifically for the unavailable execution feature.

---

## 20.1 Separate structural validation from registry validation

This is important.

`aestra-core` should validate properties that do not require a plugin:

- IDs;
- timing;
- basic document structure;
- known references;
- payload syntax;
- lifecycle topology.

Registry-aware validation belongs in the compiler/extension layer:

- stage type exists;
- plugin version compatible;
- property schema valid;
- required capability exists;
- module compatible with stage;
- backend available;
- lowering implementation exists.

This allows the editor to load and preserve assets even when an extension is absent.

---

# 21. Plugin requirements in assets

Effects should record extension requirements explicitly or derive and persist them during save.

Conceptually:

```rust
pub struct ExtensionRequirement {
    pub plugin: ExtensionId,
    pub version_requirement: String,
}
```

Example:

```text
extensions: [
    (
        plugin: "org.someauthor.aestra-fluid",
        version: "^1.4",
    ),
]
```

This provides better diagnostics than only discovering unknown type IDs after deserialization.

It also allows future:

- dependency resolution;
- project packaging;
- marketplace installation;
- reproducible builds;
- lockfiles.

---

# 22. Plugin manifest

A plugin package should eventually contain a stable manifest similar to:

```toml
id = "org.someauthor.aestra-fluid"
name = "Aestra Fluid"
version = "1.4.2"

aestra_api = "^0.3"

[features]
authoring = true
compiler = true
runtime_portable = true
runtime_native = false

[permissions]
filesystem_project = false
network = false
```

Possible package contents:

```text
plugin.toml
schemas/
wesl/
icons/
locales/
presets/
code/
```

Do not design the permanent binary ABI in the first milestone.

---

# 23. Plugin registration surface

A unified extension registrar should expose concepts such as:

```rust
pub trait AestraExtension {
    fn manifest(&self) -> &ExtensionManifest;

    fn register(&self, registry: &mut ExtensionRegistry);
}
```

and:

```rust
pub struct ExtensionRegistry {
    pub stages: StageRegistry,
    pub modules: ModuleRegistry,
    pub renderers: RendererRegistry,
    pub domains: DomainRegistry,
    pub resources: ResourceTypeRegistry,
    pub capabilities: CapabilityRegistry,
}
```

Built-in Aestra functionality should register through this API too.

The exact trait is provisional; do not freeze a Rust ABI around it for dynamic loading.

---

# 24. Extension API vs host implementation

> **As built (after M12):** one crate, `crates/aestra-extension`, holds everything extension-related,
> as two modules rather than two crates:
> - `aestra_extension::sdk` is the contract: identities in use, descriptors and registries (including
>   the built-in module catalog), lowering and migration traits, requirements, linking, and
>   `EXTENSION_API_VERSION`. It is also re-exported at the crate root.
> - `aestra_extension::host` is the declarative package format, discovery, version and dependency
>   resolution, and registration.
>
> The crate depends only on `aestra-core`, `aestra-runtime`, `ron` and `semver`, so extension authors
> no longer depend on the compiler. `aestra-compiler` depends on it and re-exports `sdk`. The host is a
> module, not a crate: its dependencies are tiny, and a split (or a `host` feature) remains possible if
> code hosting (WASM/IPC) brings heavy ones.

Originally recommended code organization:

```text
crates/
    aestra-core
    aestra-authoring

    aestra-extension-api/
        stable IDs
        manifests
        property schemas
        descriptors
        capability/resource contracts
        lowering interfaces

    aestra-compiler/
        compiler orchestration
        validation
        execution lowering

    aestra-runtime/
        execution plans
        CPU/reference runtime

    aestra-artifact/
        versioned compiled DTOs

    aestra-gpu/
        portable GPU lowering

    aestra-extension-host/        # later
        discovery
        package loading
        compatibility
        sandbox/process/WASM host

    aestra-builtins/              # optional later extraction
        built-in stage/module/renderer registration
```

It is acceptable to begin by placing descriptor types in existing crates, then extract `aestra-extension-api` once the contracts settle.

Avoid premature crate splitting if it slows migration.

---

# 25. Loading strategy

## Phase A — linked extensions

First support plugins as normal Rust crates linked into an Aestra build.

This validates:

- registry contracts;
- custom types;
- missing-type handling;
- compiler lowering;
- editor integration.

No unstable dynamic library ABI is required.

---

## Phase B — packaged authoring/compiler plugins

Once the API is proven, support separately installed packages.

Prefer a sandboxable/stable boundary rather than exposing Rust trait-object ABI directly.

Candidates can be evaluated later:

- WASM component model;
- isolated helper process + versioned IPC;
- another stable FFI boundary if justified.

The semantic model must not care which mechanism is selected.

---

## Phase C — native runtime integrations

For functionality that cannot lower to portable Aestra artifacts, allow explicit native runtime integration packages.

These must declare:

- engine/backend;
- version requirements;
- runtime capabilities;
- portability limitations.

Do not silently mix them with portable plugins.

---

# 26. Baked artifacts and runtime dependency minimization

The ideal extension path is:

```text
plugin required while authoring/compiling
            │
            ▼
portable baked Aestra artifact
            │
            ▼
game runs without plugin authoring code
```

For example, if a fluid plugin can lower to:

- Aestra compute programs;
- Aestra resources;
- generic execution blocks;
- generic renderer programs;

then the game runtime should need only the baked artifact and Aestra's standard runtime.

This dramatically improves:

- portability;
- security;
- deployment;
- reproducibility;
- game integration;
- plugin marketplace usability.

Only features that genuinely require host-native behavior should require a runtime plugin.

---

# 27. Example: external fluid plugin

Suppose a community developer publishes:

```text
Plugin:
    org.example.aestra-fluid
```

It registers:

```text
Stage types
    org.example.aestra-fluid::stage/fluid_solver

Modules
    org.example.aestra-fluid::module/vorticity
    org.example.aestra-fluid::module/buoyancy
    org.example.aestra-fluid::module/density_source

Domains
    org.example.aestra-fluid::domain/grid3d

Resources
    org.example.aestra-fluid::resource/velocity_grid
    org.example.aestra-fluid::resource/pressure_grid
    org.example.aestra-fluid::resource/density_grid

Renderer
    org.example.aestra-fluid::renderer/volume
```

The editor may show:

```text
PARTICLE UPDATE
────────────────────────────────
  Gravity
  Motion

SIMULATION
────────────────────────────────
  Fluid Solver                 FLIP · 128³

RENDER
────────────────────────────────
  Sprite Renderer
  Volume Renderer
```

Selecting Fluid Solver:

```text
FLUID SOLVER
Provided by Aestra Fluid

GENERAL
Solver                         FLIP
Resolution                     128³
Substeps                          2

PRESSURE
Iterations                       12
Tolerance                     0.001

MODULES
Vorticity                       0.8
Buoyancy                        1.2

ADVANCED                          ▸
```

The authored stage remains one semantic object.

Its lowering may produce:

```text
Compute GridDeposit
Barrier

Compute ApplySources

Repeat ×12:
    Compute Pressure
    Barrier

Compute Advection
Barrier

Compute GridToParticle
```

Aestra's generic scheduler executes this without needing a `Fluid` variant in core.

---

# 28. Properties panel redesign

The plugin architecture and the Properties redesign should be built together.

The current all-expanded-card design does not scale when an emitter contains many repeated modules, custom stages, and renderers.

The Properties panel should answer two different questions in two different regions:

> **Stack:** what executes, and in what order?  
> **Inspector:** how is the selected object configured?

---

## 28.1 Proposed layout

```text
┌────────────────────────────────────────────┐
│ PROPERTIES                                 │
│ Prism Core                             ✓ ⋮ │
├────────────────────────────────────────────┤
│ [ Search stack...                       ]  │
│                                            │
│ ▾ EMITTER SPAWN                       1    │
│   ⠿ Initialize Emitter                    │
│                                            │
│ ▾ EMITTER UPDATE                      1  + │
│   ⠿ Emission                  Rate 22      │
│                                            │
│ ▾ PARTICLE SPAWN                      2  + │
│   ⠿ Shape                     Sphere       │
│   ⠿ Initialize Particle       .7–1.25 s   │
│                                            │
│ ▾ PARTICLE UPDATE                     4  + │
│   ⠿ Gravity                   9.81         │
│   ⠿ Vortex "Orbit"           4.0          │
│   ⠿ Vortex "Detail"          1.2          │
│   ⠿ Appearance               Fade         │
│                                            │
│ ▾ SIMULATION                          1  + │
│   ⠿ Fluid Solver              FLIP · 128³ │
│                                            │
│ ▾ RENDER                              2  + │
│   ⠿ Sprite Renderer           Additive    │
│   ⠿ Volume Renderer           Fluid       │
│                                            │
├──────────── draggable splitter ────────────┤
│ FLUID SOLVER                               │
│ Provided by Aestra Fluid                   │
│                                            │
│ Solver                         FLIP         │
│ Resolution                     128³         │
│ Substeps                          2         │
│                                            │
│ PRESSURE                           ▸        │
│ ADVANCED                           ▸        │
└────────────────────────────────────────────┘
```

---

## 28.2 Stack rows, not parameter cards

A stack item should be compact:

```text
⠿ ✓ Vortex "Orbit"      Strength 4 · R 3.5   ⚠  ⋮
```

Suggested height:

```text
25–28 px
```

Use summaries generated from descriptor metadata.

Example summaries:

```text
Motion                  Drag 1.6 · Turb 5
Shape                   Sphere · Radius 14
Fluid Solver            FLIP · 128³ · ×12
Sprite Renderer         Additive · Fire Material
```

---

## 28.3 Stage sections

Each stage header should support:

- collapse/expand;
- stage diagnostic count;
- contained item count;
- add module;
- stage selection where meaningful;
- custom-stage context menu.

Example:

```text
▾ PARTICLE UPDATE              6        ⚠1   +
```

Extension stages are first-class selectable rows/sections rather than miscellaneous custom cards.

---

## 28.4 Drag and drop

Replace move-up/down as the primary interaction.

Keep keyboard/context commands for accessibility.

During drag:

```text
Gravity
Vortex A
──────────── drop here
Vortex B
Appearance
```

Commands should support target index directly:

```rust
MoveModule {
    module: ModuleId,
    target_stage: StageId,
    target_index: usize,
}
```

Compatibility validation decides whether cross-stage moves are permitted.

---

## 28.5 Repeated module names

Allow optional instance labels:

```text
Vortex "Large Orbit"
Vortex "Detail Noise"

Collision "Ground"
Collision "Characters"

Sprite Renderer "Core"
Sprite Renderer "Glow"
```

The semantic type remains unchanged.

---

## 28.6 Schema-driven inspector

The Inspector should render plugin and built-in properties through the same schema-driven Feathers controls whenever possible.

A provider badge should be subtle:

```text
Fluid Solver
Provided by Aestra Fluid
```

Do not make plugin items visually second-class.

---

## 28.7 Missing plugin presentation

Unknown types remain in place:

```text
⚠ Fluid Solver
  Plugin unavailable
```

The Inspector shows:

```text
MISSING EXTENSION

Type
org.example.aestra-fluid::stage/fluid_solver

Required plugin
org.example.aestra-fluid ^1.4

The authored data is preserved.
Compilation is unavailable until the extension is installed.
```

---

# 29. Stack vs graph

Keep these concepts distinct.

## Module stack

Best for ordered execution:

```text
Gravity
    ↓
Vortex
    ↓
Drag
    ↓
Appearance
```

## Effect graph

Best for relationships:

```text
Explosion
   ├──OnDeath──► Smoke
   ├──OnHit────► Sparks
   └───────────► Shockwave
```

Long term, Aestra may offer both as projections of the same semantic model.

Do not turn a fundamentally linear module pipeline into a node graph merely for visual sophistication.

---

# 30. Artifact format implications

The compiled artifact format must eventually encode:

- stage IDs;
- stage type IDs;
- lifecycle role;
- execution blocks;
- generic resources;
- compute programs;
- backend requirements;
- custom renderer plans;
- extension/runtime requirements;
- source mappings.

Keep using explicit versioned DTOs in `aestra-artifact`.

Do not serialize in-memory registry objects or Rust implementation details.

> **One compiled-artifact bump, shared with the hybrid roadmap.** The compiled artifact is at v2
> today. It bumps to v3 **once**, encoding both this plan's generic stages/execution IR and the
> hybrid roadmap's simulation islands / state layouts / pass plans. Do not bump it separately in each
> plan — see §44.5.

---

# 31. Authored format migration

This redesign is substantial enough to justify a new authored format version.

Recommended:

```text
format v3 → v4
```

The migration remains explicit and backup-preserving, consistent with the current editor policy.

---

## 31.1 v3 migration

Current v3 emitter:

```text
modules: [
    Emission(stage: EmitterUpdate),
    Shape(stage: ParticleSpawn),
    Initialize(stage: ParticleSpawn),
    Motion(stage: ParticleUpdate),
    Appearance(stage: ParticleUpdate),
]
```

v4:

```text
lifecycle:
    emitter_spawn:
        modules: []

    emitter_update:
        modules:
            Emission

    particle_spawn:
        modules:
            Shape
            Initialize

    particle_update:
        modules:
            Motion
            Appearance

simulation_stages: []

renderers:
    ...
```

Preserve relative module order **within each existing stage**.

If old assets use `StageKind::Simulation(name)`, migration must convert those modules into explicit simulation-stage instances deterministically.

Potential policy:

```text
same Simulation(name)
    → same migrated StageInstance

first appearance of a simulation-stage name
    → determines stage ordering
```

Document this behavior and test it.

### Effect-level modules

Any module whose v3 `stage` is `StageKind::EffectSpawn` or `EffectUpdate` migrates into the
**effect-level** lifecycle slots (§4.2, "Effect-level lifecycle"), not the emitter lifecycle.
Preserve relative order within each effect-level stage exactly as for emitter stages.

### `simulation_domain` field

The v3 `Emitter.simulation_domain: SimulationDomain { Particle, Strip, Custom(String) }` maps onto
registered `DomainTypeId`s (§10):

```text
Particle      → aestra.domain.particles
Strip         → aestra.domain.strip
Custom(name)  → preserved as a namespaced domain id; unknown domains follow the
                missing-extension rules (§20) rather than being dropped
```

Emit the migrated domain id explicitly so the field round-trips without registry access.

---

## 31.2 Empty lifecycle stages

Create all standard lifecycle stage instances even if empty.

This produces stable structure and IDs.

For example:

```text
Emitter Spawn
    <empty>
```

is preferable to semantically creating/deleting the lifecycle stage itself.

The editor may visually hide empty sections in compact mode, but the model remains stable.

---

# 32. Validation layers

Use explicit validation layers.

## Layer 1 — structural (`aestra-core`)

No plugin code required.

Checks:

- semantic IDs;
- lifecycle structure;
- references;
- timing;
- extension payload syntax;
- duplicate stage IDs;
- basic authored ordering;
- renderer/module/stage identity presence.

## Layer 2 — registry/schema (`aestra-compiler` / extension API)

Checks:

- type registered;
- version compatible;
- schema valid;
- singleton constraints;
- stage/module compatibility;
- capability requirements;
- resource declaration validity.

## Layer 3 — lowering

Checks:

- plugin compiler available;
- backend support;
- generated Execution IR valid;
- resource hazards resolvable;
- programs validate.

## Layer 4 — backend

Checks:

- GPU adapter limits;
- runtime capability;
- engine-specific integration;
- resource sizes;
- native extension availability.

Diagnostics should retain all four categories.

---

# 33. Security boundary

Community extensions must not automatically receive arbitrary host access.

Future plugin permissions should be explicit.

Potential capabilities:

```text
project file read
project file write
network
process spawn
clipboard
native engine API
```

Pure semantic/compiler plugins ideally need none of these.

A WESL/resource/property-based fluid plugin should be possible without filesystem/network privileges.

Avoid making "plugin" synonymous with "unrestricted native code loaded into the editor process".

---

# 34. Determinism and reproducibility

Plugin architecture must preserve Aestra's deterministic goals.

Require:

- stable lowering for the same asset + plugin versions;
- stable plugin/type IDs;
- versioned schemas;
- deterministic ordering;
- explicit random seed sources;
- no hidden dependence on wall clock;
- artifact fingerprints that include plugin/compiler-relevant versions;
- reproducible project/plugin lock information eventually.

Generated artifacts should record enough provenance to diagnose mismatches.

**Execution order is authored, never capability-derived.** Capabilities decide *compatibility*
(whether a module may live in a stage, §9); they must never influence *ordering* within a stage.
Order comes solely from the authored module/stage sequence (§8.1, §12.3). If execution order ever
depended on registry contents or capability matching, the same asset would lower differently as the
installed plugin set changed — breaking reproducibility. Keep the two concerns strictly separate.

---

# 35. Versioning and migrations

Every extension type needs a schema version.

Example:

```text
stage type:
    org.example.aestra-fluid::stage/fluid_solver

payload schema:
    3
```

A plugin supplies migrations:

```text
v1 → v2
v2 → v3
```

Never rely only on the overall plugin version.

One plugin release may migrate only one of several registered types.

Unknown/newer payload versions should remain preserved but non-editable/non-compilable rather than being silently rewritten.

---

# 36. Recommended implementation milestones

The work should be incremental. Do not attempt dynamic plugin loading, a new execution scheduler, a Properties rewrite, and real fluids in one change.

---

## Milestones 0–1 — shared foundation (see `aestra_shared_foundation_milestones.md`)

> This plan's original M0 (baseline fixtures) and M1 (extension identity + registry) are **merged
> with the hybrid roadmap's M0/M1 into one shared front end** — `aestra_shared_foundation_milestones.md`,
> milestones **S0** and **S1**. Run those once; do not run this plan's M0/M1 separately.
>
> S0 freezes a combined correctness + performance baseline. S1 introduces the namespaced identities
> (`ExtensionId`, `StageTypeId`, `DomainTypeId`, `ResourceTypeId`, `CapabilityId`) and the unified
> `ExtensionRegistry` from this plan **together with** the hybrid roadmap's derived simulation
> semantics, and — critically — extends `ModuleMetadata` **exactly once**: this plan's capabilities /
> reads / writes / multiplicity co-designed with the hybrid roadmap's temporal / synchronization /
> neighborhood requirements (§44.3). The backend-capability shape (§13.3) is agreed there too.
>
> Everything below (M2 onward) begins **after** S1, on the extensible track.

---

## Milestone 2 — property schema and generic extension payload

> **Status — schema + payload slice landed.** `aestra-core` now has the generic extension-data types
> (`aestra-core/src/property_schema.rs`): `ExtensionPayload { schema_version, values: PropertyBag }`
> stores authored plugin data as a schema version plus a self-describing `PropertyBag` over the existing
> semantic `Value` model, so it serializes/deserializes **without any plugin Rust code**. `PropertySchema`
> (a versioned list of `PropertyDescriptor { name, label, description, value_type, default, unit,
> control, sources }` with a `PropertyControl` generalizing the compiler's `InputControl`) is the
> generalization of `ModuleMetadata.inputs` (§19.1): `ModuleMetadata::property_schema()` /
> `InputMetadata::to_property_descriptor()` express every built-in's inputs as a schema, proven lossless
> and self-validating for the whole built-in registry (`builtin_module_inputs_express_losslessly_as_property_schemas`).
> `PropertySchema::validate` reports type mismatches and out-of-range numeric values on *described*
> properties, `apply_defaults`/`default_bag` seed authoring, typed accessors (`get_f32`, `get_bool`, …)
> serve built-ins, and **unknown bag keys are preserved** (validated as OK, reported via `unknown_keys`,
> surviving round trips) so a missing/newer plugin never loses data. The M2 acceptance workload — a fake
> "Test Module { strength: float, mode: enum }" that serializes, deserializes without plugin code,
> validates when installed, and preserves an unknown key when missing — passes in `property_schema`'s
> tests. **Deliberately deferred (per the chosen slice):** the editor still renders built-in controls
> through the live `InputMetadata` path; converging that onto the schema-driven inspector is Phase 9b,
> and embedding `ExtensionPayload` into the authored module/stage/renderer shape is the M3 format-v4
> migration — M2 adds the types and the built-in expression without a format bump.

### Goal

Make authored plugin data representable without plugin Rust structs.

### Work

- define `ExtensionPayload`;
- define `PropertySchema` as the **generalization of `ModuleMetadata.inputs` / `InputMetadata`**, not
  a parallel system (§19.1);
- define property metadata and validation;
- express existing built-in `InputMetadata` as `PropertySchema` so built-ins and plugins share one
  control-rendering path;
- support schema version;
- preserve unknown properties;
- add typed helper accessors for built-ins over the schema-backed payload.

### Acceptance

A fake plugin can define:

```text
Test Module
    strength: float
    mode: enum
```

and:

- serialize it;
- deserialize it without plugin code;
- validate/edit it when plugin is installed;
- preserve it when plugin is missing.

---

## Milestone 3 — explicit authored stages and format v4

> **Status — format v4 cut, via a flat-in-memory DTO (landed in 3 stages).** Authored format is now
> **v4**: an emitter's modules live in named lifecycle containers (`emitter_spawn` / `emitter_update` /
> `particle_spawn` / `particle_update`) plus explicit `simulation_stages`, `simulation_domain` is a
> namespaced `DomainTypeId`, and the effect reserves its own `lifecycle` slots. Per the agreed approach
> the **in-memory model stays flat**: `aestra-core::authored_v4::AuthoredV4Document` is a serde DTO that
> nests on save (`to_pretty_ron`/`save_ron`) and flattens on load (`from_ron`), reconstructing each
> module's stage from its container, so the ~300 flat `modules`/`stage` call sites and the editor did not
> churn. Simulation stages get a `StageId` derived deterministically from their name
> (`StageId::for_name`), so round trips are stable without the flat model storing a per-stage UUID.
> `StageId` and the built-in `aestra.stage.*` / `aestra.domain.*` id consts are registered. **Migration
> stance (pre-release):** the legacy v2→v3 migration was dropped and no runtime v3→v4 migration is
> carried — a one-shot `migrate_v3_to_v4` example converted the 24 in-repo assets, which are committed in
> v4; older formats now load as an explicit `UnsupportedFormat` error. Acceptance met: v3 assets migrated
> deterministically; v4 round-trips without registry access (DTO + RON round-trip tests); the standard
> lifecycle cannot be reordered/deleted (fixed named slots in the format); repeated same-type modules
> stay valid; simulation stages have stable ids + first-appearance order; the format-contract test is
> re-pinned to v4. **Deliberately deferred (pre-release lets us re-cut the format, so no "one migration"
> constraint):** the renderer `payload` generalization rides with M8's renderer work; effect-level
> module *storage* in the flat model (the reserved effect `lifecycle` is currently always empty); the
> in-memory containment refactor and editor stage-section UI ride with M9; and a dedicated Layer-1
> structural-validation pass beyond the total DTO conversion + existing `validate()`. Two pre-existing
> `aestra-editor` material-editor tests fail independently of this work (an in-progress
> `material_function_editor` refactor already uncommitted at the start), so effect-format changes are
> otherwise green across the workspace.

### Goal

Make stages first-class semantic objects — and land **every authored-format shape change in one v4
cut**, so the format migrates exactly once.

> **One migration, not three.** The registry, capability, and renderer-descriptor *machinery*
> arrive in later milestones (M4, M8), but the *authored shape* they operate on must be finalized
> here. Deferring the renderer or effect-lifecycle shape to a later milestone would force a second
> authored-format migration. Decide the shape now; fill in the behavior later.

### Work

Add:

```text
StageId
StageInstance
EmitterLifecycleStages
EffectLifecycleStages          // effect_spawn / effect_update (§4.2)
Effect.lifecycle
Emitter.simulation_stages
```

Migrate:

```text
ModuleInstance.stage           → structural containment (§31.1)
StageKind::EffectSpawn/Update  → effect-level lifecycle slots
StageKind::Simulation(name)    → explicit simulation StageInstances
Emitter.simulation_domain      → DomainTypeId (§10, §31.1)
```

Finalize the **generic authored shape** of renderers in v4 even though their descriptor/lowering
lands in M8:

```text
RendererInstance {
    id, renderer_type, enabled, name,
    material,        // stays a structural field, not payload (§16)
    payload,
}
```

Implement explicit, backup-preserving v3 → v4 migration.

Convert built-in lifecycle roles into registered stage types:

```text
aestra.stage.effect_spawn
aestra.stage.effect_update
aestra.stage.emitter_spawn
aestra.stage.emitter_update
aestra.stage.particle_spawn
aestra.stage.particle_update
```

Add structural validation (Layer 1, §32).

### Acceptance

- v3 example assets migrate deterministically, **including effect-level modules, `Simulation(name)`
  stages, and `simulation_domain`**;
- v4 round trips without registry access;
- standard lifecycle (effect and emitter) cannot be reordered/deleted accidentally;
- repeated same-type modules remain valid;
- simulation stages have stable IDs and order;
- renderer material/asset references remain structurally queryable without loading a renderer plugin;
- no further authored-format version bump is required by M4–M8.

---

## Milestone 4 — descriptor compatibility and capabilities

> **Status — capability-based compatibility landed.** The concrete stage check
> (`metadata.stages.contains(module.stage)`) is replaced by capability satisfaction: a stage
> **provides** a `CapabilitySet` and a module **requires** a `CapabilityExpression` (`AnyOf` / `AllOf` /
> `Unconstrained`), and `StageTypeDescriptor::hosts(requires)` decides compatibility by inspecting only
> what the stage provides — never its concrete type id. Six lifecycle-role capabilities
> (`aestra.capability.hosts_*`, core) are provided by the built-in stages via
> `StageTypeDescriptor::lifecycle(LifecycleRole)`, and each built-in module's requirement is derived
> from its declared roles (`ModuleMetadata::required_capabilities`), so the check is behaviour-identical
> to the old one for built-ins while letting a **third-party stage host standard modules by providing
> the right capability** — proven by `a_third_party_stage_hosts_standard_modules_by_capability` (a
> custom-type-id stage that provides `hosts_particle_update` hosts Motion/Appearance but not the
> particle-spawn Shape). `ModuleMultiplicity { Single, Multiple }` was added with singleton validation
> (the persistent solver is `Single`; two on one emitter are rejected —
> `duplicate_singleton_modules_in_one_stage_are_rejected`). **Deferred:** `ModuleMetadata.stages` is
> retained as the authoring hint that `required_capabilities` derives from rather than fully removed;
> `BackendSupport` and richer `CapabilityExpression` operators (Not/nested) land with the descriptor
> registry work in M5–M7 when a concrete need appears.

### Goal

Remove concrete stage checks from module discovery.

### Work

Add:

```text
StageTypeDescriptor
ModuleDescriptor
CapabilitySet
CapabilityExpression
ModuleMultiplicity
BackendSupport
```

Replace `ModuleMetadata.stages: Vec<StageKind>` with capability-based compatibility.

Built-in stages provide capabilities.

Built-in modules declare requirements.

Add singleton validation where needed.

### Acceptance

A third-party test stage that provides the particle-update capabilities can host standard compatible modules without hardcoded knowledge of its type ID.

---

## Milestone 5 — generic compiled stage plan

> **Status — generic stage plan landed as a first-class compiled structure.** `CompiledStage { id:
> StageId, stage_type: StageTypeId, instructions }` and `CompiledLifecycleStages { stages: Vec<_> }`
> (aestra-runtime) make the three-vector `ExecutionPlan` no longer the compiler's *only* stage model:
> every compiled emitter now also carries `stages`, a generic ordered plan where each stage has a
> stable identity. The compiler builds it from the execution plan (`from_execution_plan`, deterministic
> `StageId::for_name(emitter:stage_type)`); `to_execution_plan` rebuilds the exact legacy plan, so the
> interpreter still runs the typed `ExecutionPlan` and **CPU behavior is bit/order-identical** (asserted
> by `compiled_emitter_carries_a_stage_id_based_generic_stage_plan`). **Source navigation is stage-id
> based** via `stage_of_module` (a module resolves to the stage that runs it). The **artifact round trip
> retains the generic stage identities**: the decode reproduces `stages` from the execution plan and the
> emitter's stable source id, so the ids match — covered by the existing full-struct round-trip
> equality (`reloaded == compiled` now includes `stages`). **Deferred:** the interpreter consuming the
> generic plan directly (rather than the derived `ExecutionPlan`) and lowering stages into more than one
> pass is M6's execution IR; the Compiler Inspector / profiler still read `execution` and can adopt the
> stage-id plan when their UI surfaces stages.

### Goal

Remove the fixed three-vector runtime plan as the compiler's only stage representation.

### Work

Introduce:

```text
CompiledLifecycleStages
CompiledStage
stage-ID-based source mapping
```

Initially, a `CompiledStage` may still contain current typed `Instruction` arrays.

Update:

- compiler;
- runtime;
- artifact DTO;
- Compiler Inspector;
- profiler;
- diagnostics.

### Acceptance

- existing CPU behavior remains bit/determinism equivalent where expected;
- source navigation uses StageId;
- artifact round trip retains generic stage identities.

---

## Milestone 6 — portable Execution IR and resource model

> **Status — portable Execution IR + reference backend landed.** `aestra-runtime::execution_ir` defines
> the engine-independent IR: `ExecutionBlock { resources: Vec<ResourceDescriptor>, ops: Vec<ExecutionOp> }`
> where `ExecutionOp` is `Compute(ComputeOp)` / `Barrier` / `Copy(CopyOp)` / `Repeat { policy:
> RepeatPolicy, body }`, with `ResourceDescriptor` + `ResourceAccess`(`Read`/`Write`/`ReadWrite`) and
> `RepeatPolicy::FixedCount`. `ExecutionBlock::validate()` checks declared/unique resources, resolvable
> accesses, non-zero dispatches, and non-zero repeats (recursing into repeat bodies). A **reference
> backend** (`execute_reference`) runs a block into a deterministic ordered trace — repeats expand,
> barriers stay in place — with no GPU. The M6 acceptance workload (Compute A → Barrier → Repeat
> Compute B ×4 → Compute C) validates and traces in exactly that order
> (`a_multi_pass_stage_validates_and_executes_in_deterministic_order`). Built-in particle behavior
> lowers **fused** via `lower_stage_fused`: a whole stage (any number of modules) becomes a single
> compute pass over the particle buffer, not one dispatch per module
> (`a_built_in_particle_stage_lowers_fused_to_a_single_compute_pass`), so nothing regresses. **Deferred:**
> the native GPU backend executing these blocks (allocating declared resources, resolving barriers,
> running repeat loops, timestamps) is M7 — the reference backend proves the IR's ordering semantics
> that M7 will honor.

### Goal

Allow a stage to lower into more than one runtime/GPU pass.

### Work

Introduce:

```text
ExecutionBlock
ExecutionOp
ComputeOp
BarrierOp
CopyOp
ResourceDescriptor
ResourceAccess
RepeatPolicy
```

Define execution validation.

Lower existing built-in particle behavior through the new plan without regressing fused execution/performance.

Do **not** force each module to become a separate dispatch.

### Acceptance

A synthetic test stage can lower to:

```text
Compute A
Barrier
Repeat Compute B ×4
Compute C
```

and execute through a reference/mock backend with deterministic ordering.

---

> **Status — the M6 Execution IR runs on the real GPU.** A backend consumes an `ExecutionBlock`
> (`execution_ir_conformance.rs`): it allocates a GPU buffer per declared `ResourceDescriptor` (bounded
> by the declared bytes), builds a compute pipeline per distinct op entry point, and walks the ops in
> order — each `Compute` is a dispatch, each iteration its own compute pass (a real ordering barrier),
> `Repeat` loops its body, `Copy` copies buffers — timing every pass with GPU timestamp queries. A
> synthetic multi-pass stage (`set 1 → Barrier → double ×4 → add 100`) passes on the RTX 4070 SUPER
> with **GPU validation** (the block validates and the kernels compile), **capture** (one timestamp
> interval per dispatch == `compute_pass_count`), and **deterministic ordering** (the order- and
> repeat-dependent result is exactly `116` — a wrong order or repeat count could not produce it), and a
> rerun is identical (`a_synthetic_multi_pass_stage_executes_on_gpu_in_deterministic_order`). No fluid
> solver required, per the milestone. **Deferred:** baking an `ExecutionBlock` into the artifact DTO for
> its own round-trip (the runtime types are not serde — serialization lives in the artifact's DTOs;
> M5/M6 kept blocks runtime-only) and folding block `compute_pass_count` into the profiler's dispatch
> estimate and the diagnostics' backend-requirement report are the remaining wiring; the reference
> backend (M6) and this native run pin the ordering/validation the artifact and profiler will report.

## Milestone 7 — GPU/backend generic stage scheduling

### Goal

Execute generic stage plans on the native GPU backend.

### Work

- extend `aestra-gpu` artifact lowering;
- allocate declared resources;
- resolve barriers;
- support repeat blocks;
- preserve bounded allocation policy;
- update profiler estimates/telemetry;
- include backend requirements in diagnostics.

### Acceptance

A synthetic multi-pass plugin stage passes:

- GPU validation;
- capture;
- artifact round trip;
- deterministic ordering tests.

No real fluid solver required yet.

---

> **Status — renderer registry + generic extension renderer landed.** `RendererDescriptor` (type id,
> display name, `PropertySchema`, and a built-in-vs-extension flag) and `RendererRegistry`
> (`ExtensionRegistry.renderers`, with the five core renderer types registered as built-ins + a
> `register_renderer` for plugins) route renderer creation/validation through one catalog instead of
> hardcoded type checks. A plugin renderer is authored as a `RendererProperties::Custom` payload on a
> `RendererInstance` whose `renderer_type` is a registered extension; the compiler validates it through
> the registry and **lowers it generically** to `aestra_runtime::CompiledExtensionRenderer { source,
> renderer_type, material, payload: PropertyBag }` — carried in a separate `CompiledEmitter.extension_renderers`
> list, so **no `RendererPlanKind` variant is added to core** and the live render path is untouched.
> `material` (and asset references) stay structural on the instance, so a missing-plugin renderer still
> preserves its bindings, and core's structural renderer↔material check accepts Custom renderers (their
> deeper compatibility is the plugin's concern). The extension renderers round-trip through the artifact
> (`ExtensionRendererV1`, serde-defaulted — **no format bump**). Proven by
> `a_plugin_renderer_registers_compiles_and_produces_an_extension_plan` (a registered `glow` plugin
> renderer appears in the catalog, compiles to an extension plan with its payload and structural
> material, and is rejected when unregistered). The fan-out model is preserved (built-in renderer plans
> are unchanged; extension renderers are an additional list). **Deferred:** a `RendererLowerer` trait for
> plugin-authored lowering beyond the generic payload path, folding extension-renderer backend/capability
> requirements into the compatibility report, and a real portable/native renderer backend consuming
> `extension_renderers` — those arrive with plugin-loading (M25-adjacent) work.

## Milestone 8 — renderer registry/generalization

### Goal

Make renderers community-extensible.

> The **authored** `RendererInstance` shape (generic `payload` + structural `material`/asset
> references) already landed in v4 at M3. This milestone adds the registry/descriptor/lowering
> behavior behind that shape — **no authored-format change here**.

### Work

Introduce:

```text
RendererDescriptor
RendererLowerer
renderer property schema over the existing generic payload
renderer capability/resource requirements
```

Route built-in renderer creation/validation through the registry.

Evolve the **compiled** `RendererPlanKind` (in `aestra-runtime`) so extension renderers do not
require a new core enum variant. This is a compiled/artifact-side generalization, distinct from the
authored shape already fixed in v4.

Keep `material` and asset references structural (§16) so missing-plugin renderers still report and
preserve their bindings.

Preserve the fan-out semantic model.

### Acceptance

A test plugin renderer appears in authoring, compiles, and produces a validated mock/portable
renderer plan — without adding a `RendererPlanKind` variant to core, and without a format bump.

---

> **Status — design locked; Phase 9a foundation started.** After evaluating UE5 Niagara, Houdini,
> Unity VFX Graph and Blender, the direction is confirmed as the Niagara-style shape: **viewport stays
> central**; a Properties dock holds a compact stage-grouped **module stack** on top and a
> **selection-following inspector** below (a resizable split, §28.1) — the inspector is the current
> properties panel re-scoped to the selected stack item, not a new panel; the **module stack stays a
> stack** (it mirrors the fixed lifecycle / `CompiledLifecycleStages`), while the node **graph** is
> reserved for the material editor (today) and a future emitter-relationship view (M15) — never for the
> linear module pipeline (§29). **Landed:** the testable data foundation — `EmitterStackProjection` /
> `ModuleStackGroup` / `ModuleStackRow` and `EffectCompiler::project_emitter_stack` in
> `aestra-compiler::module_stack` (mirroring `MaterialStackProjection`): stage-grouped compact rows in
> canonical lifecycle order + first-appearance simulation stages, each with a descriptor-driven one-line
> `module_summary` (§28.2–28.3), unknown/plugin modules still projected (data preserved, §20). **Still
> to do (the visual shell, built on this projection and verified in the running editor, per Phase 9a →
> 9b):** the compact stack rows + stage sections, the focused inspector + resizable splitter, drag
> reorder, repeated-instance names, diagnostics badges, stack filtering, persisted splitter position,
> and (9b) swapping the inspector's control rendering onto `PropertySchema` (M2) with plugin/custom-stage
> rows; plus refactoring `properties.rs` (8.5k lines) into focused modules.

## Milestone 9 — Properties stack + focused Inspector redesign

> **Status — done (9a complete; 9b's editable plugin controls move to M10).** Following review against
> UE5 Niagara and Houdini, the layout became two dock panels rather than one split panel:
> - **Module Stack panel** (`ToolPanel::ModuleStack`, `properties/stack_panel.rs`): navigation only —
>   selectable Effect and Emitter items, then every fixed lifecycle section (EMITTER SPAWN shown even
>   when empty, §31.2), one section per authored simulation stage (§28.3), and RENDER. Each module or
>   renderer is one compact row: drag handle, title, descriptor-driven summary (`module_summary`),
>   diagnostics badge (count + worst severity + tooltip), enabled toggle and action menu. A filter box
>   narrows rows in place. Default layout docks it above Properties; `migrate_add_module_stack` adds it
>   to layouts saved before it existed.
> - **Properties panel** (`properties/inspector.rs`): purely the selected item's details — effect name;
>   emitter name/enabled/capacity/transform/timing/event links; a module's controls; a renderer's card;
>   plus an instance **Label** field. No always-on chrome (the dock tab, breadcrumb and status bar
>   already carry panel name, context and compile state).
> - **Drag reorder (§28.4)**: rows lift into a shadowed copy that follows the cursor from the grab
>   point; the rows it passes slide aside with eased `UiTransform` offsets (layout never changes
>   mid-drag); the drop commits from the tracked insertion point as one undoable `MoveModule`. Same-stage
>   only. Every from→to move is covered by tests through the session.
> - **Repeated instances (§28.5)**: optional authored `label` on `ModuleInstance`/`RendererInstance`
>   (omitted from files when unset; undoable `SetModuleLabel`/`SetRendererLabel`). Titles read
>   `Collision "Ground"`; unlabelled repeats are numbered `Motion #2` (`instance_title` in
>   `aestra-compiler/src/module_stack.rs`).
> - **Data foundation**: `EmitterStackProjection` / `ModuleStackRow` (with `title`) +
>   `project_emitter_stack`, engine-independent and tested.
> - **Missing plugins (§20)**: an unregistered module's preserved `Custom` payload renders read-only.
>
> **Moved to M10:** *editable* schema-driven controls for plugin modules — they need a real registered
> `PropertySchema`, which M10's example plugin provides. **Left for later:** further splitting of the
> legacy property-control code in `properties.rs` (the M9 panel code is already in its own modules), and
> keyboard reordering beyond the existing Move up/down actions.

### Goal

Replace the current vertically expanded card model with a professional scalable projection.

> **This milestone is large — treat it as two phases, not one change.** It bundles a UI shell
> (stack + inspector + splitter + drag/drop), a data-model integration (schema-driven rendering,
> repeated-instance names, custom stages), and a refactor of `properties.rs`. Do not land it as a
> single commit.
>
> - **Phase 9a — shell against today's model.** Build the compact stack + focused inspector +
>   resizable split + drag reorder over the *current* module/renderer model. This is independently
>   valuable (the all-cards panel is already painful at scale) and can begin as early as after M3,
>   before the schema work exists.
> - **Phase 9b — schema-driven inspector.** Swap the inspector's control rendering onto
>   `PropertySchema` (M2) and add custom-stage/plugin rows once M4–M8 land.
>
> Phase 9a needs only M3; Phase 9b needs M2 plus the descriptor milestones.

### Work

Build reusable Feathers/editor components:

```text
stack_group
stack_item_row
inspector_section
resizable_stack_inspector_split
```

Implement:

- compact stage groups;
- compact module rows;
- custom simulation-stage rows;
- renderer rows;
- drag reorder;
- repeated module instance names;
- summaries;
- stack filtering;
- diagnostics badges;
- selected-item inspector;
- schema-driven plugin properties;
- persisted splitter position;
- keyboard/context move actions.

Refactor large `properties.rs` responsibilities into focused modules.

### Acceptance

An emitter with:

```text
4 lifecycle sections
20+ modules
5 simulation stages
5 renderers
```

remains easy to navigate without a giant vertical property document.

Built-in and third-party items use the same visual language.

---

## Milestone 10 — linked plugin SDK vertical slice

> **Status — done.** The SDK lived in `aestra-compiler/src/extension.rs` (since extracted to `crates/aestra-extension`, see §24):
> - `AestraExtension` (a manifest naming its `ExtensionId`, plus `register(&mut ExtensionRegistry)`).
> - `ExtensionRegistry` now holds stage, domain and resource-type sub-registries and a
>   `LoweringRegistry` (`ModuleLowerer`, `StageLowerer`), next to modules, capabilities and renderers.
> - `ExtensionRegistry::install` runs a plugin's `register` against a copy of the registry and rejects
>   any id outside the plugin's `{plugin_id}::` namespace (§5, §9.1). It also rejects duplicates and
>   installing the same plugin twice. On any conflict the registry is left unchanged.
> - `link_extension` links a plugin into the process (idempotent). `ExtensionRegistry::linked()`,
>   and therefore `EffectCompiler::default()` and the editor's module catalog, include every linked
>   plugin. `ExtensionRegistry::builtin()` stays plugin-free.
>
> Authored data and validation:
> - Simulation stages now carry a registered `stage_type` (format v4 writes it only when it is not the
>   generic `aestra.stage.simulation`).
> - Module/stage compatibility looks up the host stage's registered descriptor. An unknown stage type
>   is an `UnknownStage` diagnostic.
> - Plugin module payloads are validated against their declared schema. Module metadata gains an
>   explicit `requires` capability expression and public builders (`ModuleMetadata::extension`,
>   `InputMetadata::new`).
>
> Lowering:
> - A plugin stage's enabled modules go through their `ModuleLowerer` into `ExtensionModulePlan`s,
>   with schema defaults filled in.
> - The `StageLowerer` then builds an `ExecutionBlock`, which must validate and use only registered
>   resource types; otherwise the result is a `LoweringFailed` diagnostic.
> - The results land on `CompiledEmitter.extension_stages` beside the lifecycle stages. No core enum
>   grows.
>
> The example crate `extensions/aestra-example-extension` (extension crates live under `extensions/`,
> apart from core `crates/`) depends only on the public SDK. It registers:
> - a capability, a field domain and a transient force-field resource;
> - the *Field Forces* stage, lowered to one field-building pass per module, a barrier, then an apply
>   pass;
> - the *Vortex* module (strength / radius / axis), edited by the editor's existing input controls;
> - the *Debug Points* extension renderer.
>
> The editor and viewer link it at startup, and `aestra-bevy` re-exports the SDK entry points for
> games. `sample-project/effects/plugin_lab.aestra.ron` (generated by `examples/gen_plugin_lab.rs`)
> uses it.
>
> Contract tests cover:
> - namespace governance;
> - lowering and the reference trace;
> - capability mismatch;
> - schema and lowerer rejection;
> - that a missing plugin loses no data;
> - linking;
> - an editor test for catalogue → edit → lower.
>
> **Not yet:** no runtime backend dispatches extension stages (the built-in particle path ignores them).
> (Extension stages have been encoded in compiled artifacts since artifact v4, host bindings HB2, and
> their Execution IR is validated on reload.) The editor cannot yet *create* a plugin simulation stage; it displays and edits
> existing ones. Stage authoring UI is still open.

### Goal

Prove the ecosystem API before dynamic loading.

### Work

Create a separate example plugin crate, not compiled inside core feature code.

Example:

```text
aestra-example-extension
```

It registers:

- one custom stage;
- one module;
- one resource/domain;
- one renderer or renderer mock;
- schema-driven properties;
- lowering.

Make both editor and viewer/game build able to include it through normal Rust linkage.

### Acceptance

The plugin uses only public extension contracts.

No modification to built-in stage/module/renderer match statements is required to add it.

---

## Milestone 11 — missing-plugin and versioning UX

> **Status — done.**
> - **Requirements (§21):** `EffectAsset.extensions: Vec<ExtensionRequirement { plugin, version }>`
>   (format v4 `extensions`, omitted when empty). Core validates them structurally only (§20.1);
>   `plugin_of` and `EffectAsset::referenced_plugins` derive plugin ids from namespaced type ids with
>   no registry. On save the editor records `ExtensionRegistry::derive_requirements`: `^version` of
>   each installed referenced plugin. A missing plugin's entry, or one the installed version does not
>   satisfy, is kept verbatim, so saving never hides a problem.
> - **Schema versions and migrations (§35):** `ModuleInstance`/`RendererInstance.schema_version`
>   (omitted when 1) and `ModuleMetadata.schema_version`. Plugins register per-type single-step
>   `PayloadMigration`s. `migrate_effect` chains them on a copy, so a failure leaves the payload as
>   authored. The compiler migrates a copy before validating. The editor migrates on open, and the
>   document opens dirty with a status message until saved. Newer payloads are never rewritten.
> - **Diagnostics:**
>   - `MissingExtension` is reported per unavailable module, stage and renderer, naming the plugin and
>     its recorded requirement.
>   - `IncompatibleExtension` covers an installed plugin that fails the requirement, a newer schema,
>     or a payload with no migration path.
>   - A malformed requirement is `InvalidValue`.
>   - A manifest version must be valid semver on install.
> - **Inspector (§28.7):**
>   - A plugin module shows "Provided by {name} {version}".
>   - A missing one shows the MISSING EXTENSION block (type, required plugin + requirement, "data is
>     preserved") over its read-only values.
>   - A schema the plugin cannot read shows the incompatible-data block.
> - **Example plugin:** Vortex is at schema v2 (v1's `speed` became `strength`), with its migration.
> - **Acceptance:** `missing_plugin_contract.rs` covers the full remove-plugin / reopen / edit
>   unrelated fields / save / reinstall cycle. Editor tests cover the extension-state classification
>   and open-migrates / save-records.
>
> **Not yet:**
> - Project dependency report integration. `aestra-project` does not surface plugin requirements;
>   they are only reported per effect by the compiler.
> - Renderer rows have no missing-plugin inspector block (their diagnostics are reported).
> - No "Locate / Install Plugin" action: installation is linking a crate until M12's packaged host.

### Goal

Make third-party assets safe to exchange.

### Work

- add `ExtensionRequirement`;
- preserve missing type payloads;
- missing-plugin diagnostics;
- incompatible-version diagnostics;
- schema migrations;
- newer-schema preservation;
- plugin provider information in Inspector;
- project dependency report integration.

### Acceptance

Create an effect with the example plugin, remove the plugin, reopen the project:

- effect still loads;
- unknown data remains;
- unrelated fields can be edited/saved;
- compiler explains exactly what is unavailable;
- reinstalling the plugin restores editing/compilation.

---

## Milestone 12 — packaged plugin manifest and host

> **Status — done, declaratively (the code-execution boundary is deferred by decision).**
>
> Packages are **declarative**. No package code runs on the host, so this milestone needed no
> sandbox and adds no new dependency. The choice between a WASM component, process/IPC and stable
> FFI waits until a package needs logic that data cannot express.
>
> A package is a folder holding two RON files (RON, like every other Aestra asset):
> - `extension.ron`, the manifest: id, name, version, `aestra_api` range, description, dependencies
>   (id → version requirement), requested permissions, and the content file.
> - `content.ron`, the declarations:
>   - capabilities, domains and resources;
>   - stages with an optional **Execution IR template**: `Compute` / `Barrier` / `Copy` / `Repeat` /
>     `ForEachModule`, with `{stage}`/`{module}` substitution and `Fixed` or per-`Particles` dispatch;
>   - modules with property descriptors, schema version, entry point and **declarative migrations**
>     (`Rename` / `Remove` / `Insert`);
>   - renderers with schemas.
>
> The host was built as `crates/aestra-extension-host` and now lives in `aestra_extension::host` (§24):
> - `DeclarativeExtension` turns a package into an ordinary `AestraExtension`, so the compiler and
>   editor treat it exactly like a linked one. Descriptors are prepared once per load.
> - `load_packages`/`link_packages` **discover** packages (immediate subfolders with `extension.ron`).
> - They **version-check** each package: the host implements `EXTENSION_API_VERSION` (0.1.0), and
>   dependency ranges are checked against linked or packaged extensions, including cascading failures
>   and cycles.
> - They **register** packages in dependency order. Namespace, duplicate and unregistered-capability
>   problems are rejected.
> - They honour a **disabled** set.
> - They **diagnose** every package with a `PackageStatus`: `describe()` explains it; problems are
>   logged at startup and listed in the settings UI.
>
> The editor loads packages from the project's `extensions/` folder and `<config>/extensions`. A new
> **Settings → Extensions** page lists built-in extensions and each package with its id, status,
> requested permissions, location and an enable toggle; disabled ids persist in settings and apply at
> the next start. The viewer loads the effect's project packages.
>
> The sample package `sample-project/extensions/org.example.aestra-wind`:
> - depends on the linked example extension;
> - adds a Wind stage (template-lowered), a Gust module (schema v2 with a v1 migration) that also runs
>   in the linked Field Forces stage, and a streak renderer.
>
> **Acceptance:** `host_contract.rs` covers discovery, API and dependency checks (missing, incompatible,
> disabled, cycle), registration including cross-extension hosting, disabling (its effects then report
> `MissingExtension`), namespace, duplicate and linked-id rejection, invalid content, and declarative
> migration.
>
> **Not yet:**
> - The editor loads packages only for the project open at startup; switching projects does not
>   reload them.
> - Toggles need a restart; there is no unlinking at runtime.
> - WESL compute programs are not packaged yet. Templates name entry points, and nothing executes
>   extension stages yet.
> - Permissions are recorded only.
> - No lockfile or signatures (M14).

### Goal

Separate plugin installation from rebuilding Aestra.

### Work

Define:

```text
plugin.toml
package layout
API compatibility contract
dependency resolution
permissions
plugin discovery
```

Implement an initial packaged authoring/compiler host.

Choose the code-execution boundary only after evaluating:

- WASM component model;
- process/IPC isolation;
- stable FFI.

Do not use Rust `dylib` trait objects as the permanent compatibility promise.

### Acceptance

An externally installed example extension can be discovered, version checked, registered, disabled, and diagnosed without modifying Aestra source.

---

## Milestone 13 — real fluid plugin proof

> **Status — 13a done: plugin, lowering and native GPU execution, proven outside the frame loop.
> 13b (running it in the Bevy frame loop, presentation, editor) is next.**
>
> **SDK and IR additions**, all generic, with nothing fluid-specific in core:
> - `ComputeProgramId` names a registered `ComputeProgram` (WGSL source plus declared entry
>   points). `ComputeOp.program` references it (§13.2). Programs are namespaced like every other
>   contribution. The compiler rejects an op that names an unregistered program or an undeclared entry.
> - **Binding convention:** the `i`-th declared resource of a block is `@group(0) @binding(i)`. An
>   op's `accesses` must be exactly what its entry uses.
>   `aestra_gpu::check_program_block` proves this from the WGSL with naga, no GPU needed, and also
>   rejects a write to a resource declared read-only. The hazard information the IR trusts is
>   therefore checked, not assumed.
> - Two host-written built-in resources:
>   - `aestra.resource.stage_constants` holds `ExecutionBlock.constants`, which a lowerer packs
>     from module parameters.
>   - `aestra.resource.frame` holds `FrameConstants` (tick, dt, time, seed) and is written every
>     tick.
>   - `validate()` enforces their size and lifetime.
> - `StageTypeDescriptor.backend: BackendSupport`, with `gpu_only(...)` for stages like the fluid.
>   `CompiledExtensionStage.cpu_reference` records it, and it survives the artifact round trip.
>   The example Field Forces stage and declarative template stages now declare GPU-only too,
>   truthfully.
> - `CompiledHostFieldRef.field_index`: the presence bit a GPU program needs to tell an absent
>   optional host field from a present one.
> - Artifact v4 gains `program`, `constants` and `cpu_reference` as additive serde-default fields.
>
> **Executor:** `aestra_bevy_render::execution::StageExecutor` promotes the M7 harness into a
> library that runs on a plain `wgpu` device:
> - It allocates the declared resources (bounded), uploads constants once, and uploads the frame
>   and host bindings each tick.
> - It **zeroes transient resources every tick**, so restore plus replay is exact.
> - It runs each compute op as its own pass, loops `Repeat`, and copies `Copy`.
> - It takes checkpoints and restores them. A checkpoint holds only persistent stage-owned
>   resources.
>
> **The plugin:** `extensions/aestra-fluid` (`org.example.aestra-fluid`) is linked-extension code
> that depends only on the SDK, the IR and the host-binding ABI. It registers:
> - the GPU-only stage *Fluid Solver*;
> - the domain `grid3d`;
> - resources: persistent velocity and density; transient scratch, pressure, divergence and
>   vorticity;
> - four schema-driven modules:
>   - *Fluid Grid*, one per stage: resolution 8–96 (a multiple of 4), cell size, centre, pressure
>     iterations, dissipation;
>   - *Density Source*: position and velocity can be driven by a host binding;
>   - *Buoyancy*;
>   - *Vorticity* (confinement).
> - one `solver.wgsl` program.
>
> One authored stage lowers to:
>
> ```text
> add_sources → [vorticity → confine] → advect velocity → divergence → repeat N {relax, copy}
> → project → advect density
> ```
>
> Barriers separate the steps. Every pass is a gather (no atomics). The pressure solve uses the wide
> Laplacian, which is the D·G operator the collocated central-difference projection applies. The
> compact 7-point stencil was tried first. It left about 64% of the divergence, whatever the
> iteration count.
>
> **Acceptance proven** (`fluid_contract.rs` without a GPU; `fluid_gpu.rs` on an RTX 4070 SUPER
> with `AESTRA_REQUIRE_GPU_CONFORMANCE=1`):
> - no `Fluid` in core and no fluid branch in the Properties panel;
> - lowers through the shared Execution IR, with checked accesses;
> - diagnosed (`MissingExtension`) when absent, with data preserved;
> - `cpu_reference = Unavailable`, declared and carried to the compiled stage;
> - **same asset + seed + frames → the same bits** across two independent executions;
> - checkpoint (only velocity and density) plus restore/replay reaches the uninterrupted state;
> - smoke rises;
> - projection cuts interior divergence about 40×;
> - a source bound to a host object follows it (density centroid x ≈ ±1.0 m for targets at ±1.0 m;
>   0 when unbound);
> - the compiled solver round-trips through the artifact.
>
> **Not yet (13b):**
> - The executor does not run in the Bevy frame loop, and nothing presents the grid (debug slice,
>   volume renderer).
> - No particle coupling (grid → particle advection, particle → grid deposit / FLIP).
> - The editor does not link the plugin or surface "GPU only".
> - No sample-project effect.
> - Declarative packages cannot ship programs yet.

### Goal

Use fluids as the architecture stress test, not as hardcoded core functionality.

> **This is Aestra's fluid, period.** Per §44.6, there is no first-party CPU-reference fluid; the
> hybrid roadmap's first-party fluid milestone is superseded by this one. This milestone depends on
> the hybrid roadmap's *generic staged infrastructure* (its M13) and the shared execution IR (§44.1).
> The plugin is GPU-only and declares `cpu_reference = Unavailable`; its determinism rests on fixed
> pass ordering + seed, verified GPU-vs-GPU, not against a CPU reference.

### Work

Build an external experimental plugin implementing a small solver, for example:

```text
grid allocation
particle → grid deposit
pressure iterations
advection
grid → particle transfer
```

Expose a compact semantic stage rather than every implementation pass.

Optional:

```text
Vorticity module
Buoyancy module
Volume renderer
```

### Acceptance

- no `Fluid` enum variant is added to Aestra core;
- no fluid-specific branch is added to the generic Properties panel;
- plugin lowers through the shared public Execution IR (§44.1);
- project can diagnose the plugin when absent;
- native GPU execution works, and re-running the same asset + seed reproduces the same frames
  (GPU-vs-GPU determinism);
- `cpu_reference = Unavailable` is declared truthfully — no faked CPU fluid path.

---

## Milestone 14 — plugin hardening and ecosystem readiness

### Goal

Prepare for community distribution.

### Work

- plugin developer documentation;
- API compatibility policy;
- version/lockfile strategy;
- package signatures/hashes;
- permissions UX;
- crash/failure isolation;
- compile timeout/resource limits;
- marketplace metadata hooks;
- extension diagnostics;
- plugin conformance test suite;
- sample templates;
- CI matrix against supported Aestra API versions.

### Acceptance

A third-party author can create and test an extension without editing Aestra internals.

---

# 37. Suggested implementation order summary

```text
S0 Shared baseline          (aestra_shared_foundation_milestones.md)
        ↓
S1 Shared identity + semantics + one ModuleMetadata extension
        ↓
2  Property schema / dynamic payload
        ↓
3  Explicit StageInstance + format v4
        ↓
4  Capabilities and compatibility
        ↓
5  Generic compiled stages
        ↓
6  Execution IR/resources
        ↓
7  GPU generic scheduler
        ↓
8  Renderer extension model
        ↓
9  Properties redesign
        ↓
10 Linked plugin proof
        ↓
11 Missing-plugin/versioning
        ↓
12 Packaged plugin host
        ↓
13 External fluid proof
        ↓
14 Ecosystem hardening
```

The Properties shell (Phase 9a) can start as early as after Milestone 3, against today's model;
its schema-driven phase (9b) waits on Milestone 2 plus the descriptor milestones. Milestone 8 can
overlap Milestone 9 once Milestone 5 lands.

All authored-format shape changes land in the single v4 cut at Milestone 3; Milestones 4–8 add
behavior behind that shape without a further format bump.

Dynamic plugin loading should **not** block the semantic/runtime redesign.

---

# 38. What should remain in Aestra core

Core should own contracts that every extension must obey:

- Effect/Emitter identity and lifecycle;
- lifecycle roles;
- semantic ID rules;
- stage/module/renderer container structure;
- deterministic asset serialization envelope;
- extension requirement representation;
- capability/resource vocabulary mechanics;
- execution-plan validity;
- synchronization semantics;
- artifact format;
- diagnostics;
- undoable semantic addressing;
- basic security/permissions model.

Core should **not** own:

- fluid physics;
- cloth algorithms;
- custom community stage taxonomy;
- plugin-specific grids;
- plugin-specific renderer meanings;
- plugin-specific property UI implementations.

---

# 39. What plugins may own

Depending on their declared extension class:

- stage types;
- module types;
- renderer types;
- property schemas;
- presets;
- domains;
- resource types;
- portable shader programs;
- compilation/lowering rules;
- CPU/reference evaluators;
- localization;
- icons;
- optional custom inspector projection;
- explicit native runtime integration when portability is impossible.

---

# 40. Non-goals for the first implementation

Do **not** attempt immediately:

- arbitrary native code loading from unknown plugins;
- a stable Rust dynamic-library ABI;
- unrestricted custom editor widgets;
- arbitrary plugin-defined lifecycle replacement;
- graph-only particle authoring;
- real production fluid simulation;
- cross-plugin direct Rust dependencies as the main composition model;
- a fully general task-graph scheduler before a real use case requires it;
- a general **WESL-on-CPU interpreter** (§13.3): built-ins keep their typed CPU instruction plan;
  community compute stages declare `cpu_reference = Unavailable` or ship a separate evaluator. Nothing
  in this plan depends on interpreting plugin GPU programs on the CPU.

The first objective is a sound semantic/compiler/runtime boundary.

---

# 41. Decisions to keep explicit

## Decision A — Is `Stage` generic?

**Yes.**

Lifecycle and custom simulation stages use the same stage infrastructure.

## Decision B — Are lifecycle stages arbitrary?

**No.**

Core lifecycle roles are fixed structural slots.

## Decision C — Can a stage contain multiple modules of the same type?

**Yes by default.**

A module may declare singleton-per-stage when required.

## Decision D — Are renderers stages?

**No.**

They are fan-out consumers with their own registry and lowering path.

## Decision E — Does one stage equal one GPU pass?

**No.**

A stage lowers to an execution plan containing one or many operations.

## Decision F — Does core know about fluids?

**No.**

Fluid-specific semantics live in an extension.

## Decision G — Should the normal plugin inspector be custom UI?

**No.**

Use schema-driven native Aestra controls by default.

## Decision H — Must an effect survive a missing plugin?

**Yes.**

Unknown extension data must round-trip losslessly.

## Decision I — Should built-ins bypass the plugin registry?

**No.**

Built-ins should use the same registration contracts.

## Decision J — Should plugin support initially require dynamic loading?

**No.**

Prove the contracts with linked extensions first.

## Decision K — Where do the effect-level lifecycle roles go?

**Effect scope.**

`EffectSpawn`/`EffectUpdate` are owned by an `EffectLifecycleStages` on the effect, not the emitter,
and migrate into it explicitly. `LifecycleRole` keeps all six variants (§4.2, §6.2).

## Decision L — Is a renderer's material binding structural or payload?

**Structural.**

`material` (and any asset reference) stays a first-class field so it survives a missing renderer
plugin and stays queryable by thumbnails, material drafts, and validation (§16).

## Decision M — Does declaring `cpu_reference` derive a CPU path from GPU programs?

**No.**

Aestra does not interpret plugin WESL on the CPU. Built-ins keep a typed CPU plan; a plugin either
declares `Unavailable` or supplies a separate, conformance-tested CPU evaluator (§13.3).

## Decision N — May capabilities influence execution order?

**No.**

Capabilities decide compatibility only. Order is authored, so lowering is reproducible regardless of
the installed plugin set (§34).

## Decision O — How many authored-format migrations does this redesign need?

**One.**

Every authored shape change lands in the single v3→v4 cut at Milestone 3. Later milestones add
behavior behind that shape without another format bump.

---

# 42. End-state architecture

```text
                      AUTHORED EFFECT
                            │
          ┌─────────────────┴─────────────────┐
          │                                   │
    Core lifecycle                     Extension stages
          │                                   │
   StageInstance                         StageInstance
          │                                   │
      module stack                    modules or opaque config
          │                                   │
          └─────────────────┬─────────────────┘
                            │
                    Extension Registry
        ┌───────────────────┼────────────────────┐
        │                   │                    │
    Stage types         Module types        Renderer types
        │                   │                    │
        └───────────────────┴──────────────┬─────┘
                                           │
                                      Compiler
                                           │
                                 Portable Execution IR
                                           │
                    ┌──────────────────────┼─────────────────────┐
                    │                      │                     │
              CPU reference           Aestra GPU           artifact DTO
                    │                      │                     │
                    │                      │                     │
                    └──────────────┬───────┴──────────────┬─────┘
                                   │                      │
                              editor/viewer          game runtime
                                   │
                              engine adapters
```

The editor is simply another projection over the same semantic model:

```text
Compact stack
    ↓ selection
Focused Inspector
```

while a future Effect Graph can visualize relationships without replacing ordered stage/module execution.

---

# 43. Final recommendation

Aestra is already close to an extensible architecture in several places: it has string type IDs, a module registry, custom parameter escape hatches, a separated compiler/runtime/artifact pipeline, and an engine-neutral semantic model.

The next step should **not** be to add more cases to `StageKind`, `Instruction`, or `RendererPlanKind` every time a feature appears.

Instead, use the next format/runtime evolution to establish the permanent extension boundary:

> **Fixed Aestra lifecycle + generic stage instances + registry-driven types + schema-driven authoring + portable lowering.**

That gives simple particle effects an understandable Niagara-like execution stack while allowing community authors to implement substantially different simulation systems—fluids included—without turning Aestra core into a catalogue of every VFX technique that may ever exist.

---

# 44. Relationship to the hybrid simulation roadmap

This plan does not stand alone. `aestra_hybrid_simulation_architecture_roadmap.md` proposes a parallel
evolution — analytic / stateful / staged **simulation classes**, compiler-derived **simulation
islands**, GPU checkpoints, and a generic multi-pass simulation plan. The two plans touch the same
crates, the same formats, and several of the same primitives. **They must share those primitives, or
Aestra ends up building two of everything and migrating its formats twice.** This section is the
binding contract between the two.

## 44.1 One portable execution IR, not two

The "portable Execution IR" here (§13: `ExecutionBlock`, `ExecutionOp`, `ComputeOp`,
`ResourceDescriptor`, `ResourceAccess`, `RepeatPolicy`) and the hybrid roadmap's "generic staged
simulation plan" (its §27: `SimulationPassPlan`, `SimulationResource`, `DispatchPlan`,
`IterationPlan`) are **the same IR**. They even use the same fluid example. There is exactly one
multi-pass compute IR in Aestra, with this correspondence:

```text
ExecutionBlock.repeat   ≡  IterationPlan
ComputeOp.dispatch      ≡  DispatchPlan
ResourceAccess          ≡  SimulationResource read/write
Barrier/Copy ops        ≡  the staged pass barriers/ping-pong
```

Whichever plan lands the IR first defines the types; the other consumes them. The staged-simulation
backend the hybrid roadmap needs **is** the lowering target an opaque plugin stage (§7, §13) already
produces. Do not implement `SimulationPassPlan` and `ExecutionBlock` as separate systems.

## 44.2 Islands and stages are orthogonal — define how they compose

`CompiledStage` (§14) and the hybrid roadmap's `CompiledSimulationIsland` are **different axes**, and
both currently claim "the compiled execution plan." Reconcile them explicitly:

- a **stage** is a lifecycle/composition unit *within* an emitter (owns modules, lowers to execution
  blocks);
- an **island** is a temporal/scheduling grouping that may *span* emitters and carries the resolved
  `SimulationClass` / `TemporalSemantics` and the checkpoint/seek strategy.

Composition: **islands are the scheduling grouping; each island references the compiled stages it
executes; a stage's simulation class is derived, and an island's class is the maximum over its
stages** (Analytic < Stateful < Staged). `CompiledEmitter` is restructured *once* to hold both — do
not let the two plans reshape it independently.

## 44.3 Simulation class is derived, never authored

The hybrid roadmap is emphatic (its §4.9, §11, §35): `SimulationClass` and `TemporalSemantics` are
**compiler-derived** from module requirements, not authored, and are **not** the same as
`SimulationDomain`. This plan must respect that:

- `DomainTypeId` (§10) is the hybrid roadmap's `SimulationDomain` (topology) — a registered authored
  identity;
- `SimulationClass` is derived and lives on the compiled island, never in the authored file and never
  as a capability;
- the module-requirement metadata this plan adds (capabilities, reads/writes — §8.3, §9) and the
  hybrid roadmap's `SimulationRequirements { temporal, synchronization, neighborhood }` (its §11) are
  **one metadata extension on `ModuleMetadata`, co-designed**, not two parallel requirement systems.
  A module's `synchronization = Iterative` / `neighborhood = Grid` is precisely the signal that makes
  its stage opaque and multi-pass here.

## 44.4 One backend-capability model

`BackendSupport { cpu_reference, gpu_compute }` (§13.3) and the hybrid roadmap's
`BackendSimulationCapabilities { checkpoints, staged_dispatch, … }` describe the same thing from two
angles (what a unit needs vs. what a backend offers). Merge them into one capability model that a
descriptor declares and a backend answers, so classification (this plan) and seek-strategy resolution
(hybrid roadmap: `Direct/HistoryDependent + checkpoints → SimulationSeekMode`) read the same data.

## 44.5 Coordinate the format bumps — authored once; compiled cheaply, ideally coordinated

There are two distinct formats, and they are **not** equally expensive to change:

- **Authored format v3 → v4 — bumps exactly once.** Owned by this plan (§31, M3): stage containment,
  effect lifecycle, generic renderer shape, `DomainTypeId`. The hybrid roadmap adds *no* authored
  fields (simulation class is derived), so it never touches the authored version. This is the
  expensive migration — real files, hand-authored — so it stays a single cut.
- **Compiled artifact v2 → v3 (→ v4) — regenerated, so re-bumping is cheap.** The compiled artifact
  is *derived* from source by the compiler; bumping it means recompiling, not migrating hand-authored
  data. Coordinate it where practical — encode this plan's generic stages/execution IR and the hybrid
  roadmap's islands/state-layouts/pass-plans in one version if both land in the same cycle. But do
  **not** block one track on the other merely to co-bump: in the unified sequence the hybrid track
  bumps the compiled artifact for islands (U3) well before generic stages land (U15/U16), and forcing
  a single bump would stall all of stateful/collision behind the extensible refactor — defeating the
  hybrid-first lead. A second compiled-artifact bump is an accepted, low-cost recompile.

The rule of thumb: **one authored bump is a hard constraint; one compiled bump is a preference.**
The hybrid roadmap's §29 and §30 here refer to the compiled artifact under this relaxed rule.

## 44.6 Fluids: GPU-only plugin, no first-party CPU fluid (decided)

The two plans disagreed on whether the reference fluid is first-party or a plugin. **Decision:
fluids are a GPU-only plugin from the start** (this plan's M13), and there is **no first-party
CPU-reference fluid**. Consequences, binding on both documents:

- the hybrid roadmap's first-party fluid milestone (its M14) is **superseded** by the plugin fluid
  here (M13); the hybrid roadmap keeps only the *generic staged infrastructure* (its M13), which is
  first-party;
- staged / compute plugin stages declare `cpu_reference = Unavailable` (§13.3, §40). There is no
  WESL-on-CPU interpreter and no CPU-vs-GPU fluid conformance;
- **determinism for staged stages therefore rests on fixed GPU pass ordering + seed and GPU-vs-GPU
  reproducibility**, not CPU reference. The staged infrastructure's first-party validation workload
  (the hybrid roadmap suggested 2D diffusion / reaction-diffusion) is a GPU-only determinism fixture,
  not a CPU reference;
- built-in **particle lifecycle** stages are unaffected: they keep their typed CPU instruction plan
  and remain the CPU-reference yardstick for analytic/stateful particles.

This buys a smaller core at the cost of a weaker determinism/testing story for staged simulation —
an accepted trade. Revisit only if staged effects later need CPU-reproducible fixtures.

## 44.7 One milestone spine

Both plans open with the same early milestones — baseline fixtures, then a semantics/identity pass.
These are merged into one concrete front end in **`aestra_shared_foundation_milestones.md`**
(milestones **S0** and **S1**), which supersedes this plan's M0/M1 and the hybrid roadmap's M0/M1:

```text
S0  baseline: combined correctness fixtures (this plan) + performance baselines (hybrid)
S1  identity + derived semantics + ONE ModuleMetadata extension:
        this plan: ExtensionId/StageTypeId/.../CapabilityId + ExtensionRegistry
        hybrid:    SimulationClass/TemporalSemantics (derived)
        merged:    capabilities/reads/writes/multiplicity co-designed with
                   temporal/synchronization/neighborhood (§44.3)
                   + agreed backend-capability shape (§44.4)
        │
        ├─────────────────────────────┐
        ▼                             ▼
  EXTENSIBLE track                HYBRID track (recommended lead)
  M2–M3 property schema +          compiler simulation islands →
  authored v4 (stages, effect       stateful CPU/GPU → checkpoints →
  lifecycle, renderer shape,        collision …
  domains) → IR → GPU → …
        └──────────────┬─────────────┘
                       ▼
     converge at staged simulation:
     one execution IR (§44.1), one resource model,
     one compiled-artifact v3 bump (§44.5), fluids as a GPU-only plugin (§44.6)
```

The Properties redesign (M9) and the hybrid roadmap's editor work (its M12 preview/exact seek, M16
profiler) both project the same compiled model; the derived `SimulationClass` per island (its §28
profiler view) is one more thing the focused Inspector (§28) surfaces.

> **Bottom line:** keep the two plans' *goals* separate, but share exactly five things — the
> execution IR, the resource model, the backend-capability model, the `ModuleMetadata` requirement
> extension, and the compiled-artifact version. Everything else can proceed on its own track.
