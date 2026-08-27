# Aestra — AI-Native VFX Editor Plan

## Purpose

Aestra should not be designed as only a traditional Niagara-like node editor.

The long-term direction should be an **AI-native VFX authoring environment** where humans, the visual editor, scripts, procedural generators, and AI agents all manipulate the same deterministic semantic effect model.

The node graph remains important, but it should become the **precise, inspectable, editable representation of an effect**, rather than the only primary authoring workflow.

```text
             Human
            /     \
          AI       Editor
            \     /
          Effect Model
               |
            Compiler
               |
             GPU
```

The editor and AI should be equal clients of the same underlying model.


# 1. Product Direction

## Traditional workflow

```text
Idea
 |
Create system
 |
Create emitters
 |
Add modules
 |
Wire graph
 |
Tune parameters
 |
Preview
 |
Repeat
```

## Target Aestra workflow

```text
                    +----------------+
                    | User intent    |
                    | text / image / |
                    | video / scene  |
                    +-------+--------+
                            |
                            v
                  +-------------------+
                  | Aestra AI Agent   |
                  +---------+---------+
                            |
                            v
            +--------------------------------+
            | Semantic Effect Model          |
            +---------------+----------------+
                            |
                            v
                 +---------------------+
                 | Aestra Effect Graph |
                 +---------------------+
                 | Emitters            |
                 | Modules             |
                 | Curves              |
                 | Materials           |
                 | Events              |
                 | Parameters          |
                 | Renderers           |
                 +----------+----------+
                            |
                            v
                     GPU Compiler
                            |
                            v
                          Bevy
```

Example request:

> Create a blue spectral projectile. It should form from the caster's hand, accelerate quickly, leave a wispy trail, and explode into fragments on impact. Keep it below roughly 300 particles and suitable for Steam Deck.

Aestra should produce an ordinary editable effect:

```text
Spectral Projectile
|
+-- Core
|   +-- Mesh emitter
|   +-- Fresnel material
|   +-- Pulsating emissive
|
+-- Trail
|   +-- Ribbon emitter
|   +-- Curl noise
|   +-- Fade over lifetime
|   +-- Width curve
|
+-- Wisps
|   +-- 40 particles/sec
|   +-- Sphere spawn
|   +-- Vortex force
|   +-- Dissolve
|
+-- Impact
    +-- Burst: 120
    +-- Cone velocity
    +-- Drag
    +-- Flash
```

The result must not be an opaque generated shader or unstructured blob.


# 2. Core Architectural Principle

## The UI must not be the model

Do **not** make visual-node placement or editor state the canonical representation of an effect.

Bad conceptual model:

```text
Node #47
position = (631, 287)
connected_to = Node #62
```

Preferred conceptual model:

```text
Effect
+-- systems
+-- emitters
+-- modules
+-- parameters
+-- expressions
+-- curves
+-- events
+-- renderers
```

The visual graph should be a projection of the semantic model. Node positions, grouping, comments, zoom state, and panel state are editor metadata.


# 3. Proposed Crate Architecture

```text
aestra
aestra-core
aestra-graph
aestra-compiler
aestra-runtime
aestra-bevy
aestra-editor
aestra-ai
```

## `aestra-core`

Own the canonical semantic VFX model:

- effects
- systems
- emitters
- stages
- modules
- parameters
- curves
- events
- renderers
- typed values
- IDs and references

```rust
pub struct Effect {
    pub id: EffectId,
    pub name: String,
    pub systems: Vec<System>,
}

pub struct Emitter {
    pub id: EmitterId,
    pub name: String,
    pub stages: Vec<Stage>,
    pub renderer: RendererConfig,
}

pub struct Module {
    pub id: ModuleId,
    pub kind: ModuleKind,
    pub stage: StageKind,
    pub inputs: Vec<InputBinding>,
    pub outputs: Vec<OutputBinding>,
}
```

## `aestra-graph`

Responsibilities:

- connections
- dependency resolution
- validation
- stage ordering
- cycle detection
- expression graphs
- type compatibility
- traversal
- semantic diff helpers

## `aestra-compiler`

Conceptual pipeline:

```text
Aestra Effect
     |
Semantic validation
     |
Graph normalization
     |
Intermediate representation
     |
Optimization
     |
WESL / pipeline generation
     |
Runtime artifact
```

Potential passes:

- dead-node elimination
- constant folding
- shader specialization
- resource layout generation
- platform capability checks

## `aestra-runtime`

Goals:

- minimal allocations
- no editor dependencies
- compact runtime representation
- efficient parameter updates
- suitable for many simultaneous effects

## `aestra-bevy`

Responsibilities:

- ECS components
- systems
- assets
- render-world integration
- GPU resource management
- effect spawning and lifetime
- transforms
- events
- scene integration

```rust
commands.spawn((
    AestraEffect::new("spectral_bolt"),
    Transform::default(),
));
```

## `aestra-editor`

Human-facing authoring application. It should be a client of the semantic model and command system.

## `aestra-ai`

Optional later layer. It must not own the effect format. It translates intent into structured commands and queries Aestra's APIs.


# 4. Command-Based Editing Model

All meaningful effect mutations should be explicit commands.

```rust
pub enum EffectCommand {
    AddSystem(AddSystemCommand),
    RemoveSystem(RemoveSystemCommand),

    AddEmitter(AddEmitterCommand),
    RemoveEmitter(RemoveEmitterCommand),
    DuplicateEmitter(DuplicateEmitterCommand),

    AddModule(AddModuleCommand),
    RemoveModule(RemoveModuleCommand),
    MoveModule(MoveModuleCommand),

    Connect(ConnectCommand),
    Disconnect(DisconnectCommand),

    SetParameter(SetParameterCommand),
    ResetParameter(ResetParameterCommand),

    AddCurve(AddCurveCommand),
    SetCurvePoint(SetCurvePointCommand),
    RemoveCurvePoint(RemoveCurvePointCommand),

    AddEvent(AddEventCommand),
    RemoveEvent(RemoveEventCommand),

    SetRenderer(SetRendererCommand),
}
```

Infrastructure should support:

```text
apply
undo
redo
validate
serialize
diff
```

This one decision enables:

- editor undo/redo
- AI editing
- scripting
- macros
- collaborative editing
- command recording
- deterministic transformations


# 5. Transactions

AI changes should not silently mutate a project.

```rust
EffectTransaction {
    commands: Vec<EffectCommand>,
    description: "Make the trail 30% longer",
}
```

Workflow:

```text
User request
     |
AI proposes transaction
     |
Validate
     |
Preview
     |
Show semantic diff
     |
Accept / reject
     |
Commit transaction
```

Transactions provide trust and make AI suitable for professional workflows.


# 6. Stable Semantic IDs

Use stable IDs for addressable objects:

```rust
EffectId
SystemId
EmitterId
ModuleId
ParameterId
CurveId
RendererId
EventId
```

Do not rely on vector indexes or screen-node IDs.

Stable IDs are required for:

- AI references
- undo/redo
- diffs
- collaboration
- migrations
- refactoring
- serialization


# 7. Module Metadata and Registry

Every module should expose rich metadata.

```rust
pub struct ModuleMetadata {
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub stages: &'static [StageKind],
    pub affects: &'static [ParticleAttribute],
    pub useful_for: &'static [&'static str],
    pub cost: CostClass,
    pub inputs: &'static [InputMetadata],
    pub outputs: &'static [OutputMetadata],
}
```

Example:

```text
Name: Curl Noise
Category: Forces
Description: Applies divergence-free turbulent motion to particles.
Stages: Particle Update
Affects: Velocity
Useful for: smoke, magic, fire, wisps, turbulence
Cost: Medium
```

The module registry should support queries such as:

```text
all modules
modules available during ParticleUpdate
modules affecting velocity
modules suitable for smoke
low-cost noise modules
mobile-compatible renderers
```

Do not require an LLM to memorize Aestra's API.


# 8. Tool-Oriented AI Interface

Eventually expose structured operations:

```text
inspect_effect()
inspect_selection()

find_modules()
get_module_metadata()

create_effect()
create_system()

create_emitter()
duplicate_emitter()
remove_emitter()

add_module()
remove_module()
move_module()

connect()
disconnect()

get_parameter()
set_parameter()

create_curve()
set_curve_point()

create_event()
set_renderer()

validate_effect()
profile_effect()
compile_effect()
preview_effect()

begin_transaction()
commit_transaction()
rollback_transaction()
```

The AI should use these operations instead of regenerating full serialized effect documents.


# 9. Conversational Editing

Examples:

> Make the trail approximately twice as long but do not increase particle count.

Possible transaction:

```text
Trail / Lifetime
0.45 -> 0.90

Trail / Spawn Rate
unchanged

Trail / Ribbon Sampling
adjusted
```

> The explosion is too spherical. Make it mostly forward-facing.

Possible semantic transformation:

```text
RadialVelocity
    ->
ConeVelocity

angle = 55 degrees
direction = impact direction / reflected velocity
```

> I like the trail. Do not touch it anymore. Make the impact more magical.

Aestra should understand current selection, scope, lock state, and effect context.


# 10. Three Authoring Levels

## Intent level

Examples:

```text
Make the sparks larger.
Give this more impact.
Make it feel icy instead of fiery.
Optimize this for mobile.
Create an impact effect matching this projectile.
```

## Structured effect level

For artists:

```text
Spectral Bolt
|
+-- Core
+-- Trail
+-- Wisps
+-- Impact
```

Users manipulate emitters, properties, curves, timing, materials, and event relationships without opening low-level graphs constantly.

## Graph/code level

For technical artists:

```text
position
   |
curl_noise
   |
velocity
   |
integrate
```

Advanced support can include:

- expression graphs
- custom modules
- WESL
- generated WESL inspection
- shader debugging

AI should help users move between these abstraction levels rather than hiding the lower levels.


# 11. Editor UX Direction

Avoid making the giant node graph the entire application.

```text
+----------------------------------------------------------------+
| Play        Spectral Bolt            GPU 0.18 ms     Desktop   |
+----------------+-----------------------------+-----------------+
| EFFECT         |                             | PROPERTIES      |
|                |                             |                 |
| Core           |                             | Trail           |
| Trail     *    |       LIVE PREVIEW          |                 |
| Wisps          |                             | Lifetime  0.8s  |
| Impact         |                             | Width     0.3   |
|                |                             |                 |
+----------------+-----------------------------+-----------------+
| Ask Aestra                                                     |
| "Make the trail softer and approximately 30% longer..."        |
+----------------------------------------------------------------+
| Graph | Timeline | Curves | Profiler | Diagnostics | WESL      |
+----------------------------------------------------------------+
```

The live preview should remain visually dominant.

The AI interface should preferably behave as a context-aware command interface, not necessarily a permanent chatbot panel.

Possible contextual actions:

```text
Explain this module
Simplify this section
Optimize selected emitter
Create variation
Make this effect more stylized
Match another effect
Fix validation errors
```


# 12. Selection, Scope, and Locks

Explicit AI/edit scopes:

```text
Current property
Current module
Current emitter
Current system
Current effect
Whole project
```

Selection should use semantic IDs.

```rust
pub enum Selection {
    None,
    Effect(EffectId),
    System(SystemId),
    Emitter(EmitterId),
    Module(ModuleId),
    Parameter(ParameterId),
    Curve(CurveId),
}
```

Allow users to lock parts of effects:

```text
Core      unlocked
Trail     locked
Wisps     unlocked
Impact    unlocked
```

The command executor should reject changes to locked elements unless explicitly overridden. AI must never bypass locks.


# 13. Semantic Diff Visualization

Do not rely only on textual serialization diffs.

Example user-facing diff:

```text
Trail / Lifetime
0.45 s -> 0.90 s

Trail / Spawn Rate
40/s -> 40/s

Trail / Width Curve
modified

Trail / Curl Noise / Strength
0.8 -> 1.1
```

Support:

```text
Accept all
Reject all
Accept individual changes
Undo after acceptance
```

Possible model:

```rust
pub enum EffectChange {
    ParameterChanged {
        target: ParameterId,
        before: Value,
        after: Value,
    },
    ModuleAdded {
        emitter: EmitterId,
        module: ModuleId,
    },
    ModuleRemoved {
        emitter: EmitterId,
        module: ModuleId,
    },
    ConnectionChanged {
        // ...
    },
}
```


# 14. AI Explanation and Decomposition

Aestra should be able to explain existing effects:

```text
This projectile is built from four visual layers:

1. Core
   Primary readable projectile shape.

2. Ribbon trail
   Communicates movement and velocity.

3. Wisps
   Adds irregular magical motion.

4. Impact burst
   Communicates collision energy.
```

It should answer technical questions:

```text
Why is this emitter expensive?
Where is velocity overwritten?
Why does this spawn twice?
Which module controls this movement?
Why does mobile compilation fail?
What contributes most to overdraw?
```

AI should also decompose complex visual concepts.

A beginner sees:

```text
MAGIC EXPLOSION
```

An expert may see:

```text
flash
+ shockwave
+ sparks
+ smoke
+ debris
+ point light
+ decal
```

Aestra can expose that decomposition and create the layers as normal editable systems.


