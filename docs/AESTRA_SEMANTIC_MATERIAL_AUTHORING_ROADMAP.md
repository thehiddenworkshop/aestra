# Aestra Semantic Material Authoring Roadmap

## Goal

Evolve Aestra's current material system into an AI-friendly, graph-friendly, programmable semantic authoring system without replacing the existing architecture with WESL.

The target is:

- `MaterialProgram` remains the canonical semantic material representation.
- The node graph is a projection of `MaterialProgram`, not a separate source of truth.
- AI edits materials through semantic commands and discovery APIs.
- WESL becomes the programmable implementation language for reusable/custom material functions and an advanced escape hatch.
- The compiler never needs a `WESL -> Material IR -> WESL` round-trip.
- Runtime and GPU-specific details remain separated from authoring semantics.

---

# 1. Current Architecture Assessment

Aestra already contains most of the architectural pieces needed for semantic material authoring:

- engine-independent material semantics;
- `MaterialProgram` as a typed semantic material representation;
- typed material expressions;
- semantic inputs such as particle age, velocity, world position, scene depth, view direction and effect time;
- backend-neutral `MaterialIrProgram`;
- deterministic graph projection;
- stack projection;
- semantic authoring transactions;
- semantic diffs and undo/redo-friendly operations;
- a machine-readable material API suitable for external tools and AI;
- WESL/WGSL generation in the GPU path;
- Naga validation/lowering;
- a separation between authoring/compiler crates and Bevy-specific presentation/runtime integration.

This means Aestra does **not** need a new material architecture from scratch.

The main work is to generalize the existing semantic system and add a programmable WESL-backed function layer.

---

# 2. Target Architecture

```text
                        AUTHORING
                            │
        ┌───────────────────┼───────────────────┐
        │                   │                   │
       AI               Node Graph          Stack UI
        │                   │                   │
        └───────────────────┼───────────────────┘
                            │
                    Semantic commands
                            │
                            ▼
                     MaterialProgram
                    semantic material DAG
                            │
                            ▼
                      Material IR
                            │
               ┌────────────┴────────────┐
               │                         │
      Built-in semantic funcs      Project WESL funcs
               │                         │
               └────────────┬────────────┘
                            ▼
                      WESL composer
                            │
                            ▼
                         wesl-rs
                            │
                            ▼
                           WGSL
                            │
                            ▼
                           Naga
                            │
                ┌───────────┼───────────┐
                ▼           ▼           ▼
              Bevy       future      future
                         Godot       engines
```

The compiler path should remain one-way:

```text
MaterialProgram
      ↓
Material IR
      ↓
WESL composition
      ↓
WGSL / Naga
```

There should be no compiler path like:

```text
WESL -> Material IR -> WESL
```

---

# 3. Keep `MaterialProgram` as the Source of Truth

Do not replace `MaterialProgram` with a WESL AST.

`MaterialProgram` already provides the correct semantic abstraction for Aestra because it expresses concepts such as:

- parameters;
- material inputs;
- expression dependencies;
- semantic operations;
- material outputs;
- evaluation domains;
- material domains;
- render-state policy.

This representation is much better for:

- AI manipulation;
- graph visualization;
- deterministic editing;
- validation;
- semantic diffs;
- migration/versioning;
- engine portability.

WESL should extend the semantic system, not replace it.

---

# 4. Introduce First-Class Material Functions

## Problem

The current material expression model contains one Rust enum variant for many operations.

Conceptually:

```rust
MaterialExpressionKind::Fresnel { ... }
MaterialExpressionKind::RadialMask { ... }
MaterialExpressionKind::Dissolve { ... }
MaterialExpressionKind::DepthFade { ... }
MaterialExpressionKind::SoftParticle { ... }
MaterialExpressionKind::PanUv { ... }
MaterialExpressionKind::RotateUv { ... }
```

This works while the vocabulary is small, but it will become expensive when Aestra grows to hundreds of material operations.

Examples that would otherwise require new enum variants:

```text
SimplexNoise
PerlinNoise
Voronoi
CurlNoise
FlowMap
NormalBlend
Triplanar
Parallax
PolarCoordinates
Twirl
Spherize
Blackbody
HSV
Contrast
Posterize
DistanceField
ChromaticAberration
Refraction
HeatDistortion
...
```

Every new hard-coded operation risks touching:

```text
aestra-core
    ↓
validation
    ↓
compiler IR
    ↓
graph projection
    ↓
stack projection
    ↓
GPU WESL lowering
    ↓
reflection
    ↓
AI API
```

## Proposed solution

Introduce a first-class material function registry.

Conceptually:

```rust
pub enum MaterialExpressionKind {
    Input { ... },
    Parameter { ... },
    Constant { ... },

    Add { ... },
    Multiply { ... },
    Divide { ... },
    Lerp { ... },

    Call {
        function: MaterialFunctionId,
        arguments: Vec<MaterialExpressionId>,
    },
}
```

Higher-level operations move toward `Call` instead of requiring a dedicated enum variant.

Example function definition:

```rust
pub struct MaterialFunctionDefinition {
    pub id: MaterialFunctionId,
    pub label: String,
    pub category: MaterialFunctionCategory,
    pub description: String,

    pub inputs: Vec<MaterialFunctionInput>,
    pub output: MaterialValueType,

    pub supported_domains: Vec<MaterialDomain>,
    pub evaluation_domain: EvaluationDomain,

    pub implementation: MaterialFunctionImplementation,
}
```

Example:

```text
id: aestra.material.fresnel
label: Fresnel
category: Lighting

inputs:
- normal: Vec3
- view: Vec3
- power: Float

output:
- Float

semantic description:
- "Produces a view-angle dependent edge mask."
```

The same definition should drive:

- node palette entries;
- graph pins;
- type checking;
- AI discovery;
- documentation;
- search;
- stack presentation where appropriate;
- WESL lowering;
- compatibility checks.

This creates one source of truth for the semantic material vocabulary.

---

# 5. Material Function Implementation Types

A material function should support multiple implementation strategies.

Conceptually:

```rust
pub enum MaterialFunctionImplementation {
    BuiltIn(BuiltInMaterialFunction),
    Wesl(WeslMaterialFunction),
}
```

Potential future extensions:

```rust
Graph(MaterialFunctionGraph),
NativeBackend(...),
ExternalPackage(...),
```

## Built-in functions

Use for extremely common primitives and operations requiring compiler/backend integration.

Examples:

```text
Add
Multiply
Lerp
TextureSample
SceneDepth
Derivative-aware operations
```

## WESL-backed functions

Use for reusable programmable operations.

Example:

```wesl
fn fire_turbulence(
    uv: vec2f,
    time: f32,
    scale: f32,
) -> f32 {
    // implementation
}
```

Aestra exposes this as one semantic function:

```text
┌────────────────────────────┐
│ Fire Turbulence            │
│                            │
○ UV                         │
○ Time                       │
○ Scale                      │
│                     Value ○│
└────────────────────────────┘
```

The graph does not need to visualize every WESL statement.

This provides **100% execution extensibility without turning the material graph into a general-purpose visual programming language**.

---

# 6. WESL Should Be an Implementation Language, Not the Material Format

Aestra should not initially define the material itself as arbitrary WESL.

Instead:

```text
MaterialProgram
    ↓
semantic function calls
    ↓
WESL implementation modules
```

This preserves:

- graphability;
- semantic understanding;
- AI reliability;
- typed domains;
- deterministic edits;
- clean diffs;
- portability.

WESL becomes the escape hatch when the built-in semantic vocabulary is insufficient.

---

# 7. Create an Aestra WESL Standard Library

Introduce a semantic WESL library rather than exposing raw backend bindings.

Possible package layout:

```text
aestra::math
aestra::color
aestra::texture

aestra::material
aestra::surface

aestra::particle
    age()
    normalized_age()
    lifetime()
    velocity()
    color()
    size()
    position()

aestra::scene
    depth()
    color()
    camera_position()
    view_direction()

aestra::coordinates
    world_position()
    object_position()
    uv()
    screen_uv()
    world_to_object()
    object_to_world()

aestra::noise
    simplex()
    perlin()
    voronoi()
    fbm()
    curl()
```

The AI and users should write semantic expressions such as:

```wesl
let age = particle::normalized_age();
let depth = scene::depth(screen_uv);
let edge = material::fresnel(normal, view_dir, power);
```

rather than low-level buffer/binding expressions.

This semantic vocabulary is the key to making authored WESL AI-friendly.

---

# 8. Generalize Material Outputs

## Current limitation

The current semantic material output contract is effectively centered around:

```text
Color
Alpha
```

That is enough for the existing sprite-focused path, but insufficient for the declared material domains.

## Proposed model

Introduce semantic output identifiers.

```rust
pub enum MaterialOutputSemantic {
    Color,
    Alpha,
    BaseColor,
    Emissive,
    Normal,
    Roughness,
    Metallic,
    Occlusion,
    VertexOffset,
    Distortion,
    Custom(...),
}
```

Each `MaterialDomain` exposes an output contract.

Example:

```text
Sprite
├─ Color
├─ Alpha
├─ Emissive
└─ VertexOffset

Mesh
├─ BaseColor
├─ Alpha
├─ Emissive
├─ Normal
├─ Roughness
├─ Metallic
├─ Occlusion
└─ VertexOffset

Ribbon
├─ Color
├─ Alpha
├─ Emissive
└─ VertexOffset

Decal
├─ BaseColor
├─ Alpha
├─ Normal
├─ Roughness
└─ Metallic
```

The compiler validates that:

- the output exists for the domain;
- the expression type matches;
- required outputs are present;
- unsupported outputs are rejected.

This is also important for AI discovery.

The AI should be able to query:

```text
list_material_outputs(domain = Sprite)
```

instead of guessing which channels exist.

---

# 9. Finish the Legacy Material Migration Before Adding Authored WESL

Aestra currently contains both:

- the legacy sprite material path;
- the semantic `MaterialProgram` path.

Finish the migration so the semantic material system becomes the single normal authoring path before introducing user/project-authored WESL functions.

Otherwise Aestra temporarily needs to maintain three overlapping systems:

```text
legacy sprite materials
+
semantic MaterialProgram
+
WESL-authored material extensions
```

That would make the transition harder than necessary.

Recommended order:

```text
legacy material compatibility
          ↓
MaterialProgram becomes canonical
          ↓
semantic function registry
          ↓
WESL-backed functions
```

---

# 10. Preserve the Existing Semantic Transaction Architecture

The current authoring architecture is one of Aestra's strongest pieces and should remain central.

Desired flow:

```text
UI / AI request
     ↓
semantic command
     ↓
plan transaction
     ↓
validate resulting MaterialProgram
     ↓
produce semantic diff
     ↓
commit
     ↓
undo/redo through inverse transaction
```

The graph must **not** mutate graph JSON as its source of truth.

The AI must **not** rewrite serialized RON documents directly.

Both should use the same semantic authoring commands.

---

# 11. Expand the Material Tool API for Full Material Creation

The current tool layer already supports semantic material edits, but it should evolve beyond edit-oriented commands.

Add operations in several families.

## Material lifecycle

```text
CreateMaterial
DuplicateMaterial
DeleteMaterial
CreateMaterialInstance
AssignMaterial
```

## Parameters

```text
AddParameter
RemoveParameter
RenameParameter
SetParameterDefault
SetParameterRange
SetParameterMetadata
ExposeParameter
```

## Semantic graph construction

```text
UseInput
AddConstant
AddFunctionCall
Connect
Disconnect
ReplaceExpression
RemoveExpression
SetOutput
ClearOutput
```

## Render/domain configuration