# 15. Multimodal Authoring

Long-term inputs can include:

## Image to VFX

Extract high-level properties such as:

- palette
- composition
- implied movement
- density
- visual layers
- shape
- glow behavior

Then build a structured effect.

## Video to VFX

Potential extracted information:

```text
motion
frequency
dominant colors
shape
timing
turbulence
emission pattern
dissipation
```

## Scene-aware generation

Example:

> Add dust that reacts to this character landing.

Aestra could use event source, transform, collision normal, velocity, scale, and environment to build an effect wired to scene events.


# 16. AI-Assisted Optimization

This could become one of Aestra's strongest features.

Target profiles might include:

```text
Desktop Ultra
Desktop
Steam Deck
Mobile High
Mobile Low
WebGPU
```

Example request:

> Create a mobile variant without significantly changing the appearance.

Profiler data:

```text
Original
----------------
Particles       8,400
Emitters           11
Overdraw          6.8x
Compute         0.74 ms
Draw calls          9
```

Optimized variant:

```text
Mobile
----------------
Particles       2,100
Emitters            7
Overdraw          2.4x
Compute         0.19 ms
Draw calls          5
```

Possible suggestions:

```text
merge sprite emitters
replace some particles with a ribbon
reduce particle lifetime
simplify collision
reduce texture sampling
reduce transparent overlap
bake static noise where useful
```

Always preview A/B before acceptance.


# 17. Profiling Architecture

Profiling information should be machine-readable.

```rust
EffectProfile {
    gpu_time,
    cpu_time,

    alive_particles,
    peak_particles,

    emitter_count,
    draw_calls,
    dispatch_count,

    estimated_overdraw,
    texture_sample_count,

    buffer_memory,
    texture_memory,

    collision_cost,

    platform_warnings,
}
```

Expose per-emitter and, where feasible, per-module metrics.

This enables profiler UI, AI optimization, automatic quality tiers, and regression testing.


# 18. Quality Variants

Effects may support:

```text
Spectral Bolt
+-- Ultra
+-- High
+-- Medium
+-- Low
+-- Mobile
```

Prefer semantic overrides/deltas over full duplication.

```text
Mobile overrides:

Trail.spawn_rate *= 0.5
Wisps.enabled = false
Impact.burst_count *= 0.4
Collision.mode = Approximate
```

AI can help generate and maintain these profiles.


# 19. Validation and Diagnostics

Validation must exist independently from the UI.

Examples:

```text
ERROR
Particle Update reads `previous_position` before initialization.

ERROR
Module requires a GPU capability unavailable on the selected target.

WARNING
Emitter can exceed the configured particle budget.

WARNING
Transparent layers may cause excessive overdraw.

WARNING
Event loop may recursively spawn effects without a limit.
```

Diagnostics should be structured:

```rust
Diagnostic {
    severity,
    target,
    code,
    message,
}
```

AI should consume these diagnostics through public APIs and propose fixes.


# 20. Deterministic Editing and Serialization

AI must never directly mutate arbitrary editor memory.

```text
AI
 |
creates commands
 |
validator
 |
transaction
 |
effect model
 |
compiler
```

This makes edits observable, undoable, replayable, serializable, and testable.

Serialization should emphasize:

- stable IDs
- readable diffs
- versioning
- schema migration
- deterministic output where possible
- separation of semantic and editor presentation state

Possible conceptual split:

```text
spectral_bolt.aestra
    semantic effect

spectral_bolt.aestra.editor
    graph positions
    panel state
    comments
    visual grouping
```

The exact file split is optional.

Version documents explicitly:

```rust
AestraDocument {
    format_version: u32,
    effect: Effect,
}
```

Provide migrations such as:

```text
v1 -> v2
v2 -> v3
```


# 21. Parameters and Curves

Parameters should carry semantics:

```rust
ParameterMetadata {
    name: "Lifetime",
    description: "Duration before particle removal.",
    unit: Unit::Seconds,
    min: Some(0.0),
    max: Some(60.0),
    semantic: ParameterSemantic::Lifetime,
}
```

Curves should be first-class:

- float curves
- vector curves
- color gradients
- easing functions
- normalized-lifetime domains
- absolute-time domains

A request such as:

> Fade slowly at first, then quickly near the end.

should produce a real editable curve rather than hidden logic.


# 22. Event Model

Events should be explicit semantic concepts:

```text
OnSpawn
OnDeath
OnCollision
OnThreshold
OnCustomEvent
```

Example:

```text
Projectile
   |
OnCollision
   |
   +--> Impact Burst
   +--> Flash
   +--> Sound event
   +--> Decal
```

AI should be able to reason about event relationships without parsing arbitrary visual wiring.


# 23. Preview Infrastructure

Preview should be callable programmatically.

Future AI loop:

```text
1. Apply temporary transaction
2. Compile
3. Render preview
4. Capture metrics
5. Compare result
6. Refine if necessary
7. Present proposed changes
```

Even before AI exists, this API benefits automated testing.


# 24. Testability

Effect transformations should be testable without launching the full editor.

```rust
#[test]
fn set_parameter_command_is_undoable() {}

#[test]
fn removing_emitter_removes_references() {}

#[test]
fn invalid_connection_is_rejected() {}

#[test]
fn transaction_rolls_back_on_failure() {}

#[test]
fn serialization_is_stable() {}
```

Also consider:

```text
golden compilation tests
WESL snapshot tests
effect migration tests
graph validation tests
performance regression tests
```


# 25. Suggested Editor Service Layer

Illustrative API:

```rust
pub struct EditorSession {
    document: EffectDocument,
    history: CommandHistory,
    selection: Selection,
    locks: LockState,
}

impl EditorSession {
    pub fn execute(&mut self, command: EffectCommand) -> Result<()>;
    pub fn execute_transaction(&mut self, tx: EffectTransaction) -> Result<()>;

    pub fn undo(&mut self) -> Result<()>;
    pub fn redo(&mut self) -> Result<()>;

    pub fn validate(&self) -> ValidationReport;
    pub fn diff(&self, tx: &EffectTransaction) -> EffectDiff;
}
```

Adapt this to the existing codebase rather than copying it literally.


# 26. Future AI Request Pipeline

```text
Natural language request
        |
        v
Context builder
        |
        +-- selected objects
        +-- lock state
        +-- relevant module metadata
        +-- diagnostics
        +-- profiler data
        |
        v
AI planner
        |
        v
Structured tool calls
        |
        v
EffectTransaction
        |
        v
Validator
        |
        v
Temporary preview
        |
        v
Semantic diff
        |
        v
User accepts
        |
        v
Commit
```


# 27. AI Safety and Reliability Rules

1. AI must use structured editor commands.
2. AI must not directly rewrite internal files.
3. Commands must pass normal validation.
4. Locked elements must remain untouched.
5. Destructive changes should be grouped into transactions.
6. Changes must remain undoable.
7. Invalid generated graphs must never replace the working graph.
8. Compiler errors should be surfaced before commit when practical.
9. Performance constraints are explicit requirements.
10. Generated effects remain normally inspectable and editable.
11. Aestra remains fully usable without AI or an online provider.


# 28. Longer-Term AI Roles

## AI as teacher

Example:

> Why does this effect feel weak?

Possible response:

```text
1. The flash is too dim relative to the projectile.
2. The burst expands slowly, reducing perceived force.
3. There is little persistent secondary motion after impact.
```

The user can apply selected recommendations.

## AI as technical assistant

```text
Find the module overriding velocity.
Explain why this ribbon breaks at sharp turns.
Show the most expensive emitter.
Why is particle count continuously increasing?
Make this deterministic.
Remove modules that have no effect.
Find redundant calculations.
Explain generated WESL for this module.
```

## AI as optimizer

Example constraints:

```text
Steam Deck
60 FPS
maximum 0.3 ms GPU
preserve visual appearance
```

Use real profiler/compiler data rather than generic guesses.

## AI as variant generator

```text
Create subtle / normal / legendary variants.
Create fire / frost / arcane variants.
Create a cheaper background-NPC version.
```

## AI as graph refactoring tool

```text
Extract repeated logic into a reusable module.
Merge duplicate modules.
Rename parameters consistently.
Group the effect into readable subsystems.
Replace an expensive module with a cheaper equivalent.
Normalize parameter naming.
Separate impact logic from projectile logic.
```


# 29. Reusable Components and Style Libraries

Reusable effect components:

```text
Magic Trail
Impact Flash
Ground Shockwave
Dust Burst
Ember Cloud
Spark Burst
Dissolve
```

AI should compose these when practical instead of generating everything from scratch.

Benefits:

- consistency
- speed
- optimization
- deterministic outputs
- style control

Projects can define semantic visual style guides.

Example:

```text
Oblivion Style

Palette:
black
violet
cold white highlights

Motion:
inward pull
slow orbital movement
violent collapse

Shapes:
fractures
rings
wisps

Avoid:
orange
cartoon smoke
round fireball explosions
```

Possible project assets:

```text
VFXStyleGuide
EffectPreset
ModulePreset
MaterialPreset
Palette
PerformanceProfile
PlatformProfile
```

These should be project data that AI can query through the same tool interface.


# 30. Collaboration Benefits

A command-based semantic architecture also prepares Aestra for future collaborative editing.

Instead of arbitrary UI mutations:

```text
Command A
Command B
Command C
```

This can later enable:

- operation history
- patch sharing
- review
- merge support
- collaborative sessions
- AI review

Do not implement collaboration now solely for AI, but avoid architecture that prevents it.


# 31. Recommended Implementation Phases

## Phase 1 — Semantic Foundation

Highest priority.

Implement or verify:

- stable semantic object IDs
- clean effect/system/emitter/module model
- semantic data separated from editor layout data
- serialization
- validation
- format versioning
- initial module metadata

No AI required.

## Phase 2 — Command System

Introduce:

```text
EffectCommand
CommandExecutor
CommandHistory
undo
redo
transactions
```

Move editor mutations progressively behind commands.

Acceptance criteria:

- changing a parameter uses a command
- adding/removing modules uses commands
- undo/redo uses command history
- commands execute without direct UI interaction

## Phase 3 — Editor Session

Central layer owning:

- document
- selection
- history
- locks
- diagnostics
- preview state

The UI should interact primarily through this service.

## Phase 4 — Semantic Diff

Add structured effect diffs and use them for history, debugging, and transaction previews.

## Phase 5 — Module Registry and Metadata

Every module exposes:

- name
- category
- description
- compatible stages
- inputs
- outputs
- affected attributes
- approximate cost
- semantic tags

## Phase 6 — Editor UX Restructure

Move toward:

```text
Effect hierarchy
Live preview
Properties
Graph / Timeline / Curves / Profiler
```

Do not force every task through the node graph.

## Phase 7 — Profiling and Diagnostics

Expose machine-readable:

- particle counts
- emitter costs
- GPU timing where practical
- warnings
- target capability issues

## Phase 8 — Internal Tool API

Before connecting an LLM, expose operations an agent would use:

```text
inspect_effect
find_modules
add_module
set_parameter
validate_effect
profile_effect
preview_effect
```

Test them manually or through scripts.

## Phase 9 — AI Prototype

Start with constrained, high-value tasks:

1. Explain selected emitter.
2. Find expensive parts of an effect.
3. Change parameters from natural language.
4. Add a known module.
5. Optimize a selected emitter under explicit constraints.
6. Build a simple effect from existing presets.

## Phase 10 — AI Transactions + Preview

Add:

```text
AI proposes transaction
semantic diff
temporary preview
accept / reject
```

## Phase 11 — Generative Composition

Add higher-level creation:

```text
Create projectile
Create impact
Create magic trail
Create smoke effect
```

Prefer reusable modules and presets.

## Phase 12 — Multimodal

Later:

- image reference
- video reference
- scene-aware effect generation


# 32. What Not to Do

## Do not make the node graph the canonical document

Screen position is presentation state.

## Do not couple AI directly to serialized files

AI should operate through semantic APIs.

## Do not generate opaque shaders as the normal workflow

Generated effects must remain inspectable and editable.

## Do not make AI changes impossible to undo

All mutations go through command/history infrastructure.

## Do not make the AI layer mandatory

Aestra must remain fully usable offline and without an AI provider.

## Do not expose raw internal structs as the long-term AI API

Create intentional, versionable operations.

## Do not require AI to memorize module names

Expose module discovery and metadata.

## Do not defer semantic structure until after the UI

The semantic model is foundational.


# 33. Near-Term Codex Tasks

The first editor refactor should focus on AI-readiness without adding AI.

## Task 1 — Audit state ownership

Identify:

- canonical effect state
- editor-only state
- graph-layout state
- preview/runtime state

Separate concerns where mixed.

## Task 2 — Introduce stable semantic IDs

Ensure systems, emitters, modules, curves, and other addressable objects have stable IDs.

## Task 3 — Introduce `EffectCommand`

Start with:

```text
SetParameter
AddEmitter
RemoveEmitter
AddModule
RemoveModule
Connect
Disconnect
```

## Task 4 — Add `CommandHistory`

Implement:

```text
execute
undo
redo
```

All new editor mutations should use this path.

## Task 5 — Add transactions

Support atomic grouping of multiple commands.