```text
SetMaterialDomain
SetBlendMode
SetDepthPolicy
SetCullPolicy
SetRenderState
```

## Programmable extension

```text
CreateWeslFunction
ReplaceWeslFunction
DeleteWeslFunction
ImportWeslModule
```

The important point is that these remain **semantic commands**, not graph-layout commands.

---

# 12. Add a Discoverable Material Vocabulary API

Do not force AI clients to know all Aestra material operations beforehand.

Provide discoverable capabilities.

Example API surface:

```text
list_material_functions(...)
describe_material_function(...)
search_material_functions(...)

list_material_inputs(domain)
list_material_outputs(domain)
list_material_domains()

describe_material_domain(domain)
```

Example response:

```json
{
  "id": "aestra.material.fresnel",
  "label": "Fresnel",
  "description": "Produces a view-angle dependent edge mask.",
  "inputs": [
    { "name": "normal", "type": "Vec3" },
    { "name": "view", "type": "Vec3" },
    { "name": "power", "type": "Float" }
  ],
  "output": "Float",
  "domains": ["Sprite", "Mesh", "Ribbon"]
}
```

This lets AI dynamically learn Aestra's current capabilities.

It also helps:

- editor palettes;
- command search;
- documentation generation;
- external plugins;
- scripting APIs;
- marketplace packages later.

---

# 13. AI Material Creation Workflow

The preferred AI path should be semantic first.

User request:

> Create stylized blue fire that fades late in its lifetime and becomes brighter near the edges.

The AI queries Aestra:

```text
Domain: Sprite

Available inputs:
- ParticleNormalizedAge
- ParticleColor
- Normal
- ViewDirection
- UV
- EffectTime

Available outputs:
- Color
- Alpha
- Emissive

Relevant functions:
- Smoothstep
- Gradient
- Fresnel
- Multiply
- Lerp
```

Then the AI constructs a semantic plan:

```text
CreateMaterial("Blue Fire", Sprite)

AddParameter("Base Color", Color, ...)
AddParameter("Edge Color", Color, ...)
AddParameter("Edge Power", Float, 4.0)
AddParameter("Emission", Float, 5.0)
AddParameter("Fade Start", Float, 0.8)

Age = Input(ParticleNormalizedAge)

Fade = Smoothstep(FadeStart, 1.0, Age)

Edge = Fresnel(
    Normal,
    ViewDirection,
    EdgePower
)

SetOutput(Color, ...)
SetOutput(Alpha, 1 - Fade)
SetOutput(Emissive, Edge * EdgeColor * Emission)
```

Aestra validates and returns a semantic diff:

```text
✓ types valid
✓ domain inputs available
✓ output contract valid
✓ function compatibility valid
✓ generated shader valid

Semantic diff:
+ material Blue Fire
+ 5 parameters
+ 11 expressions
+ Color output
+ Alpha output
+ Emissive output
```

The graph is then derived automatically from the resulting `MaterialProgram`.

---

# 14. AI Should Use WESL Only When Necessary

Semantic authoring should be the normal path.

Example:

```text
AI intent
   ↓
known semantic operation?
   ├─ yes -> MaterialToolCommand
   └─ no  -> create/reuse WESL-backed function
```

If the AI needs a custom algorithm not present in the function registry:

```wesl
fn curl_distortion(
    p: vec3f,
    time: f32,
    strength: f32,
) -> vec2f {
    // custom implementation
}
```

Then the material simply contains:

```text
Call(aestra.project.curl_distortion)
```

The graph sees one node.

This avoids having the AI regenerate entire shader sources for small edits.

---

# 15. Graph/WESL Parity Policy

Do not require arbitrary WESL statements to map one-to-one to graph nodes.

Target parity should be:

```text
Semantic graph -> compiled shader: 100%

Built-in semantic function -> graph: 100%

WESL-backed function -> graph node: 100%

Arbitrary WESL implementation internals -> graph statements:
not required
```