## Task 6 — Add explicit selection state

Selection should use semantic IDs.

## Task 7 — Add lock support

At minimum design the data model now, even if the UI comes later.

## Task 8 — Add module metadata

Start with:

```text
name
category
description
stage
inputs
outputs
```

Later add semantic tags and cost information.

## Task 9 — Add semantic diagnostics

Validation returns structured diagnostics.

## Task 10 — Prepare editor layout

Target:

```text
Left:
Effect hierarchy

Center:
Live preview

Right:
Properties

Bottom:
Graph
Timeline
Curves
Diagnostics
Profiler
Generated WESL
```


# 34. Initial Acceptance Criteria

The editor architecture is AI-ready when:

- [ ] A complete effect exists independently of visual graph layout.
- [ ] Semantic objects have stable IDs.
- [ ] Editor-only layout data is separate from semantic effect data.
- [ ] Important mutations are represented as commands.
- [ ] Commands execute without simulated UI input.
- [ ] Undo/redo works through command history.
- [ ] Multiple commands can form an atomic transaction.
- [ ] Effect validation is callable programmatically.
- [ ] Validation returns structured diagnostics.
- [ ] Modules expose structured metadata.
- [ ] Available modules can be queried programmatically.
- [ ] Selection is represented semantically.
- [ ] Locked parts of an effect can reject modifications.
- [ ] Changes can be represented as semantic diffs.
- [ ] Preview/compile functionality can be called independently from direct UI actions.
- [ ] The editor remains fully functional without AI.


# 35. Longer-Term Acceptance Criteria

A future agent should be able to complete this workflow using only public structured operations:

```text
1. Inspect current effect.
2. Inspect selected emitter.
3. Query compatible modules.
4. Read module metadata.
5. Create a transaction.
6. Add/change/remove modules and parameters.
7. Validate the result.
8. Compile or preview it.
9. Inspect diagnostics and performance.
10. Refine the transaction if needed.
11. Produce a semantic diff.
12. Ask the user to accept the changes.
13. Commit or roll back.
```

No direct UI automation should be required.


# 36. Final Product Principle

The long-term goal is **not**:

> Niagara for Bevy with a chatbot attached.

The goal is:

> A VFX authoring system where human editing, procedural generation, scripting, and AI all operate on the same deterministic semantic model.

Aestra should make natural-language authoring convenient without sacrificing the exactness, inspectability, performance control, and graph-level access required by professional VFX work.

The node graph remains important.

It becomes the **source code of the visual effect**, while AI becomes another powerful way to author, inspect, refactor, explain, and optimize that source.


# 37. Recommended Rust AI Stack

Aestra should **not** use a generic agent framework as the foundation of its architecture.

The semantic model, command system, transactions, validation, preview, diffing, and approval flow should remain owned by Aestra.

The AI provider should be treated as a replaceable transport/inference layer.

Recommended high-level architecture:

```text
                         Aestra Editor
                              |
                         aestra-ai
                              |
               +--------------+--------------+
               | Aestra-owned agent loop     |
               | tools / transactions / diff |
               +--------------+--------------+
                              |
                            genai
                              |
          +----------+--------+-----------+----------+
          |          |        |           |          |
       OpenAI    Anthropic  Gemini      Ollama    others
                                                  |
                                         local / cloud
```

Optional future capabilities:

```text
mistralrs
    fully embedded local models

rmcp
    expose Aestra tools through MCP

fastembed
    semantic module / preset / style retrieval
```

The important architectural rule is:

> AI frameworks and providers are replaceable. Aestra's semantic effect model and command system are not.

---

# 38. Core AI Crates

Recommended crates:

| Purpose | Crate | Role |
|---|---|---|
| Multi-provider LLM API | `genai` | Default AI transport layer |
| Serialization | `serde`, `serde_json` | Structured tool input/output |
| JSON Schema generation | `schemars` | Generate tool schemas from Rust types |
| JSON Schema validation | `jsonschema` | Defensive validation of model arguments |
| Async runtime | `tokio`, `futures` | AI calls and orchestration |
| Error handling | `thiserror` | Typed AI/tool errors |
| Observability | `tracing` | Tool calls, model activity, debugging |
| Credential storage | `keyring` | Secure desktop API-key storage |
| Secret wrappers | `secrecy` | Reduce accidental credential exposure |
| MCP | `rmcp` | Optional external AI/tool integration |
| Embedded local models | `mistralrs` | Optional true in-process local inference |
| Local embeddings | `fastembed` | Later semantic retrieval |
| OpenAI-specific API | `async-openai` | Optional specialized backend |
| Agent framework | `rig` | Reference/prototyping only, not foundational |

---

# 39. `genai` as the Default Provider Layer

Use `genai` as the default provider-neutral LLM transport.

Aestra should be able to support providers such as:

```text
OpenAI
Anthropic
Gemini
Ollama
OpenRouter
DeepSeek
Groq
other supported providers
```

The editor can eventually expose:

```text
AI Provider

○ OpenAI
○ Anthropic
○ Gemini
○ Local — Ollama
○ Local — Embedded
○ Custom / compatible provider
```

However, do not let `genai` types leak throughout Aestra.

Wrap it behind an Aestra-owned abstraction:

```rust
pub trait AiBackend {
    async fn complete(
        &self,
        request: AiRequest,
    ) -> Result<AiResponse, AiError>;
}
```

Conceptual layering:

```text
Aestra
   |
AiBackend trait
   |
GenAiBackend
   |
genai
   |
OpenAI / Anthropic / Gemini / Ollama / ...
```

This keeps provider changes isolated.

---

# 40. `schemars` for Tool Schemas

Aestra tools should use strongly typed Rust inputs.

Example:

```rust
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SetParameterArgs {
    pub emitter: EmitterId,
    pub module: ModuleId,
    pub parameter: ParameterId,
    pub value: ParameterValue,
}
```

`schemars` should generate the JSON Schema shown to the LLM.

This avoids manually maintaining both:

```text
Rust input type
+
JSON tool description
```

and prevents schema drift.

A possible common trait:

```rust
pub trait AestraTool {
    type Input:
        DeserializeOwned
        + JsonSchema;

    type Output:
        Serialize;

    const NAME: &'static str;

    fn description() -> &'static str;

    async fn execute(
        &self,
        context: &mut AiContext,
        input: Self::Input,
    ) -> Result<Self::Output>;
}
```

The exact trait can be adjusted to fit the project.

---

# 41. Defensive Tool Validation

Generated tool arguments should pass multiple validation boundaries:

```text
LLM
 |
 | JSON arguments
 v
JSON Schema validation
 |
 v
Serde deserialization
 |
 v
Aestra semantic validation
 |
 v
EffectCommand
```

Use `jsonschema` where useful for explicit schema validation.

Important distinction:

A tool call can be structurally valid but semantically invalid.

Example:

```text
set_parameter(
    parameter = Lifetime,
    value = -25
)
```

The JSON may be valid, but Aestra must reject a negative lifetime.

The semantic model remains the ultimate source of truth.

---

# 42. MCP with `rmcp`

Plan for an optional `aestra-mcp` layer using `rmcp`.

Aestra's internal tool API may eventually expose operations such as:

```text
inspect_effect
inspect_emitter
inspect_selection

find_modules
inspect_module

add_emitter
remove_emitter
add_module
remove_module
set_parameter

create_curve

validate_effect
profile_effect
preview_effect
```

These tools can be consumed by both:

```text
                 Aestra Tool API
                       |
          +------------+-------------+
          |                          |
     Aestra internal AI          aestra-mcp
                                     |
                                    MCP
                                     |
                         External AI clients
```

Important:

**MCP should not be the canonical internal API.**

Preferred layering:

```text
EffectCommand API
      ^
      |
AestraTool API
      ^
      |
 +----+----+
 |         |
AI        MCP
```

MCP is a protocol boundary, not the core editing model.

---

# 43. Local AI Strategy

Aestra should eventually support local AI, but local inference should be introduced incrementally.

## First local option: Ollama

Use the normal provider abstraction:

```text
aestra-ai
    |
   genai
    |
  Ollama
```

Advantages:

- easy initial implementation
- no embedded inference engine
- local/privacy-friendly workflow
- provider-neutral architecture remains intact

## Later option: `mistralrs`

An optional future crate:

```text
aestra-ai-local
    |
mistralrs
    |
local model
```

This can provide true in-process local inference.

Potential benefits:

- fully offline use
- no API keys
- tighter desktop integration
- potentially lower latency
- user-controlled models

Do not make `mistralrs` part of the base Aestra build.

It should remain optional because model runtimes substantially increase build complexity and binary/dependency weight.

---

# 44. Semantic Retrieval with `fastembed`

Do not introduce embeddings too early.

For a small module registry, deterministic queries are preferable:

```rust
registry.find(|module| {
    module.affects(ParticleAttribute::Velocity)
        && module.supports(StageKind::ParticleUpdate)
});
```

When Aestra eventually contains hundreds or thousands of:

```text
modules
presets
materials
effect templates
style definitions
examples
documentation
```

then semantic retrieval becomes useful.

Example:

```text
"irregular spiraling movement"
              |
              v
          embedding
              |
              v
      semantic retrieval
              |
       +------+------+ 
       |      |      |
     Vortex  Curl  Noise
     Force   Noise Field
```

Use `fastembed` later for local embeddings and possibly reranking.

Structured filtering should still be used alongside semantic retrieval.

---

# 45. Credential Storage

API keys should not be stored in ordinary project configuration.

Avoid:

```text
aestra.toml

openai_key = "sk-..."
```

Recommended desktop flow:

```text
Windows Credential Manager
        |
      keyring
        |
   SecretString
        |
   GenAiBackend
```

Use:

- `keyring` for native credential storage
- `secrecy` to reduce accidental logging/debug exposure

Provider configuration can contain identifiers such as:

```text
provider = "openai"
model = "..."
```

but secrets should remain in the OS credential store.

---

# 46. Why Not Build Aestra Around `rig`

`rig` can be useful for experiments and provides many agent-oriented features.

However, Aestra should not make a generic agent framework the owner of the editing loop.

Aestra requires a specialized workflow:

```text
Intent
 |
Plan
 |
Aestra tool calls
 |
EffectTransaction
 |
Validate
 |
Compile
 |
Preview
 |
Profile
 |
Semantic diff
 |
Human approval
 |
Commit
```

This state machine should remain under Aestra's control.

A generic agent abstraction may otherwise obscure important behavior such as:

- locks
- transaction boundaries
- preview state
- validation
- deterministic undo
- partial acceptance
- performance constraints
- editor selection scope

Use agent frameworks as inspiration or for prototypes, not as the architectural core.

---

# 47. Optional OpenAI-Specific Backend

Aestra should remain provider-neutral by default.

If a future capability requires OpenAI-specific functionality that is not conveniently available through the generic provider layer, add an optional adapter using `async-openai`.

Conceptually:

```text
                AiBackend
               /         \
              /           \
     GenAiBackend     OpenAiBackend
          |                 |
        genai          async-openai
```

Do this only when there is a concrete capability benefit.

Do not allow OpenAI-specific types to enter `aestra-core`, `aestra-graph`, or the command model.

---

# 48. Suggested Cargo Dependencies

Initial AI dependencies could look approximately like:

```toml
[dependencies]

# AI transport
genai = "0.6"

# Structured tool data
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = "1"
jsonschema = "0.50"

# Async
tokio = { version = "1", features = [
    "rt-multi-thread",
    "sync",
    "time",
] }
futures = "0.3"

# Errors / observability
thiserror = "2"
tracing = "0.1"

# Credentials
keyring = "4"
secrecy = "0.10"
```

Treat versions as examples to verify when implementing.

Optional features:

```toml
[features]

default = []

ai-cloud = [
    "dep:genai",
]

ai-local = [
    "dep:mistralrs",
]

ai-mcp = [
    "dep:rmcp",
]

ai-rag = [
    "dep:fastembed",
]
```

Possible optional dependencies:

```toml
[dependencies]

mistralrs = {
    version = "0.8",
    optional = true,
}

rmcp = {
    version = "3",
    optional = true,
    features = [
        "server",
        "macros",
        "schemars",
    ],
}

fastembed = {
    version = "6",
    optional = true,
}
```

The exact feature names and dependency versions must be verified against the crates used when implementation begins.

---

# 49. AI Should Be Entirely Optional

Aestra must remain fully functional without AI.

Ideal build behavior:

```text
cargo build
```

builds the normal VFX editor with no heavyweight AI runtime dependencies.

Optional variants:

```text
cargo build --features ai-cloud

cargo build --features ai-local

cargo build --features ai-mcp

cargo build --features ai-rag
```

This matters for:

- offline users
- reproducible builds
- minimal runtime footprint
- Linux packaging
- CI
- users who do not want AI functionality
- projects embedding Aestra components without the editor

---

# 50. Proposed AI Crate Layout

A possible structure:

```text
aestra-ai
|
+-- backend.rs
+-- context.rs
+-- conversation.rs
+-- orchestration.rs
+-- tool.rs
+-- transaction.rs
+-- error.rs
|
+-- providers/
|   +-- mod.rs
|   +-- genai.rs
|
+-- tools/
    +-- inspect_effect.rs
    +-- inspect_selection.rs
    +-- find_modules.rs
    +-- inspect_module.rs
    +-- set_parameter.rs
    +-- add_module.rs
    +-- remove_module.rs
    +-- validate_effect.rs
```

Possible future crates:

```text
aestra-mcp
aestra-ai-local
```

Important dependency direction:

```text
                    aestra-core
                         ^
                         |
                    EffectCommand
                         ^
                         |
                    AestraTool
                      ^     ^
                     /       \
               aestra-ai   aestra-mcp
                   ^
                   |
                 genai
```

Critical rule:

> `aestra-ai` may depend on `aestra-core`, but `aestra-core` must never depend on `aestra-ai`.

The same rule should apply to `aestra-graph`, `aestra-compiler`, and runtime crates.

---

# 51. First AI Tool Set

Do not begin with full prompt-to-VFX generation.

Start with a small, constrained tool surface:

```text
inspect_effect
inspect_selection

find_modules
inspect_module

set_parameter
add_module
remove_module

validate_effect
```

This is enough to support useful initial requests such as:

> Increase the lifetime of the selected emitter by 30%.

Conceptual flow:

```text
AI
 |
set_parameter(...)
 |
EffectCommand::SetParameter
 |
EffectTransaction
 |
Validation
 |
Semantic diff
 |
[Apply] [Discard]
```

Only after this loop is robust should Aestra add:

```text
create_emitter
create_curve
create_event
profile_effect
optimize_effect
create_effect
```

---

# 52. First AI Milestone

The first AI milestone should prove the editing architecture, not generative creativity.

## Required capabilities

- [ ] Configure one cloud provider.
- [ ] Configure Ollama as a local provider.
- [ ] Read API keys through secure OS credential storage.
- [ ] Inspect the current effect.
- [ ] Inspect the current editor selection.
- [ ] Query available modules.
- [ ] Inspect module metadata.
- [ ] Set a parameter through an `EffectCommand`.
- [ ] Add a module through an `EffectCommand`.
- [ ] Remove a module through an `EffectCommand`.
- [ ] Group AI changes into an `EffectTransaction`.
- [ ] Validate the proposed transaction.
- [ ] Produce a semantic diff.
- [ ] Allow the user to apply or discard the transaction.
- [ ] Undo an accepted AI transaction using the ordinary editor history.

## Explicitly out of scope for the first milestone

- full text-to-VFX generation
- image-to-VFX
- video-to-VFX
- autonomous multi-minute agent loops
- embeddings/vector databases
- bundled local models
- automatic visual judging
- unrestricted project-wide modifications

---

# 53. AI Backend Interface Goals

`AiBackend` should expose the minimum provider-neutral concepts Aestra needs.

Possible conceptual types:

```rust
pub struct AiRequest {
    pub messages: Vec<AiMessage>,
    pub tools: Vec<AiToolDefinition>,
    pub response_format: Option<AiResponseFormat>,
}

pub struct AiResponse {
    pub message: Option<String>,
    pub tool_calls: Vec<AiToolCall>,
}

pub trait AiBackend {
    async fn complete(
        &self,
        request: AiRequest,
    ) -> Result<AiResponse, AiError>;
}
```

Avoid mirroring a specific provider's entire API.

Only introduce abstractions required by Aestra.

Potential later capabilities can include:

```text
vision input
structured output
streaming
tool calls
model capability discovery
usage/cost metadata
```

---

# 54. Aestra-Owned Orchestration

Aestra should explicitly own the AI execution loop.

Conceptually:

```rust
loop {
    let response = backend.complete(request).await?;

    if response.tool_calls.is_empty() {
        break;
    }

    for call in response.tool_calls {
        let result = tool_registry.execute(call, context).await?;
        conversation.push_tool_result(result);
    }
}
```

But Aestra should impose its own limits and rules.

Examples:

```text
maximum tool iterations
maximum commands per transaction
allowed editing scope
locked objects
performance budget
validation before preview
validation before commit
human approval requirements
```

The provider should never independently decide these policies.

---

# 55. Tool Registry

Aestra should expose its AI-editable operations through a registry.

Conceptual API:

```rust
pub struct ToolRegistry {
    tools: HashMap<ToolName, Box<dyn AestraTool>>,
}
```

It should support:

```text
list tools
get tool schema
get tool description
execute tool
check permissions/scope
```

Tools should be categorized.

Example:

```text
Inspection
    inspect_effect
    inspect_selection
    inspect_module

Discovery
    find_modules

Mutation
    set_parameter
    add_module
    remove_module

Validation
    validate_effect

Performance
    profile_effect

Preview
    preview_effect
```

Mutation tools should generally write to a temporary transaction rather than directly to the live document.

---

# 56. Relationship to the Command System

Do not duplicate editor logic inside AI tools.

Bad:

```text
SetParameterTool
    manually locates parameter
    manually mutates effect
    manually creates undo data
```

Preferred:

```text
SetParameterTool
       |
       v
EffectCommand::SetParameter
       |
       v
CommandExecutor
```

Therefore:

```text
Human UI
   |
   +--------> EffectCommand
   |
AI Tool
   |
   +--------> EffectCommand
```

The same validation, locks, undo behavior, migrations, and invariants apply regardless of who initiated the edit.

---

# 57. Recommended Implementation Order for AI Dependencies

Introduce dependencies gradually.

## Step 1

Add:

```text
serde
serde_json
schemars
tracing
```

These are useful even before model integration.

Build the tool registry and command bridge without any LLM.

## Step 2

Add:

```text
genai
```

Implement one provider and one local Ollama configuration through the same backend.

## Step 3

Add:

```text
keyring
secrecy
```

Implement secure provider credential configuration.

## Step 4

Implement transactions, validation, semantic diff, and user approval for AI actions.

## Step 5

Optionally add:

```text
rmcp
```

Expose the already-existing Aestra tool API externally.

## Step 6

Only when justified by real usage, add:

```text
fastembed
```

for semantic retrieval.

## Step 7

Only if true embedded/offline inference becomes a product goal, add:

```text
mistralrs
```

as a separate optional capability.

This keeps early builds small and prevents AI infrastructure from overwhelming the core VFX editor work.

---

# 58. Updated Final Architecture

The long-term architecture should converge toward:

```text
                         Aestra Editor
                              |
                      +-------+-------+
                      |               |
                 Human UI         AI Intent UI
                      |               |
                      |          aestra-ai
                      |               |
                      |          AiBackend
                      |               |
                      |             genai
                      |               |
                      |      cloud/local providers
                      |               |
                      +-------+-------+
                              |
                        AestraTool API
                              |
                      EffectTransaction
                              |
                        EffectCommand
                              |
                      CommandExecutor
                              |
                     Semantic Effect Model
                              |
              +---------------+---------------+
              |                               |
          Validator                        Compiler
              |                               |
          Diagnostics                       WESL
                                              |
                                            GPU
                                              |
                                            Bevy
```

Optional external path:

```text
External AI client
       |
      MCP
       |
  aestra-mcp
       |
 AestraTool API
```

Optional embedded local path:

```text
aestra-ai
    |
AiBackend
    |
MistralRsBackend
    |
mistralrs
    |
Local model
```

The guiding principle remains:

> AI is an authoring client of Aestra, not the owner of Aestra.

# 59. Meshes, Sprites, Flipbooks, and Render Assets

Shaders are only one part of spell VFX.

A Niagara-class editor must treat the following as first-class authoring concepts:

- meshes
- sprites
- flipbooks
- ribbons
- trails
- beams
- decals
- textures and masks
- materials
- eventually volumes

Aestra should therefore model effect creation as a combination of:

```text
                    User intent
                        |
                        v
                 Effect decomposition
                        |
       +----------------+----------------+
       |                |                |
     Geometry        Appearance        Motion
       |                |                |
 mesh/sprite/       material/          emitters/
 ribbon/etc.          shader           modules
       \                |                /
        +---------------+---------------+
                        |
                 Aestra Effect
```

AI should behave as a compositor/director across these subsystems rather than as a shader generator only.

---

# 60. Prefer Procedural VFX Geometry

Most spell VFX do not require arbitrary authored 3D models.

A large portion of useful VFX geometry can be represented procedurally:

```text
Quad
Disc
Ring
Arc
Cone
Cylinder
Tube
Ribbon
Beam
Sphere
Icosphere
Torus
Crossed planes
Shard
Custom polygon
```

Example:

Instead of generating an opaque "shockwave mesh", Aestra should prefer:

```text
ProceduralMesh
    kind: Ring

    inner_radius: 0.72
    outer_radius: 1.00
    segments: 64

Material
    distortion_noise
    dissolve
    emissive
    edge_fade

Animation
    scale: 0 -> 8
    opacity: 1 -> 0
```

Advantages:

- deterministic
- compact
- editable
- efficient
- easy to serialize
- easy for AI to manipulate
- easy to regenerate for different quality levels

---

# 61. `MeshRecipe`

Add a semantic procedural geometry representation.

Conceptually:

```rust
pub enum MeshRecipe {
    Quad(QuadMesh),
    Disc(DiscMesh),
    Ring(RingMesh),
    Arc(ArcMesh),
    Cone(ConeMesh),
    Cylinder(CylinderMesh),
    Tube(TubeMesh),
    Sphere(SphereMesh),
    Icosphere(IcosphereMesh),
    Torus(TorusMesh),
    Shards(ShardMesh),
}
```

Example:

```rust
RingMesh {
    segments: 64,
    inner_radius: 0.8,
    outer_radius: 1.0,
}
```

The editor/runtime may generate a Bevy `Mesh`, but the canonical Aestra data should remain the compact semantic recipe where possible.

Avoid storing generated vertex/index buffers as the primary authored representation when the mesh can be reconstructed from parameters.

---

# 62. Dynamic Geometry Renderers

Some VFX geometry should not exist as static mesh assets.

Examples:

```text
Ribbon
Trail
Beam
Lightning
Spline
Arc
```

These should be modeled as dynamic renderers that generate or stream geometry at runtime.

Example:

```text
RibbonRenderer
    source: particle_position
    width: Curve
    facing: Camera
    smoothing: CatmullRom
    uv_mode: Stretch
```

AI edits should manipulate renderer semantics:

> Make the trail thinner near the end and smoother.

Possible semantic changes:

```text
Ribbon.WidthCurve
1.0 -> 0.0

Ribbon.Smoothing
Linear -> CatmullRom
```

No generative mesh model is required for this class of effect.

---

# 63. Sprite Assets as First-Class Resources

Sprites are essential for spell VFX.

Common sprite categories include:

```text
soft circle
smoke puff
spark
star
glow
flare
magic rune
noise blob
fire shape
lightning shape
dust
debris silhouette
```

A possible source model:

```rust
pub enum SpriteSource {
    Project(TextureId),
    Procedural(SpriteRecipe),
    Generated(GeneratedImageId),
    Flipbook(FlipbookId),
}
```

This makes it explicit whether the sprite comes from:

- a project asset
- a procedural recipe
- AI/external generation
- a flipbook atlas

---

# 64. Procedural Sprites

Many VFX sprites should be generated procedurally rather than through image-generation models.

Possible procedural sprite recipes:

```text
SoftCircle
RadialGradient
Ring
Star
Polygon
NoiseBlob
Voronoi
LightningMask
SDFShape
Gradient
PerlinNoise
CurlNoise
```

Example:

```text
SpriteRecipe
    Shape = Star
    Points = 6
    Softness = 0.15
    RadialFade = 0.7
```

The recipe can either:

- generate a texture asset, or
- be evaluated directly by the material/shader

Preferred principle:

> AI should generate a semantic recipe instead of pixels whenever the visual can be represented procedurally.

---

# 65. Authored and AI-Generated Sprites

Some effects require more artistic sprite assets:

```text
stylized smoke
painted fire
arcane glyph
complex magical cloud
specific magic symbol
```

For these cases, define an asset-generation abstraction.

Conceptual pipeline:

```text
Aestra AI
   |
   | determines asset requirement
   v
AssetGenerator
   |
   +-- image generation provider
   +-- local model
   +-- project asset library
   +-- external editor
   |
   v
GeneratedAsset
   |
Asset processor
   |
   +-- alpha cleanup
   +-- crop
   +-- resize
   +-- mipmaps
   +-- compression
   +-- metadata
   |
   v
Aestra Asset Registry
```

The effect must reference a stable local asset ID:

```text
SpriteAssetId("spectral_wisp_03")
```

Never keep runtime dependencies on temporary remote generation URLs.

---

# 66. Asset Search Before Generation

AI should search existing assets before creating new ones.

Preferred decision tree:

```text
User intent
    |
    v
Need: "violet magic spark"
    |
    v
Asset search
    |
 +-- found ------> use it
 |
 +-- none
       |
       v
 procedural possible?
       |
 +-- yes -------> create recipe
 |
 +-- no
       |
       v
 external/AI generation
```

Search order should generally include:

```text
Current project assets
Project presets
Built-in Aestra assets
Current effect assets
Style library assets
```

Benefits:

- consistent project art direction
- fewer duplicate assets
- smaller project size
- easier optimization
- better reuse

---

# 67. Flipbooks

Flipbooks are especially important for:

```text
fire
smoke
explosions
shockwaves
fluid effects
dissolves
```

A flipbook should be a first-class semantic asset.

Example:

```text
FlipbookAsset
    texture
    columns: 8
    rows: 8
    frames: 64
    fps: 30
```

---

# 68. Imported Flipbooks

Support traditional atlas import:

```text
PNG / EXR / supported texture format
```

with explicit metadata:

```text
columns
rows
frame_count
fps
loop_mode
frame_order
```

---

# 69. Video to Flipbook

Do not generate dozens of independent frames with a generic image model because temporal consistency will be poor.

A better future workflow:

```text
"slow swirling violet smoke"
          |
          v
temporally coherent video/animation
          |
          v
frame extraction
          |
          v
alpha/background processing
          |
          v
crop / normalize
          |
          v
atlas packing
          |
          v
FlipbookAsset
```

This can be connected to future video-generation providers without making them part of the core architecture.

---

# 70. Simulation to Flipbook Baking

Aestra should eventually support baking procedural effects into flipbooks.

Example:

```text
High-quality procedural smoke
          |
      Offscreen render
          |
       64 frames
          |
      Atlas packing
          |
      FlipbookAsset
```

This enables an important optimization workflow:

```text
expensive procedural effect
          |
          v
      bake flipbook
          |
          v
cheap sprite renderer
```

This feature is valuable even without AI.

---

# 71. Renderer Substitution as an Optimization

AI can later reason about render representation.

Example:

```text
Before:
24 animated mesh wisps

After:
12 camera-facing sprite particles
using a baked flipbook
```

Possible comparison:

```text
Before
GPU: 0.72 ms
Draw calls: 11

After
GPU: 0.21 ms
Draw calls: 4
```

This is one of the strongest arguments for separating simulation from rendering.

---

# 72. Generated 3D Meshes

Procedural geometry should be preferred, but arbitrary generated meshes still have use cases.

Example:

> Summon spectral fragments shaped like broken gothic statues.

This may require a specialized 3D-generation provider.

Aestra should expose an abstract interface:

```text
AssetGenerator3d
   |
   +-- RemoteProvider
   +-- LocalProvider
   +-- future image-to-3D adapters
   +-- future text-to-3D adapters
```

The core should not depend on any specific generation model.

---

# 73. Generated Mesh Processing Pipeline

Never use raw generated meshes directly in production effects.

Required processing pipeline:

```text
Generated mesh
     |
     v
Import
     |
     +-- topology validation
     +-- normals
     +-- tangents
     +-- UV validation
     +-- simplification
     +-- decimation
     +-- LOD generation
     +-- bounds
     +-- scale normalization
     +-- pivot normalization
     +-- material extraction
     +-- texture processing
     |
     v
Aestra MeshAsset
```

AI generation requests should support technical constraints.

Example:

```text
Maximum triangles: 2,000
Expected instances: 300
Target: Steam Deck
Needs UVs: yes
Needs LOD: yes
```

Generation and postprocessing should respect these constraints.

---

# 74. Aestra VFX Asset Model

Introduce a first-class asset model.

Conceptually:

```rust
pub enum VfxAsset {
    Mesh(MeshAsset),
    Sprite(SpriteAsset),
    Flipbook(FlipbookAsset),
    Texture(TextureAsset),
    Material(MaterialAsset),
}
```

Track provenance:

```rust
pub enum AssetSource {
    Builtin,
    Project,
    Procedural,
    Imported,
    Generated,
}
```

Common metadata:

```rust
pub struct AssetMetadata {
    pub name: String,
    pub tags: Vec<String>,
    pub source: AssetSource,
    pub style_tags: Vec<String>,
    pub gpu_cost: Option<AssetCost>,
}
```

---

# 75. Asset-Specific Metadata

Mesh metadata may include:

```text
triangle_count
vertex_count
LOD_count
skinned
bounds
instancing_compatible
```

Sprite metadata may include:

```text
resolution
alpha_mode
compression
color_space
```

Flipbook metadata may include:

```text
frame_count
resolution
fps
columns
rows
loop_mode
```

This lets both humans and AI reason about asset cost and suitability.

---

# 76. Renderer Model

Renderers should reference semantic assets.

Conceptually:

```text
MeshRenderer
    mesh: MeshAssetId
    material: MaterialId

SpriteRenderer
    sprite: SpriteAssetId
    material: MaterialId

FlipbookRenderer
    flipbook: FlipbookAssetId
    material: MaterialId

RibbonRenderer
    material: MaterialId

BeamRenderer
    material: MaterialId
```

This separates simulation from visual representation.

---

# 77. Critical Separation: Simulation != Renderer != Asset != Material

This distinction should be foundational.

```text
Emitter / Simulation
        |
        | generates particles/data
        v
Renderer
        |
        | chooses representation
        v
Asset
        |
        | mesh / sprite / flipbook
        v
Material
        |
        v
Shader
```

Therefore:

```text
Particle simulation
!=
Renderer
!=
Asset
!=
Material
```

A particle simulation can be rendered using:

```text
SpriteRenderer
MeshRenderer
RibbonRenderer
```

without rewriting the emitter logic.

This flexibility is important both for manual authoring and for AI optimization.

---

# 78. AI Renderer Replacement

Because render representation is independent, AI can propose semantic substitutions.

Example:

> These particles look too flat.

Possible change:

```text
SpriteRenderer
      |
      v
MeshRenderer

mesh:
ProceduralShard
```

Reverse optimization:

> There are too many small rocks and the effect is expensive.

Possible change:

```text
MeshRenderer
      |
      v
SpriteRenderer

sprite:
rock_fragment_impactor
```

The simulation remains unchanged.

---

# 79. Full Spell Example

Example request:

> Create an Oblivion meteor. A black/violet projectile should tear through space, leave fractured energy behind, then implode on impact before exploding outward.

Possible decomposition:

```text
Oblivion Meteor
|
+-- Core
|   +-- MeshRenderer
|   |   +-- Icosphere
|   |
|   +-- Material
|       +-- noise deformation
|       +-- black core
|       +-- violet Fresnel
|
+-- Trail
|   +-- RibbonRenderer
|       +-- procedural geometry
|       +-- distorted UV
|       +-- dissolve material
|
+-- Fractures
|   +-- Mesh particles
|       +-- ProceduralShard
|   +-- emissive material
|
+-- Wisps
|   +-- SpriteRenderer
|       +-- project/generated smoke sprite
|
+-- Implosion
|   +-- RingMesh
|       +-- radius 5 -> 0
|       +-- distortion material
|
+-- Explosion
    +-- Sprite flipbook
    +-- shard particles
    +-- radial ribbon arcs
    +-- flash
```

Only some assets may require artistic generation.

Much of the effect should be assembled from:

```text
procedural geometry
+
existing assets
+
materials
+
shaders
+
simulation
```

This should remain the default philosophy.

---

# 80. Asset Browser UX

Extend the editor with an asset-focused workspace.

Possible layout:

```text
+----------------------------------------------------------------+
| Effect | Assets | Presets                                      |
+----------------+-----------------------------+-----------------+
| EFFECT         |                             | PROPERTIES      |
|                |                             |                 |
| Core           |                             | Renderer        |
| Trail          |       LIVE PREVIEW          | Mesh: Ring      |
| Wisps          |                             | Material: ...   |
| Impact         |                             |                 |
|                |                             |                 |
+----------------+-----------------------------+-----------------+
| Graph | Timeline | Curves | Assets | Profiler | Diagnostics    |
+----------------------------------------------------------------+
```

Asset browser categories:

```text
Meshes
+-- Procedural
+-- Imported
+-- Generated

Sprites
+-- Shapes
+-- Imported
+-- Generated

Flipbooks
Materials
Textures
Presets
```

AI commands should operate on the current asset selection/context.

---

# 81. Asset AI Tools

Extend the Aestra tool registry with asset operations.

Inspection:

```text
find_assets()
inspect_asset()
```

Procedural asset creation:

```text
create_procedural_mesh()
create_procedural_sprite()
```

Assignment:

```text
assign_mesh()
assign_sprite()
assign_flipbook()
```

Processing:

```text
optimize_mesh()
create_lod()
bake_flipbook()
create_sprite_atlas()
```

External generation:

```text
generate_sprite()
generate_mesh()
```

Renderer operations:

```text
set_renderer()
replace_renderer()
```

All mutating operations should still create normal Aestra commands/transactions.

---

# 82. Example AI Asset Workflow

User:

> Use a ring instead of that sprite for the shockwave.

Possible AI flow:

```text
inspect selected renderer
        |
find/create procedural Ring mesh
        |
replace renderer:
SpriteRenderer -> MeshRenderer
        |
assign existing material
        |
create EffectTransaction
        |
validate
        |
preview
        |
semantic diff
        |
apply / discard
```

The same command/history system used by manual edits must be used.

---

# 83. Asset Generation Decision Policy

For any requested visual asset, AI should prefer the following order:

```text
1. Reuse existing suitable project asset.
2. Reuse built-in/preset asset.
3. Create procedural asset.
4. Bake an existing procedural effect.
5. Generate/import a new external asset.
```

This should be an explicit product rule.

Reasons:

- consistency
- performance
- reproducibility
- smaller project size
- better editability
- fewer external dependencies

---

# 84. Asset Commands Should Reuse Core Editor Infrastructure

Do not create a separate mutation path for assets.

Preferred:

```text
AI Tool
   |
AssetCommand / EffectCommand
   |
CommandExecutor
   |
Asset Registry / Effect Model
```

Manual UI must use the same commands.

Examples of future commands:

```rust
pub enum AssetCommand {
    AddAsset(AddAssetCommand),
    RemoveAsset(RemoveAssetCommand),
    RenameAsset(RenameAssetCommand),

    CreateProceduralMesh(CreateProceduralMeshCommand),
    CreateProceduralSprite(CreateProceduralSpriteCommand),

    AssignMesh(AssignMeshCommand),
    AssignSprite(AssignSpriteCommand),
    AssignFlipbook(AssignFlipbookCommand),

    ReplaceRenderer(ReplaceRendererCommand),

    BakeFlipbook(BakeFlipbookCommand),
    GenerateLod(GenerateLodCommand),
}
```

Whether these live in a distinct `AssetCommand` enum or are integrated into `EffectCommand` is an implementation detail.

The important rule is that they remain:

- undoable
- serializable
- validated
- transaction-friendly
- usable without AI

---

# 85. Asset Registry

Introduce an asset registry independent from the visual editor.

Conceptually:

```rust
pub struct AssetRegistry {
    meshes: HashMap<MeshAssetId, MeshAsset>,
    sprites: HashMap<SpriteAssetId, SpriteAsset>,
    flipbooks: HashMap<FlipbookAssetId, FlipbookAsset>,
    textures: HashMap<TextureAssetId, TextureAsset>,
    materials: HashMap<MaterialId, MaterialAsset>,
}
```

Required operations:

```text
find by ID
find by tag
find by source
find by style
find by compatibility
find by performance tier
list unused assets
list references
```

This registry later becomes an important AI retrieval source.

---

# 86. Asset References and Dependency Tracking

Aestra should know which effects reference which assets.

Conceptually:

```text
Effect
  |
  +--> MeshAsset
  +--> MaterialAsset
  +--> FlipbookAsset
```

This enables:

- safe asset deletion
- unused-asset cleanup
- dependency packaging
- effect export
- quality variants
- AI refactoring
- asset replacement
- style migration

Generated assets must follow the same dependency system as imported assets.

---

# 87. Asset Portability

Aestra effect packages should be able to describe all required asset dependencies.

Possible export structure:

```text
spectral_bolt/
|
+-- effect.aestra
+-- assets/
|   +-- meshes/
|   +-- sprites/
|   +-- flipbooks/
|   +-- textures/
|   +-- materials/
|
+-- metadata/
```

Procedural assets may remain recipes and therefore require no external binary asset until compile/build time.

This can substantially reduce package size.

---

# 88. Quality-Level Asset Overrides

Quality profiles should also be able to replace renderers/assets.

Example:

```text
Ultra:
    Renderer = MeshRenderer
    Mesh = DetailedShard

Medium:
    Renderer = MeshRenderer
    Mesh = ProceduralShard

Mobile:
    Renderer = SpriteRenderer
    Sprite = BakedShard
```

This should be representable as semantic quality overrides.

AI optimization can then propose renderer substitutions while preserving the original effect intent.

---

# 89. Asset Validation

Add structured diagnostics for assets.

Examples:

```text
ERROR
Mesh asset has no usable vertex positions.

ERROR
Flipbook references frame 48 but contains only 32 frames.

WARNING
Mesh has 120,000 triangles but is instanced up to 500 times.

WARNING
Sprite texture is 4096x4096 for a 32px on-screen effect.

WARNING
Generated mesh has no UVs but the assigned material requires UV0.

WARNING
Asset is not available in the selected export package.
```

These diagnostics should be available both to the UI and AI.

---

# 90. Asset Profiling

Profiler data should extend beyond emitters.

Mesh asset metrics:

```text
triangle count
vertex count
instance count
draw calls
LOD level
memory
```

Sprite/flipbook metrics:

```text
texture resolution
texture memory
frame count
sampling cost
average screen size
overdraw estimate
```

This data enables AI recommendations such as:

```text
Replace mesh with sprite.
Reduce flipbook resolution.
Generate a cheaper LOD.
Use a procedural ring instead of imported geometry.
```

---

# 91. Revised AI Context

The AI context builder should eventually include asset information.

```text
Current effect
Current selection
Locks
Module metadata
Renderer metadata
Referenced asset metadata
Available project assets
Style library
Diagnostics
Profiler data
Platform target
```

Do not dump the entire asset library into every request.

Use structured search/query tools.

---

# 92. Revised AI Tool Categories

The tool registry now has five broad areas:

```text
Inspection
    inspect_effect
    inspect_selection
    inspect_asset
    inspect_module

Discovery
    find_modules
    find_assets

Effect mutation
    set_parameter
    add_module
    remove_module

Asset / renderer mutation
    create_procedural_mesh
    create_procedural_sprite
    assign_mesh
    assign_sprite
    assign_flipbook
    replace_renderer

Validation / optimization
    validate_effect
    validate_asset
    profile_effect
    profile_asset
    bake_flipbook
    optimize_mesh
```

External generation tools should remain optional and provider-specific behind Aestra abstractions.

---

# 93. Near-Term Mesh/Sprite Implementation Tasks

Before introducing external asset generation, prioritize the deterministic asset model.

## Task 1 — Separate renderer from simulation

Ensure emitters do not own implicit rendering assumptions.

An emitter should produce simulation data.

A renderer consumes that data.

## Task 2 — Introduce renderer types

At minimum:

```text
SpriteRenderer
MeshRenderer
RibbonRenderer
```

Later:

```text
FlipbookRenderer
BeamRenderer
DecalRenderer
```

## Task 3 — Introduce asset IDs and registry

At minimum:

```text
MeshAssetId
SpriteAssetId
TextureAssetId
MaterialId
```

## Task 4 — Add `MeshRecipe`

Start with:

```text
Quad
Disc
Ring
Sphere
Icosphere
Cone
Torus
Shard
```

## Task 5 — Add procedural sprite recipes

Start with:

```text
SoftCircle
RadialGradient
Ring
Star
NoiseBlob
```

## Task 6 — Add flipbook asset type

Support imported atlas data and frame metadata.

## Task 7 — Add asset browser

Expose:

```text
Meshes
Sprites
Flipbooks
Materials
Textures
```

## Task 8 — Add renderer replacement commands

Example:

```text
SpriteRenderer -> MeshRenderer
MeshRenderer -> SpriteRenderer
```

## Task 9 — Add asset validation

Return structured asset diagnostics.

## Task 10 — Add asset metadata

Track:

```text
source
tags
style tags
performance metadata
```

---

# 94. Mesh/Sprite Acceptance Criteria

The asset architecture is ready for future AI integration when:

- [ ] Simulation and rendering are separate concepts.
- [ ] A simulation can switch between sprite and mesh renderers without rewriting emitter logic.
- [ ] Renderers reference stable semantic asset IDs.
- [ ] Mesh, sprite, texture, material, and flipbook assets exist independently of the UI.
- [ ] Procedural meshes are represented as recipes where practical.
- [ ] Procedural sprites are represented as recipes where practical.
- [ ] Flipbooks have explicit frame metadata.
- [ ] Asset provenance is tracked.
- [ ] Assets can be searched by semantic metadata/tags.
- [ ] Asset changes use the same command/transaction/history infrastructure as effect changes.
- [ ] Renderer replacement is undoable.
- [ ] Asset validation is callable programmatically.
- [ ] Asset diagnostics are structured.
- [ ] Effects track their asset dependencies.
- [ ] Quality profiles can override renderer or asset choices.
- [ ] External AI generation is not required for basic Aestra authoring.

---

# 95. Updated Asset-Aware Architecture

The broader Aestra model becomes:

```text
"I want a magical explosion"
             |
             v
     Effect decomposition
             |
   +---------+----------+
   |                    |
Simulation            Renderer
   |                    |
   |               Asset selection
   |                    |
   |              existing asset?
   |                /        \
   |              yes         no
   |               |           |
   |               |      procedural?
   |               |        /       \
   |               |      yes        no
   |               |       |          |
   |               |    recipe     generate
   |               |       |          |
   +---------------+-------+----------+
                   |
                Material
                   |
                 Shader
                   |
                Preview
```

And the semantic dependency chain should remain:

```text
Simulation
    |
Renderer
    |
Asset
    |
Material
    |
Shader
```

The central principle is:

> AI should compose and transform explicit Aestra semantics. It should reuse existing assets first, create procedural assets second, and invoke external generation only when genuinely necessary.

# 96. Aestra Simulation, Execution & GPU Runtime Architecture

The current Aestra plan defines the authoring model well, but the runtime semantics must be equally explicit.

The core execution model should be treated as a first-class part of the Aestra language.

Aestra should define **what an effect means**, independently from the editor UI and independently from Bevy.

Conceptually:

```text
EffectAsset
    |
    v
Semantic validation
    |
    v
Compiler IR
    |
    +----------------------+
    |                      |
Stateful simulation     Stateless evaluation
    |                      |
    +----------+-----------+
               |
               v
           Renderers
               |
               v
        CompiledEffect
               |
               v
        EffectInstance
               |
               v
              Bevy
```

---

# 97. Effect Asset, Compiled Effect, and Runtime Instance

Do not blur authored data, compiled GPU data, and runtime instance state.

Use three distinct concepts.

## `EffectAsset`

Human-editable semantic source representation.

Contains:

```text
parameters
emitters
modules
stages
events
renderers
assets
materials
quality profiles
metadata
```

## `CompiledEffect`

Optimized immutable representation generated by the compiler.

May contain:

```text
shader programs
pipeline descriptors
buffer layouts
attribute layouts
renderer batches
dependency tables
constant data
compiled curves
event routing
platform specialization
```

## `EffectInstance`

Runtime state for one spawned effect.

May contain:

```text
world transform
runtime parameters
simulation time
seed
active emitters
GPU allocation handles
event state
quality tier
visibility state
```

Preferred lifecycle:

```text
EffectAsset
    |
 compile
    v
CompiledEffect
    |
 instantiate
    v
EffectInstance
```

This separation should be foundational.

---

# 98. Simplify Effect/System Terminology

Avoid introducing both `Effect` and `System` unless they have clearly different semantics.

Preferred authored hierarchy:

```text
EffectAsset
|
+-- Parameters
+-- Emitters
+-- Events
+-- Dependencies
+-- QualityProfiles
```

If a higher-order multi-system concept is needed later, it can be introduced explicitly.

The term `EffectAsset` should represent the complete authored VFX asset.

---

# 99. Execution Stage Model

Aestra needs an explicit execution-stage model.

Recommended conceptual hierarchy:

```text
Effect
|
+-- Effect Spawn
+-- Effect Update
|
+-- Emitters
    |
    +-- Emitter Spawn
    +-- Emitter Update
    |
    +-- Particle Spawn
    +-- Particle Update
    |
    +-- Simulation Stage *
    |
    +-- Renderers *
```

Every module executes in an explicit stage.

Example:

```rust
pub enum StageKind {
    EffectSpawn,
    EffectUpdate,

    EmitterSpawn,
    EmitterUpdate,

    ParticleSpawn,
    ParticleUpdate,

    SimulationStage(SimulationStageId),
}
```

The exact type structure can differ, but execution timing must never be implicit.

---

# 100. Execution Domain

Aestra should distinguish where logic runs.

Conceptually:

```rust
pub enum ExecutionDomain {
    Cpu,
    Gpu,
    Stateless,
}
```

Potential semantics:

## CPU

Used for:

```text
effect-level orchestration
low-frequency game integration
event routing
parameter updates
small control logic
```

## GPU

Used for:

```text
particle spawning
particle updates
sorting
large-scale simulation
grid simulation
GPU events
renderer preparation
```

## Stateless

Used for analytical effects that can derive state directly from:

```text
spawn time
current time
seed
initial values
parameters
```

without persistent per-frame simulation state.

The compiler should be free to specialize or reject unsupported domain combinations.

---

# 101. Simulation Domains

Do not assume every effect is permanently limited to ordinary particles.

Introduce an extensible simulation-domain concept.

Initial domains:

```rust
pub enum SimulationDomain {
    Particle,
    Strip,
}
```

Future-compatible domains:

```text
Grid2D
Grid3D
Volume
Custom
```

Grid simulation does not need to ship in the first versions.

However, the compiler/runtime architecture should not make it impossible without a redesign.

---

# 102. Particle Schema

Particle attributes should be explicit semantic data.

Example:

```rust
pub struct ParticleSchema {
    pub attributes: Vec<ParticleAttributeDefinition>,
}
```

Typical built-ins:

```text
Position      Vec3
PreviousPosition Vec3
Velocity      Vec3
Acceleration  Vec3

Age           F32
Lifetime      F32

Color         Vec4
Size          Vec2
Rotation      F32

ParticleId    U32
Seed          U32

Custom attributes...
```

Custom attributes must be supported.

Example:

```text
Heat          F32
Charge        F32
TrailWidth    F32
SurfaceNormal Vec3
```

The graph/compiler should use typed attribute references rather than hard-coded field offsets.

---

# 103. Particle Attribute Metadata

Attribute definitions may include:

```rust
pub struct ParticleAttributeDefinition {
    pub id: AttributeId,
    pub name: String,
    pub value_type: ValueType,
    pub semantic: Option<AttributeSemantic>,
    pub default_value: Value,
}
```

Potential semantics:

```text
Position
Velocity
Color
Lifetime
Age
Size
Normal
Tangent
Custom
```

This semantic metadata supports:

- compiler validation
- material bindings
- renderer compatibility
- AI understanding
- editor autocomplete

---

# 104. GPU Memory Model

Aestra must explicitly design particle storage instead of letting it emerge accidentally from WESL generation.

The runtime should define strategies for:

```text
particle capacity
alive particle count
free/dead particle allocation
spawn allocation
particle IDs
particle compaction
GPU counters
indirect dispatch
indirect draw
attribute buffer layout
```

Possible data layout strategies:

```text
Array of Structures
Structure of Arrays
hybrid layout
```

The compiler should be able to choose or specialize storage depending on the effect.

The semantic graph must not depend on raw GPU offsets.

---

# 105. Particle Allocation and Lifetime

The runtime should define:

```text
maximum capacity
spawn budget
alive list
dead/free list
particle death
particle reuse
```

Potential GPU flow:

```text
Spawn requests
      |
      v
Allocation counter
      |
      v
Initialize particle attributes
      |
      v
Alive particle set
      |
      v
Update
      |
      +-- alive --> next frame
      |
      +-- dead --> free list
```

This needs deterministic and profileable behavior.

---

# 106. Stable Particle Identity

Where practical, distinguish between:

```text
storage index
particle ID
random seed
```

Do not assume storage index is stable across compaction.

A stable particle identity is valuable for:

```text
events
ribbons
debugging
deterministic randomness
parent-child relationships
```

---

# 107. Sorting

Transparent VFX commonly require sorting.

Aestra should define renderer-level sorting options:

```text
None
ViewDepth
Distance
Age
CustomKey
```

Sorting may be:

```text
per emitter
per renderer
global batch
```

The runtime/compiler should support GPU sorting where required.

Sorting must be optional because it is expensive.

---

# 108. Culling and Bounds

Every effect needs explicit bounds behavior.

Possible modes:

```text
Automatic
Fixed
DynamicCPU
DynamicGPU
Infinite
```

Bounds affect:

```text
visibility culling
simulation throttling
draw submission
runtime budgets
editor preview
```

The editor should visualize bounds.

AI optimization should be able to detect bad/infinite bounds.

---

# 109. Indirect Dispatch and Indirect Draw

The GPU architecture should support indirect execution where useful.

Conceptually:

```text
GPU computes alive count
        |
        +--> indirect compute dispatch
        |
        +--> indirect draw
```

This reduces CPU synchronization and is important for large effect counts.

The authored model should remain independent of this implementation detail.

---

# 110. Multiple Renderers per Emitter

An emitter should support multiple renderers.

Do not model:

```rust
renderer: RendererConfig
```

Prefer:

```rust
renderers: Vec<Renderer>
```

Example:

```text
Particle Simulation
        |
        +-- SpriteRenderer
        +-- RibbonRenderer
        +-- LightRenderer
```

This allows one simulation to feed several visual representations without duplicating simulation work.

This is a P0 requirement.

---

# 111. Renderer Types

Initial renderer types should likely include:

```text
SpriteRenderer
MeshRenderer
RibbonRenderer
```

Later:

```text
FlipbookRenderer
BeamRenderer
LightRenderer
DecalRenderer
VolumeRenderer
```

Renderer definitions should be extensible.

Each renderer declares:

```text
required particle attributes
compatible simulation domains
material interface
sorting behavior
bounds contribution
render pipeline needs
```

---

# 112. Material System

Aestra needs an explicit material architecture.

Separate:

```text
MaterialDefinition
MaterialInstance
```

## `MaterialDefinition`

Defines:

```text
shader graph / WESL logic
input schema
particle attribute bindings
render state
texture bindings
feature flags
```

## `MaterialInstance`

Defines concrete values:

```text
textures
colors
scalar parameters
vectors
feature overrides
```

Example:

```text
MaterialDefinition: DissolveGlow

Inputs:
    BaseColor
    NoiseTexture
    DissolveAmount
    EdgeColor
    ParticleAge
```

---

# 113. Material Render State

Materials should express render behavior explicitly.

Examples:

```text
BlendMode
    Opaque
    Alpha
    Additive
    Premultiplied
    Multiply

Depth
    test
    write

Cull mode

Double-sided

Soft particles

Alpha clipping

Lighting mode

Emissive

Distortion

Motion vectors
```

The compiler should validate compatibility with the renderer and target platform.

---

# 114. Simulation-to-Material Attribute Binding

Materials must be able to consume simulation attributes.

Example:

```text
Particle.Color
Particle.Age
Particle.Lifetime
Particle.Custom0
        |
        v
Material inputs
```

Bindings should be typed and semantic.

Example:

```text
MaterialInput:
    AgeNormalized
        <- particle.age / particle.lifetime
```

Avoid hard-coding renderer-specific WESL variable names in authored data.

---

# 115. Data Interfaces

Aestra needs a generic way for VFX logic to access external scene/game data.

This should be a core semantic concept, not a Bevy-specific hack.

Conceptually:

```rust
pub trait DataInterfaceDefinition {
    ...
}
```

Potential interfaces:

```text
Transform
Camera
SceneDepth
SceneNormals
StaticMesh
SkinnedMesh
Texture2D
Texture3D
SignedDistanceField
VectorField
Curve
AudioSpectrum
Physics
ECSQuery
Grid2D
Grid3D
```

---

# 116. Data Interface Examples

## Spawn from a skinned mesh

```text
SkinnedMesh
    |
SampleSurfacePosition
    |
Particle.Position
```

## Scene collision

```text
SceneDepth
    |
Collision
    |
    +-- bounce
    +-- kill
    +-- emit impact event
```

## Audio-reactive effect

```text
AudioSpectrum
    |
FrequencyBand
    |
Particle.Size
```

The effect graph should interact with these interfaces semantically.

---

# 117. Bevy Data Interface Implementations

`aestra-core` defines the interface concept.

`aestra-bevy` provides concrete implementations.

Example:

```text
Aestra StaticMeshInterface
        |
        v
Bevy Mesh / Asset system
```

or:

```text
Aestra ECSQueryInterface
        |
        v
Bevy ECS query adapter
```

This preserves engine independence in the semantic layer.

---

# 118. Runtime Parameter Binding

Effects need dynamic inputs from gameplay.

Examples:

```text
SpellColor
Damage
ProjectileSpeed
TargetPosition
ImpactNormal
CasterTransform
ChargeAmount
Intensity
```

Use explicit parameter definitions.

Conceptually:

```rust
EffectParameter {
    id: ParameterId,
    name: String,
    value_type: ValueType,
    default_value: Value,
}
```

At runtime:

```text
EffectInstance
    |
set_parameter("ChargeAmount", 0.8)
```

Parameters should support:

```text
effect scope
emitter scope
material scope
renderer scope
```

with clear inheritance/override rules.

---

# 119. Parameter Sources

Parameters may come from:

```text
constant authored value
runtime game binding
curve
expression
data interface
other parameter
AI/user override
```

Astra should represent these sources semantically rather than as arbitrary graph hacks.

---

# 120. Events

Events should support both CPU and GPU paths where appropriate.

Potential events:

```text
OnSpawn
OnDeath
OnCollision
OnThreshold
OnCustom
```

Event actions:

```text
spawn particles
spawn child emitter
spawn child effect
set parameter
send gameplay notification
```

The runtime must define whether events:

```text
remain on GPU
cross to CPU
are delayed
are buffered
```

Crossing GPU -> CPU should be explicit because it can introduce synchronization cost.

---

# 121. GPU Spawn Events

Complex spell effects benefit from GPU-only relationships.

Example:

```text
Parent sparks
    |
OnDeath
    |
    v
Child embers
```

Prefer GPU-side event routing when the child simulation can remain on GPU.

This avoids unnecessary CPU readback.

---

# 122. Child Effects

Support explicit effect composition.

Example:

```text
Projectile Effect
    |
OnCollision
    |
    v
Spawn Child Effect: Impact
```

Child effects should support parameter forwarding:

```text
Impact.Position <- Collision.Position
Impact.Normal   <- Collision.Normal
Impact.Color    <- Projectile.Color
```

This should be semantic and inspectable.

---

# 123. Stateful and Stateless Emitters

Aestra should support both.

## Stateful

Persistent particle state is updated each frame.

Suitable for:

```text
collision
complex forces
neighbor interactions
dynamic events
long-running simulations
```

## Stateless

Particle state is derived analytically from:

```text
spawn time
current time
seed
initial conditions
parameters
```

Example:

```text
position(t) =
    initial_position
    + velocity * t
    + 0.5 * gravity * t²
```

No persistent per-frame position buffer is required for this class of effect.

---

# 124. Stateless Benefits

Stateless emitters can reduce:

```text
memory usage
simulation dispatches
tick overhead
buffer bandwidth
runtime allocation
```

They are ideal for:

```text
sparks
simple dust
simple projectile trails
ambient particles
short decorative effects
```

AI can later propose:

> Convert this emitter to stateless simulation.

when the graph is compatible.

---

# 125. Stateless Compatibility Analysis

The compiler should be able to determine whether an emitter can be stateless.

For example:

```text
Compatible:
    initial velocity
    gravity
    deterministic scale-over-life
    deterministic color-over-life

Not trivially compatible:
    arbitrary collision
    neighborhood forces
    particle-to-particle interaction
    feedback from prior frame
```

Expose the reason when stateless conversion is impossible.

---

# 126. Time Model

Time must be a first-class semantic/runtime concept.

Support:

```text
play
pause
restart
step frame
fixed timestep
variable timestep
time scale
pre-roll / warmup
looping
random seed
```

Editor preview needs predictable behavior.

---

# 127. Fixed vs Variable Timestep

Allow effects to declare or inherit simulation timing.

Potential modes:

```text
Variable
Fixed(60 Hz)
Fixed(120 Hz)
```

Fixed simulation may be useful for:

```text
determinism
collision stability
simulation cache
network replay
AI A/B comparison
```

The compiler/runtime should avoid forcing fixed timestep on effects that do not need it.

---

# 128. Timeline Scrubbing

Backward scrubbing is not trivial for stateful GPU simulation.

Aestra should define a strategy.

Possible editor approach:

```text
0s ------ 1s ------ 2s ------ 3s
          ^         ^
       snapshot   snapshot
```

When scrubbing backward:

```text
restore nearest prior snapshot
        |
re-simulate to target time
```

Alternative for deterministic cheap effects:

```text
restart
re-simulate from zero
```

The preview system can choose based on effect cost and available snapshots.

---

# 129. Simulation Checkpoints

Editor-only simulation checkpoints may contain:

```text
particle buffers
counters
emitter state
event state
random state
```

These are temporary preview data and should not be part of the authored effect asset.

---

# 130. Deterministic Randomness

Define explicit random seed hierarchy.

Example:

```text
EffectSeed
    |
EmitterSeed
    |
ParticleSeed
```

Preferred property:

```text
same effect
+ same seed
+ same parameters
+ same inputs
+ same timestep
=
same result
```

where technically feasible.

This is important for:

- testing
- editor preview
- regression checks
- AI comparisons
- recordings
- multiplayer replay

---

# 131. Random Functions

Random graph operations should derive from semantic IDs/seeds rather than hidden global RNG state.

Example:

```text
random(
    particle_seed,
    module_id,
    sample_index
)
```

This helps keep reordering and parallel execution deterministic.

---

# 132. Collision Architecture

Collision should be modular.

Initial collision types may include:

```text
Plane
Sphere
Box
```

GPU scene collision options:

```text
DepthBuffer
SceneNormals
```

Future:

```text
SignedDistanceField
Physics scene
Mesh distance
Voxel/grid collision
```

---

# 133. Collision Responses

Supported semantic responses:

```text
Kill
Bounce
Slide
Stick
EmitEvent
SpawnChildEffect
```

Collision output may include:

```text
position
normal
relative velocity
surface ID
```

These outputs should be available to event/module graphs.

---

# 134. Custom Modules

Aestra must not remain limited to a closed enum of built-in module kinds.

Avoid making the permanent architecture:

```rust
enum ModuleKind {
    Gravity,
    Drag,
    CurlNoise,
    ...
}
```

Instead distinguish:

```text
ModuleDefinition
ModuleInstance
```

---

# 135. Module Definitions

Conceptually:

```rust
pub struct ModuleDefinition {
    pub id: ModuleDefinitionId,
    pub name: String,
    pub inputs: Vec<PortDefinition>,
    pub outputs: Vec<PortDefinition>,
    pub supported_stages: Vec<StageKind>,
    pub implementation: ModuleImplementation,
}
```

Possible implementations:

```rust
pub enum ModuleImplementation {
    Builtin(BuiltinModuleId),
    Graph(ModuleGraph),
    Wgsl(CustomWgslModule),
}
```

This allows:

```text
built-in modules
project modules
plugins
subgraphs/functions
advanced WESL modules
```

---

# 136. User-Defined Module Example

Example technical-artist module:

```text
MyFancyForce

Inputs:
    origin: Vec3
    power: Float

Graph:
    Particle.Position
         |
    subtract origin
         |
      normalize
         |
    multiply power
         |
    add to Velocity
```

It should compile through the same IR as built-in modules.

---

# 137. Subgraphs and Functions

Support reusable graph functions.

Example:

```text
NoiseVector(position, frequency, speed)
```

used by multiple modules.

Benefits:

```text
reuse
consistency
smaller graphs
easier optimization
better AI composition
```

Subgraphs should have typed inputs and outputs.

---

# 138. Compiler Intermediate Representation

Do not compile the editor graph directly into ad-hoc WESL strings.

Introduce a compiler IR.

Conceptual pipeline:

```text
EffectAsset
    |
Semantic validation
    |
Graph lowering
    |
Aestra IR
    |
Optimization
    |
Backend lowering
    |
WESL / pipelines / runtime descriptors
```

The IR should represent:

```text
typed values
attributes
parameters
data-interface accesses
stage boundaries
control flow
math operations
events
renderer dependencies
```

---

# 139. Compiler Optimization Passes

Possible passes:

```text
constant folding
dead-code elimination
dead-attribute elimination
common subexpression elimination
parameter specialization
module fusion
stage fusion
buffer-layout optimization
renderer dependency pruning
stateless conversion
```

Do not require all of these initially.

The architecture should allow passes to be added incrementally.

---

# 140. Dead Attribute Elimination

If a graph never uses:

```text
Particle.Normal
Particle.Custom3
```

the compiler should ideally avoid allocating/storage for them.

This can materially reduce GPU bandwidth.

The particle schema used at runtime may therefore be a compiled subset of the authored schema.

---

# 141. Module Fusion

Modules may be semantically separate for editor readability but compile into one optimized shader.

Example authored graph:

```text
Gravity
  |
Drag
  |
Wind
```

may lower into a single compute kernel.

Editor modularity should not imply runtime dispatch overhead.

---

# 142. Stage Fusion

Where dependencies permit, multiple semantic stages/modules can be fused into fewer GPU passes.

The compiler is responsible for runtime efficiency.

This is another reason not to model the authored graph as raw GPU passes.

---

# 143. Runtime VFX Budget Manager

A production-quality engine needs runtime scalability based on aggregate cost.

Introduce a budget manager.

Example global budget:

```text
VFX Budget

GPU time            2.0 ms
Alive particles     500,000
Transparent pixels  25M
Draw calls          150
```

These are illustrative categories rather than fixed required metrics.

---

# 144. Effect Importance

Effects should have semantic importance categories.

Example:

```text
Critical
Hero
GameplayHigh
GameplayMedium
Ambient
Cosmetic
```

Runtime budget decisions may consider:

```text
importance
distance
visibility
screen size
current GPU pressure
effect count
platform profile
```

---

# 145. Runtime Quality Adaptation

The budget manager may choose:

```text
full quality
lower authored quality tier
reduced spawn rate
reduced capacity
cheaper renderer
baked representation
stateless variant
simulation throttling
culled
```

This should be data-driven and inspectable.

---

# 146. Effect Pooling

Effect instances should support pooling where useful.

Benefits:

```text
lower allocation churn
lower GPU resource churn
lower CPU overhead
predictable spikes
```

Pool compatibility may depend on compiled effect layout.

---

# 147. Simulation Throttling

Distant or low-importance effects may update less frequently.

Example:

```text
near camera:
60 Hz

medium distance:
30 Hz

far ambient:
15 Hz
```

Render interpolation may preserve visual smoothness where possible.

---

# 148. Renderer Batching

The runtime should batch compatible renderer instances.

Compatibility may depend on:

```text
compiled material
mesh
render state
shader variant
texture bindings
sort requirements
```

Batching is critical when many identical spell effects exist simultaneously.

---

# 149. GPU Instancing

Mesh and sprite renderers should use instancing where possible.

Example:

```text
300 shard particles
        |
one mesh
one material
        |
GPU instanced draw
```

Avoid per-particle draw calls.

---

# 150. Texture Atlas and Bindless Future Compatibility

Aestra should avoid assumptions that force one texture binding per effect forever.

Potential future strategies:

```text
texture atlases
texture arrays
bindless resources
material tables
```

The semantic asset model should remain independent of the chosen GPU binding strategy.

---

# 151. Debugging Architecture

A next-generation editor needs first-class VFX debugging.

Potential views:

```text
Particle Inspector
Attribute Heatmap
Event Viewer
Spawn Graph
GPU Pass Viewer
Bounds Viewer
Overdraw Viewer
Renderer Cost Breakdown
```

---

# 152. Particle Inspector

For selected particles, expose attributes:

```text
Particle #1284

Position
Velocity
Age
Lifetime
Color
Size
Custom attributes
```

For GPU effects, this may require selective readback or debug capture.

Debug features should be optional and disabled in production runtime builds.

---

# 153. Attribute Visualization

Allow semantic debug visualization.

Examples:

```text
Color by velocity magnitude
Color by age
Color by custom scalar
Draw velocity vectors
Draw normals
Draw bounds
```

These are extremely valuable for technical artists.

---

# 154. Event Debugging

Expose event flow:

```text
Emitter A
  OnDeath: 128 events
      |
      v
Emitter B
  Spawned: 128 particles
```

This helps diagnose event loops and unexpected spawning.

---

# 155. GPU Pass Debugging

Profiler/debugger should expose compiled execution stages.

Example:

```text
Pass 1
ParticleSpawn
0.03 ms

Pass 2
ParticleUpdate
0.11 ms

Pass 3
Sort
0.07 ms

Pass 4
RibbonPrepare
0.04 ms

Draw
0.18 ms
```

This can later feed AI optimization.

---

# 156. Runtime Parameter Debugging

The editor should show:

```text
authored default
runtime override
source binding
final resolved value
```

Example:

```text
ChargeAmount

Authored: 0.5
Runtime binding: PlayerSpell.charge
Current: 0.82
```

This prevents difficult debugging when gameplay drives effects dynamically.

---

# 157. Viewport Gizmos

Professional authoring needs direct manipulation in the preview.

Potential gizmos:

```text
spawn sphere
spawn cone
force direction
vortex center
collision plane
beam endpoints
ribbon source
vector field bounds
effect bounds
```

Changes through gizmos should create normal semantic commands.

Therefore:

```text
Viewport gizmo
      |
EffectCommand
      |
CommandExecutor
```

AI and manual interactions continue sharing the same mutation system.

---

# 158. Curve and Gradient Editing

Curves and gradients should be first-class editors.

Support:

```text
multi-key curves
tangent editing
presets
normalized lifetime mode
absolute time mode
color gradients
alpha gradients
```

The AI can create/edit the same semantic curves.

---

# 159. Presets and Templates

Aestra should support reusable semantic presets.

Examples:

```text
EmitterPreset
ModulePreset
RendererPreset
MaterialPreset
EffectTemplate
```

Examples:

```text
Impact Burst
Magic Trail
Soft Smoke
Spark Shower
Shockwave
```

Presets should remain editable after insertion.

---

# 160. Style-Safe AI Composition

AI generation should compose from project-approved presets when possible.

Example:

```text
Project style:
Arcana / Oblivion
```

The AI can preferentially use:

```text
OblivionTrailPreset
OblivionImpactPreset
OblivionMaterialPreset
```

instead of inventing unrelated assets.

This improves visual consistency.

---

# 161. Plugin and Extension Model

Aestra should be extensible without modifying the core repository.

Possible extension categories:

```text
Module definitions
Renderer types
Data interfaces
Asset processors
Importers
Exporters
AI providers
Generation providers
Editor panels
```

The exact plugin ABI/API can be designed later.

However, core registries should be built so extensions can eventually register semantic definitions.

---

# 162. Runtime Capability Model

Compiled effects should validate against target runtime capabilities.

Possible capabilities:

```text
compute shaders
storage buffers
indirect draw
texture arrays
specific texture formats
subgroup operations
atomics
```

Effects can declare:

```text
required capabilities
optional capabilities
fallback path
```

This matters for:

```text
desktop
Steam Deck
WebGPU
mobile
```

---

# 163. Platform Profiles

Platform profiles should define:

```text
capabilities
performance budgets
texture limits
particle limits
preferred quality tier
```

Example:

```text
SteamDeckProfile
MobileHighProfile
WebGpuProfile
DesktopUltraProfile
```

The compiler can specialize effects per profile.

---

# 164. Compile-Time Quality Specialization

Do not rely only on runtime branches.

For each target profile, the compiler may specialize:

```text
shader variants
buffer layout
renderer selection
module selection
texture assets
capacity
```

This reduces runtime overhead.

---

# 165. Simulation Cache

Longer term, support simulation baking/cache.

Use cases:

```text
cinematics
expensive deterministic simulations
editor scrubbing
flipbook baking
network replay
```

Potential cache content:

```text
particle snapshots
renderer geometry
event data
```

This is a P2 feature but should align with the time/checkpoint model.

---

# 166. Grid Simulation Hook

Do not implement full fluids immediately.

But reserve a semantic path for:

```text
Grid2D
Grid3D
```

Potential future use cases:

```text
smoke
fire
fog
fluid advection
vector fields
reaction-diffusion
volumetric magic
```

Grid stages should conceptually fit into the same compiler/runtime model.

---

# 167. Example Full Execution Flow

Example authored effect:

```text
Oblivion Meteor
|
+-- Projectile Emitter
|   |
|   +-- Particle Spawn
|   |   +-- InitializePosition
|   |   +-- InitializeVelocity
|   |
|   +-- Particle Update
|   |   +-- Gravity
|   |   +-- CurlNoise
|   |   +-- Lifetime
|   |
|   +-- Renderers
|       +-- MeshRenderer
|       +-- RibbonRenderer
|
+-- Impact Emitter
    |
    +-- triggered by collision event
```

Compiler flow:

```text
EffectAsset
    |
    v
Semantic validation
    |
    v
Stage graph
    |
    v
Attribute liveness analysis
    |
    v
Aestra IR
    |
    +-- remove unused attributes
    +-- fold constants
    +-- fuse modules
    +-- choose stateful/stateless path
    |
    v
WESL + pipeline descriptors
    |
    v
CompiledEffect
```

Runtime:

```text
Spawn EffectInstance
        |
Resolve runtime parameters
        |
Allocate GPU state
        |
Dispatch spawn/update
        |
GPU event routing
        |
Renderer preparation
        |
Sort/batch/cull
        |
Draw
```

---

# 168. Revised Critical Dependency Order

The project roadmap should be interpreted in this order.

## P0 — Semantic language and runtime foundation

```text
1. EffectAsset / CompiledEffect / EffectInstance separation
2. Stable semantic IDs
3. Execution stages
4. Particle schema and attribute system
5. Parameters and runtime bindings
6. Data interfaces
7. Renderer abstraction
8. Multiple renderers per emitter
9. Asset/material separation
10. ModuleDefinition / ModuleInstance model
11. Compiler IR
12. GPU memory/allocation model
13. Basic GPU runtime
14. Preview runtime
```

## P1 — Professional authoring and scalability

```text
15. Commands / undo / transactions
16. Semantic diff
17. Timeline / deterministic seeds
18. Scrubbing/checkpoints
19. Collision
20. Events / child effects
21. Stateful + stateless emitters
22. Culling / bounds
23. Sorting / batching / indirect draw
24. Runtime budget manager
25. Material editor
26. Custom modules / subgraphs
27. Profiler/debugger
28. Viewport gizmos
29. Quality/platform profiles
```

## P2 — Advanced capabilities

```text
30. Simulation cache
31. Grid2D/Grid3D hooks
32. Flipbook baking pipeline
33. External generation providers
34. MCP
35. Embedded local models
36. Semantic retrieval
37. Multimodal AI
```

## AI integration order

AI should begin only after the deterministic editing model and core VFX language are stable enough that AI can manipulate them safely.

Recommended AI sequence:

```text
Inspect
    |
Explain
    |
Edit parameters
    |
Add/remove known modules
    |
Create transactions
    |
Preview/diff
    |
Optimize
    |
Compose presets
    |
Generate full effects
    |
Generate/import external assets
```

---

# 169. Updated P0 Acceptance Criteria

Aestra should not consider the core runtime architecture stable until:

- [ ] `EffectAsset`, `CompiledEffect`, and `EffectInstance` are distinct concepts.
- [ ] Execution stages are explicit.
- [ ] Every module declares valid stages.
- [ ] Particle attributes are typed and semantic.
- [ ] Custom particle attributes are supported.
- [ ] The compiler controls runtime particle layout.
- [ ] Simulation storage indices are not treated as permanent particle IDs.
- [ ] Runtime parameters are explicit and bindable.
- [ ] Data interfaces exist as a core abstraction.
- [ ] Bevy-specific data-interface implementations live outside `aestra-core`.
- [ ] An emitter supports multiple renderers.
- [ ] Renderers declare required particle attributes.
- [ ] Simulation, renderer, asset, material, and shader remain separate concepts.
- [ ] Materials expose typed inputs.
- [ ] Particle attributes can bind to material inputs.
- [ ] Module definitions are not permanently limited to a closed Rust enum.
- [ ] User/project-defined modules are architecturally possible.
- [ ] A compiler IR exists between semantic graphs and WESL generation.
- [ ] The runtime can allocate, update, kill, and compact particles on GPU.
- [ ] Indirect dispatch/draw is architecturally possible.
- [ ] Sorting is optional and renderer-controlled.
- [ ] Bounds/culling are explicit.
- [ ] The editor can preview a compiled effect independently of the production game.
- [ ] The architecture does not require AI to function.

---

# 170. Updated P1 Acceptance Criteria

Professional editor/runtime readiness should additionally require:

- [ ] Fixed and variable timestep modes are defined.
- [ ] Seeded deterministic preview exists where feasible.
- [ ] Timeline restart and frame stepping work.
- [ ] Backward scrubbing has a defined checkpoint/re-simulation strategy.
- [ ] Collision is modular.
- [ ] Collision outputs can generate events.
- [ ] GPU-side emitter relationships are supported or explicitly planned.
- [ ] Child effects can receive forwarded parameters.
- [ ] Stateful and stateless emitter paths exist or share a compatible semantic model.
- [ ] Runtime quality tiers can alter spawn rate/capacity/renderers.
- [ ] A global VFX budget manager exists.
- [ ] Compatible renderer instances can batch.
- [ ] Mesh particle rendering uses instancing.
- [ ] Profiler exposes per-effect and per-emitter cost.
- [ ] Debugger can inspect attributes/events at least in debug mode.
- [x] Viewport gizmos mutate semantic data through normal commands.
- [ ] Technical artists can create reusable subgraphs/functions.
- [ ] Quality/platform profiles are explicit and compiler-visible.

---

# 171. Revised Product Architecture

The complete intended architecture should converge toward:

```text
                      Human Artist
                           |
           +---------------+---------------+
           |                               |
      Structured UI                    AI Intent
           |                               |
           +---------------+---------------+
                           |
                    Aestra Commands
                           |
                    EffectTransaction
                           |
                    Semantic Effect Model
                           |
        +------------------+------------------+
        |                  |                  |
    Parameters         Data Interfaces     Asset Registry
        |                  |                  |
        +------------------+------------------+
                           |
                    Execution Stages
                           |
                     Compiler Frontend
                           |
                       Aestra IR
                           |
        +------------------+------------------+
        |                  |                  |
   Optimization       Stateful GPU       Stateless
      Passes           Simulation        Evaluation
        |                  |                  |
        +------------------+------------------+
                           |
                       Renderers
                           |
        +------------------+------------------+
        |                  |                  |
      Sprite              Mesh              Ribbon
        |                  |                  |
        +------------------+------------------+
                           |
                       Materials
                           |
                         WESL
                           |
                        wgpu/Bevy
```

External systems remain adapters:

```text
AI Providers
MCP
3D Generation
Image Generation
Importers
Exporters
```

They must not define Aestra's internal semantic model.

---

# 172. Revised Definition of "Next-Generation"

Aestra should not define "next-generation" as:

```text
Niagara
+
Rust
+
AI chat
```

The stronger target is:

```text
A semantic VFX language
+
GPU-native compiler/runtime
+
stateful and stateless execution
+
multi-renderer composition
+
procedural and authored assets
+
technical-artist extensibility
+
deterministic editing
+
production profiling/scalability
+
AI as a first-class authoring client
```

The AI is powerful because the VFX language is structured.

The editor is powerful because the runtime semantics are explicit.

The runtime is efficient because the compiler owns the lowering and optimization strategy.

This is the architecture that gives Aestra a realistic path toward becoming more than a Bevy particle editor and instead becoming a full VFX authoring platform.