A custom WESL function should appear as an atomic graph node with typed pins.

This keeps the graph readable.

---

# 16. Source Mapping and Diagnostics

As WESL-backed functions are added, preserve mapping between:

```text
MaterialExpressionId
MaterialFunctionId
WESL module/function
Generated WGSL
Naga diagnostic
```

Diagnostics shown to users or AI should preferably refer back to semantic concepts.

Avoid exposing only low-level messages such as:

```text
@group(2) @binding(7) invalid
```

Prefer diagnostics such as:

```text
SceneDepth cannot be sampled in this material domain.

Expression:
    Depth Fade

Suggestion:
    Use SoftParticle or change the material domain/capabilities.
```

Semantic diagnostics make automated AI correction much more reliable.

---

# 17. Consider a Dedicated Shader Composition Layer Later

Currently WESL generation lives in the GPU path.

That is acceptable while WESL is only an implementation detail.

Once project-defined WESL functions, packages and authoring-time shader validation become important, consider introducing a clearer portable shader composition boundary.

Possible options:

```text
crates/aestra-shader/
```

or:

```text
crates/aestra-compiler/src/shader/
```

Potential responsibilities:

```text
Material IR
+
Aestra WESL standard library
+
project WESL modules
+
function registry
+
source maps
+
resource declarations
+
capability requirements
        ↓
portable resolved shader artifact
```

Then:

```text
aestra-gpu
```

handles:

- backend/GPU capabilities;
- pipeline layouts;
- GPU resource creation;
- runtime specialization;
- Bevy/wgpu integration.

Do **not** perform this crate split immediately.

First implement WESL-backed semantic functions through the current path and move the boundary only once the responsibilities become concrete.

---

# 18. Suggested Material Function Categories

A function registry should support semantic categories from the start.

Possible initial taxonomy:

```text
Math
├─ arithmetic
├─ remapping
├─ interpolation
└─ comparison

Color
├─ RGB/HSV
├─ gradient
├─ contrast
├─ saturation
└─ blackbody

Coordinates
├─ UV
├─ world/object
├─ polar
├─ rotate
├─ scale
└─ pan

Noise
├─ simplex
├─ perlin
├─ voronoi
├─ FBM
└─ curl

Particle
├─ age
├─ lifetime
├─ velocity
├─ size
└─ color

Scene
├─ depth
├─ scene color
├─ camera
└─ view direction

Mask
├─ radial
├─ box
├─ gradient
├─ dissolve
└─ threshold

Surface
├─ fresnel
├─ normals
├─ lighting helpers
└─ depth fade

Distortion
├─ flow map
├─ heat distortion
├─ refraction
└─ UV turbulence
```

This taxonomy should drive both the editor node palette and AI search.

---

# 19. Semantic Metadata for AI and UI

Each material function should expose more than just types.

Recommended metadata:

```rust
pub struct MaterialFunctionMetadata {
    pub label: String,
    pub short_description: String,
    pub semantic_tags: Vec<String>,
    pub category: MaterialFunctionCategory,

    pub cost_hint: MaterialCostHint,
    pub determinism: Determinism,
    pub supported_domains: Vec<MaterialDomain>,
    pub required_capabilities: Vec<MaterialCapability>,

    pub recommended_for_ai: bool,
}
```

Useful semantic tags:

```text
edge glow
fade
lifetime
noise
fire
smoke
distortion
soft particles
depth
flow
mask
```

This allows an AI to search semantically rather than relying purely on exact function names.

---

# 20. Parameters Should Remain First-Class Semantic Controls

AI should prefer reusable parameters over hard-coded constants when creating reusable effects.

Example:

Instead of:

```text
Emissive = Color * 7.37
```

create:

```text
Parameter: EmissionStrength
Default: 5.0
Range: 0.0 .. 20.0

Emissive = Color * EmissionStrength
```

Parameter metadata should support:

```text
label
category/group
default value
range
step
units
color space
description
visibility
advanced/basic flag
```

This improves:

- inspectors;
- effect reuse;
- animation/automation;
- material instances;
- AI edits;
- marketplace assets.

---

# 21. Relationship with Effect Automation

Material parameters should be addressable from Aestra's automation/timeline system.

Conceptually:

```text
MaterialProgram
    └─ ParameterId("emission_strength")

EffectClip / Region automation
    └─ target MaterialParameterId("emission_strength")
```

This is important because semantic material authoring should integrate with the wider VFX timeline rather than become an isolated shader editor.

AI should be able to express requests such as:

> Increase distortion during the last 20% of the particle lifetime.

and choose between:

- a material expression based on normalized particle age;
- an exposed parameter driven by effect automation;

based on the intended semantics.

---

# 22. Package and Marketplace Compatibility

A first-class function registry also creates a natural future package boundary.

A package could provide:

```text
material functions
WESL modules
textures
curves
gradients
material presets
complete materials
```

Example:

```text
aestra-fire-pack
├─ functions/
│  ├─ flame_shape
│  ├─ ember_noise
│  └─ heat_distortion
├─ materials/
│  ├─ stylized_fire
│  └─ realistic_fire
└─ textures/
```

The semantic function ID should therefore be stable and namespaced from the beginning.

Example:

```text
aestra.material.fresnel
aestra.noise.simplex
project.fire.curl_distortion
marketplace.vendor.package.flame_shape
```

---

# 23. Implementation Milestones

## Milestone 1 — Canonical semantic material path

- finish legacy sprite-material migration;
- make `MaterialProgram` the normal material source of truth;
- keep compatibility adapters only where required;
- add regression tests covering legacy visual equivalence.

## Milestone 2 — Rich material output contracts

- introduce `MaterialOutputSemantic`;
- define outputs per `MaterialDomain`;
- update validation;
- update `MaterialIrProgram`;
- update graph/stack projections;
- update GPU lowering;
- expose output discovery through the material API.

## Milestone 3 — Material function registry

- introduce `MaterialFunctionId`;
- add `MaterialExpressionKind::Call`;
- define function metadata and typed interfaces;
- add registry discovery/search;
- migrate selected higher-level enum operations to registry-backed functions;
- preserve compatibility during migration.

Start by migrating semantic operations such as:

```text
Fresnel
RadialMask
Dissolve
DepthFade
SoftParticle
PanUv
RotateUv
ScaleUv
```

Do not migrate low-level primitives prematurely.

## Milestone 4 — AI/tool discovery API

Add API operations for:

```text
list functions
search functions
describe function
list inputs
list outputs
list domains
describe domain
```

Ensure responses are versioned and machine-readable.

## Milestone 5 — Full semantic material construction API

Add high-level commands for:

```text
CreateMaterial
AddParameter
UseInput
AddFunctionCall
Connect
SetOutput
SetRenderState
```

Maintain the existing plan/validate/diff/commit transaction model.

## Milestone 6 — Aestra WESL standard library

Create the semantic WESL library:

```text
aestra::material
aestra::particle
aestra::scene
aestra::coordinates
aestra::color
aestra::noise
```

Map semantic runtime inputs to backend bindings internally.

## Milestone 7 — WESL-backed material functions

- define `WeslMaterialFunction`;
- parse/validate function signatures;
- register typed inputs/outputs;
- resolve WESL modules through the existing compiler path;
- expose WESL functions as graph nodes;
- preserve source maps and diagnostics;
- support project-local functions first.

## Milestone 8 — Editable graph projection

- keep the graph as a projection;
- graph actions emit semantic material commands;
- node movement/grouping remains UI metadata only;
- graph edits must produce exactly the same transactions available to AI/scripts.

## Milestone 9 — AI-assisted material creation

Implement a tool workflow:

```text
inspect capabilities
    ↓
plan semantic material
    ↓
construct material
    ↓
compile/validate
    ↓
return semantic diff + diagnostics
```

The AI should only create WESL functions when the semantic registry cannot express the requested behavior cleanly.

## Milestone 10 — Advanced WESL authoring

Only after WESL-backed functions are mature, consider:

- embedded WESL editor;
- double-click graph function node to edit source;
- reusable WESL modules;
- package imports;
- AI WESL repair/refactoring;
- optional whole-material expert mode if a concrete use case justifies it.

Do not make arbitrary whole-material WESL the primary workflow by default.

---

# 24. Testing Strategy

## Semantic round-trip tests

Validate:

```text
MaterialProgram
    ↓ compile
Material IR
    ↓ compile
shader
```

without semantic drift.

## Function registry tests

For every function:

- input count validation;
- input type validation;
- output type validation;
- domain compatibility;
- evaluation-domain compatibility;
- deterministic registry lookup;
- stable function IDs.

## WESL function tests

Test:

- signature extraction;
- type compatibility;
- missing imports;
- invalid WESL;
- diagnostics mapped to the semantic function;
- source mapping into generated WGSL;
- multiple custom functions in one material;
- conflicting names/namespaces.

## AI API tests

Use fixture requests such as:

```text
"Create a particle material that fades near the end of life."

"Add a blue Fresnel edge to this material."

"Add animated turbulent distortion."
```

Validate that the resulting semantic plan:

- contains no graph-layout mutations;
- uses discoverable functions;
- creates parameters where appropriate;
- produces a valid material;
- compiles successfully.

---

# 25. Non-Goals

Do not attempt to solve these immediately:

- arbitrary statement-by-statement WESL-to-node conversion;
- a general visual programming language;
- arbitrary WGSL editing;
- every possible material domain at once;
- immediate shader package marketplace support;
- premature cross-engine shader backend abstraction beyond preserving semantic boundaries;
- forcing graph and source formatting to round-trip identically.

---

# 26. Design Principles

## Semantic first

Aestra's authoring model should describe **what an effect means**, not how a GPU binding happens to be wired.

## One semantic source of truth

`MaterialProgram` owns material meaning.

The graph, stack UI, inspector, AI and compiler all consume or mutate that representation.

## AI and humans use the same operations

There should not be a special hidden AI material representation.

AI calls the same semantic tools used by the editor.

## WESL is the extensibility language

Use WESL when users or AI need programmable shader behavior beyond the built-in semantic vocabulary.

## Custom WESL functions are atomic semantic nodes

Do not visually expand arbitrary program control flow unless explicitly requested by an expert tooling mode.

## Backends remain replaceable

Do not encode Bevy/wgpu binding details in `MaterialProgram`.

The semantic representation should remain portable to future runtime integrations.

---

# 27. Final Recommended Architecture

```text
Natural language / Human editing
             │
             ▼
      Semantic authoring API
             │
             ▼
        MaterialProgram
             │
      ┌──────┼─────────┐
      │      │         │
      ▼      ▼         ▼
    Graph  Inspector   AI inspection
      │
      └──────── semantic edits ───────┐
                                      │
                                      ▼
                                MaterialProgram
                                      │
                                      ▼
                                Material IR
                                      │
                    ┌─────────────────┼─────────────────┐
                    │                                   │
              Built-in functions                 WESL functions
                    │                                   │
                    └─────────────────┬─────────────────┘
                                      ▼
                                WESL composition
                                      │
                                      ▼
                                   wesl-rs
                                      │
                                      ▼
                                     WGSL
                                      │
                                      ▼
                                     Naga
                                      │
                                      ▼
                                  GPU runtime
```

The central idea is:

> **`MaterialProgram` is Aestra's semantic authoring language. WESL is the programmable implementation language behind semantic functions and the shader backend.**

This gives Aestra:

- a clean node graph;
- reliable AI editing;
- deterministic semantic diffs;
- arbitrary programmable shader extensions;
- reusable material libraries;
- future package/marketplace compatibility;
- better diagnostics;
- engine portability;
- no WESL → IR → WESL round-trip.
